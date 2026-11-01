use super::*;

#[test]
fn outline_prefers_language_server_symbols() {
    let (_home, mut app) = test_app();
    let path = app.workspace.join("outline.ts");
    std::fs::write(&path, "function alpha() {}\nfunction beta() {}\n").unwrap();
    app.open_file(&path);
    let uri = path_to_uri(&path).unwrap();
    assert_eq!(app.outline_for_active().len(), 2, "fallback outline");
    app.lsp.outline.insert(
        uri,
        vec![SymbolItem {
            name: "alpha".to_owned(),
            detail: String::new(),
            uri: None,
            line: 4,
            depth: 0,
        }],
    );
    let outline = app.outline_for_active();
    assert_eq!(outline.len(), 1);
    assert_eq!(outline[0], (4, "alpha".to_owned()));
}

#[test]
fn stale_lsp_responses_are_rejected_after_editing() {
    let (_home, mut app) = test_app();
    let path = app.workspace.join("stale.ts");
    std::fs::write(&path, "const value = 1;\n").unwrap();
    app.open_file(&path);
    app.lsp.generation = 4;
    app.lsp.origin.revision = app.workbench.active_editor().unwrap().revision();
    app.lsp.origin.document = app
        .workbench
        .active_editor()
        .map(|editor| editor.document.clone());
    app.lsp.origin.pane = app.workbench.focused();
    app.workbench
        .active_editor_mut()
        .unwrap()
        .insert_text("// edit\n");
    assert!(!with_ui!(app, |ui| app.lsp.current(4, ui)));
    app.handle_background(AppEvent::Lsp(LspEvent::Hover {
        generation: 4,
        text: "stale hover".to_owned(),
    }));
    assert!(app.overlay.is_none());
}

#[test]
fn lsp_code_action_modal_applies_the_selected_edit() {
    let (_home, mut app) = test_app();
    let path = app.workspace.join("action.ts");
    std::fs::write(&path, "const answer = 42;\n").unwrap();
    app.open_file(&path);
    let uri = path_to_uri(&path).unwrap();
    app.lsp.generation = 7;
    app.lsp.origin.revision = app.workbench.active_editor().unwrap().revision();
    app.lsp.origin.document = app
        .workbench
        .active_editor()
        .map(|editor| editor.document.clone());
    app.lsp.origin.pane = app.workbench.focused();
    app.handle_background(AppEvent::Lsp(LspEvent::CodeActions {
        generation: 7,
        actions: vec![CodeActionItem {
            title: "Replace const".to_owned(),
            kind: Some("quickfix".to_owned()),
            edit: Some(serde_json::json!({
                "changes": {
                    uri: [{
                        "range": {
                            "start": {"line": 0, "character": 0},
                            "end": {"line": 0, "character": 5}
                        },
                        "newText": "let"
                    }]
                }
            })),
            command: None,
            is_preferred: true,
        }],
    }));
    assert!(matches!(
        app.overlay.as_ref().map(|overlay| overlay.kind().clone()),
        Some(ModalKind::CodeActions)
    ));
    let key = KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE);
    app.handle_key(key);
    let editor = app.workbench.active_editor().unwrap();
    assert!(editor.document.text().starts_with("let answer"));
    assert!(editor.is_dirty());
}

#[test]
fn applies_lsp_text_edits_to_a_closed_document() {
    let (_home, mut app) = test_app();
    let path = app.workspace.join("closed.ts");
    std::fs::write(&path, "const answer = 42;\n").unwrap();
    let uri = path_to_uri(&path).unwrap();
    with_ui!(app, |ui| apply_edits(
        vec![LspFileEdits {
            uri,
            edits: vec![TextEdit {
                range: tide_lsp::Range {
                    start: LspPosition {
                        line: 0,
                        character: 6,
                    },
                    end: LspPosition {
                        line: 0,
                        character: 12,
                    },
                },
                new_text: "value".to_owned(),
            }],
        }],
        ui
    ));
    assert_eq!(
        std::fs::read_to_string(&path).unwrap(),
        "const value = 42;\n"
    );
}

#[test]
fn multi_file_lsp_edits_abort_before_touching_open_buffers() {
    let (_home, mut app) = test_app();
    let open_path = app.workspace.join("open.ts");
    std::fs::write(&open_path, "const answer = 42;\n").unwrap();
    app.open_file(&open_path);
    let open_uri = path_to_uri(&open_path).unwrap();
    with_ui!(app, |ui| apply_edits(
        vec![
            LspFileEdits {
                uri: open_uri,
                edits: vec![TextEdit {
                    range: tide_lsp::Range {
                        start: LspPosition {
                            line: 0,
                            character: 6,
                        },
                        end: LspPosition {
                            line: 0,
                            character: 12,
                        },
                    },
                    new_text: "value".to_owned(),
                }],
            },
            LspFileEdits {
                uri: "file:///definitely/outside/workspace.ts".to_owned(),
                edits: Vec::new(),
            },
        ],
        ui
    ));
    let editor = app.workbench.active_editor().unwrap();
    assert_eq!(editor.document.text(), "const answer = 42;\n");
    assert!(!editor.is_dirty());
}

#[test]
fn applies_lsp_text_edits_to_an_open_document() {
    let (_home, mut app) = test_app();
    let path = app.workspace.join("rename.ts");
    std::fs::write(&path, "const answer = 42;\n").unwrap();
    app.open_file(&path);
    let uri = path_to_uri(&path).unwrap();
    with_ui!(app, |ui| apply_edits(
        vec![LspFileEdits {
            uri,
            edits: vec![TextEdit {
                range: tide_lsp::Range {
                    start: LspPosition {
                        line: 0,
                        character: 6,
                    },
                    end: LspPosition {
                        line: 0,
                        character: 12,
                    },
                },
                new_text: "value".to_owned(),
            }],
        }],
        ui
    ));
    let editor = app.workbench.active_editor().unwrap();
    assert_eq!(editor.document.text(), "const value = 42;\n");
    assert!(editor.is_dirty());
}

#[test]
fn completion_replaces_utf16_ranges_applies_imports_once_and_undoes_together() {
    let (_home, mut app) = test_app();
    let path = app.workspace.join("completion.ts");
    std::fs::write(&path, "a😀 con\n").unwrap();
    app.open_file(&path);
    app.workbench
        .active_editor_mut()
        .unwrap()
        .document
        .set_cursor(Position::new(0, 6));
    let mut terminal = ratatui::Terminal::new(ratatui::backend::TestBackend::new(120, 40)).unwrap();
    terminal.draw(|f| app.draw(f)).unwrap();
    freeze_lsp(&mut app, 4);
    let item = CompletionItem {
        label: "console (documentation)".into(),
        detail: None,
        insert_text: "wrong".into(),
        kind: None,
        text_edit: Some(TextEdit {
            range: LspRange {
                start: LspPosition {
                    line: 0,
                    character: 4,
                },
                end: LspPosition {
                    line: 0,
                    character: 7,
                },
            },
            new_text: "console".into(),
        }),
        additional_edits: vec![TextEdit {
            range: LspRange {
                start: LspPosition {
                    line: 0,
                    character: 0,
                },
                end: LspPosition {
                    line: 0,
                    character: 0,
                },
            },
            new_text: "imported\n".into(),
        }],
    };
    app.handle_background(AppEvent::Lsp(LspEvent::Completion {
        generation: 4,
        items: vec![item],
    }));
    assert!(app.overlay.as_ref().unwrap().base().anchor.is_some());
    app.handle_event(Event::Key(KeyEvent::new(
        KeyCode::Enter,
        KeyModifiers::NONE,
    )));
    assert_eq!(
        app.workbench.active_editor().unwrap().document.text(),
        "imported\na😀 console\n"
    );
    app.workbench.active_editor_mut().unwrap().undo();
    assert_eq!(
        app.workbench.active_editor().unwrap().document.text(),
        "a😀 con\n"
    );
    app.workbench
        .active_editor_mut()
        .unwrap()
        .document
        .set_cursor(Position::new(0, 6));
    freeze_lsp(&mut app, 5);
    with_ui!(app, |ui| app.lsp.apply_completion(
        CompletionItem {
            label: "console label".into(),
            detail: None,
            insert_text: "console.log()".into(),
            kind: None,
            text_edit: None,
            additional_edits: vec![],
        },
        ui
    ));
    assert_eq!(
        app.workbench.active_editor().unwrap().document.text(),
        "a😀 console.log()\n"
    );
}

#[test]
fn completion_typing_edits_the_buffer_and_escape_cancels_pending_popup() {
    let (_home, mut app) = test_app();
    let path = app.workspace.join("completion.rs");
    std::fs::write(&path, "con").unwrap();
    app.open_file(&path);
    app.workbench.active_editor_mut().unwrap().end(false);
    freeze_lsp(&mut app, 1);
    app.handle_background(AppEvent::Lsp(LspEvent::Completion {
        generation: 1,
        items: tide_lsp::parse_completion_items(
            &serde_json::json!([{"label":"constant","insertText":"constant"}]),
        ),
    }));
    app.handle_event(Event::Key(KeyEvent::new(
        KeyCode::Char('s'),
        KeyModifiers::NONE,
    )));
    assert_eq!(
        app.workbench.active_editor().unwrap().document.text(),
        "cons"
    );
    assert!(app.overlay.is_none());
    assert!(app.lsp.pending.is_some());
    app.handle_event(Event::Key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE)));
    assert!(app.lsp.pending.is_none());
    freeze_lsp(&mut app, 9);
    app.workbench
        .active_editor_mut()
        .unwrap()
        .document
        .set_cursor(Position::new(0, 0));
    app.handle_background(AppEvent::Lsp(LspEvent::Completion {
        generation: 9,
        items: tide_lsp::parse_completion_items(&serde_json::json!([{"label":"stale"}])),
    }));
    assert!(app.overlay.is_none());
}

#[test]
fn server_edits_acknowledge_targets_and_atomic_writes_never_follow_predictable_temp_links() {
    let (_home, mut app) = test_app();
    let path = app.workspace.join("target.ts");
    let outside = app.workspace.parent().unwrap().join("outside");
    std::fs::write(&path, "let x=1;").unwrap();
    std::fs::write(&outside, "PRIVATE").unwrap();
    #[cfg(unix)]
    std::os::unix::fs::symlink(&outside, app.workspace.join(".target.ts.tide-tmp")).unwrap();
    let uri = path_to_uri(&path).unwrap();
    let edit = serde_json::json!({"changes":{&uri:[{"range":{"start":{"line":0,"character":4},"end":{"line":0,"character":5}},"newText":"answer"}]}});
    let (reply, mut rx) = tokio::sync::oneshot::channel();
    app.handle_background(AppEvent::Lsp(LspEvent::ServerEdit {
        edit: edit.clone(),
        versions: HashMap::new(),
        reply,
    }));
    assert!(rx.try_recv().unwrap());
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "let answer=1;");
    assert_eq!(std::fs::read_to_string(&outside).unwrap(), "PRIVATE");
    app.open_file(&path);
    let version = app.workbench.active_editor().unwrap().revision();
    app.workbench
        .active_editor_mut()
        .unwrap()
        .insert_text("changed");
    let before = app.workbench.active_editor().unwrap().document.text();
    let (reply, mut rx) = tokio::sync::oneshot::channel();
    app.handle_background(AppEvent::Lsp(LspEvent::ServerEdit {
        edit,
        versions: HashMap::from([(uri, i64::try_from(version).unwrap())]),
        reply,
    }));
    assert!(!rx.try_recv().unwrap());
    assert_eq!(
        app.workbench.active_editor().unwrap().document.text(),
        before
    );
}

#[test]
fn bulk_close_and_session_switch_invalidate_old_lsp_lifetimes() {
    for switch in [false, true] {
        let (_home, mut app) = test_app();
        if switch {
            app.save_session_as("empty");
            app.session_name = None;
        }
        let file = app.workspace.join("reopened.rs");
        std::fs::write(&file, "fn first() {}").unwrap();
        app.open_file(&file);
        let uri = path_to_uri(&file).unwrap();
        let old = DocumentLease::default();
        app.lsp.leases.insert(uri.clone(), old.clone());
        if switch {
            app.switch_named_session("empty");
        } else {
            app.force_close_all();
        }
        assert!(!old.is_valid());
        assert!(!app.lsp.leases.contains_key(&uri));
        std::fs::write(&file, "fn second() {}").unwrap();
        app.open_file(&file);
        assert_eq!(
            app.workbench.active_editor().unwrap().document.text(),
            "fn second() {}"
        );
    }
}

#[test]
fn lsp_positions_are_utf16_and_response_identity_survives_equal_revisions() {
    let (_home, mut app) = test_app();
    let first = app.workspace.join("first.rs");
    let second = app.workspace.join("second.rs");
    std::fs::write(&first, "a😀b").unwrap();
    std::fs::write(&second, "second").unwrap();
    app.open_file(&first);
    app.workbench
        .active_editor_mut()
        .unwrap()
        .document
        .set_cursor(Position::new(0, 2));
    assert_eq!(
        with_ui!(app, |ui| active_document(ui))
            .unwrap()
            .position
            .character,
        3
    );
    app.lsp.generation = 9;
    app.lsp.origin.revision = app.workbench.active_editor().unwrap().revision();
    app.lsp.origin.document = app
        .workbench
        .active_editor()
        .map(|editor| editor.document.clone());
    app.lsp.origin.pane = app.workbench.focused();
    app.lsp.origin.cursor = app
        .workbench
        .active_editor()
        .unwrap()
        .document
        .cursor()
        .position;
    assert!(with_ui!(app, |ui| app.lsp.current(9, ui)));
    app.open_file(&second);
    assert!(!with_ui!(app, |ui| app.lsp.current(9, ui)));
    app.workbench.focus_editor(&first);
    app.force_close_active();
    app.open_file(&first);
    assert!(!with_ui!(app, |ui| app.lsp.current(9, ui)));
}

#[test]
fn invalid_open_buffer_edits_abort_closed_file_writes() {
    let (_home, mut app) = test_app();
    let open = app.workspace.join("open.rs");
    let closed = app.workspace.join("closed.rs");
    std::fs::write(&open, "a😀b").unwrap();
    std::fs::write(&closed, "before").unwrap();
    app.open_file(&open);
    let edit = |uri: String, start, end| LspFileEdits {
        uri,
        edits: vec![TextEdit {
            range: LspRange {
                start: LspPosition {
                    line: 0,
                    character: start,
                },
                end: LspPosition {
                    line: 0,
                    character: end,
                },
            },
            new_text: "after".into(),
        }],
    };
    assert!(!with_ui!(app, |ui| apply_edits(
        vec![
            edit(path_to_uri(&closed).unwrap(), 0, 6),
            edit(path_to_uri(&open).unwrap(), 1, 2)
        ],
        ui
    )));
    assert_eq!(std::fs::read_to_string(closed).unwrap(), "before");
    assert_eq!(
        app.workbench.active_editor().unwrap().document.text(),
        "a😀b"
    );
}

#[test]
fn lsp_servers_come_from_settings_and_share_status() {
    let (_home, mut app) = test_app();
    app.settings.set(
        "lsp.servers",
        serde_json::json!({"rust": {"command": "custom-ra", "args": ["--x"]}, "go": null}),
    );
    let rust = server_for(&app.settings, "rust").unwrap();
    assert_eq!(
        (rust.command.as_str(), rust.args.as_slice()),
        ("custom-ra", &["--x".to_owned()][..])
    );
    assert!(server_for(&app.settings, "go").is_none());
    assert_eq!(
        server_for(&app.settings, "python").unwrap().command,
        "pylsp"
    );

    let path = app.workspace.join("view.tsx");
    std::fs::write(&path, "export {}\n").unwrap();
    app.open_file(&path);
    app.handle_background(AppEvent::Lsp(LspEvent::Status {
        language_id: "typescript".to_owned(),
        message: "ready".to_owned(),
    }));
    let mut terminal = ratatui::Terminal::new(ratatui::backend::TestBackend::new(160, 40)).unwrap();
    terminal.draw(|frame| app.draw(frame)).unwrap();
    assert!(crate::test_support::screen_text(&terminal).contains("LSP: ready"));
}

#[tokio::test(flavor = "multi_thread")]
async fn saving_notifies_a_server_that_wants_did_save() {
    let (_home, mut app) = test_app_async();
    let mock = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../tide-lsp/tests/mock_server.py"
    );
    app.settings.set(
        "lsp.servers",
        serde_json::json!({"rust": {
            "command": "python3",
            "args": [mock, "--sync", "full", "--save", "include-text"],
        }}),
    );
    let path = app.workspace.join("main.rs");
    std::fs::write(&path, "fn main() {}\n").unwrap();
    app.open_file(&path);
    app.workbench
        .active_editor_mut()
        .unwrap()
        .insert_text("// saved\n");
    app.save_active();
    let server = server_for(&app.settings, "rust").unwrap();
    let uri = path_to_uri(&path).unwrap();
    let saves = tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            if let Some(client) = app.lsp.clients.running(&server, &app.workspace).await
                && let Ok(state) = client
                    .execute_command("fixture.syncState", serde_json::json!([uri]))
                    .await
                && state["saves"]
                    .as_array()
                    .is_some_and(|saves| !saves.is_empty())
            {
                break state["saves"].clone();
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    })
    .await
    .unwrap();
    assert_eq!(saves[0]["text"], "// saved\nfn main() {}\n");
    app.lsp.clients.shutdown_all().await;
}
