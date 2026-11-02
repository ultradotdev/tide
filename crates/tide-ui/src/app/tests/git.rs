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
    app.execute_command(Command::GitOpenFileDiff);
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
        Command::GitStage,
        Command::GitUnstage,
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
