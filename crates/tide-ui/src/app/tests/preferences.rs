use super::*;

#[test]
fn settings_validate_apply_persist_and_hide_excluded_or_ignored_controls() {
    let (home, mut app) = test_app();
    app.sidebar_width = 42;
    app.update_setting("editor.tabSize", "4");
    assert_eq!(app.sidebar_width, 42);
    let saved = std::fs::read(&app.paths.settings_file).unwrap();
    app.update_setting("editor.tabSize", "0");
    assert_eq!(std::fs::read(&app.paths.settings_file).unwrap(), saved);
    assert_eq!(app.tab_size(), 4);
    app.update_setting("tui.sidebar.location", "right");
    assert!(!app.sidebar_left);
    assert_eq!(app.sidebar_width, 42);
    app.update_setting("tui.sidebar.width", "52");
    assert_eq!(app.sidebar_width, 52);
    app.update_setting("session.databaseQueryHistory", "false");
    assert!(
        app.db
            .history
            .add_entry(
                "fixture",
                "Fixture",
                "select 1",
                1,
                1,
                QueryStatus::Success,
                None
            )
            .unwrap()
            .is_none()
    );
    app.open_settings();
    assert!(
        app.overlay
            .as_ref()
            .unwrap()
            .base()
            .items
            .iter()
            .all(|item| crate::preferences::supported(&item.value))
    );
    app.overlay = None;
    let before = app.themes.selected_id().to_owned();
    app.execute_command(Command::ThemeNext);
    assert_ne!(app.themes.selected_id(), before);
    let kind = app.themes.selected().theme_type.clone();
    app.execute_command(Command::ThemeToggleDarkMode);
    assert_ne!(app.themes.selected().theme_type, kind);
    let chosen = app.themes.selected_id().to_owned();
    let workspace = app.workspace.clone();
    let paths = Paths::from_env(home.path(), |_| None);
    let restored = App::new(AppConfig {
        workspace,
        paths: Some(paths),
        initial_file: None,
        session_name: None,
        restore_session: false,
        jobs: Jobs::manual(),
        agent_discovery: no_agents,
        install: crate::help::Install::Unknown { executable: None },
    })
    .unwrap();
    assert_eq!(restored.themes.selected_id(), chosen);
    assert_eq!(restored.sidebar_width, 52);
    assert!(!restored.sidebar_left);
}

#[test]
fn unknown_user_settings_load_without_controls_and_survive_saves() {
    let home = tempfile::tempdir().unwrap();
    let workspace = home.path().join("workspace");
    std::fs::create_dir_all(&workspace).unwrap();
    let paths = Paths::from_env(home.path(), |_| None);
    std::fs::create_dir_all(paths.settings_file.parent().unwrap()).unwrap();
    std::fs::write(
        &paths.settings_file,
        r#"{ "editor.fontSize": 18, "editor.tabSize": 3 }"#,
    )
    .unwrap();
    let mut app = App::new(AppConfig {
        workspace,
        paths: Some(paths),
        initial_file: None,
        session_name: None,
        restore_session: false,
        jobs: Jobs::manual(),
        agent_discovery: no_agents,
        install: crate::help::Install::Unknown { executable: None },
    })
    .unwrap();
    assert_eq!(app.tab_size(), 3);
    app.open_settings();
    assert!(
        !app.overlay
            .as_ref()
            .unwrap()
            .base()
            .items
            .iter()
            .any(|item| item.value == "editor.fontSize")
    );
    app.overlay = None;
    app.update_setting("editor.tabSize", "4");
    let persisted: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&app.paths.settings_file).unwrap()).unwrap();
    assert_eq!(persisted["editor.fontSize"], 18);
    assert_eq!(persisted["editor.tabSize"], 4);
}

#[test]
fn edited_settings_apply_and_persist() {
    let (_home, mut app) = test_app();
    app.update_setting("editor.tabSize", "4");
    assert_eq!(
        app.settings
            .get("editor.tabSize")
            .and_then(serde_json::Value::as_u64),
        Some(4)
    );
    let persisted = std::fs::read_to_string(&app.paths.settings_file).unwrap();
    assert!(persisted.contains("editor.tabSize"));
}

#[test]
fn edited_keybindings_apply_and_persist() {
    let (_home, mut app) = test_app();
    let (old_key, command) = app
        .keybindings
        .iter()
        .find(|binding| binding.command == "workbench.commandPalette")
        .map(|binding| (binding.key.clone(), binding.command.clone()))
        .unwrap();
    app.update_keybinding(&old_key, "ctrl+alt+k");
    assert_eq!(
        app.resolve_binding("ctrl+alt+k"),
        Some(BindingAction::Run(command.parse().unwrap()))
    );
    let persisted = std::fs::read_to_string(&app.paths.keybindings_file).unwrap();
    assert!(persisted.contains("ctrl+alt+k"));
}

#[test]
fn an_unset_minimap_setting_follows_the_catalog_default() {
    let (home, mut app) = test_app();
    let mut values = app.settings.as_map().clone();
    values.remove("editor.minimap.enabled");
    app.settings = Settings::new(values);
    let file = home.path().join("workspace/minimap.txt");
    std::fs::write(&file, "minimap\n".repeat(50)).unwrap();
    app.open_file(&file);
    let mut terminal = ratatui::Terminal::new(ratatui::backend::TestBackend::new(120, 40)).unwrap();
    terminal.draw(|frame| app.draw(frame)).unwrap();
    let default = crate::catalog::settings()
        .get("editor.minimap.enabled")
        .and_then(serde_json::Value::as_bool);
    assert_eq!(
        Some(app.workbench.active_editor().unwrap().minimap),
        default
    );
    assert_eq!(Some(app.build_session().ui.minimap_enabled), default);
}
