use super::*;

#[test]
fn the_save_session_command_reports_success_and_failure() {
    let (_home, mut app) = test_app();
    app.execute_command(Command::SessionSave);
    assert_eq!(app.status, "Session saved");
    app.session_write_blocked = true;
    app.execute_command(Command::SessionSave);
    assert!(
        app.status.starts_with("Session save failed"),
        "{}",
        app.status
    );
}

#[tokio::test]
async fn a_panicking_event_loop_writes_the_recovery_snapshot_before_unwinding() {
    use futures_util::FutureExt as _;
    let (_home, mut app) = test_app_async();
    app.new_file();
    app.workbench
        .active_editor_mut()
        .unwrap()
        .insert_text("recover me");
    let panic = std::panic::AssertUnwindSafe(crate::run::guard_panics(
        &mut app,
        async |_: &mut App| -> u8 { panic!("event loop failed") },
    ))
    .catch_unwind()
    .await
    .unwrap_err();
    assert_eq!(panic.downcast_ref::<&str>(), Some(&"event loop failed"));
    let restored = app.session_store.load_last().unwrap().unwrap();
    assert_eq!(
        restored.documents[0].unsaved_content.as_deref(),
        Some("recover me")
    );
}

#[test]
fn snapshot_failure_blocks_quit() {
    let (_home, mut app) = test_app();
    app.session_write_blocked = true;
    app.finish_quit();
    assert!(!app.should_quit());
    assert!(app.status.contains("Quit blocked"));
}

#[tokio::test(flavor = "multi_thread")]
async fn recovery_autosave_writes_off_the_ui_thread_and_never_regresses() {
    let (_home, mut app) = test_app_async();
    let mut events = app.take_event_receiver().unwrap();
    app.new_file();
    app.workbench
        .active_editor_mut()
        .unwrap()
        .insert_text("first draft");
    app.session_dirty = true;
    app.last_autosave = Instant::now() - Duration::from_secs(60);
    let saves = app.session_saves.clone();
    let disk = saves.lock().unwrap();
    app.on_tick();
    assert!(app.autosave_running);
    assert!(!app.session_dirty);
    assert!(app.session_store.load_last().unwrap().is_none());
    drop(disk);
    app.workbench
        .active_editor_mut()
        .unwrap()
        .insert_text(" and more");
    app.save_session().unwrap();
    let event = tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            let event = events.recv().await.unwrap();
            if matches!(event, AppEvent::SessionAutosaved(_)) {
                break event;
            }
        }
    })
    .await
    .unwrap();
    app.handle_background(event);
    assert!(!app.autosave_running);
    let saved = app.session_store.load_last().unwrap().unwrap();
    assert!(
        serde_json::to_string(&saved)
            .unwrap()
            .contains("first draft and more")
    );
}

#[test]
fn session_restore_reopens_dirty_documents() {
    let (_home, mut app) = test_app();
    let path = app.workspace.join("session.txt");
    std::fs::write(&path, "original\n").unwrap();
    app.open_file(&path);
    app.workbench
        .active_editor_mut()
        .unwrap()
        .insert_text("draft\n");
    app.save_session().unwrap();
    let paths = app.paths.clone();
    let workspace = app.workspace.clone();
    drop(app);

    let restored = App::new(AppConfig {
        workspace,
        initial_file: None,
        session_name: None,
        restore_session: true,
        ephemeral_session: false,
        paths: Some(paths),
        jobs: Jobs::manual(),
        agent_discovery: no_agents,
        install: crate::help::Install::Unknown { executable: None },
    })
    .unwrap();
    let editor = restored.workbench.active_editor().unwrap();
    assert!(editor.document.text().starts_with("draft"));
}

#[test]
fn no_session_leaves_the_saved_session_alone_until_the_user_saves() {
    let (_home, mut app) = test_app();
    app.new_file();
    app.workbench
        .active_editor_mut()
        .unwrap()
        .insert_text("kept");
    app.save_session().unwrap();
    let saved_path = app.session_store.workspace_session_path();
    let saved = std::fs::read(&saved_path).unwrap();
    let paths = app.paths.clone();
    let workspace = app.workspace.clone();
    drop(app);

    let mut fresh = App::new(AppConfig {
        workspace,
        initial_file: None,
        session_name: None,
        restore_session: false,
        ephemeral_session: true,
        paths: Some(paths),
        jobs: Jobs::manual(),
        agent_discovery: no_agents,
        install: crate::help::Install::Unknown { executable: None },
    })
    .unwrap();
    fresh.new_file();
    fresh
        .workbench
        .active_editor_mut()
        .unwrap()
        .insert_text("scratch");
    fresh.mark_session_dirty();
    fresh.autosave_session().unwrap();
    assert!(!fresh.save_recovery().unwrap());
    fresh.finish_quit();
    assert!(fresh.should_quit);
    assert_eq!(std::fs::read(&saved_path).unwrap(), saved);

    fresh.execute_command(Command::SessionSave);
    assert_eq!(fresh.status, "Session saved");
    assert_ne!(std::fs::read(&saved_path).unwrap(), saved);
}

#[test]
fn blocked_session_writes_do_not_touch_disk() {
    let (_home, mut app) = test_app();
    app.session_write_blocked = true;
    assert!(app.save_session().is_err());
    assert!(!app.session_store.workspace_session_path().exists());
}

#[test]
fn session_layout_round_trips() {
    let mut workbench = Workbench::new();
    let second = workbench.split_focused(SplitDirection::Vertical);
    let layout = session_layout(workbench.root());
    assert_eq!(layout.node_type, "horizontal");
    let mut pane_map = HashMap::new();
    let mut next = 0;
    let converted = convert_session_layout(&layout, &mut pane_map, &mut next);
    assert!(matches!(converted, LayoutNode::Split { .. }));
    assert_eq!(pane_map.len(), 2);
    assert!(pane_map.contains_key("pane-0"));
    assert!(pane_map.contains_key(&format!("pane-{second}")));
}

#[test]
fn editor_and_sql_view_state_round_trip_without_changing_shared_write_guards() {
    let (_home, mut app) = test_app();
    let path = app.workspace.join("views.py");
    std::fs::write(&path, "if ready:\n    one\n    two\nafter\n").unwrap();
    app.open_file(&path);
    let editor = app.workbench.active_editor_mut().unwrap();
    editor.document.set_selection(tide_core::Selection::new(
        Position::new(3, 0),
        Position::new(3, 3),
    ));
    editor.restore_folds(&[0]);
    editor.scroll_left = 2;
    app.with_db(|db, ui| db.open_tab(Some("SELECT α;\nSELECT β;"), ui));
    if let Some(Tab::Sql(sql)) = app.workbench.active_tab_mut() {
        sql.editor.document.set_selection(tide_core::Selection::new(
            Position::new(1, 1),
            Position::new(1, 5),
        ));
        sql.editor.scroll_left = 3;
        sql.view_mode = 2;
    }
    for section in &mut app.accordion.sections {
        section.expanded = false;
    }
    app.settings
        .set("editor.minimap.enabled", serde_json::json!(false));
    let saved = app.build_session();
    assert!(!saved.ui.minimap_enabled);
    app.settings
        .set("editor.minimap.enabled", serde_json::json!(true));
    app.restore_session(saved).unwrap();
    assert!(!app.setting_bool("editor.minimap.enabled", true));
    let tabs = app
        .workbench
        .pane_ids()
        .iter()
        .flat_map(|id| app.workbench.pane(*id).unwrap().tabs.iter())
        .collect::<Vec<_>>();
    let editor = tabs
        .iter()
        .find_map(|tab| {
            if let Tab::Editor(editor) = tab {
                Some(editor)
            } else {
                None
            }
        })
        .unwrap();
    assert_eq!(editor.folded_ranges(), &[(0, 2)]);
    assert_eq!(editor.scroll_left, 2);
    assert_eq!(editor.selected_text().as_deref(), Some("aft"));
    let sql = tabs
        .iter()
        .find_map(|tab| {
            if let Tab::Sql(sql) = tab {
                Some(sql)
            } else {
                None
            }
        })
        .unwrap();
    assert_eq!(sql.editor.scroll_left, 3);
    assert_eq!(sql.editor.selected_text().as_deref(), Some("ELEC"));
    assert_eq!(sql.view_mode, 2);
    assert!(app.accordion.sections.iter().all(|s| !s.expanded));
}
