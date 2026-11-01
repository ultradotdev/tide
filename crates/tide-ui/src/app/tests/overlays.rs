use super::*;

#[test]
fn outside_click_dismisses_lightweight_overlays_without_activating_the_host() {
    let (_home, mut app) = test_app();
    app.layout.screen = Rect::new(0, 0, 120, 40);
    let click = |column, row| MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column,
        row,
        modifiers: KeyModifiers::NONE,
    };
    let tabs = app.workbench.focused_pane().tabs.len();
    for kind in [
        ModalKind::CommandPalette,
        ModalKind::QuickOpen,
        ModalKind::Find,
        ModalKind::AllTabs,
        ModalKind::ThemePicker,
        ModalKind::Settings,
        ModalKind::SchemaTables,
        ModalKind::QueryHistory,
        ModalKind::Info,
        ModalKind::Completion,
        ModalKind::Prompt(PromptAction::GotoLine),
    ] {
        let mut modal = CommandPaletteModal::new(Vec::new());
        modal.base_mut().kind = kind.clone();
        // Also exercise the formerly click-through anchored hover path.
        if kind == ModalKind::Info {
            modal.base_mut().anchor = Some((50, 5));
        }
        let popup = modal.base().geometry(app.layout.screen);
        app.overlay = Some(Box::new(modal));
        app.layout.hits = vec![(Rect::new(0, 39, 1, 1), HitTarget::Command(Command::FileNew))];
        app.handle_mouse(click(popup.x, popup.y));
        assert!(app.overlay.is_some(), "border click dismissed {kind:?}");
        app.handle_mouse(click(0, 39));
        assert!(app.overlay.is_none(), "outside click retained {kind:?}");
        assert_eq!(app.workbench.focused_pane().tabs.len(), tabs);
    }
    app.search_cancel
        .store(false, std::sync::atomic::Ordering::Release);
    app.overlay = Some(Box::new(SearchResultsModal::new(Vec::new())));
    app.handle_mouse(click(0, 39));
    assert!(app.search_cancel.load(std::sync::atomic::Ordering::Acquire));
}

#[test]
fn outside_click_keeps_save_decisions_and_unsaved_forms_open() {
    let (_home, mut app) = test_app();
    app.layout.screen = Rect::new(0, 0, 120, 40);
    let outside = MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: 0,
        row: 39,
        modifiers: KeyModifiers::NONE,
    };
    for kind in [
        ModalKind::ConnectionForm,
        ModalKind::RowInsert,
        ModalKind::Prompt(PromptAction::EditRowCell("query".into(), "value".into())),
        ModalKind::Confirm(ConfirmAction::DeleteRow(Box::new(tide_db::RowMutation {
            connection_id: "connection".into(),
            query_id: "query".into(),
            target: tide_db::RowTarget {
                oid: 1,
                schema: "public".into(),
                table: "items".into(),
                primary_key: vec!["id".into()],
                columns: vec!["id".into()],
            },
            original: BTreeMap::new(),
            changes: BTreeMap::new(),
            delete: true,
        }))),
    ] {
        let mut modal = CommandPaletteModal::new(Vec::new());
        modal.base_mut().kind = kind.clone();
        app.overlay = Some(Box::new(modal));
        app.handle_mouse(outside);
        assert_eq!(*app.overlay.as_ref().unwrap().kind(), kind);
    }
    app.overlay = None;
    app.new_file();
    app.workbench
        .active_editor_mut()
        .unwrap()
        .insert_text("keep this draft");
    app.close_active();
    let kind = app.overlay.as_ref().unwrap().kind().clone();
    app.handle_mouse(outside);
    assert_eq!(*app.overlay.as_ref().unwrap().kind(), kind);
    app.handle_modal_key(KeyEvent::new(KeyCode::Char('s'), KeyModifiers::NONE));
    app.handle_mouse(outside);
    assert_eq!(*app.overlay.as_ref().unwrap().kind(), ModalKind::SaveAs);
    assert!(app.save_before.is_some());
    // An in-flight save can use an informational overlay instead of a form.
    app.overlay = Some(Box::new(InfoModal::new("Saving", "Waiting for write")));
    app.handle_mouse(outside);
    assert!(app.overlay.is_some());
    assert!(app.save_before.is_some());
    assert!(app.workbench.active_editor().unwrap().is_dirty());
    assert!(!app.should_quit);
}

#[test]
fn find_field_select_all_cut_and_paste_never_edit_the_underlying_document() {
    let (_home, mut app) = test_app();
    app.new_file();
    app.workbench
        .active_editor_mut()
        .unwrap()
        .insert_text("document stays");
    app.execute_command(Command::SearchReplace);
    for ch in "old query".chars() {
        app.handle_key(KeyEvent::new(KeyCode::Char(ch), KeyModifiers::NONE));
    }
    app.handle_key(KeyEvent::new(KeyCode::Char('a'), KeyModifiers::CONTROL));
    app.handle_paste("new query");
    assert_eq!(app.overlay.as_ref().unwrap().input(), "new query");
    app.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
    for ch in "replacement".chars() {
        app.handle_key(KeyEvent::new(KeyCode::Char(ch), KeyModifiers::NONE));
    }
    app.handle_key(KeyEvent::new(KeyCode::Char('a'), KeyModifiers::CONTROL));
    app.handle_key(KeyEvent::new(KeyCode::Char('x'), KeyModifiers::CONTROL));
    assert!(app.overlay.as_ref().unwrap().base().replacement.is_empty());
    assert_eq!(app.clipboard, "replacement");
    assert_eq!(
        app.workbench.active_editor().unwrap().document.text(),
        "document stays"
    );
}

#[test]
fn unified_find_preserves_fields_options_custom_bindings_and_invalid_regex_does_not_edit() {
    let (_home, mut app) = test_app();
    app.new_file();
    app.workbench
        .active_editor_mut()
        .unwrap()
        .insert_text("foo FOO food foo");
    app.execute_command(Command::SearchReplace);
    for ch in "foo".chars() {
        app.handle_key(KeyEvent::new(KeyCode::Char(ch), KeyModifiers::NONE));
    }
    for ch in ['c', 'w'] {
        app.handle_key(KeyEvent::new(KeyCode::Char(ch), KeyModifiers::ALT));
    }
    assert_eq!(app.overlay.as_ref().unwrap().base().search_matches, Some(2));
    app.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
    for ch in "BAR".chars() {
        app.handle_key(KeyEvent::new(KeyCode::Char(ch), KeyModifiers::NONE));
    }
    app.keybindings.push(Keybinding {
        key: "ctrl+j".into(),
        command: "search.replace".into(),
        when: Some("modalFocus".into()),
        args: None,
    });
    app.handle_key(KeyEvent::new(KeyCode::Char('j'), KeyModifiers::CONTROL));
    assert!(!app.overlay.as_ref().unwrap().base().replace_mode);
    app.handle_key(KeyEvent::new(KeyCode::Char('j'), KeyModifiers::CONTROL));
    assert!(app.overlay.as_ref().unwrap().base().replace_mode);
    assert_eq!(app.overlay.as_ref().unwrap().base().replacement, "BAR");
    app.handle_key(KeyEvent::new(KeyCode::Char('f'), KeyModifiers::CONTROL));
    assert!(!app.overlay.as_ref().unwrap().base().replace_mode);
    assert_eq!(app.overlay.as_ref().unwrap().input(), "foo");
    app.handle_key(KeyEvent::new(KeyCode::Char('j'), KeyModifiers::CONTROL));
    assert!(app.overlay.as_ref().unwrap().base().replace_mode);

    app.handle_key(KeyEvent::new(KeyCode::Char('u'), KeyModifiers::CONTROL));
    app.handle_key(KeyEvent::new(KeyCode::Char('['), KeyModifiers::NONE));
    app.handle_key(KeyEvent::new(KeyCode::Char('r'), KeyModifiers::ALT));
    assert!(app.overlay.as_ref().unwrap().base().message.is_some());
    app.overlay.as_mut().unwrap().base_mut().actions_focused = true;
    app.overlay.as_mut().unwrap().base_mut().action_index = 7;
    app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    assert_eq!(
        app.workbench.active_editor().unwrap().document.text(),
        "foo FOO food foo"
    );
    app.overlay.as_mut().unwrap().base_mut().actions_focused = false;
    app.handle_key(KeyEvent::new(KeyCode::Char('u'), KeyModifiers::CONTROL));
    for ch in "foo".chars() {
        app.handle_key(KeyEvent::new(KeyCode::Char(ch), KeyModifiers::NONE));
    }
    app.overlay.as_mut().unwrap().base_mut().actions_focused = true;
    app.overlay.as_mut().unwrap().base_mut().action_index = 7;
    app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    app.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
    assert_eq!(
        app.workbench.active_editor().unwrap().document.text(),
        "BAR FOO food BAR"
    );
    app.execute_command(Command::EditUndo);
    assert_eq!(
        app.workbench.active_editor().unwrap().document.text(),
        "foo FOO food foo"
    );
}

#[test]
fn unified_replacement_keeps_selection_scope_around_length_changes() {
    let (_home, mut app) = test_app();
    app.new_file();
    let editor = app.workbench.active_editor_mut().unwrap();
    editor.insert_text("foo foo foo");
    editor.document.set_selection(tide_core::Selection {
        anchor: Position::new(0, 0),
        active: Position::new(0, 7),
    });
    app.execute_command(Command::SearchReplace);
    for ch in "foo".chars() {
        app.handle_key(KeyEvent::new(KeyCode::Char(ch), KeyModifiers::NONE));
    }
    app.handle_key(KeyEvent::new(KeyCode::Char('s'), KeyModifiers::ALT));
    app.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
    for ch in "longer".chars() {
        app.handle_key(KeyEvent::new(KeyCode::Char(ch), KeyModifiers::NONE));
    }
    app.overlay.as_mut().unwrap().base_mut().actions_focused = true;
    app.overlay.as_mut().unwrap().base_mut().action_index = 7;
    app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    assert_eq!(
        app.workbench.active_editor().unwrap().document.text(),
        "longer longer foo"
    );
    assert_eq!(
        app.search_options.selection.unwrap().end,
        Position::new(0, 13)
    );
}

#[test]
fn theme_picker_updates_the_palette() {
    let (_home, mut app) = test_app();
    app.execute_command(Command::WorkbenchSelectTheme);
    assert!(app.overlay.is_some());
    let before = app.palette.background;
    app.select_theme("dracula");
    assert_eq!(app.themes.selected_id(), "dracula");
    assert_ne!(app.palette.background, before);
}

#[test]
fn unified_replace_all_works_through_visible_controls_and_undo() {
    let (_home, mut app) = test_app();
    app.new_file();
    app.workbench
        .active_editor_mut()
        .unwrap()
        .insert_text("foo 😀 foo");
    app.handle_key(KeyEvent::new(KeyCode::Char('h'), KeyModifiers::CONTROL));
    for ch in "foo".chars() {
        app.handle_key(KeyEvent::new(KeyCode::Char(ch), KeyModifiers::NONE));
    }
    app.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
    for ch in "bar".chars() {
        app.handle_key(KeyEvent::new(KeyCode::Char(ch), KeyModifiers::NONE));
    }
    app.overlay.as_mut().unwrap().base_mut().actions_focused = true;
    app.overlay.as_mut().unwrap().base_mut().action_index = 7;
    app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    app.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
    assert_eq!(
        app.workbench.active_editor().unwrap().document.text(),
        "bar 😀 bar"
    );
    app.execute_command(Command::EditUndo);
    assert_eq!(
        app.workbench.active_editor().unwrap().document.text(),
        "foo 😀 foo"
    );
}

#[test]
fn ctrl_k_opens_the_palette_immediately_without_waiting_for_a_chord() {
    let (_home, mut app) = test_app();
    app.handle_key(KeyEvent::new(KeyCode::Char('k'), KeyModifiers::CONTROL));
    assert!(matches!(
        app.overlay.as_ref().map(|modal| modal.kind()),
        Some(ModalKind::CommandPalette)
    ));
    assert!(app.pending_chord.is_none());
    app.handle_key(KeyEvent::new(KeyCode::Char('s'), KeyModifiers::NONE));
    assert_eq!(app.overlay.as_ref().unwrap().input(), "s");
}

#[test]
fn command_palette_lists_only_implemented_commands() {
    let (_home, mut app) = test_app();
    app.execute_command(Command::WorkbenchCommandPalette);
    let overlay = app.overlay.as_ref().unwrap();
    assert!(
        overlay
            .base()
            .items
            .iter()
            .all(|item| !item.value.starts_with("aiChat.")
                && !item.value.starts_with("ensemble.")
                && !item.label.contains("experimental"))
    );
    assert!(
        overlay
            .base()
            .items
            .iter()
            .any(|item| item.value == "workbench.quickOpen")
    );
    assert!(
        !overlay
            .base()
            .items
            .iter()
            .any(|item| item.value == "ensemble.toggle")
    );
}

#[test]
fn quick_open_queues_early_enter_but_cancellation_or_further_typing_revokes_it() {
    let (_home, mut app) = test_app();
    let wanted = app.workspace.join("wanted.txt");
    std::fs::write(&wanted, "TARGET").unwrap();
    for action in ["open", "cancel", "cancel-button", "edit", "no-match"] {
        app.quick_open_generation += 1;
        let mut modal = modal::quick_open(Vec::new());
        modal.base_mut().loading = true;
        modal.base_mut().push_str(if action == "no-match" {
            "absent"
        } else {
            "wanted"
        });
        app.overlay = Some(modal);
        app.handle_modal_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        assert!(app.overlay.as_ref().unwrap().base().pending_submit);
        if action == "cancel" {
            app.handle_modal_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
        }
        if action == "edit" {
            app.handle_modal_key(KeyEvent::new(KeyCode::Char('.'), KeyModifiers::NONE));
        }
        if action == "cancel-button" {
            let modal = app.overlay.as_mut().unwrap().base_mut();
            modal.actions_focused = true;
            modal.action_index = 1;
            app.handle_modal_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        }
        app.handle_background(AppEvent::QuickOpen {
            generation: app.quick_open_generation,
            files: vec![wanted.clone()],
        });
        match action {
            "open" => {
                assert!(app.overlay.is_none());
                assert_eq!(
                    app.workbench.active_editor().unwrap().path(),
                    Some(wanted.as_path())
                );
            }
            "cancel" | "cancel-button" => assert!(app.overlay.is_none()),
            _ => {
                let modal = app.overlay.as_ref().unwrap();
                assert_eq!(*modal.kind(), ModalKind::QuickOpen);
                assert!(!modal.base().loading);
                assert!(!modal.base().pending_submit);
            }
        }
    }
}

#[tokio::test]
async fn quick_open_runs_through_background_events() {
    let (_home, mut app) = test_app_async();
    let mut receiver = app.take_event_receiver().expect("receiver");
    app.open_quick_open();
    assert_eq!(*app.overlay.as_ref().unwrap().kind(), ModalKind::QuickOpen);
    app.handle_key(KeyEvent::new(KeyCode::Char('s'), KeyModifiers::NONE));
    assert_eq!(app.overlay.as_ref().unwrap().base().input, "s");
    assert!(
        !app.workbench
            .active_editor()
            .is_some_and(EditorState::is_dirty)
    );
    tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            let event = receiver.recv().await.expect("event channel closed");
            let quick_open = matches!(&event, AppEvent::QuickOpen { .. });
            app.handle_background(event);
            if quick_open {
                break;
            }
        }
    })
    .await
    .expect("quick open should complete");
    assert_eq!(app.overlay.as_ref().unwrap().base().input, "s");
    app.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
    app.handle_background(AppEvent::QuickOpen {
        generation: app.quick_open_generation,
        files: Vec::new(),
    });
    assert!(
        app.overlay.is_none(),
        "late scan reopened a canceled picker"
    );
    app.open_quick_open();
    assert!(matches!(
        app.overlay.as_ref().map(|modal| modal.kind()),
        Some(ModalKind::QuickOpen)
    ));
}
