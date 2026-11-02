use super::*;

#[test]
fn stages_and_commits_a_git_change() {
    let (_home, mut app) = test_app();
    let git = |args: &[&str]| {
        let status = std::process::Command::new("git")
            .arg("-C")
            .arg(&app.workspace)
            .args(args)
            .status()
            .unwrap();
        assert!(status.success(), "git {args:?} failed");
    };
    git(&["init", "-q"]);
    git(&["config", "user.email", "tide@example.test"]);
    git(&["config", "user.name", "Tide Test"]);
    std::fs::write(app.workspace.join("file.txt"), "one\n").unwrap();
    git(&["add", "file.txt"]);
    git(&["commit", "-q", "-m", "initial"]);
    std::fs::write(app.workspace.join("file.txt"), "two\n").unwrap();
    with_ui!(app, |ui| app.git.refresh(ui));
    app.settle();
    assert!(
        app.git
            .state
            .changed()
            .iter()
            .any(|path| path == "file.txt")
    );
    with_ui!(app, |ui| app.git.stage_all(ui));
    app.settle();
    with_ui!(app, |ui| app.git.commit("phase 3", ui));
    app.settle();
    let output = std::process::Command::new("git")
        .arg("-C")
        .arg(&app.workspace)
        .args(["log", "-1", "--format=%s"])
        .output()
        .unwrap();
    assert_eq!(String::from_utf8_lossy(&output.stdout).trim(), "phase 3");
}

#[test]
fn git_groups_edit_modes_history_and_recovery_preserve_versions() {
    let (_home, mut app) = test_app();
    initialize_git(&app);
    let path = app.workspace.join("file.txt");
    std::fs::write(&path, "one\ntwo\n").unwrap();
    with_ui!(app, |ui| app.git.stage_all(ui));
    app.settle();
    with_ui!(app, |ui| app.git.commit("initial", ui));
    app.settle();
    let hash = app
        .git
        .state
        .snapshot
        .as_ref()
        .unwrap()
        .head
        .clone()
        .unwrap();
    std::fs::write(&path, "ONE\ntwo\n").unwrap();
    with_ui!(app, |ui| app.git.refresh(ui));
    app.settle();
    let mut terminal = ratatui::Terminal::new(ratatui::backend::TestBackend::new(120, 40)).unwrap();
    terminal.draw(|f| app.draw(f)).unwrap();
    assert!(matches!(
        app.git.panel.file(),
        Some((crate::git_panel::Group::Unstaged, _))
    ));
    with_ui!(app, |ui| app.git.open_selected_diff(ui));
    app.settle();
    assert!(
        matches!(app.workbench.active_tab(),Some(Tab::Diff(tab)) if tab.browser.as_ref().unwrap().diff.new_text=="ONE\ntwo\n")
    );
    with_ui!(app, |ui| app
        .git
        .diff_action(crate::diff_browser::Action::Edit, ui));
    app.settle();
    app.workbench
        .active_editor_mut()
        .unwrap()
        .document
        .set_content("custom\ntwo\n");
    app.save_active();
    app.settle();
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "ONE\ntwo\n");
    assert_eq!(
        git_output_text(&app.workspace, &["show", ":file.txt"]),
        "custom\ntwo\n"
    );
    // A dirty hunk survives a session round trip with its guarded target identity.
    app.workbench
        .active_editor_mut()
        .unwrap()
        .document
        .set_content("recovered\ntwo\n");
    let session = app.build_session();
    let mut restored = test_app().1;
    restored.workspace = app.workspace.clone();
    restored.git.state = GitState::refresh(&restored.workspace);
    restored.restore_session(session).unwrap();
    let edit = restored.workbench.active_editor().unwrap();
    assert_eq!(edit.document.text(), "recovered\ntwo\n");
    assert!(edit.hunk_edit.is_some());
    // A stale index rejects the recovered hunk without changing either version.
    with_ui!(app, |ui| app.git.stage_all(ui));
    app.settle();
    restored.save_active();
    restored.settle();
    assert!(restored.status.contains("Index changed"));
    assert!(restored.workbench.active_editor().unwrap().is_dirty());
    app.handle_background(AppEvent::Git(GitEvent::HistoricalFile {
        path: PathBuf::from("file.txt"),
        hash: hash.clone(),
        result: Ok("one\ntwo\n".into()),
    }));
    let history = app.workbench.active_editor_mut().unwrap();
    history.document.set_content("bad");
    history.insert_text("bad");
    assert_eq!(history.document.text(), "one\ntwo\n");
    assert!(!history.is_dirty());
    assert!(history.save().is_err());
    let copy = app.workspace.join("copy.txt");
    history.save_as(copy.clone()).unwrap();
    history.insert_text("editable");
    assert!(history.is_dirty());
    assert_eq!(std::fs::read_to_string(&copy).unwrap(), "one\ntwo\n");
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "ONE\ntwo\n");
    app.execute_command(Command::GitDiff);
    app.settle();
}

#[test]
fn git_direct_hunk_write_rejects_dirty_authoritative_buffer_and_discard_cancel() {
    let (_home, mut app) = test_app();
    initialize_git(&app);
    let path = app.workspace.join("file.txt");
    std::fs::write(&path, "one\n").unwrap();
    with_ui!(app, |ui| app.git.stage_all(ui));
    app.settle();
    with_ui!(app, |ui| app.git.commit("initial", ui));
    app.settle();
    std::fs::write(&path, "two\n").unwrap();
    with_ui!(app, |ui| app.git.refresh(ui));
    app.settle();
    with_ui!(app, |ui| app.git.open_file_diff(
        "file.txt".into(),
        None,
        false,
        None,
        ui
    ));
    app.settle();
    with_ui!(app, |ui| app
        .git
        .diff_action(crate::diff_browser::Action::Edit, ui));
    app.settle();
    app.settings
        .set("tui.diffViewer.editMode", serde_json::json!("direct-write"));
    app.workbench
        .active_editor_mut()
        .unwrap()
        .document
        .set_content("written\n");
    app.save_active();
    app.settle();
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "written\n");
    // Establish a new snapshot, then make the authoritative buffer dirty.
    std::fs::write(&path, "two\n").unwrap();
    app.documents.forget(&path);
    app.open_file(&path);
    app.settle();
    assert!(app.workbench.active_editor().unwrap().inline.is_some());
    app.workbench
        .active_editor_mut()
        .unwrap()
        .insert_text("unsaved ");
    with_ui!(app, |ui| app.git.open_file_diff(
        "file.txt".into(),
        None,
        false,
        None,
        ui
    ));
    app.settle();
    with_ui!(app, |ui| app
        .git
        .diff_action(crate::diff_browser::Action::Edit, ui));
    app.settle();
    app.workbench
        .active_editor_mut()
        .unwrap()
        .document
        .set_content("three\n");
    app.settings
        .set("tui.diffViewer.editMode", serde_json::json!("direct-write"));
    app.save_active();
    app.settle();
    assert!(app.status.contains("Working buffer has changed"));
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "two\n");
    with_ui!(app, |ui| app.git.discard_selected(ui));
    app.settle();
    assert!(app.overlay.is_some());
    app.handle_modal_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
    app.settle();
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "two\n");
    app.overlay = Some(modal::confirm(
        ConfirmAction::DiscardGitChange("file.txt".into()),
        "Discard",
        "Discard?",
    ));
    app.handle_modal_key(KeyEvent::new(KeyCode::Char('y'), KeyModifiers::NONE));
    app.settle();
    assert!(app.status.contains("dirty working buffer"));
}

#[test]
fn git_stash_identity_guard_branch_cancel_and_outline_hierarchy() {
    let (_home, mut app) = test_app();
    initialize_git(&app);
    let path = app.workspace.join("file.txt");
    std::fs::write(&path, "one\n").unwrap();
    with_ui!(app, |ui| app.git.stage_all(ui));
    app.settle();
    with_ui!(app, |ui| app.git.commit("initial", ui));
    app.settle();
    std::fs::write(&path, "two\n").unwrap();
    with_ui!(app, |ui| app.git.run(&["stash", "push", "-m", "saved"], ui));
    app.settle();
    let oid = git_output_text(&app.workspace, &["rev-parse", "stash@{0}"])
        .trim()
        .to_owned();
    assert!(
        run_git_checked(
            &app.workspace,
            &["__stash_verified", "drop", "stash@{0}", &"0".repeat(40)],
            &std::sync::atomic::AtomicBool::new(false)
        )
        .is_err()
    );
    assert_eq!(
        git_output_text(&app.workspace, &["rev-parse", "stash@{0}"]).trim(),
        oid
    );
    with_ui!(app, |ui| app.git.open_choices("deleteBranch", ui));
    app.settle();
    assert!(app.overlay.as_ref().unwrap().selected_item().is_some());
    app.handle_modal_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
    app.settle();
    assert!(app.git.state.snapshot.as_ref().unwrap().branch.is_some());
    assert!(!valid_branch_name("--force"));
    assert!(!valid_branch_name("a..b"));
    assert!(valid_branch_name("topic/repair"));
    let mut editor = EditorState::scratch(
        "class Outer {\n  function child() {\n  }\n}\nfunction next() {}\n",
        "ts",
    );
    editor.goto_line(2);
    app.workbench.insert_tab_focused(Tab::Editor(editor));
    assert_eq!(app.outline_for_active().len(), 3);
    app.outline_selected = 0;
    app.toggle_outline(Some(false));
    assert_eq!(app.outline_for_active().len(), 2);
    app.toggle_outline(Some(true));
    assert_eq!(app.outline_for_active().len(), 3);
}

#[tokio::test(flavor = "multi_thread")]
async fn startup_defers_git_status_to_the_background_queue() {
    let home = tempfile::tempdir().unwrap();
    let workspace = home.path().join("workspace");
    std::fs::create_dir_all(&workspace).unwrap();
    assert!(
        tide_services::git_process::git(&workspace, &["init", "-q"], None)
            .unwrap()
            .success
    );
    std::fs::write(workspace.join("new.txt"), "x").unwrap();
    let mut app = App::new(AppConfig {
        workspace,
        initial_file: None,
        session_name: None,
        restore_session: false,
        ephemeral_session: false,
        paths: Some(Paths::from_env(home.path(), |_| None)),
        jobs: Jobs::runtime(tokio::runtime::Handle::current()),
        agent_discovery: no_agents,
        install: crate::help::Install::Unknown { executable: None },
    })
    .unwrap();
    assert!(app.git.state.snapshot.is_none());
    let mut events = app.take_event_receiver().unwrap();
    tokio::time::timeout(Duration::from_secs(10), async {
        while app.git.state.snapshot.is_none() {
            app.handle_background(events.recv().await.unwrap());
        }
    })
    .await
    .unwrap();
    assert_eq!(app.git.state.changed(), ["new.txt"]);
}

#[test]
fn first_async_git_refresh_invalidates_rows_when_startup_revision_collides() {
    let (_home, mut app) = test_app();
    let mut snapshot = GitSnapshot {
        root: app.workspace.clone(),
        branch: Some("main".into()),
        head: None,
        dirty: true,
        ahead_behind: None,
        changed_paths: vec!["file.txt".into()],
        changes: vec![tide_services::git::GitChange {
            path: "file.txt".into(),
            previous_path: None,
            index: None,
            worktree: Some(tide_services::git::GitStatus::Modified),
            lossy_path: false,
        }],
    };
    app.git.state = GitState {
        snapshot: Some(snapshot.clone()),
        selected: 0,
        revision: 1,
    };
    app.git.sync_panel();
    assert!(app.git.panel.rows.iter().any(|row| matches!(
        row,
        crate::git_panel::Row::Header(crate::git_panel::Group::Unstaged, 1)
    )));
    snapshot.changes[0].index = Some(tide_services::git::GitStatus::Modified);
    snapshot.changes[0].worktree = None;
    app.git.generation = 1;
    app.set_status("Edited hunk staged; working file preserved");
    app.handle_background(AppEvent::Git(GitEvent::Refreshed {
        generation: 1,
        git: GitState {
            snapshot: Some(snapshot),
            selected: 0,
            revision: 1,
        },
        timeline: vec![],
        message: None,
    }));
    app.git.sync_panel();
    assert_ne!(app.git.state.revision, 1);
    assert!(app.git.panel.rows.iter().any(|row| matches!(
        row,
        crate::git_panel::Row::Header(crate::git_panel::Group::Staged, 1)
    )));
    assert!(!app.git.panel.rows.iter().any(|row| matches!(
        row,
        crate::git_panel::Row::Header(crate::git_panel::Group::Unstaged, _)
    )));
    assert_eq!(app.status, "Edited hunk staged; working file preserved");
}

#[test]
fn lossy_git_paths_are_never_selected_for_path_actions() {
    let change = |path: &str, lossy_path| tide_services::git::GitChange {
        path: path.into(),
        previous_path: None,
        index: None,
        worktree: Some(tide_services::git::GitStatus::Modified),
        lossy_path,
    };
    let mut git = GitState {
        snapshot: Some(GitSnapshot {
            root: PathBuf::from("/repo"),
            branch: None,
            head: None,
            dirty: true,
            ahead_behind: None,
            changed_paths: vec!["bad\u{fffd}.txt".into(), "file.txt".into()],
            changes: vec![change("bad\u{fffd}.txt", true), change("file.txt", false)],
        }),
        selected: 0,
        revision: 1,
    };
    assert_eq!(git.selected_path(), None);
    let (_home, mut app) = test_app();
    app.git.state = git.clone();
    for action in [
        Command::GitPanelStage,
        Command::GitPanelUnstage,
        Command::GitPanelOpenFile,
        Command::GitPanelDiscard,
        Command::GitLogFile,
    ] {
        app.set_status("");
        app.execute_command(action);
        assert!(
            app.status.contains("not valid UTF-8"),
            "{action}: {}",
            app.status
        );
    }
    git.selected = 1;
    assert_eq!(git.selected_path(), Some("file.txt"));
}

#[tokio::test]
async fn git_jobs_are_fifo_and_closed_preview_results_do_not_steal_focus() {
    let (_home, mut app) = test_app_async();
    initialize_git(&app);
    let mut rx = app.take_event_receiver().unwrap();
    std::fs::write(app.workspace.join("file.txt"), "one\n").unwrap();
    with_ui!(app, |ui| app.git.stage_all(ui));
    with_ui!(app, |ui| app.git.commit("queued commit", ui));
    with_ui!(app, |ui| app.git.refresh(ui));
    let deadline = tokio::time::Instant::now() + Duration::from_secs(8);
    while tokio::time::Instant::now() < deadline {
        if let Ok(Some(event)) = tokio::time::timeout(Duration::from_millis(100), rx.recv()).await {
            app.handle_background(event);
        }
        if app
            .git
            .state
            .snapshot
            .as_ref()
            .and_then(|s| s.head.as_ref())
            .is_some()
        {
            break;
        }
    }
    assert_eq!(
        git_output_text(&app.workspace, &["log", "-1", "--format=%s"]).trim(),
        "queued commit"
    );
    with_ui!(app, |ui| app.git.open_output_tab("Log", &["log", "-1"], ui));
    let pane = app.workbench.focused();
    let index = app.workbench.focused_pane().active;
    app.force_close_tab(pane, index);
    app.new_file();
    if let Ok(Some(event)) = tokio::time::timeout(Duration::from_secs(3), rx.recv()).await {
        app.handle_background(event);
    }
    assert!(matches!(app.workbench.active_tab(), Some(Tab::Editor(_))));
    app.shutdown().await;
}

#[test]
fn space_in_source_control_collapses_the_selected_group_not_the_section() {
    let (_home, mut app) = test_app();
    let git = |args: &[&str]| {
        let status = std::process::Command::new("git")
            .arg("-C")
            .arg(&app.workspace)
            .args(args)
            .status()
            .unwrap();
        assert!(status.success(), "git {args:?} failed");
    };
    git(&["init", "-q"]);
    std::fs::write(app.workspace.join("new.txt"), "one\n").unwrap();
    with_ui!(app, |ui| app.git.refresh(ui));
    app.settle();
    app.focus_sidebar_section(1, true);
    app.git.sync_panel();
    let rows = app.git.panel.rows.len();
    assert!(rows > 1, "the Changes group should list new.txt");

    app.handle_key(KeyEvent::new(KeyCode::Char(' '), KeyModifiers::NONE));

    assert!(
        app.accordion.sections[1].expanded,
        "the section stayed open"
    );
    assert_eq!(app.git.panel.collapsed.len(), 1);
    assert!(app.git.panel.rows.len() < rows);
}

/// Runs Git for test setup; panics with its stderr when it fails.
fn git_in(root: &Path, args: &[&str]) -> String {
    let output = std::process::Command::new("git")
        .arg("-C")
        .arg(root)
        .args(args)
        .env("GIT_EDITOR", "true")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).trim().to_owned()
}

/// A repository on `main` with `file.txt` committed as `base`.
fn repository() -> (tempfile::TempDir, App) {
    let (home, app) = test_app();
    initialize_git(&app);
    git_in(&app.workspace, &["symbolic-ref", "HEAD", "refs/heads/main"]);
    commit_file(&app.workspace, "file.txt", "base\n", "base");
    (home, app)
}

fn commit_file(root: &Path, path: &str, content: &str, message: &str) {
    std::fs::write(root.join(path), content).unwrap();
    git_in(root, &["add", "--", path]);
    git_in(root, &["commit", "-q", "-m", message]);
}

fn refreshed(app: &mut App) {
    with_ui!(app, |ui| app.git.refresh(ui));
    app.settle();
}

fn confirm_overlay(app: &mut App) -> String {
    let overlay = app.overlay.as_ref().expect("a confirmation");
    assert!(
        matches!(overlay.kind(), ModalKind::Confirm(_)),
        "{:?}",
        overlay.kind()
    );
    let message = overlay.base().message.clone().unwrap_or_default();
    app.handle_modal_key(KeyEvent::new(KeyCode::Char('y'), KeyModifiers::NONE));
    app.settle();
    message
}

fn choose(app: &mut App, text: &str) {
    let base = app.overlay.as_mut().expect("a picker").base_mut();
    let labels = base
        .items
        .iter()
        .map(|item| item.label.clone())
        .collect::<Vec<_>>();
    let index = labels
        .iter()
        .position(|label| label.contains(text))
        .unwrap_or_else(|| panic!("{text} is not in {labels:?}"));
    base.selected = base.filtered.iter().position(|i| *i == index).unwrap();
    app.handle_modal_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    app.settle();
}

fn answer(app: &mut App, input: &str) {
    let base = app.overlay.as_mut().expect("a prompt").base_mut();
    base.input = input.to_owned();
    base.cursor = input.chars().count();
    app.handle_modal_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    app.settle();
}

#[test]
fn discard_discard_all_and_clean_each_touch_only_their_own_changes() {
    let (_home, mut app) = repository();
    commit_file(&app.workspace, "other.txt", "other\n", "other");
    commit_file(&app.workspace, ".gitignore", "*.log\n", "ignore logs");
    std::fs::write(app.workspace.join("file.txt"), "changed\n").unwrap();
    std::fs::write(app.workspace.join("other.txt"), "changed\n").unwrap();
    std::fs::write(app.workspace.join("new.txt"), "new\n").unwrap();
    std::fs::write(app.workspace.join("build.log"), "ignored\n").unwrap();
    refreshed(&mut app);

    app.open_file(app.workspace.join("file.txt"));
    app.execute_command(Command::GitDiscard);
    assert!(confirm_overlay(&mut app).contains("file.txt"));
    let root = app.workspace.clone();
    let read = |path: &str| std::fs::read_to_string(root.join(path)).unwrap();
    assert_eq!(read("file.txt"), "base\n");
    assert_eq!(read("other.txt"), "changed\n");
    app.execute_command(Command::GitDiscard);
    assert!(app.status.contains("no unstaged changes"), "{}", app.status);

    app.execute_command(Command::GitDiscardAll);
    assert!(confirm_overlay(&mut app).contains("1 tracked file"));
    assert_eq!(read("other.txt"), "other\n");
    assert!(
        app.workspace.join("new.txt").exists(),
        "untracked files wait for Clean"
    );
    app.execute_command(Command::GitDiscardAll);
    assert!(app.status.contains("no unstaged changes"), "{}", app.status);

    app.execute_command(Command::GitClean);
    let message = confirm_overlay(&mut app);
    assert!(message.contains("new.txt"), "{message}");
    assert!(!message.contains("build.log"), "{message}");
    assert!(!app.workspace.join("new.txt").exists());
    assert!(
        app.workspace.join("build.log").exists(),
        "ignored files stay"
    );
    app.execute_command(Command::GitClean);
    assert!(app.status.contains("no untracked files"), "{}", app.status);
}

#[test]
fn amend_prefills_the_message_keeps_the_body_and_warns_when_pushed() {
    let (home, mut app) = test_app();
    initialize_git(&app);
    git_in(&app.workspace, &["symbolic-ref", "HEAD", "refs/heads/main"]);
    std::fs::write(app.workspace.join("file.txt"), "one\n").unwrap();
    git_in(&app.workspace, &["add", "file.txt"]);
    git_in(
        &app.workspace,
        &["commit", "-q", "-m", "first", "-m", "keep this body"],
    );
    let remote = home.path().join("remote.git");
    git_in(
        home.path(),
        &["init", "-q", "--bare", remote.to_str().unwrap()],
    );
    git_in(
        &app.workspace,
        &["remote", "add", "origin", remote.to_str().unwrap()],
    );
    git_in(&app.workspace, &["push", "-q", "-u", "origin", "main"]);
    std::fs::write(app.workspace.join("file.txt"), "two\n").unwrap();
    git_in(&app.workspace, &["add", "file.txt"]);
    refreshed(&mut app);

    app.execute_command(Command::GitCommitAmend);
    app.settle();
    let prompt = app.overlay.as_ref().expect("the amend prompt");
    assert!(matches!(
        prompt.kind(),
        ModalKind::Prompt(PromptAction::CommitAmend { upstream: Some(upstream), .. })
            if upstream == "origin/main"
    ));
    assert_eq!(prompt.base().input, "first");
    answer(&mut app, "first, amended");
    let warning = confirm_overlay(&mut app);
    assert!(warning.contains("already on origin/main"), "{warning}");
    assert_eq!(
        git_in(&app.workspace, &["log", "--format=%B", "-n", "1"]),
        "first, amended\n\nkeep this body"
    );
    assert_eq!(
        git_in(&app.workspace, &["rev-list", "--count", "HEAD"]),
        "1"
    );
    assert_eq!(git_in(&app.workspace, &["show", "HEAD:file.txt"]), "two");

    // Force with lease replaces the rewritten remote branch.
    app.execute_command(Command::GitPushForce);
    assert!(matches!(
        app.overlay.as_ref().unwrap().kind(),
        ModalKind::Confirm(ConfirmAction::GitCommand(args)) if args == &["push", "--force-with-lease"]
    ));
    confirm_overlay(&mut app);
    assert_eq!(
        git_in(&remote, &["rev-parse", "main"]),
        git_in(&app.workspace, &["rev-parse", "HEAD"])
    );

    // An unpushed commit amends without the warning; an unchanged message keeps it exactly.
    commit_file(&app.workspace, "local.txt", "local\n", "local only");
    app.execute_command(Command::GitCommitAmend);
    app.settle();
    answer(&mut app, "local only");
    assert!(!confirm_overlay(&mut app).contains("already on"));
    assert_eq!(
        git_in(&app.workspace, &["log", "--format=%s", "-n", "1"]),
        "local only"
    );
}

#[test]
fn rebase_stops_on_conflicts_and_continues_or_aborts() {
    let (_home, mut app) = repository();
    git_in(&app.workspace, &["checkout", "-q", "-b", "topic"]);
    commit_file(&app.workspace, "file.txt", "topic\n", "topic change");
    let topic = git_in(&app.workspace, &["rev-parse", "HEAD"]);
    git_in(&app.workspace, &["checkout", "-q", "main"]);
    commit_file(&app.workspace, "file.txt", "main\n", "main change");
    git_in(&app.workspace, &["checkout", "-q", "topic"]);
    refreshed(&mut app);

    app.execute_command(Command::GitContinueRebase);
    app.settle();
    assert!(
        app.status.contains("no rebase in progress"),
        "{}",
        app.status
    );
    app.execute_command(Command::GitAbortRebase);
    app.settle();
    assert!(app.overlay.is_none());
    assert!(
        app.status.contains("no rebase in progress"),
        "{}",
        app.status
    );
    app.execute_command(Command::GitConflicts);
    assert!(app.status.contains("no conflicted files"), "{}", app.status);

    let rebase = |app: &mut App| {
        app.execute_command(Command::GitRebase);
        app.settle();
        let items = &app.overlay.as_ref().unwrap().base().items;
        assert!(
            !items.iter().any(|item| item.label == "topic"),
            "the current branch is not offered"
        );
        choose(app, "main");
        assert!(confirm_overlay(app).contains("onto main"));
    };
    rebase(&mut app);
    assert!(
        app.status
            .contains("Rebase stopped with conflicts in 1 file(s)"),
        "{}",
        app.status
    );
    app.execute_command(Command::GitConflicts);
    choose(&mut app, "file.txt");
    assert_eq!(
        app.workbench.active_editor().unwrap().path(),
        Some(app.workspace.join("file.txt").as_path())
    );
    app.execute_command(Command::GitAbortRebase);
    app.settle();
    confirm_overlay(&mut app);
    assert_eq!(git_in(&app.workspace, &["rev-parse", "HEAD"]), topic);
    assert_eq!(
        crate::controllers::git::operation_in_progress(
            &app.workspace,
            &std::sync::atomic::AtomicBool::new(false)
        ),
        None
    );

    rebase(&mut app);
    std::fs::write(app.workspace.join("file.txt"), "resolved\n").unwrap();
    git_in(&app.workspace, &["add", "file.txt"]);
    app.execute_command(Command::GitContinueRebase);
    app.settle();
    assert_eq!(app.status, "Rebase complete");
    assert_eq!(
        git_in(&app.workspace, &["log", "--format=%s", "-n", "2"]),
        "topic change\nmain change"
    );
}

#[test]
fn cherry_pick_revert_and_reset_pick_commits_from_a_log() {
    let (_home, mut app) = repository();
    let base = git_in(&app.workspace, &["rev-parse", "HEAD"]);
    git_in(&app.workspace, &["checkout", "-q", "-b", "other"]);
    commit_file(&app.workspace, "picked.txt", "picked\n", "add picked");
    commit_file(&app.workspace, "file.txt", "other\n", "conflicting change");
    git_in(&app.workspace, &["checkout", "-q", "main"]);
    refreshed(&mut app);

    app.execute_command(Command::GitCherryPick);
    app.settle();
    choose(&mut app, "add picked");
    assert!(app.workspace.join("picked.txt").exists());
    assert_eq!(
        git_in(&app.workspace, &["log", "--format=%s", "-n", "1"]),
        "add picked"
    );

    // Soft keeps the undone work staged.
    app.execute_command(Command::GitReset);
    app.settle();
    choose(&mut app, "base");
    choose(&mut app, "Soft");
    assert_eq!(git_in(&app.workspace, &["rev-parse", "HEAD"]), base);
    assert_eq!(
        git_in(&app.workspace, &["status", "--porcelain"]),
        "A  picked.txt"
    );
    git_in(&app.workspace, &["commit", "-q", "-m", "picked again"]);

    app.execute_command(Command::GitRevert);
    app.settle();
    choose(&mut app, "picked again");
    assert!(!app.workspace.join("picked.txt").exists());
    assert!(git_in(&app.workspace, &["log", "--format=%s", "-n", "1"]).starts_with("Revert"));

    // Hard asks first and discards uncommitted work too.
    std::fs::write(app.workspace.join("file.txt"), "uncommitted\n").unwrap();
    app.execute_command(Command::GitReset);
    app.settle();
    choose(&mut app, "base");
    choose(&mut app, "Hard");
    assert!(confirm_overlay(&mut app).contains("cannot be undone"));
    assert_eq!(git_in(&app.workspace, &["status", "--porcelain"]), "");
    assert!(!app.workspace.join("picked.txt").exists());

    // A conflicting pick stays conflicted and says how to finish.
    std::fs::write(app.workspace.join("file.txt"), "main edit\n").unwrap();
    git_in(&app.workspace, &["commit", "-q", "-am", "main edit"]);
    app.execute_command(Command::GitCherryPick);
    app.settle();
    choose(&mut app, "conflicting change");
    assert!(
        app.status.contains("Cherry-pick stopped with conflicts"),
        "{}",
        app.status
    );
    app.execute_command(Command::GitContinueRebase);
    app.settle();
    assert!(app.status.contains("a cherry-pick is"), "{}", app.status);
}

#[test]
fn create_tag_is_annotated_with_a_message_and_lightweight_without() {
    let (_home, mut app) = repository();
    refreshed(&mut app);
    app.execute_command(Command::GitCreateTag);
    answer(&mut app, "bad..name");
    assert_eq!(app.status, "Invalid tag name");
    app.execute_command(Command::GitCreateTag);
    answer(&mut app, "v1.0");
    answer(&mut app, "First release");
    assert_eq!(git_in(&app.workspace, &["cat-file", "-t", "v1.0"]), "tag");
    assert_eq!(app.status, "Created annotated tag v1.0");
    app.execute_command(Command::GitCreateTag);
    answer(&mut app, "light");
    answer(&mut app, "");
    assert_eq!(
        git_in(&app.workspace, &["cat-file", "-t", "light"]),
        "commit"
    );
}

#[test]
fn fetch_all_and_pull_rebase_bring_in_remote_commits_linearly() {
    let (home, mut app) = repository();
    let remote = home.path().join("remote.git");
    git_in(
        home.path(),
        &["init", "-q", "--bare", remote.to_str().unwrap()],
    );
    git_in(
        &app.workspace,
        &["remote", "add", "origin", remote.to_str().unwrap()],
    );
    git_in(&app.workspace, &["push", "-q", "-u", "origin", "main"]);
    let clone = home.path().join("clone");
    git_in(
        home.path(),
        &[
            "clone",
            "-q",
            remote.to_str().unwrap(),
            clone.to_str().unwrap(),
        ],
    );
    git_in(&clone, &["config", "user.email", "other@example.test"]);
    git_in(&clone, &["config", "user.name", "Other"]);
    commit_file(&clone, "theirs.txt", "theirs\n", "their change");
    git_in(&clone, &["push", "-q", "origin", "main"]);
    let theirs = git_in(&clone, &["rev-parse", "HEAD"]);
    commit_file(&app.workspace, "mine.txt", "mine\n", "my change");
    refreshed(&mut app);

    app.execute_command(Command::GitFetchAll);
    app.settle();
    assert_eq!(
        git_in(&app.workspace, &["rev-parse", "origin/main"]),
        theirs
    );
    app.execute_command(Command::GitPullRebase);
    app.settle();
    assert_eq!(git_in(&app.workspace, &["rev-parse", "HEAD~1"]), theirs);
    assert_eq!(
        git_in(&app.workspace, &["rev-list", "--merges", "--count", "HEAD"]),
        "0"
    );
    assert!(app.workspace.join("theirs.txt").exists());
}

#[test]
fn continue_and_abort_finish_stopped_cherry_picks_and_reverts() {
    let (_home, mut app) = repository();
    refreshed(&mut app);
    for command in [Command::GitContinue, Command::GitAbort] {
        app.execute_command(command);
        app.settle();
        assert!(app.overlay.is_none());
        assert!(app.status.contains("is in progress"), "{}", app.status);
    }
    let in_progress = |app: &App| {
        crate::controllers::git::operation_in_progress(
            &app.workspace,
            &std::sync::atomic::AtomicBool::new(false),
        )
    };
    git_in(&app.workspace, &["checkout", "-q", "-b", "other"]);
    commit_file(&app.workspace, "file.txt", "theirs\n", "their change");
    git_in(&app.workspace, &["checkout", "-q", "main"]);
    commit_file(&app.workspace, "file.txt", "ours\n", "our change");
    let ours = git_in(&app.workspace, &["rev-parse", "HEAD"]);
    let pick = |app: &mut App| {
        app.execute_command(Command::GitCherryPick);
        app.settle();
        choose(app, "their change");
        assert!(
            app.status.contains("Cherry-pick stopped with conflicts")
                && app
                    .status
                    .contains("Continue Rebase, Cherry-Pick or Revert"),
            "{}",
            app.status
        );
    };

    // Abort a stopped cherry-pick.
    pick(&mut app);
    app.execute_command(Command::GitAbort);
    app.settle();
    assert!(confirm_overlay(&mut app).contains("cherry-pick"));
    assert_eq!(in_progress(&app), None);
    assert_eq!(git_in(&app.workspace, &["rev-parse", "HEAD"]), ours);

    // Continue one after resolving it; Git never waits for an editor.
    pick(&mut app);
    std::fs::write(app.workspace.join("file.txt"), "both\n").unwrap();
    git_in(&app.workspace, &["add", "file.txt"]);
    app.execute_command(Command::GitContinue);
    app.settle();
    assert_eq!(app.status, "Cherry-pick complete");
    assert_eq!(
        git_in(&app.workspace, &["log", "--format=%s", "-n", "1"]),
        "their change"
    );

    // Reverting "our change" now conflicts with the resolved content.
    let revert = |app: &mut App| {
        app.execute_command(Command::GitRevert);
        app.settle();
        choose(app, "our change");
        assert!(
            app.status.contains("Revert stopped with conflicts"),
            "{}",
            app.status
        );
    };
    let head = git_in(&app.workspace, &["rev-parse", "HEAD"]);
    revert(&mut app);
    app.execute_command(Command::GitContinueRebase);
    app.settle();
    assert!(app.status.contains("a revert is"), "{}", app.status);
    app.execute_command(Command::GitAbort);
    app.settle();
    assert!(confirm_overlay(&mut app).contains("revert"));
    assert_eq!(in_progress(&app), None);
    assert_eq!(git_in(&app.workspace, &["rev-parse", "HEAD"]), head);

    revert(&mut app);
    std::fs::write(app.workspace.join("file.txt"), "base\n").unwrap();
    git_in(&app.workspace, &["add", "file.txt"]);
    app.execute_command(Command::GitContinue);
    app.settle();
    assert_eq!(app.status, "Revert complete");
    assert!(git_in(&app.workspace, &["log", "--format=%s", "-n", "1"]).starts_with("Revert"));
    assert_eq!(in_progress(&app), None);
}

#[test]
fn stage_and_unstage_act_on_the_active_file() {
    let (_home, mut app) = test_app();
    let root = app.workspace.clone();
    git_in(&root, &["init", "-q"]);
    git_in(&root, &["config", "user.email", "tide@example.test"]);
    git_in(&root, &["config", "user.name", "Tide Test"]);
    std::fs::write(root.join("a.txt"), "one\n").unwrap();
    std::fs::write(root.join("b.txt"), "one\n").unwrap();
    git_in(&root, &["add", "."]);
    git_in(&root, &["commit", "-q", "-m", "initial"]);
    std::fs::write(root.join("a.txt"), "two\n").unwrap();
    std::fs::write(root.join("b.txt"), "two\n").unwrap();
    app.open_file(root.join("a.txt"));
    with_ui!(app, |ui| app.git.refresh(ui));
    app.settle();

    app.execute_command(Command::GitStage);
    app.settle();
    assert_eq!(
        git_in(&root, &["diff", "--cached", "--name-only"]).trim(),
        "a.txt"
    );

    app.execute_command(Command::GitStage);
    app.settle();
    assert_eq!(app.status, "Git: a.txt has no unstaged changes");

    app.execute_command(Command::GitUnstage);
    app.settle();
    assert_eq!(
        git_in(&root, &["diff", "--cached", "--name-only"]).trim(),
        ""
    );
    assert_eq!(
        "git.checkout".parse::<Command>().unwrap(),
        Command::GitSwitchBranch
    );
    assert_eq!(
        "git.status".parse::<Command>().unwrap(),
        Command::GitFocusPanel
    );
}
