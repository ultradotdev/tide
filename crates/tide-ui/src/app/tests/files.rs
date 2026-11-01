use super::*;

#[test]
fn save_before_close_saves_the_inactive_target_and_preserves_the_active_view() {
    let (_home, mut app) = test_app();
    let target = app.workspace.join("target.txt");
    let keep = app.workspace.join("keep.txt");
    std::fs::write(&target, "original").unwrap();
    std::fs::write(&keep, "keep").unwrap();
    app.open_file(&target);
    app.workbench
        .active_editor_mut()
        .unwrap()
        .insert_text("changed ");
    let pane = app.workbench.focused();
    let index = app.workbench.focused_pane().active;
    app.open_file(&keep);
    app.close_tab(pane, index);
    app.handle_modal_key(KeyEvent::new(KeyCode::Char('s'), KeyModifiers::NONE));
    assert_eq!(std::fs::read_to_string(target).unwrap(), "changed original");
    assert_eq!(
        app.workbench.active_editor().unwrap().path(),
        Some(keep.as_path())
    );
    assert!(app.overlay.is_none());
    assert!(!app.workbench.focused_pane().tabs.iter().any(|tab| {
        tab.editor()
            .and_then(EditorState::path)
            .is_some_and(|p| p.file_name().is_some_and(|name| name == "target.txt"))
    }));
}

#[test]
fn save_before_close_blocks_failed_saves_and_keeps_external_content() {
    let (_home, mut app) = test_app();
    let file = app.workspace.join("changed.txt");
    std::fs::write(&file, "original").unwrap();
    app.open_file(&file);
    app.workbench
        .active_editor_mut()
        .unwrap()
        .insert_text("draft ");
    std::fs::write(&file, "external").unwrap();
    app.close_active();
    app.handle_modal_key(KeyEvent::new(KeyCode::Char('s'), KeyModifiers::NONE));
    assert_eq!(std::fs::read_to_string(file).unwrap(), "external");
    assert!(app.workbench.active_editor().unwrap().is_dirty());
    assert!(
        app.overlay
            .as_ref()
            .unwrap()
            .base()
            .message
            .as_ref()
            .unwrap()
            .contains("Save failed")
    );
    assert!(!app.should_quit);
    app.handle_modal_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
    assert!(app.pending_close.is_none());
}

#[test]
fn save_before_close_draft_cancel_and_overwrite_cancel_preserve_the_form_and_identity() {
    let (_home, mut app) = test_app();
    app.new_file();
    app.workbench
        .active_editor_mut()
        .unwrap()
        .insert_text("draft");
    app.close_active();
    app.handle_modal_key(KeyEvent::new(KeyCode::Char('s'), KeyModifiers::NONE));
    assert_eq!(*app.overlay.as_ref().unwrap().kind(), ModalKind::SaveAs);
    app.handle_modal_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
    assert!(app.save_before.is_none());
    assert!(app.workbench.active_editor().unwrap().is_dirty());
    let existing = app.workspace.join("existing.txt");
    std::fs::write(&existing, "keep existing").unwrap();
    app.close_active();
    app.handle_modal_key(KeyEvent::new(KeyCode::Char('s'), KeyModifiers::NONE));
    submit_save_path(&mut app, &existing);
    assert_eq!(
        *app.overlay.as_ref().unwrap().kind(),
        ModalKind::Confirm(ConfirmAction::SaveAs(existing.clone()))
    );
    app.handle_modal_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
    assert_eq!(*app.overlay.as_ref().unwrap().kind(), ModalKind::SaveAs);
    assert_eq!(
        app.overlay.as_ref().unwrap().input(),
        existing.display().to_string()
    );
    assert!(app.save_before.is_some());
    let saved = app.workspace.join("saved.txt");
    submit_save_path(&mut app, &saved);
    assert_eq!(std::fs::read_to_string(saved).unwrap(), "draft");
    assert_eq!(std::fs::read_to_string(existing).unwrap(), "keep existing");
    assert!(
        app.workbench
            .focused_pane()
            .tabs
            .iter()
            .all(|tab| tab.editor().is_none())
    );
    assert!(app.save_before.is_none());
}

#[test]
fn save_before_restart_waits_for_every_draft_and_cancel_revokes_restart() {
    let (_home, mut app) = test_app();
    let file = app.workspace.join("saved.txt");
    std::fs::write(&file, "original").unwrap();
    app.open_file(&file);
    app.workbench
        .active_editor_mut()
        .unwrap()
        .insert_text("changed ");
    app.new_file();
    app.workbench
        .active_editor_mut()
        .unwrap()
        .insert_text("draft");
    app.restart = Some(true);
    app.quit();
    app.handle_modal_key(KeyEvent::new(KeyCode::Char('s'), KeyModifiers::NONE));
    assert!(!app.should_quit);
    assert_eq!(std::fs::read_to_string(&file).unwrap(), "changed original");
    app.handle_modal_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
    assert_eq!(app.restart, None);
    assert!(!app.should_quit);
    app.quit();
    app.handle_modal_key(KeyEvent::new(KeyCode::Char('s'), KeyModifiers::NONE));
    let draft = app.workspace.join("draft.txt");
    submit_save_path(&mut app, &draft);
    assert!(app.should_quit);
    assert_eq!(std::fs::read_to_string(draft).unwrap(), "draft");
}

#[test]
fn save_before_quit_handles_sql_as_a_file_and_never_commits_row_edits() {
    let (_home, mut app) = test_app();
    app.execute_command(Command::DatabaseNewQuery);
    app.workbench
        .active_editor_mut()
        .unwrap()
        .insert_text("SELECT 1;");
    app.quit();
    app.handle_modal_key(KeyEvent::new(KeyCode::Char('s'), KeyModifiers::NONE));
    assert_eq!(app.overlay.as_ref().unwrap().input(), "query.sql");
    app.handle_modal_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
    if let Some(Tab::Sql(sql)) = app.workbench.active_tab_mut() {
        sql.editor.document.with_mut(|d| d.mark_clean());
        sql.row_edits
            .insert("label".into(), serde_json::json!("pending"));
    }
    app.quit();
    app.handle_modal_key(KeyEvent::new(KeyCode::Char('s'), KeyModifiers::NONE));
    assert!(!app.should_quit);
    assert!(
        app.overlay
            .as_ref()
            .unwrap()
            .base()
            .message
            .as_ref()
            .unwrap()
            .contains("Database row edits remain")
    );
    assert!(app.db.tasks.is_empty());
}

#[tokio::test]
async fn save_before_close_waits_for_hunk_save_and_preserves_canceled_or_newer_edits() {
    for mode in ["close", "cancel", "newer"] {
        let (_home, mut app) = test_app_async();
        let mut receiver = app.take_event_receiver().unwrap();
        initialize_git(&app);
        let path = app.workspace.join("file.txt");
        std::fs::write(&path, "one\n").unwrap();
        for args in [&["add", "-A"][..], &["commit", "-q", "-m", "initial"]] {
            assert!(
                tide_services::git_process::git(&app.workspace, args, None)
                    .unwrap()
                    .success
            );
        }
        std::fs::write(&path, "two\n").unwrap();
        app.git.state = GitState::refresh(&app.workspace);
        with_ui!(app, |ui| app.git.open_file_diff(
            "file.txt".into(),
            None,
            false,
            None,
            ui
        ));
        tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                let event = receiver.recv().await.unwrap();
                let done = matches!(event, AppEvent::Git(GitEvent::DiffLoaded { .. }));
                app.handle_background(event);
                if done {
                    break;
                }
            }
        })
        .await
        .unwrap();
        with_ui!(app, |ui| app
            .git
            .diff_action(crate::diff_browser::Action::Edit, ui));
        let document = app.workbench.active_editor().unwrap().document.clone();
        document.set_content("custom\n");
        app.close_active();
        app.handle_modal_key(KeyEvent::new(KeyCode::Char('s'), KeyModifiers::NONE));
        assert!(app.save_before.is_some());
        if mode == "cancel" {
            app.handle_modal_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
        }
        if mode == "newer" {
            document.set_content("newer\n");
        }
        tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                let event = receiver.recv().await.unwrap();
                let done = matches!(event, AppEvent::Git(GitEvent::HunkSaved { .. }));
                app.handle_background(event);
                if done {
                    break;
                }
            }
        })
        .await
        .unwrap();
        let still_open = app.workbench.pane_ids().into_iter().any(|id| {
            app.workbench.pane(id).unwrap().tabs.iter().any(|tab| {
                tab.editor()
                    .is_some_and(|editor| editor.document.same_document(&document))
            })
        });
        assert_eq!(still_open, mode != "close");
        assert_eq!(
            git_output_text(&app.workspace, &["show", ":file.txt"]),
            "custom\n"
        );
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "two\n");
        if mode == "newer" {
            assert!(document.is_dirty());
            assert_eq!(document.text(), "newer\n");
        }
        app.shutdown().await;
    }
}

#[test]
fn save_before_close_scopes_cover_all_tabs_panes_session_switch_and_legacy_close() {
    for scope in ["all", "pane", "session", "legacy"] {
        let (_home, mut app) = test_app();
        if scope == "session" {
            app.save_session_as("other");
            app.session_name = None;
        }
        let first = app.workspace.join("first.txt");
        let second = app.workspace.join("second.txt");
        std::fs::write(&first, "first").unwrap();
        std::fs::write(&second, "second").unwrap();
        app.open_file(&first);
        app.workbench
            .active_editor_mut()
            .unwrap()
            .insert_text("changed ");
        app.split(SplitDirection::Vertical);
        let pane = app.workbench.focused();
        app.open_file(&second);
        app.workbench
            .active_editor_mut()
            .unwrap()
            .insert_text("changed ");
        match scope {
            "all" => app.close_all(),
            "pane" => app.close_pane(pane),
            "session" => app.open_named_session("other"),
            "legacy" => {
                app.overlay = Some(modal::confirm(
                    ConfirmAction::CloseDirty,
                    "Close",
                    "Save changes?",
                ))
            }
            _ => unreachable!(),
        }
        app.handle_modal_key(KeyEvent::new(KeyCode::Char('s'), KeyModifiers::NONE));
        assert_eq!(std::fs::read_to_string(&second).unwrap(), "changed second");
        assert_eq!(
            std::fs::read_to_string(&first).unwrap(),
            if scope == "legacy" {
                "first"
            } else {
                "changed first"
            }
        );
        assert!(app.overlay.is_none());
        match scope {
            "all" => assert!(
                app.workbench.pane_ids().iter().all(|id| app
                    .workbench
                    .pane(*id)
                    .unwrap()
                    .tabs
                    .is_empty())
            ),
            "pane" => assert_eq!(app.workbench.pane_ids().len(), 1),
            "session" => assert_eq!(app.session_name.as_deref(), Some("other")),
            "legacy" => {
                assert!(app.workbench.pane_ids().iter().all(|id| {
                    app.workbench.pane(*id).unwrap().tabs.iter().all(|tab| {
                        tab.editor().and_then(EditorState::path) != Some(second.as_path())
                    })
                }))
            }
            _ => unreachable!(),
        }
    }
}

#[test]
fn failed_snapshot_after_saving_blocks_exit_and_revokes_restart() {
    let (_home, mut app) = test_app();
    let file = app.workspace.join("saved.txt");
    std::fs::write(&file, "original").unwrap();
    app.open_file(&file);
    app.workbench
        .active_editor_mut()
        .unwrap()
        .insert_text("saved ");
    app.session_write_blocked = true;
    app.restart = Some(false);
    app.quit();
    app.handle_modal_key(KeyEvent::new(KeyCode::Char('s'), KeyModifiers::NONE));
    assert_eq!(std::fs::read_to_string(file).unwrap(), "saved original");
    assert!(!app.should_quit);
    assert_eq!(app.restart, None);
    assert!(app.status.contains("Quit blocked"));
}

#[test]
fn pending_tab_close_tracks_view_identity_when_an_earlier_tab_disappears() {
    let (_home, mut app) = test_app();
    app.new_file();
    app.workbench
        .active_editor_mut()
        .unwrap()
        .insert_text("earlier");
    app.new_file();
    app.workbench
        .active_editor_mut()
        .unwrap()
        .insert_text("target");
    let pane = app.workbench.focused();
    let target = app.workbench.focused_pane().active;
    app.new_file();
    app.workbench
        .active_editor_mut()
        .unwrap()
        .insert_text("keep");
    app.close_tab(pane, target);
    app.force_close_tab(pane, target - 1);
    app.handle_modal_key(KeyEvent::new(KeyCode::Char('n'), KeyModifiers::NONE));
    assert!(
        app.workbench
            .focused_pane()
            .tabs
            .iter()
            .any(|tab| matches!(tab,Tab::Editor(editor) if editor.document.text()=="keep"))
    );
    assert!(
        !app.workbench
            .focused_pane()
            .tabs
            .iter()
            .any(|tab| matches!(tab,Tab::Editor(editor) if editor.document.text()=="target"))
    );
}

#[test]
fn opening_the_same_file_twice_reuses_the_view() {
    let (_home, mut app) = test_app();
    let path = app.workspace.join("same.txt");
    std::fs::write(&path, "one\n").unwrap();
    app.open_file(&path);
    app.open_file(&path);
    let count = app
        .workbench
        .pane_ids()
        .into_iter()
        .filter_map(|id| app.workbench.pane(id))
        .flat_map(|pane| pane.tabs.iter())
        .filter(|tab| matches!(tab, Tab::Editor(editor) if editor.path() == Some(path.as_path())))
        .count();
    assert_eq!(count, 1);
}

#[test]
fn tab_close_targets_inactive_dirty_tab_and_preserves_the_active_document() {
    let (_home, mut app) = test_app();
    let first = app.workspace.join("first.txt");
    let second = app.workspace.join("second.txt");
    std::fs::write(&first, "first").unwrap();
    std::fs::write(&second, "second").unwrap();
    app.open_file(&first);
    app.workbench
        .active_editor_mut()
        .unwrap()
        .insert_text("draft");
    app.open_file(&second);
    let pane = app.workbench.focused();
    let count = app.workbench.focused_pane().tabs.len();
    let first_index = app
        .workbench
        .focused_pane()
        .tabs
        .iter()
        .position(
            |tab| matches!(tab, Tab::Editor(editor) if editor.path() == Some(first.as_path())),
        )
        .unwrap();
    let mut terminal = ratatui::Terminal::new(ratatui::backend::TestBackend::new(160, 40)).unwrap();
    terminal.draw(|frame| app.draw(frame)).unwrap();
    click_hit(&mut app, |target| {
        *target == HitTarget::TabClose(pane, first_index)
    });
    assert!(matches!(
        app.overlay.as_ref().map(|modal| modal.kind()),
        Some(ModalKind::Confirm(ConfirmAction::CloseTab { .. }))
    ));
    app.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
    assert_eq!(app.workbench.focused_pane().tabs.len(), count);
    click_hit(&mut app, |target| {
        *target == HitTarget::TabClose(pane, first_index)
    });
    app.handle_key(KeyEvent::new(KeyCode::Char('n'), KeyModifiers::NONE));
    assert_eq!(app.workbench.focused_pane().tabs.len(), count - 1);
    assert_eq!(
        app.workbench.active_editor().unwrap().path(),
        Some(second.as_path())
    );
    assert_eq!(std::fs::read_to_string(first).unwrap(), "first");
}

#[test]
fn file_creation_and_save_as_preserve_existing_files_until_confirmed() {
    let (_home, mut app) = test_app();
    let target = app.workspace.join("existing.txt");
    std::fs::write(&target, "keep").unwrap();
    app.create_workspace_path("existing.txt", false);
    assert_eq!(std::fs::read_to_string(&target).unwrap(), "keep");
    app.new_file();
    app.workbench
        .active_editor_mut()
        .unwrap()
        .insert_text("replacement");
    app.save_as_to(target.clone(), false);
    assert!(matches!(
        app.overlay.as_ref().map(|modal| modal.kind()),
        Some(ModalKind::Confirm(ConfirmAction::SaveAs(_)))
    ));
    app.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
    assert_eq!(std::fs::read_to_string(&target).unwrap(), "keep");
    app.save_as_to(target.clone(), false);
    app.handle_key(KeyEvent::new(KeyCode::Char('y'), KeyModifiers::NONE));
    assert_eq!(std::fs::read_to_string(target).unwrap(), "replacement");
}

#[test]
fn rename_and_save_as_rekey_open_documents() {
    let (_home, mut app) = test_app();
    let old = app.workspace.join("old.txt");
    let renamed = app.workspace.join("renamed.txt");
    std::fs::write(&old, "original").unwrap();
    app.open_file(&old);
    app.lsp.origin.document = Some(app.workbench.active_editor().unwrap().document.clone());
    app.lsp.origin.revision = app.workbench.active_editor().unwrap().revision();
    app.lsp.origin.pane = app.workbench.focused();
    assert!(with_ui!(app, |ui| app.lsp.current(0, ui)));
    app.overlay = Some(modal::prompt(
        PromptAction::RenameFile(old.clone()),
        "Rename",
        "renamed.txt",
    ));
    app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    assert!(!old.exists());
    assert!(renamed.exists());
    assert!(!with_ui!(app, |ui| app.lsp.current(0, ui)));
    assert_eq!(
        app.workbench.active_editor().unwrap().path(),
        Some(renamed.as_path())
    );
    let saved = app.workspace.join("saved.txt");
    app.lsp.origin.document = Some(app.workbench.active_editor().unwrap().document.clone());
    app.lsp.origin.revision = app.workbench.active_editor().unwrap().revision();
    app.lsp.origin.pane = app.workbench.focused();
    assert!(with_ui!(app, |ui| app.lsp.current(0, ui)));
    app.save_as_to(saved.clone(), false);
    assert!(!with_ui!(app, |ui| app.lsp.current(0, ui)));
    assert!(app.documents.get(&renamed).is_none());
    assert!(app.documents.get(&saved).is_some());
    std::fs::write(&renamed, "separate").unwrap();
    app.open_file(&renamed);
    assert_eq!(
        app.workbench.active_editor().unwrap().document.text(),
        "separate"
    );
}

#[test]
fn recovered_drafts_survive_two_snapshots_and_block_external_overwrite() {
    let (_home, mut app) = test_app();
    let path = app.workspace.join("recover.txt");
    std::fs::write(&path, "disk").unwrap();
    app.open_file(&path);
    app.workbench
        .active_editor_mut()
        .unwrap()
        .insert_text("draft ");
    let session = app.build_session();
    app.documents = DocumentRegistry::default();
    app.restore_session(session).unwrap();
    assert!(app.workbench.active_editor().unwrap().is_dirty());
    let session = app.build_session();
    assert_eq!(
        session.documents[0].unsaved_content.as_deref(),
        Some("draft disk")
    );
    app.documents = DocumentRegistry::default();
    std::fs::write(&path, "external").unwrap();
    app.restore_session(session).unwrap();
    assert!(app.workbench.active_editor_mut().unwrap().save().is_err());
    assert_eq!(std::fs::read_to_string(path).unwrap(), "external");
}

#[test]
fn close_reopen_reads_disk_and_bulk_close_asks_before_discarding() {
    let (_home, mut app) = test_app();
    let path = app.workspace.join("closed.txt");
    std::fs::write(&path, "before").unwrap();
    app.open_file(&path);
    app.force_close_active();
    std::fs::write(&path, "after").unwrap();
    app.open_file(&path);
    assert_eq!(
        app.workbench.active_editor().unwrap().document.text(),
        "after"
    );
    app.workbench
        .active_editor_mut()
        .unwrap()
        .insert_text("draft ");
    app.close_all();
    assert!(matches!(
        app.overlay.as_ref().map(|modal| modal.kind()),
        Some(ModalKind::Confirm(ConfirmAction::CloseAllDirty))
    ));
    app.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
    assert!(app.workbench.active_editor().unwrap().is_dirty());
    app.open_named_session("other");
    assert!(matches!(
        app.overlay.as_ref().map(|modal| modal.kind()),
        Some(ModalKind::Confirm(ConfirmAction::SwitchSession(_)))
    ));
}

#[tokio::test(flavor = "multi_thread")]
async fn external_change_checks_run_off_the_ui_thread() {
    let (_home, mut app) = test_app_async();
    let mut events = app.take_event_receiver().unwrap();
    let path = app.workspace.join("watched.txt");
    std::fs::write(&path, "one\n").unwrap();
    app.open_file(&path);
    std::fs::write(&path, "external\n").unwrap();
    crate::editor::HASHES.set(0);
    app.last_external_check = Instant::now() - Duration::from_secs(3);
    app.on_tick();
    assert_eq!(crate::editor::HASHES.get(), 0);
    assert!(!app.status.contains("changed on disk"));
    let event = tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let event = events.recv().await.unwrap();
            if matches!(event, AppEvent::ExternalChanges(_)) {
                break event;
            }
        }
    })
    .await
    .unwrap();
    app.handle_background(event);
    assert!(
        app.status
            .contains("watched.txt changed on disk; save is blocked until reload"),
        "{}",
        app.status
    );
}

#[test]
fn lossy_files_are_flagged_read_only_when_opened() {
    let (_home, mut app) = test_app();
    let path = app.workspace.join("latin1.txt");
    std::fs::write(&path, b"caf\xe9\n").unwrap();
    app.open_file(&path);
    assert!(app.status.contains("not valid UTF-8"), "{}", app.status);
    let mut terminal = ratatui::Terminal::new(ratatui::backend::TestBackend::new(160, 40)).unwrap();
    terminal.draw(|frame| app.draw(frame)).unwrap();
    assert!(crate::test_support::screen_text(&terminal).contains("Read-only: invalid UTF-8"));
}

#[test]
fn reload_and_overwrite_resolve_external_changes() {
    let (_home, mut app) = test_app();
    let path = app.workspace.join("external.txt");
    std::fs::write(&path, "one\n").unwrap();
    app.open_file(&path);
    app.workbench
        .active_editor_mut()
        .unwrap()
        .insert_text("buffer\n");
    std::fs::write(&path, "disk\n").unwrap();
    assert!(
        app.workbench
            .active_editor()
            .unwrap()
            .is_externally_changed()
    );

    app.workbench.active_editor_mut().unwrap().reload().unwrap();
    assert_eq!(
        app.workbench.active_editor().unwrap().document.text(),
        "disk\n"
    );
    assert!(!app.workbench.active_editor().unwrap().is_dirty());

    app.workbench
        .active_editor_mut()
        .unwrap()
        .insert_text("new\n");
    std::fs::write(&path, "changed again\n").unwrap();
    app.workbench
        .active_editor_mut()
        .unwrap()
        .save_force()
        .unwrap();
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "disk\nnew\n");
}

#[test]
fn opens_edits_and_saves_a_file() {
    let (_home, mut app) = test_app();
    let path = app.workspace.join("notes.txt");
    std::fs::write(&path, "one\n").unwrap();
    app.open_file(&path);
    assert!(app.workbench.active_editor().is_some());
    app.workbench
        .active_editor_mut()
        .unwrap()
        .insert_text("two\n");
    app.save_active();
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "two\none\n");
}

#[tokio::test]
async fn file_watcher_refreshes_the_tree() {
    let (_home, mut app) = test_app_async();
    let mut receiver = app.take_event_receiver().expect("receiver");
    std::fs::write(app.workspace.join("watched.txt"), "hello").unwrap();
    let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(3);
    loop {
        let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        if remaining.is_zero() {
            break;
        }
        if let Ok(Some(AppEvent::FilesChanged)) =
            tokio::time::timeout(remaining, receiver.recv()).await
        {
            app.handle_background(AppEvent::FilesChanged);
            assert!(
                !app.file_tree
                    .entries
                    .iter()
                    .any(|entry| entry.name == "watched.txt")
            );
            app.files_pending = Some(Instant::now() - Duration::from_millis(300));
            app.on_tick();
            break;
        }
    }
    assert!(
        app.file_tree
            .entries
            .iter()
            .any(|entry| entry.name == "watched.txt")
    );
}
