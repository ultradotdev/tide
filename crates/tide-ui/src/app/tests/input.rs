use super::*;

#[test]
fn canonicalizes_modifier_order() {
    assert_eq!(canonical_key("shift+meta+s"), "meta+shift+s");
    assert_eq!(canonical_key("meta+alt+right"), "alt+meta+arrowright");
    assert_eq!(canonical_key("Ctrl+Shift+P"), "ctrl+shift+p");
    assert_eq!(canonical_key("up"), "arrowup");
    assert_eq!(canonical_key("ctrl+k ctrl+c"), "ctrl+k ctrl+c");
    assert_eq!(canonical_key(" "), "space");
}

#[test]
fn key_events_map_to_binding_strings() {
    let key = KeyEvent::new(
        KeyCode::Char('p'),
        KeyModifiers::CONTROL | KeyModifiers::SHIFT,
    );
    assert_eq!(key_string(key).as_deref(), Some("ctrl+shift+p"));
}

#[test]
fn control_symbol_fallbacks_normalize_for_legacy_terminals() {
    let ctrl_bracket = KeyEvent::new(KeyCode::Char('5'), KeyModifiers::CONTROL);
    assert_eq!(key_string(ctrl_bracket).as_deref(), Some("ctrl+]"));
    let ctrl_backslash = KeyEvent::new(KeyCode::Char('4'), KeyModifiers::CONTROL);
    assert_eq!(key_string(ctrl_backslash).as_deref(), Some("ctrl+\\"));
}

#[test]
fn shifted_symbols_normalize_to_keybinding_names() {
    let key = KeyEvent::new(
        KeyCode::Char('|'),
        KeyModifiers::CONTROL | KeyModifiers::SHIFT,
    );
    assert_eq!(key_string(key).as_deref(), Some("ctrl+shift+\\"));
    let key = KeyEvent::new(
        KeyCode::Char('}'),
        KeyModifiers::CONTROL | KeyModifiers::SHIFT,
    );
    assert_eq!(key_string(key).as_deref(), Some("ctrl+shift+]"));
}

#[test]
fn repeated_keys_are_handled_and_idle_events_do_not_request_paint() {
    let (_home, mut app) = test_app();
    let path = app.workspace.join("repeat.txt");
    std::fs::write(&path, "abc").unwrap();
    app.open_file(path);
    assert!(app.handle_event(Event::Key(KeyEvent::new_with_kind(
        KeyCode::Right,
        KeyModifiers::NONE,
        KeyEventKind::Repeat
    ))));
    assert_eq!(
        app.workbench
            .active_editor()
            .unwrap()
            .document
            .cursor()
            .position
            .column,
        1
    );
    assert!(!app.handle_event(Event::Key(KeyEvent::new_with_kind(
        KeyCode::Right,
        KeyModifiers::NONE,
        KeyEventKind::Release
    ))));
    assert!(!app.handle_event(Event::Mouse(MouseEvent {
        kind: MouseEventKind::Moved,
        column: 1,
        row: 1,
        modifiers: KeyModifiers::NONE
    })));
    app.on_tick(); // First tick synchronizes the active-file timeline.
    assert!(!app.on_tick());
}

#[test]
fn implemented_command_guard_is_small() {
    assert!(command_supported("file.save"));
    assert!(!command_supported("ensemble.toggle"));
}

#[test]
fn every_frozen_retained_command_has_a_dispatch_route_and_restart_is_dirty_safe() {
    let commands: Vec<String> = serde_json::from_str(include_str!(
        "../../../../../tests/fixtures/rust-parity/retained-commands.json"
    ))
    .unwrap();
    for id in &commands {
        assert!(command_supported(id), "unsupported retained command {id}");
    }
    let (_home, mut app) = test_app();
    app.new_file();
    app.workbench
        .active_editor_mut()
        .unwrap()
        .insert_text("recover me");
    app.execute_command(Command::WorkbenchRestart);
    assert!(!app.should_quit);
    assert_eq!(app.restart, Some(false));
    app.handle_modal_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
    assert_eq!(app.restart, None);
    assert!(!app.should_quit);
    app.execute_command(Command::WorkbenchRestart);
    app.handle_modal_key(KeyEvent::new(KeyCode::Char('n'), KeyModifiers::NONE));
    assert!(app.should_quit);
    assert_eq!(app.restart, Some(false));
    let restored = app.session_store.load_last().unwrap().unwrap();
    assert_eq!(
        restored.state.documents[0].unsaved_content.as_deref(),
        Some("recover me")
    );
}

#[test]
fn real_dispatch_handles_plain_rebindings_unbinding_and_chords() {
    let (_home, mut app) = test_app();
    std::fs::write(app.workspace.join("one"), "one").unwrap();
    std::fs::write(app.workspace.join("two"), "two").unwrap();
    app.file_tree.refresh();
    app.focus_sidebar_section(0, true);
    app.keybindings.push(Keybinding {
        key: "q".into(),
        command: "fileTree.moveDown".into(),
        when: Some("fileTreeFocus".into()),
        args: None,
    });
    app.handle_key(KeyEvent::new(KeyCode::Char('q'), KeyModifiers::NONE));
    assert_eq!(app.file_tree.selected, 1);
    app.keybindings.push(Keybinding {
        key: "q".into(),
        command: String::new(),
        when: Some("fileTreeFocus".into()),
        args: None,
    });
    app.handle_key(KeyEvent::new(KeyCode::Char('q'), KeyModifiers::NONE));
    assert_eq!(app.file_tree.selected, 1);
    app.focus = Focus::Main;
    app.keybindings.push(Keybinding {
        key: "ctrl+k x".into(),
        command: "workbench.toggleSidebar".into(),
        when: None,
        args: None,
    });
    let before = app.sidebar_visible;
    app.handle_key(KeyEvent::new(KeyCode::Char('k'), KeyModifiers::CONTROL));
    assert!(matches!(
        app.overlay.as_ref().map(|modal| modal.kind()),
        Some(ModalKind::CommandPalette)
    ));
    app.handle_key(KeyEvent::new(KeyCode::Char('x'), KeyModifiers::NONE));
    assert_ne!(app.sidebar_visible, before);
    let before = app.sidebar_visible;
    app.handle_key(KeyEvent::new(KeyCode::Char('k'), KeyModifiers::CONTROL));
    app.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
    assert!(app.pending_chord.is_none());
    assert_eq!(app.sidebar_visible, before);
}

#[test]
fn keybinding_resolver_applies_contexts() {
    let (_home, mut app) = test_app();
    let path = app.workspace.join("context.ts");
    std::fs::write(&path, "const x = 1;\n").unwrap();
    app.open_file(path);
    assert_eq!(
        app.resolve_binding("ctrl+r"),
        Some(BindingAction::Run(Command::EditorGotoSymbol))
    );
    app.with_db(|db, ui| db.open_tab(Some("SELECT 1;"), ui));
    assert_eq!(
        app.resolve_binding("ctrl+r"),
        Some(BindingAction::Run(Command::DatabaseExecuteQuery))
    );
    assert_eq!(
        app.resolve_binding("f5"),
        Some(BindingAction::Run(Command::DatabaseExecuteQuery))
    );
    app.focus_sidebar_section(0, true);
    assert_eq!(
        app.resolve_binding("f2"),
        Some(BindingAction::Run(Command::FileTreeRename))
    );
    app.focus = Focus::Main;
    assert!(app.resolve_binding("f2").is_none());
}

#[test]
fn when_expression_parser_handles_boolean_grammar() {
    let (_home, app) = test_app();
    assert!(!app.eval_when("editorFocus || fileTreeFocus"));
    assert!(app.eval_when("unknownContext || !unknownContext"));
    assert!(!app.eval_when("editorFocus && fileTreeFocus"));
    assert!(app.eval_when("!(editorFocus && fileTreeFocus)"));
    assert!(!app.eval_when("unknownContext"));
    assert!(!app.eval_when("editorFocus &&"));
}

#[tokio::test(flavor = "multi_thread")]
async fn a_slow_clipboard_helper_never_blocks_input() {
    fn slow(copy: bool) -> Result<std::process::Command, String> {
        let mut command = std::process::Command::new("sh");
        command.args(if copy {
            ["-c", "sleep 1; cat >/dev/null"]
        } else {
            ["-c", "sleep 1; printf pasted"]
        });
        Ok(command)
    }
    let (_home, mut app) = test_app_async();
    let mut events = app.take_event_receiver().unwrap();
    app.clipboard_commands = slow;
    app.new_file();
    let started = Instant::now();
    app.execute_command(Command::EditPaste);
    app.handle_event(Event::Key(KeyEvent::new(
        KeyCode::Char('x'),
        KeyModifiers::NONE,
    )));
    app.workbench.active_editor_mut().unwrap().select_all();
    app.execute_command(Command::EditCopy);
    assert!(started.elapsed() < Duration::from_millis(500));
    assert_eq!(app.workbench.active_editor().unwrap().document.text(), "x");
    let mut pending = 2;
    tokio::time::timeout(Duration::from_secs(10), async {
        while pending > 0 {
            let event = events.recv().await.unwrap();
            if matches!(
                event,
                AppEvent::ClipboardRead { .. } | AppEvent::ClipboardWritten { .. }
            ) {
                pending -= 1;
            }
            app.handle_background(event);
        }
    })
    .await
    .unwrap();
    assert_eq!(
        app.workbench.active_editor().unwrap().document.text(),
        "pasted"
    );
    assert!(app.status_history.contains(&"Copied selection".to_owned()));
}

#[test]
fn keybinding_executes_save_and_split() {
    let (_home, mut app) = test_app();
    app.new_file();
    app.workbench
        .active_editor_mut()
        .unwrap()
        .insert_text("hello");
    app.handle_key(KeyEvent::new(KeyCode::Char('\\'), KeyModifiers::CONTROL));
    assert_eq!(app.workbench.pane_ids().len(), 2);
    app.execute_command(Command::FileSaveAs);
    assert!(app.overlay.is_some());
}

#[test]
fn no_single_key_binding_is_also_the_first_key_of_a_chord() {
    let (_home, app) = test_app();
    let singles = app
        .keybindings
        .iter()
        .map(|binding| canonical_key(&binding.key))
        .filter(|key| !key.contains(' '))
        .collect::<std::collections::HashSet<_>>();
    let shadowing = app
        .keybindings
        .iter()
        .map(|binding| canonical_key(&binding.key))
        .filter(|key| {
            key.split_once(' ')
                .is_some_and(|(first, _)| singles.contains(first))
        })
        .collect::<Vec<_>>();
    assert!(
        shadowing.is_empty(),
        "these chords make their first key wait: {shadowing:?}"
    );
}

#[test]
fn shell_editing_keys_stay_with_a_focused_terminal_while_ctrl_t_reaches_the_palette() {
    let (_home, app) = test_app();
    let ctrl = |ch| KeyEvent::new(KeyCode::Char(ch), KeyModifiers::CONTROL);
    assert!(app.is_terminal_control_key(ctrl('k')));
    assert!(app.is_terminal_control_key(ctrl('e')));
    assert!(!app.is_terminal_control_key(ctrl('t')));
    assert_eq!(
        app.keymap.get("ctrl+t").copied(),
        Some(Command::WorkbenchCommandPalette)
    );
}

#[test]
fn terminal_safe_alternate_shortcuts_resolve() {
    let (_home, app) = test_app();
    assert_eq!(
        app.keymap.get("ctrl+k").copied(),
        Some(Command::WorkbenchCommandPalette)
    );
    assert_eq!(
        app.keymap.get(&canonical_key("ctrl+alt+f")).copied(),
        Some(Command::SearchFindInFiles)
    );
    assert_eq!(
        app.keymap.get("ctrl+b").copied(),
        Some(Command::WorkbenchToggleSidebar)
    );
    assert_eq!(
        app.keymap.get(&canonical_key("ctrl+alt+g")).copied(),
        Some(Command::GitFocusPanel)
    );
    // The original Ctrl+Shift bindings remain available.
    assert_eq!(
        app.keymap.get("ctrl+shift+p").copied(),
        Some(Command::WorkbenchCommandPalette)
    );
}
