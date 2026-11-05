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
        initial_folder: None,
        session_name: None,
        restore_session: false,
        ephemeral_session: false,
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
        initial_folder: None,
        session_name: None,
        restore_session: false,
        ephemeral_session: false,
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
    let binding = app
        .keybindings
        .iter()
        .find(|binding| binding.command == "workbench.commandPalette")
        .cloned()
        .unwrap();
    let command = binding.command.clone();
    app.update_keybinding(&binding, "ctrl+alt+k");
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

fn app_with_config(settings: Option<&str>, keybindings: Option<&str>) -> (tempfile::TempDir, App) {
    let home = tempfile::tempdir().unwrap();
    let workspace = home.path().join("workspace");
    std::fs::create_dir_all(&workspace).unwrap();
    let paths = Paths::from_env(home.path(), |_| None);
    std::fs::create_dir_all(&paths.config_dir).unwrap();
    if let Some(text) = settings {
        std::fs::write(&paths.settings_file, text).unwrap();
    }
    if let Some(text) = keybindings {
        std::fs::write(&paths.keybindings_file, text).unwrap();
    }
    let app = App::new(AppConfig {
        workspace,
        paths: Some(paths),
        initial_file: None,
        initial_folder: None,
        session_name: None,
        restore_session: false,
        ephemeral_session: false,
        jobs: Jobs::manual(),
        agent_discovery: no_agents,
        install: crate::help::Install::Unknown { executable: None },
    })
    .unwrap();
    (home, app)
}

#[test]
fn changing_a_setting_or_key_keeps_the_users_file_as_written() {
    let settings = "{\n  // Wider indents for this machine\n  \"editor.tabSize\": 3,\n}\n";
    let keys = "[\n  // My palette key\n  { \"key\": \"f5\", \"command\": \"workbench.commandPalette\" }\n]\n";
    let (_home, mut app) = app_with_config(Some(settings), Some(keys));
    assert!(app.overlay.is_none(), "valid files raise no problems");

    app.update_setting("editor.tabSize", "4");
    app.update_setting("editor.wordWrap", "\"off\"");
    let written = std::fs::read_to_string(&app.paths.settings_file).unwrap();
    assert!(
        written.contains("// Wider indents for this machine"),
        "{written}"
    );
    let values: serde_json::Value = tide_config::parse_jsonc(&written).unwrap();
    assert_eq!(
        values,
        serde_json::json!({"editor.tabSize": 4, "editor.wordWrap": "off"}),
        "only the user's own values are written, not every default"
    );

    let theme = app
        .themes
        .iter()
        .map(|theme| theme.id.clone())
        .find(|id| id != app.themes.selected_id())
        .unwrap();
    app.select_theme(&theme);
    let written = std::fs::read_to_string(&app.paths.settings_file).unwrap();
    assert!(written.contains("// Wider indents"), "{written}");
    assert_eq!(
        tide_config::parse_jsonc::<serde_json::Value>(&written).unwrap()["workbench.colorTheme"],
        serde_json::json!(theme)
    );

    // F5 is also Refresh in query results; only the palette binding moves.
    let palette = Keybinding {
        key: "f5".into(),
        command: "workbench.commandPalette".into(),
        when: None,
        args: None,
    };
    app.update_keybinding(&palette, "ctrl+alt+f11");
    let written = std::fs::read_to_string(&app.paths.keybindings_file).unwrap();
    assert!(written.contains("// My palette key"), "{written}");
    let bindings: Vec<Keybinding> = tide_config::parse_jsonc(&written).unwrap();
    assert_eq!(
        bindings.len(),
        3,
        "the change is appended to the user's own list"
    );
    assert_eq!(
        app.resolve_binding("ctrl+alt+f11"),
        Some(BindingAction::Run(Command::WorkbenchCommandPalette))
    );
}

#[test]
fn defaults_saved_by_older_releases_load_silently_as_todays_defaults() {
    // Tide 0.10.0 wrote every default into settings.jsonc, including this one,
    // whose values have since changed.
    let settings = "{\n  \"workbench.startupEditor\": \"\",\n  \"editor.tabSize\": 3\n}\n";
    let (_home, app) = app_with_config(Some(settings), None);
    assert!(app.overlay.is_none(), "an old default is not a problem");
    assert_eq!(
        app.settings.get("workbench.startupEditor"),
        crate::catalog::settings().get("workbench.startupEditor")
    );
    assert_eq!(
        app.settings.get("editor.tabSize"),
        Some(&serde_json::json!(3))
    );

    let (_home, app) = app_with_config(Some("{ \"workbench.startupEditor\": \"bogus\" }"), None);
    let overlay = app
        .overlay
        .as_ref()
        .expect("other invalid values are reported");
    let message = overlay.base().message.clone().unwrap_or_default();
    assert!(message.contains("workbench.startupEditor"), "{message}");
}

#[test]
fn a_broken_settings_file_is_reported_at_startup_and_never_overwritten() {
    let broken = "{\n  // half-finished edit\n  \"editor.tabSize\": \n}\n";
    let (_home, mut app) = app_with_config(Some(broken), Some("[ oops"));
    let overlay = app.overlay.as_ref().expect("a problems dialog");
    let message = overlay.base().message.clone().unwrap_or_default();
    assert!(message.contains("settings.jsonc"), "{message}");
    assert!(message.contains("keybindings.jsonc"), "{message}");
    assert!(message.contains("default settings"), "{message}");
    app.overlay = None;

    app.update_setting("editor.tabSize", "8");
    assert!(app.status.contains("left unchanged"), "{}", app.status);
    assert_eq!(
        std::fs::read_to_string(&app.paths.settings_file).unwrap(),
        broken
    );
}

#[test]
fn an_invalid_setting_value_is_reported_at_startup() {
    let (_home, app) = app_with_config(Some(r#"{ "editor.tabSize": 0 }"#), None);
    let message = app
        .overlay
        .as_ref()
        .and_then(|overlay| overlay.base().message.clone())
        .unwrap_or_default();
    assert!(message.contains("editor.tabSize"), "{message}");
    assert_eq!(app.tab_size(), 2);
}

fn write_config(path: &std::path::Path, text: &str) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, text).unwrap();
}

#[test]
fn settings_edited_outside_tide_apply_without_a_restart() {
    let (_home, mut app) = test_app();
    assert!(!app.reload_config_if_changed(), "nothing changed yet");
    let other_theme = app
        .themes
        .iter()
        .map(|theme| theme.id.clone())
        .find(|id| id != app.themes.selected_id())
        .unwrap();
    write_config(
        &app.paths.settings_file,
        &format!(
            "{{\n  // edited by hand\n  \"editor.tabSize\": 6,\n  \"tui.sidebar.width\": 50,\n  \"workbench.colorTheme\": \"{other_theme}\"\n}}\n"
        ),
    );
    assert!(app.reload_config_if_changed());
    assert_eq!(app.tab_size(), 6);
    assert_eq!(app.sidebar_width, 50);
    assert_eq!(app.themes.selected_id(), other_theme);
    assert!(
        app.status.starts_with("Settings reloaded"),
        "{}",
        app.status
    );
    assert!(app.overlay.is_none());
    assert!(
        !app.reload_config_if_changed(),
        "an unchanged file is read once"
    );

    write_config(
        &app.paths.settings_file,
        "{ \"terminal.shell\": \"/bin/zsh\", \"session.restoreOnStartup\": false }",
    );
    assert!(app.reload_config_if_changed());
    assert!(
        app.status
            .contains("terminal.shell applies to new terminals")
            && app
                .status
                .contains("session.restoreOnStartup takes effect after restart"),
        "{}",
        app.status
    );
    assert_eq!(
        app.tab_size(),
        2,
        "removed settings return to their defaults"
    );
}

#[test]
fn a_broken_settings_file_is_reported_and_the_last_good_settings_stay() {
    let (_home, mut app) = test_app();
    write_config(&app.paths.settings_file, "{ \"editor.tabSize\": 5 }");
    assert!(app.reload_config_if_changed());
    write_config(&app.paths.settings_file, "{ \"editor.tabSize\": 7, oops }");
    assert!(app.reload_config_if_changed());
    assert_eq!(app.tab_size(), 5);
    let overlay = app.overlay.as_ref().unwrap();
    assert_eq!(overlay.base().title, "Configuration Problems");
    assert!(
        overlay
            .base()
            .message
            .as_deref()
            .unwrap()
            .contains("Tide is using the settings it had"),
        "{:?}",
        overlay.base().message
    );
    write_config(&app.paths.settings_file, "{ \"editor.tabSize\": 7 }");
    assert!(app.reload_config_if_changed());
    assert_eq!(app.tab_size(), 7);
    assert!(
        app.overlay.is_none(),
        "the problem report closes once fixed"
    );

    write_config(
        &app.paths.settings_file,
        "{ \"workbench.colorTheme\": \"no-such-theme\" }",
    );
    let theme = app.themes.selected_id().to_owned();
    assert!(app.reload_config_if_changed());
    assert_eq!(app.themes.selected_id(), theme);
    assert!(
        app.overlay
            .as_ref()
            .and_then(|overlay| overlay.base().message.clone())
            .unwrap()
            .contains("no-such-theme")
    );
}

#[test]
fn keybindings_edited_outside_tide_apply_and_tides_own_writes_are_not_reported() {
    let (_home, mut app) = test_app();
    write_config(
        &app.paths.keybindings_file,
        r#"[ { "key": "ctrl+alt+y", "command": "workbench.quickOpen" } ]"#,
    );
    assert!(app.reload_config_if_changed());
    assert_eq!(
        app.resolve_binding("ctrl+alt+y"),
        Some(BindingAction::Run(Command::WorkbenchQuickOpen))
    );
    assert!(
        app.status.contains("Keybindings reloaded"),
        "{}",
        app.status
    );

    write_config(&app.paths.keybindings_file, "[ { \"key\": ");
    assert!(app.reload_config_if_changed());
    assert_eq!(
        app.resolve_binding("ctrl+alt+y"),
        Some(BindingAction::Run(Command::WorkbenchQuickOpen)),
        "a broken file keeps the keys Tide had"
    );
    app.overlay = None;
    write_config(&app.paths.keybindings_file, "[]");
    assert!(app.reload_config_if_changed());
    assert_ne!(
        app.resolve_binding("ctrl+alt+y"),
        Some(BindingAction::Run(Command::WorkbenchQuickOpen))
    );

    app.status.clear();
    app.update_setting("editor.tabSize", "3");
    let binding = app
        .keybindings
        .iter()
        .find(|binding| binding.key == "ctrl+shift+j")
        .cloned()
        .unwrap();
    app.update_keybinding(&binding, "ctrl+alt+j");
    app.status.clear();
    assert!(
        !app.reload_config_if_changed(),
        "Tide's own writes are already applied"
    );
    assert!(app.status.is_empty());
}

/// Opens the Settings list, types `filter` and presses Enter on `key`.
fn edit_from_settings(app: &mut App, filter: &str, key: &str) {
    app.open_settings();
    let base = app.overlay.as_mut().unwrap().base_mut();
    base.input = filter.to_owned();
    base.cursor = filter.chars().count();
    base.refilter();
    base.selected = base
        .filtered
        .iter()
        .position(|index| base.items[*index].value == key)
        .unwrap();
    app.handle_modal_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
}

fn selected_setting(app: &App) -> (String, String, String) {
    let base = app.overlay.as_ref().unwrap().base();
    assert_eq!(base.kind, ModalKind::Settings);
    let item = base.selected_item().unwrap();
    (base.input.clone(), item.value.clone(), item.detail.clone())
}

#[test]
fn enter_flips_an_on_off_setting_and_keeps_the_list_where_it_was() {
    let (_home, mut app) = test_app();
    let before = app
        .settings
        .get("editor.minimap.enabled")
        .unwrap()
        .as_bool()
        .unwrap();
    edit_from_settings(&mut app, "minimap", "editor.minimap.enabled");
    assert_eq!(
        app.settings.get("editor.minimap.enabled"),
        Some(&serde_json::json!(!before))
    );
    let saved = std::fs::read_to_string(&app.paths.settings_file).unwrap();
    assert!(
        saved.contains(&format!("\"editor.minimap.enabled\": {}", !before)),
        "{saved}"
    );
    assert_eq!(
        selected_setting(&app),
        (
            "minimap".to_owned(),
            "editor.minimap.enabled".to_owned(),
            (!before).to_string()
        )
    );
}

#[test]
fn a_setting_with_fixed_values_is_picked_from_a_list() {
    let (_home, mut app) = test_app();
    edit_from_settings(&mut app, "icon", "tui.iconStyle");
    let base = app.overlay.as_ref().unwrap().base();
    assert_eq!(
        base.kind,
        ModalKind::SettingChoice {
            key: "tui.iconStyle".to_owned(),
            filter: "icon".to_owned()
        }
    );
    let choices = base
        .items
        .iter()
        .map(|item| (item.label.as_str(), item.detail.as_str(), item.current))
        .collect::<Vec<_>>();
    assert_eq!(
        choices,
        [
            ("auto", "default", true),
            ("nerd", "", false),
            ("ascii", "", false)
        ]
    );
    assert_eq!(base.selected_item().unwrap().label, "auto");

    // Esc goes back to the list without changing anything.
    app.handle_modal_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
    assert_eq!(selected_setting(&app).1, "tui.iconStyle");
    assert_eq!(
        app.settings.get("tui.iconStyle"),
        Some(&serde_json::json!("auto"))
    );

    app.overlay = None;
    edit_from_settings(&mut app, "icon", "tui.iconStyle");
    app.handle_modal_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
    app.handle_modal_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    assert_eq!(
        app.settings.get("tui.iconStyle"),
        Some(&serde_json::json!("nerd"))
    );
    assert_eq!(
        selected_setting(&app),
        (
            "icon".to_owned(),
            "tui.iconStyle".to_owned(),
            "nerd".to_owned()
        )
    );
}

#[test]
fn prompts_name_the_allowed_range_and_start_with_plain_text() {
    let (_home, mut app) = test_app();
    edit_from_settings(&mut app, "", "editor.tabSize");
    let base = app.overlay.as_ref().unwrap().base();
    assert_eq!(base.title, "Edit Setting: editor.tabSize (1–16)");
    app.overlay = None;
    app.update_setting("editor.tabSize", "40");
    assert_eq!(
        app.status,
        "editor.tabSize must be a whole number from 1 to 16"
    );
    app.update_setting("tui.colorMode", "16");
    assert_eq!(
        app.status,
        "tui.colorMode must be one of: auto, truecolor, 256"
    );

    app.update_setting("terminal.shell", "/bin/sh");
    edit_from_settings(&mut app, "shell", "terminal.shell");
    assert_eq!(app.overlay.as_ref().unwrap().input(), "/bin/sh");
}

#[test]
fn the_settings_list_shows_plain_values_and_the_selected_description() {
    let (_home, mut app) = test_app();
    app.open_settings();
    let base = app.overlay.as_mut().unwrap().base_mut();
    let icon = base
        .items
        .iter()
        .find(|item| item.value == "tui.iconStyle")
        .unwrap();
    assert_eq!(icon.detail, "auto");
    base.input = "tui.iconStyle".to_owned();
    base.refilter();
    base.selected = 0;
    let mut terminal = ratatui::Terminal::new(ratatui::backend::TestBackend::new(120, 40)).unwrap();
    terminal.draw(|frame| app.draw(frame)).unwrap();
    let text = terminal
        .backend()
        .buffer()
        .content
        .iter()
        .map(|cell| cell.symbol())
        .collect::<String>();
    assert!(
        text.contains("Icon style:"),
        "the description shows under the list"
    );
}

#[test]
fn the_theme_setting_opens_the_theme_list_at_the_current_theme() {
    let (_home, mut app) = test_app();
    let current = app.themes.selected_id().to_owned();
    edit_from_settings(&mut app, "theme", "workbench.colorTheme");
    let base = app.overlay.as_ref().unwrap().base();
    assert_eq!(base.kind, ModalKind::ThemePicker);
    let item = base.selected_item().unwrap();
    assert!(item.current);
    assert_eq!(item.value, current);
}
