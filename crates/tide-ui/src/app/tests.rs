use super::input::key_string;
use super::session::{convert_session_layout, session_layout};
use super::*;
use crate::connection_form::ConnectionAction;
use crate::controllers::ai::normalize_workspace_sessions;
use crate::controllers::db::DbEvent;
use crate::controllers::git::{GitState, run_git_checked, valid_branch_name};
use crate::controllers::lsp::{LspEvent, LspFileEdits, active_document, apply_edits, server_for};
use crate::modal::{CommandPaletteModal, InfoModal, SearchResultsModal};
use tide_ai::cli::{CliStatus, DiscoveryReport, DiscoveryState, ProviderDiscovery};
use tide_db::{ConnectionConfig, QueryResult, QueryStatus};
use tide_lsp::{
    CodeActionItem, CompletionItem, DocumentLease, Position as LspPosition, Range as LspRange,
    SymbolItem, TextEdit,
};
use tide_services::GitSnapshot;

mod ai;
mod db;
mod editing;
mod files;
mod git;
mod input;
mod lsp;
mod overlays;
mod panes;
mod preferences;
mod render;
mod session;

pub(super) fn submit_save_path(app: &mut App, path: &Path) {
    assert_eq!(*app.overlay.as_ref().unwrap().kind(), ModalKind::SaveAs);
    let base = app.overlay.as_mut().unwrap().base_mut();
    base.input = path.display().to_string();
    base.cursor = base.input.chars().count();
    base.refilter();
    app.handle_modal_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
}

pub(super) fn initialize_git(app: &App) {
    for args in [
        &["init", "-q"][..],
        &["config", "user.email", "tide@example.test"],
        &["config", "user.name", "Tide Test"],
    ] {
        assert!(
            tide_services::git_process::git(&app.workspace, args, None)
                .unwrap()
                .success
        );
    }
}

pub(super) fn freeze_lsp(app: &mut App, generation: u64) {
    app.focus = Focus::Main;
    app.lsp.generation = generation;
    app.lsp.origin.pane = app.workbench.focused();
    let editor = app.workbench.active_editor().unwrap();
    app.lsp.origin.document = Some(editor.document.clone());
    app.lsp.origin.revision = editor.revision();
    app.lsp.origin.cursor = editor.document.cursor().position;
}

pub(super) fn git_output_text(root: &Path, args: &[&str]) -> String {
    match tide_services::git_process::git(root, args, None) {
        Ok(output) if output.success => String::from_utf8_lossy(&output.stdout).into_owned(),
        Ok(output) => format!("Git failed: {}", String::from_utf8_lossy(&output.stderr)),
        Err(error) => format!("Git failed: {error:#}"),
    }
}

pub(super) fn no_agents(_: &Path, _: &(dyn Fn(ProviderDiscovery) + Sync)) -> DiscoveryReport {
    DiscoveryReport::default()
}

/// An app whose background work waits for [`App::settle`] and starts no language servers.
pub(super) fn test_app() -> (tempfile::TempDir, App) {
    let (home, mut app) = test_app_with(Jobs::manual());
    app.settings.set("lsp.servers", no_language_servers());
    (home, app)
}

pub(super) fn no_language_servers() -> serde_json::Value {
    tide_lsp::default_servers()
        .into_iter()
        .map(|server| (server.language_id, serde_json::Value::Null))
        .collect::<serde_json::Map<_, _>>()
        .into()
}

/// An app whose background work runs on the test's tokio runtime.
pub(super) fn test_app_async() -> (tempfile::TempDir, App) {
    test_app_with(Jobs::runtime(tokio::runtime::Handle::current()))
}

pub(super) fn test_app_with(jobs: Jobs) -> (tempfile::TempDir, App) {
    let home = tempfile::tempdir().unwrap();
    let workspace = home.path().join("workspace");
    std::fs::create_dir_all(&workspace).unwrap();
    let paths = Paths::from_env(home.path(), |_| None);
    let app = App::new(AppConfig {
        workspace,
        initial_file: None,
        session_name: None,
        restore_session: false,
        shared_sessions: false,
        paths: Some(paths),
        jobs,
        agent_discovery: no_agents,
    })
    .unwrap();
    (home, app)
}

pub(super) fn click_hit(app: &mut App, predicate: impl Fn(&HitTarget) -> bool) {
    let rect = app
        .layout
        .hits
        .iter()
        .find(|(_, target)| predicate(target))
        .expect("missing click target")
        .0;
    app.handle_event(Event::Mouse(MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: rect.x,
        row: rect.y,
        modifiers: KeyModifiers::NONE,
    }));
}

/// The startup workbench at `width`×`height`, drawn with host-dependent state pinned.
pub(super) fn pinned_workbench(
    width: u16,
    height: u16,
) -> (
    tempfile::TempDir,
    App,
    ratatui::Terminal<ratatui::backend::TestBackend>,
) {
    let home = tempfile::tempdir().unwrap();
    let workspace = home.path().join("workspace");
    std::fs::create_dir_all(workspace.join("src")).unwrap();
    std::fs::write(workspace.join("src/main.ts"), "const answer = 42;\n").unwrap();
    std::fs::write(workspace.join("README.md"), "# Tide\n").unwrap();
    let paths = Paths::from_env(home.path(), |_| None);
    std::fs::create_dir_all(paths.settings_file.parent().unwrap()).unwrap();
    let settings = serde_json::json!({ "lsp.servers": no_language_servers() });
    std::fs::write(&paths.settings_file, settings.to_string()).unwrap();
    let mut app = App::new(AppConfig {
        workspace: workspace.clone(),
        initial_file: Some(workspace.join("src/main.ts")),
        session_name: None,
        restore_session: false,
        shared_sessions: false,
        paths: Some(paths),
        jobs: Jobs::manual(),
        agent_discovery: no_agents,
    })
    .unwrap();
    app.settle();
    app.status.clear();
    // Detection probes the host PATH; pin it so the snapshot is machine-independent.
    app.ai.cli_status = ai_cli::CLI_PROVIDERS
        .into_iter()
        .map(|provider| CliStatus {
            provider,
            installed: false,
            executable: None,
        })
        .collect();
    let backend = ratatui::backend::TestBackend::new(width, height);
    let mut terminal = ratatui::Terminal::new(backend).unwrap();
    terminal.draw(|frame| app.draw(frame)).unwrap();
    (home, app, terminal)
}

pub(super) fn snapshot_app(width: u16, height: u16) -> String {
    let (_home, _app, terminal) = pinned_workbench(width, height);
    crate::test_support::screen_text(&terminal)
}

pub(super) fn snapshot_modal(width: u16, height: u16, open: impl FnOnce(&mut App)) -> String {
    let (_home, mut app) = test_app();
    // Startup work settles first; its status messages are not part of a modal snapshot.
    let status = app.status.clone();
    app.settle();
    app.status = status;
    open(&mut app);
    let backend = ratatui::backend::TestBackend::new(width, height);
    let mut terminal = ratatui::Terminal::new(backend).unwrap();
    terminal.draw(|frame| app.draw(frame)).unwrap();
    crate::test_support::screen_text(&terminal)
}
