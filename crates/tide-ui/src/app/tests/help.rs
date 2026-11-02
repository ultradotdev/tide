use super::*;

/// Starts an app on the profile under `home`, as a launch of the binary would.
fn launch(home: &Path, initial_file: Option<PathBuf>, restore_session: bool) -> App {
    let workspace = home.join("workspace");
    std::fs::create_dir_all(&workspace).unwrap();
    App::new(AppConfig {
        workspace,
        initial_file,
        session_name: None,
        restore_session,
        ephemeral_session: false,
        paths: Some(Paths::from_env(home, |_| None)),
        jobs: Jobs::manual(),
        agent_discovery: no_agents,
        install: crate::help::Install::Managed { local: false },
    })
    .unwrap()
}

fn startup_editor(home: &Path, value: &str) {
    let paths = Paths::from_env(home, |_| None);
    std::fs::create_dir_all(&paths.config_dir).unwrap();
    std::fs::write(
        &paths.settings_file,
        serde_json::json!({ "workbench.startupEditor": value }).to_string(),
    )
    .unwrap();
}

fn tab_titles(app: &App) -> Vec<String> {
    app.workbench
        .focused_pane()
        .tabs
        .iter()
        .map(Tab::title)
        .collect()
}

fn active_help(app: &App) -> Option<&'static str> {
    app.workbench
        .active_editor()
        .and_then(|editor| editor.help)
        .map(|page| page.id)
}

#[test]
fn welcome_opens_on_the_first_launch_only() {
    let home = tempfile::tempdir().unwrap();
    let marker = crate::help::welcome_marker(&Paths::from_env(home.path(), |_| None));
    let first = launch(home.path(), None, false);
    assert_eq!(active_help(&first), Some("welcome"));
    assert_eq!(tab_titles(&first), ["Welcome"]);
    let text = first.workbench.active_editor().unwrap().document.text();
    assert!(text.starts_with("# Welcome to Tide"));
    assert!(text.contains("| Open the command palette | `Ctrl+K` or `Ctrl+Shift+P` |"));
    assert!(marker.is_file());
    drop(first);

    let second = launch(home.path(), None, false);
    assert!(matches!(second.workbench.active_tab(), Some(Tab::Welcome)));
    assert_eq!(active_help(&second), None);
}

#[test]
fn a_first_launch_with_a_file_points_to_welcome_and_keeps_it_for_later() {
    let home = tempfile::tempdir().unwrap();
    let file = home.path().join("workspace/notes.txt");
    std::fs::create_dir_all(file.parent().unwrap()).unwrap();
    std::fs::write(&file, "notes\n").unwrap();
    let app = launch(home.path(), Some(file), false);
    assert_eq!(tab_titles(&app), ["notes.txt"]);
    assert_eq!(
        app.status,
        "New to Tide? Run “Help: Welcome” from the command palette"
    );
    assert!(!crate::help::welcome_marker(&app.paths).exists());
    drop(app);
    assert_eq!(
        active_help(&launch(home.path(), None, false)),
        Some("welcome")
    );
}

#[test]
fn startup_editor_welcome_opens_it_on_every_empty_start() {
    let home = tempfile::tempdir().unwrap();
    startup_editor(home.path(), "welcome");
    crate::help::record_welcome(&Paths::from_env(home.path(), |_| None));
    assert_eq!(
        active_help(&launch(home.path(), None, false)),
        Some("welcome")
    );
    assert_eq!(
        active_help(&launch(home.path(), None, false)),
        Some("welcome")
    );
    let file = home.path().join("workspace/a.txt");
    std::fs::write(&file, "a\n").unwrap();
    assert_eq!(
        tab_titles(&launch(home.path(), Some(file), false)),
        ["a.txt"]
    );
}

#[test]
fn startup_editor_none_never_opens_welcome_or_records_it() {
    let home = tempfile::tempdir().unwrap();
    startup_editor(home.path(), "none");
    let app = launch(home.path(), None, false);
    assert!(matches!(app.workbench.active_tab(), Some(Tab::Welcome)));
    assert!(!crate::help::welcome_marker(&app.paths).exists());
}

#[test]
fn an_unwritable_state_directory_still_starts_with_welcome() {
    let home = tempfile::tempdir().unwrap();
    let paths = Paths::from_env(home.path(), |_| None);
    std::fs::create_dir_all(paths.state_dir.parent().unwrap()).unwrap();
    std::fs::write(&paths.state_dir, "not a directory").unwrap();
    let app = launch(home.path(), None, false);
    assert_eq!(active_help(&app), Some("welcome"));
    assert!(!crate::help::welcome_marker(&paths).exists());
}

#[test]
fn help_pages_refuse_edits_and_saves() {
    let (home, mut app) = test_app();
    app.execute_command(Command::HelpGit);
    assert_eq!(active_help(&app), Some("git"));
    assert_eq!(app.workbench.active_tab().unwrap().title(), "Help: Git");
    let before = app.workbench.active_editor().unwrap().document.text();
    for code in [KeyCode::Char('x'), KeyCode::Enter, KeyCode::Backspace] {
        app.handle_event(Event::Key(KeyEvent::new(code, KeyModifiers::NONE)));
    }
    app.handle_paste("pasted");
    let editor = app.workbench.active_editor().unwrap();
    assert_eq!(editor.document.text(), before);
    assert!(!editor.is_dirty());
    assert_eq!(app.status, "Help: Git is read-only");
    for command in [Command::FileSave, Command::FileSaveAs, Command::FileSaveAll] {
        app.execute_command(command);
        assert!(app.overlay.is_none(), "{command}");
    }
    assert!(app.status.starts_with("Saved 0 file(s)"));
    app.execute_command(Command::FileSave);
    assert!(app.status.contains("Help: Git is read-only"));
    assert!(
        app.workbench
            .active_editor_mut()
            .unwrap()
            .save_as(home.path().join("copy.md"))
            .is_err()
    );
    assert!(!home.path().join("copy.md").exists());
    app.execute_command(Command::FileClose);
    assert!(app.overlay.is_none(), "a help page closes without a prompt");
    assert_eq!(active_help(&app), None);
}

#[test]
fn reopening_a_help_page_renders_it_again_in_place() {
    let (_home, mut app) = test_app();
    app.execute_command(Command::HelpKeyboardShortcuts);
    let tabs = app.workbench.focused_pane().tabs.len();
    assert!(
        !app.workbench
            .active_editor()
            .unwrap()
            .document
            .text()
            .contains("`Ctrl+Alt+J`")
    );
    let binding = app
        .keybindings
        .iter()
        .find(|binding| binding.key == "ctrl+shift+j")
        .cloned()
        .unwrap();
    app.update_keybinding(&binding, "ctrl+alt+j");
    app.execute_command(Command::EditorPreviousTab);
    app.execute_command(Command::HelpKeyboardShortcuts);
    assert_eq!(app.workbench.focused_pane().tabs.len(), tabs);
    assert_eq!(active_help(&app), Some("keyboard-shortcuts"));
    let text = app.workbench.active_editor().unwrap().document.text();
    assert!(text.contains("`Ctrl+Alt+J`"), "{text}");
    assert!(
        app.workbench
            .active_editor()
            .unwrap()
            .document
            .with(Document::is_read_only)
    );
}

#[test]
fn every_help_command_opens_its_page() {
    let (_home, mut app) = test_app();
    for page in crate::help::PAGES {
        app.execute_command(page.command);
        assert_eq!(active_help(&app), Some(page.id));
        assert_eq!(app.workbench.active_tab().unwrap().title(), page.title);
        assert!(
            !app.workbench
                .active_editor()
                .unwrap()
                .document
                .text()
                .contains("{{")
        );
    }
}

#[test]
fn help_tabs_are_saved_as_pages_and_rendered_again_on_restore() {
    let home = tempfile::tempdir().unwrap();
    let mut app = launch(home.path(), None, true);
    app.execute_command(Command::HelpSettings);
    let session = app.build_session();
    let saved = session
        .documents
        .iter()
        .find(|document| document.extra.get("rustHelp") == Some(&serde_json::json!("settings")))
        .unwrap();
    assert!(saved.file_path.is_empty());
    assert!(saved.unsaved_content.is_none());
    app.save_session().unwrap();
    drop(app);

    std::fs::create_dir_all(Paths::from_env(home.path(), |_| None).config_dir).unwrap();
    std::fs::write(
        Paths::from_env(home.path(), |_| None).settings_file,
        r#"{ "editor.tabSize": 7 }"#,
    )
    .unwrap();
    let restored = launch(home.path(), None, true);
    assert_eq!(tab_titles(&restored), ["Welcome", "Help: Settings"]);
    assert_eq!(active_help(&restored), Some("settings"));
    let text = restored.workbench.active_editor().unwrap().document.text();
    assert!(text.contains("| `editor.tabSize` | Number of spaces per tab | **`7`** | `2` |"));
}

#[test]
fn online_help_opens_the_browser_and_reports_failures() {
    static OPENED: std::sync::Mutex<Vec<String>> = std::sync::Mutex::new(Vec::new());
    let (_home, mut app) = test_app();
    app.url_opener = |url| {
        OPENED.lock().unwrap().push(url.to_owned());
        Ok(())
    };
    app.execute_command(Command::HelpDocumentation);
    app.execute_command(Command::HelpReportIssue);
    let opened = OPENED.lock().unwrap().clone();
    assert_eq!(
        opened[0],
        "https://github.com/ultradotdev/tide/tree/main/docs"
    );
    assert!(opened[1].starts_with("https://github.com/ultradotdev/tide/issues/new?body="));
    assert!(
        app.status
            .starts_with("Opened https://github.com/ultradotdev/tide/issues/new")
    );
    app.url_opener = |_| Err("xdg-open: not found".into());
    app.execute_command(Command::HelpDocumentation);
    assert_eq!(
        app.status,
        "Could not open a browser (xdg-open: not found); visit https://github.com/ultradotdev/tide/tree/main/docs"
    );
}

#[test]
fn the_start_screen_shows_the_live_keys() {
    let (_home, mut app) = test_app();
    assert_eq!(
        app.start_hints(),
        [
            "Ctrl+P  quick open    Ctrl+K  commands",
            "Ctrl+`  terminal      Ctrl+\\  split pane",
            "",
            "New here? Run “Help: Welcome” from the command palette.",
        ]
    );
    app.user_keybindings.push(Keybinding {
        key: "f2".into(),
        command: "workbench.quickOpen".into(),
        when: None,
        args: None,
    });
    app.keybindings.extend(app.user_keybindings.clone());
    app.keybindings.push(Keybinding {
        key: "ctrl+alt+w".into(),
        command: "help.welcome".into(),
        when: None,
        args: None,
    });
    let hints = app.start_hints();
    assert_eq!(hints[0], "F2  quick open        Ctrl+K  commands");
    assert_eq!(hints[3], "New here? Ctrl+Alt+W opens the Welcome guide.");
}

#[test]
fn check_for_updates_shows_the_install_specific_step() {
    static CALLS: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let (_home, mut app) = test_app();
    app.latest_release = |url| {
        CALLS.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        assert_eq!(url, "https://github.com/ultradotdev/tide/releases/latest");
        Box::pin(async { Ok("v999.0.0".to_owned()) })
    };
    app.settle();
    assert_eq!(
        CALLS.load(std::sync::atomic::Ordering::SeqCst),
        0,
        "building the app never checks; the run loop starts the daily check"
    );
    app.install = crate::help::Install::Homebrew {
        formula: "tide".into(),
    };
    app.execute_command(Command::HelpCheckForUpdates);
    assert_eq!(app.status, "Checking for updates…");
    app.settle();
    assert_eq!(CALLS.load(std::sync::atomic::Ordering::SeqCst), 1);
    let overlay = app.overlay.as_ref().expect("the update dialog");
    let message = overlay.base().message.clone().unwrap();
    assert!(message.contains("Tide 999.0.0 is available"), "{message}");
    assert!(
        message.contains("brew update && brew upgrade tide"),
        "{message}"
    );

    app.overlay = None;
    app.latest_release = |_| Box::pin(async { Ok(format!("v{}", env!("CARGO_PKG_VERSION"))) });
    app.execute_command(Command::HelpCheckForUpdates);
    app.settle();
    assert!(app.overlay.is_none());
    assert!(app.status.contains("is up to date"), "{}", app.status);

    app.latest_release = |_| Box::pin(async { Err("could not reach GitHub: offline".to_owned()) });
    app.execute_command(Command::HelpCheckForUpdates);
    app.settle();
    assert_eq!(
        app.status,
        "Update check failed: could not reach GitHub: offline"
    );
}

#[test]
fn the_automatic_check_runs_at_most_daily_and_only_reports_a_newer_release() {
    static CALLS: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let calls = || CALLS.load(std::sync::atomic::Ordering::SeqCst);
    let (_home, mut app) = test_app();
    app.latest_release = |_| {
        CALLS.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        Box::pin(async { Ok("v999.0.0".to_owned()) })
    };
    app.settle();
    app.set_status("");
    app.check_for_updates_automatically();
    app.settle();
    assert_eq!(calls(), 1);
    assert!(
        app.overlay.is_none(),
        "the daily check never opens a dialog"
    );
    assert_eq!(
        app.status,
        "Tide 999.0.0 is available; run “Help: Check for Updates” for the update step"
    );
    assert!(crate::help::update_check_marker(&app.paths).exists());

    app.check_for_updates_automatically();
    app.settle();
    assert_eq!(calls(), 1, "a second start the same day does not ask again");

    let marker = crate::help::update_check_marker(&app.paths);
    std::fs::write(&marker, "0").unwrap();
    app.settings
        .set("update.checkAutomatically", serde_json::json!(false));
    app.check_for_updates_automatically();
    app.settle();
    assert_eq!(calls(), 1, "the setting turns the check off");

    app.settings
        .set("update.checkAutomatically", serde_json::json!(true));
    app.latest_release = |_| {
        CALLS.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        Box::pin(async { Ok(format!("v{}", env!("CARGO_PKG_VERSION"))) })
    };
    app.set_status("unchanged");
    app.check_for_updates_automatically();
    app.settle();
    assert_eq!(calls(), 2);
    assert_eq!(app.status, "unchanged", "an up-to-date result says nothing");

    std::fs::write(&marker, "0").unwrap();
    app.latest_release = |_| Box::pin(async { Err("could not reach GitHub: offline".to_owned()) });
    app.check_for_updates_automatically();
    app.settle();
    assert_eq!(app.status, "unchanged", "a failed daily check stays quiet");
    assert!(
        !crate::help::update_check_due(&app.paths, std::time::SystemTime::now()),
        "a failed check still waits a day before retrying"
    );
}

#[test]
fn the_update_check_is_due_after_a_day_or_when_its_record_is_unusable() {
    let (_home, app) = test_app();
    let marker = crate::help::update_check_marker(&app.paths);
    let now = std::time::UNIX_EPOCH + std::time::Duration::from_secs(1_800_000_000);
    let due = || crate::help::update_check_due(&app.paths, now);
    assert!(due(), "never checked");
    std::fs::create_dir_all(marker.parent().unwrap()).unwrap();
    for (recorded, expected) in [
        ("1799999000", false),
        ("1799913601", false),
        ("1799913600", true),
        ("1800000500", true),
        ("not a time", true),
    ] {
        std::fs::write(&marker, recorded).unwrap();
        assert_eq!(due(), expected, "{recorded}");
    }
}
