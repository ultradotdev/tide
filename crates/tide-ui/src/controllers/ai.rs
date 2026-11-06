//! AI: the Agents sidebar over native AI CLI sessions and their terminal tabs.
use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use tide_ai::cli::{
    self as ai_cli, CliProvider, CliSession, CliStatus, DiscoveryReport, DiscoveryState,
    ProviderDiscovery,
};
use tokio::sync::mpsc::Sender;

use super::Ui;
use crate::app::{AppEvent, Focus};
use crate::jobs::Jobs;
use crate::modal::{self, ConfirmAction, ModalItem, ModalKind, PromptAction, Submitted};
use crate::terminal_tab::{SpawnSpec, TerminalTab};
use crate::workbench::Tab;

#[derive(Debug)]
pub enum AiEvent {
    Sessions(DiscoveryReport),
    Provider(ProviderDiscovery),
    CliPrepared {
        provider: CliProvider,
        executable: PathBuf,
        result: Result<(Vec<String>, Option<String>), String>,
    },
    CliRenamed {
        provider: String,
        session: String,
        title: String,
        error: Option<String>,
    },
    SessionAction {
        provider: String,
        session: String,
        archive: bool,
        error: Option<String>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AgentRow {
    Provider(CliProvider),
    Session(usize),
    Status(CliProvider),
}

pub(crate) struct AiController {
    pub cli_status: Vec<CliStatus>,
    pub agent_sessions: Vec<CliSession>,
    pub agent_candidates: Vec<CliSession>,
    pub agent_reports: HashMap<CliProvider, ProviderDiscovery>,
    pub agent_query: String,
    pub agent_search: bool,
    next_cli_id: usize,
    last_catalog_refresh: Instant,
    native_tasks: Vec<tokio::task::JoinHandle<()>>,
    native_cancel: tide_ai::native_cli::Cancellation,
    pub agent_row: usize,
    pub agent_scroll: usize,
    pub agent_collapsed: HashSet<String>,
    pub agents_visible: bool,
    pub agents_width: u16,
    pub agents_loading: bool,
    pub agents_error: Option<String>,
    discovery: AgentDiscovery,
    jobs: Jobs,
    events: Sender<AppEvent>,
}

impl AiController {
    pub fn load(discovery: AgentDiscovery, jobs: &Jobs, events: Sender<AppEvent>) -> Self {
        let cli_status = ai_cli::detect_all();
        Self {
            cli_status,
            agent_sessions: Vec::new(),
            agent_candidates: Vec::new(),
            agent_reports: HashMap::new(),
            agent_query: String::new(),
            agent_search: false,
            next_cli_id: 1,
            last_catalog_refresh: Instant::now(),
            native_tasks: Vec::new(),
            native_cancel: tide_ai::native_cli::Cancellation::default(),
            agent_row: 0,
            agent_scroll: 0,
            agent_collapsed: HashSet::new(),
            agents_visible: false,
            agents_width: 34,
            agents_loading: false,
            agents_error: None,
            discovery,
            jobs: jobs.clone(),
            events,
        }
    }

    /// Drops finished CLI work and refreshes a visible Agents sidebar every 15 seconds;
    /// true when it started a refresh.
    pub fn tick(&mut self, ui: &mut Ui<'_>) -> bool {
        self.native_tasks.retain(|task| !task.is_finished());
        if !self.agents_loading
            && self.agents_visible
            && self.last_catalog_refresh.elapsed() > Duration::from_secs(15)
        {
            self.refresh_sessions(ui);
            return true;
        }
        false
    }

    /// Cancels native CLI work and waits briefly for it to stop.
    pub async fn stop_native_tasks(&mut self) {
        self.native_cancel.cancel();
        let native = std::mem::take(&mut self.native_tasks);
        let _ = tokio::time::timeout(Duration::from_secs(10), async {
            for task in native {
                let _ = task.await;
            }
        })
        .await;
    }

    pub fn handle_event(&mut self, event: AiEvent, ui: &mut Ui<'_>) {
        match event {
            AiEvent::CliPrepared {
                provider,
                executable,
                result,
            } => match result {
                Ok((args, id)) => self.spawn_prepared_cli(provider, executable, args, id, ui),
                Err(error) => ui.status(format!("Agents: {error}")),
            },
            AiEvent::CliRenamed {
                provider,
                session,
                title,
                error,
            } => {
                if let Some(error) = error {
                    ui.status(format!("Agents: {error}"));
                } else {
                    for item in &mut self.agent_candidates {
                        if item.provider_id == provider && item.session_id == session {
                            item.title = title.clone();
                        }
                    }
                    self.rebuild_sessions(ui);
                    self.refresh_sessions(ui);
                    ui.status("Agents: session renamed");
                }
            }
            AiEvent::SessionAction {
                provider,
                session,
                archive,
                error,
            } => {
                if let Some(error) = error {
                    ui.status(format!("Agents: {error}"));
                } else {
                    ui.workbench.close_ai_cli(&provider, &session);
                    ui.status(format!(
                        "Agents: {} {session}",
                        if archive { "archived" } else { "deleted" }
                    ));
                    self.refresh_sessions(ui);
                }
            }
            AiEvent::Sessions(report) => {
                self.agents_loading = false;
                if !report.statuses.is_empty() {
                    self.cli_status = report.statuses;
                }
                if report.providers.is_empty() {
                    if !report.sessions.is_empty() || report.errors.is_empty() {
                        self.agent_candidates = report.sessions;
                        self.rebuild_sessions(ui);
                    }
                } else {
                    for provider in report.providers {
                        self.merge_provider(provider, ui);
                    }
                }
                self.agents_error = (!report.errors.is_empty()).then(|| report.errors.join("; "));
                self.update_picker(ui);
            }
            AiEvent::Provider(report) => self.merge_provider(report, ui),
        }
    }

    /// Handles the AI parts of a modal submission and hands back anything else.
    pub fn submit(&mut self, submitted: Submitted, ui: &mut Ui<'_>) -> Option<Submitted> {
        match submitted {
            Submitted::Prompt(PromptAction::RenameAgent(provider, id), input) => {
                self.rename_session(&provider, &id, &input, ui)
            }
            Submitted::AgentSession { provider, session } => {
                self.launch_cli(provider, Some(&session), ui);
            }
            Submitted::AiCli(provider) => self.launch_cli(provider, None, ui),
            Submitted::Confirm(ConfirmAction::DeleteAiSession {
                provider,
                session,
                archive,
            }) => {
                self.run_session_action(&provider, &session, archive, ui);
            }
            other => return Some(other),
        }
        None
    }

    pub fn open_cli_picker(&mut self, ui: &mut Ui<'_>) {
        let items = self
            .cli_status
            .iter()
            .filter(|status| status.installed)
            .map(|status| {
                modal::ModalItem::new(
                    status.provider.display_name(),
                    status
                        .executable
                        .as_ref()
                        .map(|path| path.display().to_string())
                        .unwrap_or_default(),
                    status.provider.id(),
                )
            })
            .collect::<Vec<_>>();
        if items.is_empty() {
            ui.status("AI CLI: no supported agents found in PATH");
            return;
        }
        let mut picker = modal::ai_cli_picker(items);
        if let Some(index) = ui
            .settings
            .get("ai.cli.defaultProvider")
            .and_then(|v| v.as_str())
            .and_then(|id| picker.base().items.iter().position(|item| item.value == id))
        {
            picker.base_mut().selected = index;
        }
        *ui.overlay = Some(picker);
    }

    pub fn open_session_picker(&mut self, ui: &mut Ui<'_>) {
        self.refresh_sessions(ui);
        *ui.overlay = Some(modal::agent_sessions(self.session_items()));
        self.update_picker(ui);
    }

    pub fn launch_cli(&mut self, provider: CliProvider, session_id: Option<&str>, ui: &mut Ui<'_>) {
        if let Some(session) = session_id
            && ui.workbench.focus_ai_cli(provider.id(), session)
        {
            *ui.focus = Focus::Main;
            ui.status(format!(
                "Agents: focused {} session",
                provider.display_name()
            ));
            return;
        }
        let status = self
            .cli_status
            .iter()
            .find(|status| status.provider == provider);
        let Some(executable) = status.and_then(|status| status.executable.clone()) else {
            ui.status(format!(
                "AI CLI: {} is not installed (expected `{}`)",
                provider.display_name(),
                provider.executable()
            ));
            return;
        };
        if session_id.is_none() && matches!(provider, CliProvider::Codex | CliProvider::OpenCode) {
            let sender = self.events.clone();
            let workspace = ui.workspace.to_path_buf();
            ui.status(format!(
                "Agents: creating native {} session...",
                provider.display_name()
            ));
            let cancellation = self.native_cancel.clone();
            let task = self.jobs.spawn_blocking(move || {
                let _scope = tide_ai::native_cli::CancellationScope::enter(cancellation);
                let result = tide_ai::native_cli::create_session(provider, &executable, &workspace);
                let _ = sender.blocking_send(AppEvent::Ai(AiEvent::CliPrepared {
                    provider,
                    executable,
                    result,
                }));
            });
            self.native_tasks.push(task);
            return;
        }
        let (args, captured_id) = session_id.map_or_else(
            || provider.prepare_new(ui.workspace),
            |id| {
                (
                    provider.launch_args(ui.workspace, Some(id)),
                    Some(id.to_owned()),
                )
            },
        );
        self.spawn_prepared_cli(provider, executable, args, captured_id, ui);
    }

    pub fn spawn_prepared_cli(
        &mut self,
        provider: CliProvider,
        executable: PathBuf,
        args: Vec<String>,
        session_id: Option<String>,
        ui: &mut Ui<'_>,
    ) {
        if let Some(id) = &session_id
            && ui.workbench.focus_ai_cli(provider.id(), id)
        {
            *ui.focus = Focus::Main;
            return;
        }
        let terminal_id = self.next_cli_id;
        self.next_cli_id += 1;
        let scrollback = ui
            .settings
            .get("tui.terminal.scrollback")
            .and_then(serde_json::Value::as_u64)
            .map_or(5_000, tide_core::num::saturating_usize);
        let mut spec = SpawnSpec::new(executable.display().to_string(), args);
        spec.scrollback = scrollback;
        match TerminalTab::spawn_configured(
            terminal_id,
            ui.workspace,
            spec,
            ui.setting_bool("ai.autoStartCli", true),
        ) {
            Ok(mut terminal) => {
                terminal.title = session_id.as_deref().map_or_else(
                    || provider.display_name().to_owned(),
                    |id| {
                        format!(
                            "{} {}",
                            provider.display_name(),
                            crate::display::short_id(id)
                        )
                    },
                );
                terminal.ai_provider = Some(provider.id().to_owned());
                terminal.ai_session_id = session_id;
                ui.workbench.insert_tab_focused(Tab::Terminal(terminal));
                *ui.focus = Focus::Main;
                ui.mark_session_dirty();
            }
            Err(error) => ui.status(format!("AI CLI: failed to launch: {error:#}")),
        }
    }

    pub fn selected_session(&self, ui: &Ui<'_>) -> Option<(CliProvider, String)> {
        if *ui.focus == Focus::Agents {
            let rows = self.rows();
            let AgentRow::Session(index) = rows.get(self.agent_row)? else {
                return None;
            };
            let session = self.agent_sessions.get(*index)?;
            return Some((
                CliProvider::parse(&session.provider_id)?,
                session.session_id.clone(),
            ));
        }
        match ui.workbench.active_tab() {
            Some(Tab::Terminal(terminal)) => Some((
                CliProvider::parse(terminal.ai_provider.as_deref()?)?,
                terminal.ai_session_id.clone()?,
            )),
            _ => None,
        }
    }

    pub fn delete_selected_session(&mut self, archive: bool, ui: &mut Ui<'_>) {
        let Some((provider, session)) = self.selected_session(ui) else {
            ui.status("Agents: select a session first");
            return;
        };
        if archive && !provider.supports_archive() {
            ui.status(format!(
                "Agents: {} does not support archiving",
                provider.display_name()
            ));
            return;
        }
        if !archive && !provider.supports_delete() {
            ui.status(format!(
                "Agents: {} does not support deleting sessions",
                provider.display_name()
            ));
            return;
        }
        *ui.overlay = Some(modal::confirm(
            ConfirmAction::DeleteAiSession {
                provider: provider.id().to_owned(),
                session,
                archive,
            },
            if archive {
                "Archive Session"
            } else {
                "Delete Session"
            },
            if archive {
                "Archive this agent session?"
            } else {
                "Permanently delete this agent session? This cannot be undone."
            },
        ));
    }

    pub fn run_session_action(
        &mut self,
        provider_id: &str,
        session: &str,
        archive: bool,
        ui: &mut Ui<'_>,
    ) {
        let Some(provider) = CliProvider::parse(provider_id) else {
            return;
        };
        let args = if archive {
            provider.archive_args(session)
        } else {
            provider.delete_args(session)
        };
        let Some(args) = args else {
            ui.status("Agents: unsupported provider action");
            return;
        };
        let sender = self.events.clone();
        let workspace = ui.workspace.to_path_buf();
        let provider_id = provider_id.to_owned();
        let session = session.to_owned();
        let cancellation = self.native_cancel.clone();
        let task = self.jobs.spawn_blocking(move || {
            let _scope = tide_ai::native_cli::CancellationScope::enter(cancellation);
            let error = ai_cli::run_session_action(provider, &workspace, &args).err();
            let _ = sender.blocking_send(AppEvent::Ai(AiEvent::SessionAction {
                provider: provider_id,
                session,
                archive,
                error,
            }));
        });
        self.native_tasks.push(task);
    }

    pub fn rename_selected_session(&mut self, ui: &mut Ui<'_>) {
        let Some((provider, id)) = self.selected_session(ui) else {
            ui.status("Agents: select a session first");
            return;
        };
        if !provider.supports_rename() {
            ui.status(format!(
                "Agents: {} does not support native rename",
                provider.display_name()
            ));
            return;
        }
        let title = self
            .agent_sessions
            .iter()
            .find(|session| session.provider_id == provider.id() && session.session_id == id)
            .map(|s| s.title.as_str())
            .unwrap_or("");
        *ui.overlay = Some(modal::prompt(
            PromptAction::RenameAgent(provider.id().into(), id),
            "Rename Agent Session",
            title,
        ));
    }

    pub fn rename_session(&mut self, provider_id: &str, id: &str, title: &str, ui: &mut Ui<'_>) {
        let Some(provider) = CliProvider::parse(provider_id) else {
            return;
        };
        let Some(executable) = self
            .cli_status
            .iter()
            .find(|s| s.provider == provider)
            .and_then(|s| s.executable.clone())
        else {
            ui.status("Agents: provider is unavailable");
            return;
        };
        let sender = self.events.clone();
        let workspace = ui.workspace.to_path_buf();
        let id = id.to_owned();
        let title = title.to_owned();
        let cancellation = self.native_cancel.clone();
        let task = self.jobs.spawn_blocking(move || {
            let _scope = tide_ai::native_cli::CancellationScope::enter(cancellation);
            let error =
                tide_ai::native_cli::rename_session(provider, &executable, &workspace, &id, &title)
                    .err();
            let _ = sender.blocking_send(AppEvent::Ai(AiEvent::CliRenamed {
                provider: provider.id().into(),
                session: id,
                title,
                error,
            }));
        });
        self.native_tasks.push(task);
    }

    pub fn merge_provider(&mut self, report: ProviderDiscovery, ui: &mut Ui<'_>) {
        if report.state == DiscoveryState::Ok {
            self.agent_candidates
                .retain(|s| s.provider_id != report.provider.id());
        }
        self.agent_candidates.extend(report.sessions.clone());
        ai_cli::normalize_sessions(&mut self.agent_candidates);
        self.agent_reports.insert(report.provider, report);
        self.rebuild_sessions(ui);
    }

    pub fn rebuild_sessions(&mut self, ui: &mut Ui<'_>) {
        let selected = self.rows().get(self.agent_row).and_then(|row| match row {
            AgentRow::Provider(provider) | AgentRow::Status(provider) => {
                Some((provider.id().to_owned(), None))
            }
            AgentRow::Session(index) => self
                .agent_sessions
                .get(*index)
                .map(|s| (s.provider_id.clone(), Some(s.session_id.clone()))),
        });
        let mut roots = vec![ui.workspace.to_path_buf()];
        if let Some(aliases) = ui
            .settings
            .get("ai.cli.workspaceAliases")
            .and_then(serde_json::Value::as_array)
        {
            roots.extend(
                aliases
                    .iter()
                    .filter_map(serde_json::Value::as_str)
                    .map(PathBuf::from)
                    .filter(|p| p.is_absolute() && p.parent().is_some()),
            );
        }
        let roots = roots
            .iter()
            .map(|p| canonical_session_path(p))
            .collect::<Vec<_>>();
        let mut sessions = self.agent_candidates.clone();
        if roots.len() == 1 {
            normalize_workspace_sessions(&mut sessions, ui.workspace);
        }
        sessions.retain(|session| {
            session.cwd.as_deref().is_some_and(|cwd| {
                Path::new(cwd).is_absolute()
                    && roots.iter().any(|root| {
                        ai_cli::normalize_session_path(Path::new(cwd)).starts_with(root)
                    })
            })
        });
        ai_cli::normalize_sessions(&mut sessions);
        sort_agent_sessions(&mut sessions);
        self.agent_sessions = sessions;
        if let Some((provider, id)) = selected {
            self.agent_row = self
                .rows()
                .iter()
                .position(|row| match row {
                    AgentRow::Provider(p) | AgentRow::Status(p) => {
                        id.is_none() && p.id() == provider
                    }
                    AgentRow::Session(index) => self.agent_sessions.get(*index).is_some_and(|s| {
                        s.provider_id == provider && Some(&s.session_id) == id.as_ref()
                    }),
                })
                .unwrap_or(0);
        }
        self.sync_selection();
        self.update_picker(ui);
    }

    pub fn update_picker(&mut self, ui: &mut Ui<'_>) {
        let items = self.session_items();
        if let Some(overlay) = &mut ui.overlay
            && *overlay.kind() == ModalKind::AgentSessions
        {
            let old = overlay.selected_item().map(|item| item.value.clone());
            overlay.base_mut().items = items;
            overlay.base_mut().refilter();
            if let Some(old) = old
                && let Some(index) = overlay
                    .base()
                    .filtered
                    .iter()
                    .position(|i| overlay.base().items[*i].value == old)
            {
                overlay.base_mut().selected = index;
            }
            overlay.base_mut().hints = Some(
                if self.agents_loading {
                    "Refreshing providers..."
                } else {
                    "Enter open · Esc cancel"
                }
                .into(),
            );
        }
    }

    pub fn session_items(&self) -> Vec<ModalItem> {
        self.agent_sessions
            .iter()
            .map(|session| {
                ModalItem::new(
                    session.title.clone(),
                    format!(
                        "{} · {}",
                        session.provider_id,
                        session.cwd.as_deref().unwrap_or("")
                    ),
                    format!("{}\t{}", session.provider_id, session.session_id),
                )
            })
            .collect()
    }

    pub fn open_workspaces(&mut self, ui: &mut Ui<'_>) {
        let mut paths = BTreeMap::<String, usize>::new();
        for session in &self.agent_candidates {
            if let Some(cwd) = &session.cwd
                && Path::new(cwd).is_absolute()
                && Path::new(cwd).parent().is_some()
            {
                *paths.entry(cwd.clone()).or_default() += 1;
            }
        }
        *ui.overlay = Some(modal::agent_workspaces(
            paths
                .into_iter()
                .map(|(path, count)| {
                    ModalItem::new(path.clone(), format!("{count} sessions"), path)
                })
                .collect(),
        ));
    }

    pub fn refresh_sessions(&mut self, ui: &mut Ui<'_>) {
        if self.agents_loading {
            return;
        }
        self.agents_loading = true;
        self.last_catalog_refresh = Instant::now();
        self.agents_error = None;
        let sender = self.events.clone();
        let workspace = ui.workspace.to_path_buf();
        let cancellation = self.native_cancel.clone();
        let discover = self.discovery;
        let task = self.jobs.spawn_blocking(move || {
            let _scope = tide_ai::native_cli::CancellationScope::enter(cancellation);
            let updates = sender.clone();
            let report = discover(&workspace, &move |provider| {
                let _ = updates.blocking_send(AppEvent::Ai(AiEvent::Provider(provider)));
            });

            let _ = sender.blocking_send(AppEvent::Ai(AiEvent::Sessions(report)));
        });
        self.native_tasks.push(task);
    }

    pub fn rows(&self) -> Vec<AgentRow> {
        let mut rows = Vec::new();
        for provider in ai_cli::CLI_PROVIDERS {
            let sessions = self
                .agent_sessions
                .iter()
                .enumerate()
                .filter(|(_, session)| {
                    session.provider_id == provider.id()
                        && (self.agent_query.is_empty()
                            || format!(
                                "{} {} {}",
                                session.title,
                                session.session_id,
                                session.model.as_deref().unwrap_or("")
                            )
                            .to_lowercase()
                            .contains(&self.agent_query.to_lowercase()))
                })
                .map(|(index, _)| index)
                .collect::<Vec<_>>();
            if sessions.is_empty()
                && !self
                    .cli_status
                    .iter()
                    .any(|status| status.provider == provider)
                && !self.agent_reports.contains_key(&provider)
            {
                continue;
            }
            rows.push(AgentRow::Provider(provider));
            if !self.agent_collapsed.contains(provider.id()) {
                if sessions.is_empty()
                    || self
                        .agent_reports
                        .get(&provider)
                        .is_some_and(|report| report.error.is_some())
                {
                    rows.push(AgentRow::Status(provider));
                }
                rows.extend(sessions.into_iter().map(AgentRow::Session));
            }
        }
        rows
    }

    pub fn sync_selection(&mut self) {
        let rows = self.rows();
        self.agent_row = self.agent_row.min(rows.len().saturating_sub(1));
    }

    pub fn toggle_provider(&mut self, provider: CliProvider, ui: &mut Ui<'_>) {
        *ui.focus = Focus::Agents;
        if !self.agent_collapsed.remove(provider.id()) {
            self.agent_collapsed.insert(provider.id().to_owned());
        }
        self.agent_row = self
            .rows()
            .iter()
            .position(|row| *row == AgentRow::Provider(provider))
            .unwrap_or(0);
        ui.mark_session_dirty();
    }

    pub fn open_session(&mut self, index: usize, ui: &mut Ui<'_>) {
        if let Some(session) = self.agent_sessions.get(index).cloned()
            && let Some(provider) = CliProvider::parse(&session.provider_id)
        {
            self.agent_row = self
                .rows()
                .iter()
                .position(|row| *row == AgentRow::Session(index))
                .unwrap_or(0);
            self.launch_cli(provider, Some(&session.session_id), ui);
        }
    }

    pub fn handle_agents_key(&mut self, key: KeyEvent, ui: &mut Ui<'_>) {
        if self.agent_search {
            match key.code {
                KeyCode::Esc | KeyCode::Enter => self.agent_search = false,
                KeyCode::Backspace => {
                    let end = tide_core::grapheme_boundaries(&self.agent_query)
                        .into_iter()
                        .rev()
                        .nth(1)
                        .unwrap_or(0);
                    let byte = self
                        .agent_query
                        .char_indices()
                        .nth(end)
                        .map_or(self.agent_query.len(), |(byte, _)| byte);
                    self.agent_query.truncate(byte);
                    self.agent_row = 0;
                }
                KeyCode::Char(ch)
                    if !key.modifiers.intersects(
                        KeyModifiers::CONTROL | KeyModifiers::ALT | KeyModifiers::SUPER,
                    ) =>
                {
                    if self.agent_query.chars().count() < 512 {
                        self.agent_query.push(ch);
                    }
                    self.agent_row = 0;
                }
                _ => {}
            }
            self.sync_selection();
            return;
        }
        let rows = self.rows();
        match key.code {
            KeyCode::Up | KeyCode::Char('k') => self.agent_row = self.agent_row.saturating_sub(1),
            KeyCode::Down | KeyCode::Char('j') => {
                self.agent_row = (self.agent_row + 1).min(rows.len().saturating_sub(1))
            }
            KeyCode::Home => self.agent_row = 0,
            KeyCode::End => self.agent_row = rows.len().saturating_sub(1),
            KeyCode::PageUp => self.agent_row = self.agent_row.saturating_sub(10),
            KeyCode::PageDown => {
                self.agent_row = (self.agent_row + 10).min(rows.len().saturating_sub(1))
            }
            KeyCode::Enter | KeyCode::Char('o') | KeyCode::Char(' ') => {
                match rows.get(self.agent_row) {
                    Some(AgentRow::Provider(provider)) => self.toggle_provider(*provider, ui),
                    Some(AgentRow::Session(index)) => self.open_session(*index, ui),
                    Some(AgentRow::Status(provider)) => self.show_status(*provider, ui),
                    None => {}
                }
            }
            KeyCode::Left | KeyCode::Right => {
                let provider = match rows.get(self.agent_row) {
                    Some(AgentRow::Provider(provider) | AgentRow::Status(provider)) => {
                        Some(*provider)
                    }
                    Some(AgentRow::Session(index)) => {
                        CliProvider::parse(&self.agent_sessions[*index].provider_id)
                    }
                    None => None,
                };
                if let Some(provider) = provider {
                    let collapsed = self.agent_collapsed.contains(provider.id());
                    if key.code == KeyCode::Left && !collapsed
                        || key.code == KeyCode::Right && collapsed
                    {
                        self.toggle_provider(provider, ui);
                    }
                }
            }
            KeyCode::Char('/') => self.agent_search = true,
            KeyCode::Char('w') => self.open_workspaces(ui),
            KeyCode::Char('n') => self.open_cli_picker(ui),
            KeyCode::Char('d') => self.delete_selected_session(false, ui),
            KeyCode::Char('a') => self.delete_selected_session(true, ui),
            KeyCode::Char('R') => self.rename_selected_session(ui),
            KeyCode::Char('r') => self.refresh_sessions(ui),
            KeyCode::Esc => *ui.focus = Focus::Main,
            _ => {}
        }
        self.sync_selection();
    }

    pub fn show_status(&mut self, provider: CliProvider, ui: &mut Ui<'_>) {
        let installed = self
            .cli_status
            .iter()
            .any(|status| status.provider == provider && status.installed);
        let count = self
            .agent_sessions
            .iter()
            .filter(|session| session.provider_id == provider.id())
            .count();
        let discovered = self
            .agent_candidates
            .iter()
            .filter(|session| session.provider_id == provider.id())
            .count();
        let report = self.agent_reports.get(&provider);
        let notice = report
            .and_then(|report| report.notice.as_deref())
            .map(|notice| match tide_ai::cli::SetupNotice::of(notice) {
                tide_ai::cli::SetupNotice::SignIn => format!(
                    "{} is installed but needs signing in or setting up before it can list sessions. Start a session to sign in, or run {} in a terminal. It said:\n\n{notice}",
                    provider.display_name(),
                    provider.executable()
                ),
                tide_ai::cli::SetupNotice::Ineligible => format!(
                    "{} is signed in, but the account can't use it, so it can't list sessions. Signing in again won't help; use an account or plan the CLI supports. It said:\n\n{notice}",
                    provider.display_name()
                ),
            });
        let status = report
            .and_then(|report| report.error.clone())
            .or(notice)
            .unwrap_or_else(|| {
                if installed {
                    "Discovery completed"
                } else {
                    "Provider is not installed or is not on PATH"
                }
                .to_owned()
            });
        *ui.overlay = Some(modal::info(
            provider.display_name(),
            &format!(
                "{status}\n\n{count} sessions associated with this workspace; {discovered} discovered overall.\n\nPress r in Agents to retry. Press w to associate a previous workspace after moving a project. No unrelated sessions are included automatically."
            ),
        ));
    }
}

fn sort_agent_sessions(sessions: &mut [CliSession]) {
    let provider_order = |provider: &str| {
        CliProvider::parse(provider)
            .and_then(|provider| {
                ai_cli::CLI_PROVIDERS
                    .iter()
                    .position(|candidate| *candidate == provider)
            })
            .unwrap_or(usize::MAX)
    };
    sessions.sort_by(|left, right| {
        provider_order(&left.provider_id)
            .cmp(&provider_order(&right.provider_id))
            .then_with(|| ai_cli::session_updated_at(right).cmp(&ai_cli::session_updated_at(left)))
    });
}

pub(crate) fn canonical_session_path(path: &Path) -> PathBuf {
    std::fs::canonicalize(path).unwrap_or_else(|_| {
        let mut result = PathBuf::new();
        for component in path.components() {
            match component {
                std::path::Component::ParentDir => {
                    result.pop();
                }
                std::path::Component::CurDir => {}
                _ => result.push(component.as_os_str()),
            }
        }
        result
    })
}

pub(crate) fn normalize_workspace_sessions(sessions: &mut Vec<CliSession>, workspace: &Path) {
    let workspace = canonical_session_path(workspace);
    sessions.retain(|session| {
        session.cwd.as_deref().is_some_and(|cwd| {
            let cwd = Path::new(cwd);
            cwd.is_absolute() && ai_cli::normalize_session_path(cwd).starts_with(&workspace)
        })
    });
    ai_cli::normalize_sessions(sessions);
    sort_agent_sessions(sessions);
}

/// Finds native AI CLI sessions for the Agents sidebar, reporting each provider as it finishes.
pub type AgentDiscovery = fn(&Path, &(dyn Fn(ProviderDiscovery) + Sync)) -> DiscoveryReport;

/// Discovers sessions from the installed AI CLIs and their stores under `HOME`.
pub fn discover_agent_sessions(
    workspace: &Path,
    update: &(dyn Fn(ProviderDiscovery) + Sync),
) -> DiscoveryReport {
    let Some(home) = std::env::var_os("HOME").map(std::path::PathBuf::from) else {
        return DiscoveryReport {
            state: Some(DiscoveryState::Error),
            errors: vec!["HOME is not set".to_owned()],
            ..DiscoveryReport::default()
        };
    };
    ai_cli::discover_all_with_updates(&home, workspace, update)
}
