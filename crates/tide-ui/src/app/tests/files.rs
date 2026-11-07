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
fn save_before_close_scopes_cover_all_tabs_panes_session_switch_and_active_close() {
    for scope in ["all", "pane", "session", "active"] {
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
            "active" => {
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
            if scope == "active" {
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
            "active" => {
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
            .contains("Reloaded watched.txt (changed on disk)"),
        "{}",
        app.status
    );
    assert_eq!(
        app.workbench.active_editor().unwrap().document.text(),
        "external\n"
    );
}

#[test]
fn changes_on_disk_reload_clean_files_and_report_unsaved_or_deleted_ones_once() {
    let (_home, mut app) = test_app();
    let clean = app.workspace.join("clean.txt");
    let edited = app.workspace.join("edited.txt");
    std::fs::write(&clean, "one\ntwo\nthree\n").unwrap();
    std::fs::write(&edited, "mine\n").unwrap();
    app.open_file(&edited);
    app.workbench
        .active_editor_mut()
        .unwrap()
        .insert_text("draft ");
    app.open_file(&clean);
    app.workbench.active_editor_mut().unwrap().move_down(false);

    std::fs::write(&clean, "one\ntwo changed by a formatter\nthree\n").unwrap();
    std::fs::write(&edited, "theirs\n").unwrap();
    app.handle_background(AppEvent::ExternalChanges(vec![
        clean.clone(),
        edited.clone(),
    ]));
    let editor = app.workbench.active_editor().unwrap();
    assert_eq!(
        editor.document.text(),
        "one\ntwo changed by a formatter\nthree\n"
    );
    assert!(!editor.is_dirty());
    assert_eq!(
        editor.document.cursor().position.line,
        1,
        "the cursor stays on its line"
    );
    assert!(
        app.status_history
            .iter()
            .any(|line| line.contains("Reloaded clean.txt"))
    );
    assert!(
        app.status
            .contains("edited.txt changed on disk and has unsaved edits"),
        "{}",
        app.status
    );

    // Undo brings back what was on screen before the reload.
    app.workbench.active_editor_mut().unwrap().undo();
    assert_eq!(
        app.workbench.active_editor().unwrap().document.text(),
        "one\ntwo\nthree\n"
    );
    app.workbench.active_editor_mut().unwrap().redo();

    // The same unresolved change is not announced again on every check.
    app.set_status("Saved something");
    app.handle_background(AppEvent::ExternalChanges(vec![edited.clone()]));
    assert_eq!(app.status, "Saved something");

    std::fs::remove_file(&clean).unwrap();
    app.handle_background(AppEvent::ExternalChanges(vec![
        clean.clone(),
        edited.clone(),
    ]));
    assert!(
        app.status.contains("clean.txt was deleted on disk"),
        "{}",
        app.status
    );
    assert_eq!(
        app.workbench.active_editor().unwrap().document.text(),
        "one\ntwo changed by a formatter\nthree\n",
        "a deleted file's text stays open"
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

/// Types `input` into the open prompt and submits it.
fn submit_prompt(app: &mut App, input: &str) {
    let base = app.overlay.as_mut().unwrap().base_mut();
    base.input = input.to_owned();
    base.cursor = input.chars().count();
    app.handle_modal_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
}

fn active_path(app: &App) -> Option<PathBuf> {
    app.workbench
        .active_editor()
        .and_then(EditorState::path)
        .map(Path::to_path_buf)
}

#[test]
fn rename_file_acts_on_the_active_file_or_the_focused_explorer_selection() {
    let (_home, mut app) = test_app();
    std::fs::create_dir_all(app.workspace.join("src")).unwrap();
    let old = app.workspace.join("src/old.rs");
    std::fs::write(&old, "fn main() {}\n").unwrap();
    app.open_file(&old);
    app.workbench
        .active_editor_mut()
        .unwrap()
        .insert_text("// draft\n");
    app.execute_command(Command::FileRename);
    assert_eq!(app.overlay.as_ref().unwrap().input(), "old.rs");
    submit_prompt(&mut app, "new.rs");
    let new = app.workspace.join("src/new.rs");
    assert!(!old.exists() && new.exists());
    assert_eq!(active_path(&app), Some(new.clone()));
    assert!(app.workbench.active_editor().unwrap().is_dirty());

    // With the Explorer focused, its selection is renamed instead of the active file.
    std::fs::write(app.workspace.join("notes.txt"), "notes").unwrap();
    app.file_tree.refresh();
    let index = app
        .file_tree
        .entries
        .iter()
        .position(|entry| entry.name == "notes.txt")
        .unwrap();
    app.file_tree.selected = index;
    app.focus_sidebar_section(0, true);
    app.execute_command(Command::FileRename);
    submit_prompt(&mut app, "renamed.txt");
    assert!(app.workspace.join("renamed.txt").exists());
    assert_eq!(active_path(&app), Some(new));

    app.focus = Focus::Main;
    app.new_file();
    app.execute_command(Command::FileRename);
    assert!(app.overlay.is_none());
    assert!(app.status.contains("Save this buffer"), "{}", app.status);
    app.execute_command(Command::HelpEditor);
    app.execute_command(Command::FileDelete);
    assert!(app.overlay.is_none());
    assert!(
        app.status.contains("help page, not a file"),
        "{}",
        app.status
    );
}

#[test]
fn delete_file_confirms_closes_its_tabs_and_stays_inside_the_workspace() {
    let (home, mut app) = test_app();
    let doomed = app.workspace.join("doomed.txt");
    std::fs::write(&doomed, "bye").unwrap();
    app.open_file(&doomed);
    app.workbench.active_editor_mut().unwrap().insert_text("x");
    app.execute_command(Command::FileDelete);
    let message = app
        .overlay
        .as_ref()
        .unwrap()
        .base()
        .message
        .clone()
        .unwrap();
    assert!(
        message.contains("Permanently delete doomed.txt"),
        "{message}"
    );
    assert!(message.contains("1 unsaved change"), "{message}");
    app.handle_modal_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
    assert!(doomed.exists());
    app.execute_command(Command::FileDelete);
    app.handle_modal_key(KeyEvent::new(KeyCode::Char('y'), KeyModifiers::NONE));
    assert!(!doomed.exists());
    assert!(app.overlay.is_none());
    assert!(
        app.workbench
            .tabs()
            .all(|tab| tab.editor().and_then(EditorState::path).is_none())
    );

    let outside = home.path().join("outside.txt");
    std::fs::write(&outside, "keep").unwrap();
    app.open_file(&outside);
    app.execute_command(Command::FileDelete);
    assert!(app.overlay.is_none());
    assert!(
        app.status.contains("outside the workspace"),
        "{}",
        app.status
    );
    assert!(outside.exists());
}

#[cfg(unix)]
#[test]
fn deleting_a_symlink_removes_the_link_and_keeps_its_target() {
    let (_home, mut app) = test_app();
    let target = app.workspace.join("target.txt");
    std::fs::write(&target, "kept").unwrap();
    let link = app.workspace.join("link.txt");
    std::os::unix::fs::symlink(&target, &link).unwrap();
    app.confirm_delete(&link);
    app.handle_modal_key(KeyEvent::new(KeyCode::Char('y'), KeyModifiers::NONE));
    assert!(std::fs::symlink_metadata(&link).is_err());
    assert_eq!(std::fs::read_to_string(&target).unwrap(), "kept");
}

#[test]
fn new_folder_copy_path_and_reveal_use_the_active_file() {
    static REVEALED: std::sync::Mutex<Vec<PathBuf>> = std::sync::Mutex::new(Vec::new());
    let (_home, mut app) = test_app();
    std::fs::create_dir_all(app.workspace.join("src")).unwrap();
    let file = app.workspace.join("src/lib.rs");
    std::fs::write(&file, "").unwrap();
    app.open_file(&file);
    app.execute_command(Command::FileNewFolder);
    let separator = std::path::MAIN_SEPARATOR;
    assert_eq!(
        app.overlay.as_ref().unwrap().input(),
        format!("src{separator}")
    );
    submit_prompt(&mut app, "src/nested");
    assert!(app.workspace.join("src/nested").is_dir());

    app.execute_command(Command::FileCopyPath);
    assert_eq!(app.clipboard, file.display().to_string());
    app.execute_command(Command::FileCopyRelativePath);
    assert_eq!(
        app.clipboard,
        Path::new("src").join("lib.rs").display().to_string()
    );

    app.path_revealer = |path| {
        REVEALED.lock().unwrap().push(path.to_path_buf());
        Ok(())
    };
    app.execute_command(Command::FileRevealInFinder);
    assert_eq!(*REVEALED.lock().unwrap(), [file]);
    assert!(app.status.contains("Showing src"), "{}", app.status);
    app.path_revealer = |_| Err("xdg-open: not found".into());
    app.execute_command(Command::FileRevealInFinder);
    assert!(app.status.contains("xdg-open: not found"), "{}", app.status);
}

#[test]
fn close_others_keeps_the_active_tab_and_asks_about_unsaved_ones() {
    let (_home, mut app) = test_app();
    let open = |app: &mut App, name: &str| {
        let path = app.workspace.join(name);
        std::fs::write(&path, name).unwrap();
        app.open_file(&path);
    };
    open(&mut app, "a.txt");
    open(&mut app, "b.txt");
    app.workbench
        .active_editor_mut()
        .unwrap()
        .insert_text("dirty ");
    open(&mut app, "c.txt");
    let tabs = |app: &App| app.workbench.focused_pane().tabs.len();
    let before = tabs(&app);
    app.execute_command(Command::FileCloseOthers);
    assert!(matches!(
        app.overlay.as_ref().unwrap().kind(),
        ModalKind::Confirm(ConfirmAction::CloseOtherTabs(_))
    ));
    app.handle_modal_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
    assert_eq!(tabs(&app), before);
    app.execute_command(Command::FileCloseOthers);
    app.handle_modal_key(KeyEvent::new(KeyCode::Char('n'), KeyModifiers::NONE));
    assert_eq!(tabs(&app), 1);
    assert_eq!(active_path(&app), Some(app.workspace.join("c.txt")));
    assert_eq!(
        std::fs::read_to_string(app.workspace.join("b.txt")).unwrap(),
        "b.txt"
    );
    app.execute_command(Command::FileCloseOthers);
    assert!(app.status.contains("No other tabs"), "{}", app.status);

    // Saving first writes the other tabs, then closes them.
    open(&mut app, "d.txt");
    app.workbench
        .active_editor_mut()
        .unwrap()
        .insert_text("saved ");
    open(&mut app, "e.txt");
    app.execute_command(Command::FileCloseOthers);
    app.handle_modal_key(KeyEvent::new(KeyCode::Char('y'), KeyModifiers::NONE));
    assert_eq!(tabs(&app), 1);
    assert_eq!(active_path(&app), Some(app.workspace.join("e.txt")));
    assert_eq!(
        std::fs::read_to_string(app.workspace.join("d.txt")).unwrap(),
        "saved d.txt"
    );
}

/// Root may write any file, so permission tests only mean something for other users.
#[cfg(unix)]
fn running_as_root() -> bool {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().unwrap();
    let probe = dir.path().join("probe");
    std::fs::write(&probe, "").unwrap();
    std::fs::set_permissions(&probe, std::fs::Permissions::from_mode(0o444)).unwrap();
    std::fs::OpenOptions::new().write(true).open(&probe).is_ok()
}

fn draw(app: &mut App) -> ratatui::Terminal<ratatui::backend::TestBackend> {
    let mut terminal = ratatui::Terminal::new(ratatui::backend::TestBackend::new(120, 30)).unwrap();
    terminal.draw(|frame| app.draw(frame)).unwrap();
    terminal
}

#[cfg(unix)]
#[test]
fn saving_a_read_only_file_asks_first_and_keeps_its_permissions() {
    use std::os::unix::fs::PermissionsExt;
    if running_as_root() {
        return;
    }
    let (_home, mut app) = test_app();
    let path = app.workspace.join("locked.txt");
    std::fs::write(&path, "original\n").unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o444)).unwrap();
    app.open_file(&path);
    assert!(app.workbench.active_editor().unwrap().is_file_read_only());
    let screen = crate::test_support::screen_text(&draw(&mut app));
    assert!(screen.contains("locked.txt [RO]"), "{screen}");
    assert!(screen.contains("Read-only"), "{screen}");
    app.workbench
        .active_editor_mut()
        .unwrap()
        .insert_text("changed ");

    // Save All and saving before closing refuse instead of asking.
    app.save_all();
    assert!(app.status.contains("is read-only"), "{}", app.status);
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "original\n");

    // Save asks, and Cancel (the default button) leaves the file alone.
    app.save_active();
    let overlay = app.overlay.as_ref().unwrap();
    assert_eq!(
        *overlay.kind(),
        ModalKind::Confirm(ConfirmAction::OverwriteReadOnly(path.clone()))
    );
    assert!(
        overlay
            .base()
            .message
            .as_deref()
            .unwrap()
            .contains("locked.txt is read-only. Overwrite it anyway?")
    );
    app.handle_modal_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    assert!(app.overlay.is_none());
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "original\n");
    assert!(app.workbench.active_editor().unwrap().is_dirty());

    app.save_active();
    app.handle_modal_key(KeyEvent::new(KeyCode::Char('y'), KeyModifiers::NONE));
    assert_eq!(
        std::fs::read_to_string(&path).unwrap(),
        "changed original\n"
    );
    assert!(!app.workbench.active_editor().unwrap().is_dirty());
    let mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode, 0o444);
    assert!(app.workbench.active_editor().unwrap().is_file_read_only());
}

#[cfg(unix)]
#[test]
fn a_file_in_a_read_only_folder_fails_to_save_with_a_clear_message() {
    use std::os::unix::fs::PermissionsExt;
    if running_as_root() {
        return;
    }
    let (_home, mut app) = test_app();
    let folder = app.workspace.join("locked");
    std::fs::create_dir_all(&folder).unwrap();
    let path = folder.join("notes.txt");
    std::fs::write(&path, "original\n").unwrap();
    app.open_file(&path);
    std::fs::set_permissions(&folder, std::fs::Permissions::from_mode(0o555)).unwrap();
    app.workbench
        .active_editor_mut()
        .unwrap()
        .insert_text("changed ");
    app.save_active();
    std::fs::set_permissions(&folder, std::fs::Permissions::from_mode(0o755)).unwrap();
    assert!(
        app.status.contains("permission denied writing in"),
        "{}",
        app.status
    );
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "original\n");
    assert!(app.workbench.active_editor().unwrap().is_dirty());
}

#[test]
fn indentation_follows_the_file_then_editorconfig_and_shows_in_the_status_bar() {
    let (_home, mut app) = test_app();
    let rust = app.workspace.join("main.rs");
    std::fs::write(
        &rust,
        "fn main() {\n    if true {\n        call();\n    }\n}\n",
    )
    .unwrap();
    app.open_file(&rust);
    let screen = crate::test_support::screen_text(&draw(&mut app));
    assert!(screen.contains("Spaces: 4"), "{screen}");
    {
        let editor = app.workbench.active_editor_mut().unwrap();
        editor.document.set_cursor(Position::new(4, 1));
    }
    app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    app.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
    assert!(
        app.workbench
            .active_editor()
            .unwrap()
            .document
            .text()
            .ends_with("}\n    \n"),
        "{:?}",
        app.workbench.active_editor().unwrap().document.text()
    );

    // Detection off: the settings apply.
    app.settings
        .set("editor.detectIndentation", serde_json::Value::Bool(false));
    let screen = crate::test_support::screen_text(&draw(&mut app));
    assert!(screen.contains("Spaces: 2"), "{screen}");
    app.settings
        .set("editor.detectIndentation", serde_json::Value::Bool(true));

    // An .editorconfig wins over the file's own indentation once read again.
    std::fs::write(
        app.workspace.join(".editorconfig"),
        "root = true\n[*.rs]\nindent_style = tab\ntab_width = 8\n",
    )
    .unwrap();
    app.reset_file_styles();
    let screen = crate::test_support::screen_text(&draw(&mut app));
    assert!(screen.contains("Tab Size: 8"), "{screen}");
    app.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
    assert!(
        app.workbench
            .active_editor()
            .unwrap()
            .document
            .text()
            .ends_with("    \t\n")
    );
}

#[test]
fn editorconfig_save_rules_trim_whitespace_and_add_the_final_newline() {
    let (_home, mut app) = test_app();
    std::fs::write(
        app.workspace.join(".editorconfig"),
        "root = true\n[*]\ntrim_trailing_whitespace = true\ninsert_final_newline = true\n[*.md]\ntrim_trailing_whitespace = false\n",
    )
    .unwrap();
    let text = app.workspace.join("a.txt");
    std::fs::write(&text, "one  \ntwo\t\nthree").unwrap();
    app.open_file(&text);
    app.workbench.active_editor_mut().unwrap().insert_text("x");
    app.save_active();
    assert_eq!(app.status, "Saved a.txt");
    assert_eq!(
        std::fs::read_to_string(&text).unwrap(),
        "xone\ntwo\nthree\n"
    );
    let editor = app.workbench.active_editor().unwrap();
    assert!(!editor.is_dirty());
    assert_eq!(editor.document.text(), "xone\ntwo\nthree\n");

    let markdown = app.workspace.join("b.md");
    std::fs::write(&markdown, "hard break  \nend  ").unwrap();
    app.open_file(&markdown);
    app.workbench.active_editor_mut().unwrap().insert_text("x");
    app.save_active();
    assert_eq!(
        std::fs::read_to_string(&markdown).unwrap(),
        "xhard break  \nend  \n"
    );
}

#[test]
fn a_new_file_takes_its_line_ending_from_editorconfig() {
    let (_home, mut app) = test_app();
    std::fs::write(
        app.workspace.join(".editorconfig"),
        "root = true\n[*.bat]\nend_of_line = crlf\n",
    )
    .unwrap();
    let path = app.workspace.join("run.bat");
    std::fs::write(&path, "").unwrap();
    app.open_file(&path);
    draw(&mut app);
    app.workbench.active_editor_mut().unwrap().insert_text("a");
    app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    app.save_active();
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "a\r\n");
}

#[test]
fn markdown_opens_formatted_and_switches_to_its_source_in_place() {
    let (_home, mut app) = test_app();
    let readme = app.workspace.join("README.md");
    std::fs::write(&readme, "# Title\n\nSome **bold** text\n").unwrap();
    app.open_file(&readme);
    let screen = |app: &mut App| {
        let mut terminal =
            ratatui::Terminal::new(ratatui::backend::TestBackend::new(120, 30)).unwrap();
        terminal.draw(|frame| app.draw(frame)).unwrap();
        let buffer = terminal.backend().buffer();
        (0..30)
            .map(|y| {
                (0..120)
                    .map(|x| buffer[(x, y)].symbol())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
    };
    let tabs = |app: &App| app.workbench.focused_pane().tabs.len();
    let previewing = |app: &App| app.workbench.active_editor().unwrap().is_previewing();

    // Formatted by default, with the view in the status bar and no cursor position.
    assert!(previewing(&app));
    let rows = screen(&mut app);
    let text = rows.join("\n");
    assert!(text.contains("Some bold text"), "{text}");
    assert!(
        !text.contains("**bold**") && !text.contains("# Title"),
        "{text}"
    );
    let status = rows.last().unwrap();
    assert!(
        status.contains("Preview") && !status.contains("Ln 1"),
        "{status}"
    );

    // Typing does nothing to the hidden source.
    let before = tabs(&app);
    app.handle_key(KeyEvent::new(KeyCode::Char('x'), KeyModifiers::NONE));
    app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    app.handle_key(KeyEvent::new(KeyCode::Backspace, KeyModifiers::NONE));
    let editor = app.workbench.active_editor().unwrap();
    assert!(!editor.is_dirty());
    assert_eq!(editor.document.text(), "# Title\n\nSome **bold** text\n");

    // The status bar item switches to the source in the same tab.
    click_hit(&mut app, |target| {
        *target == HitTarget::Command(Command::MarkdownTogglePreview)
    });
    assert!(!previewing(&app));
    assert_eq!(tabs(&app), before);
    let rows = screen(&mut app);
    assert!(rows.join("\n").contains("# Title"));
    assert!(rows.last().unwrap().contains("Edit"));

    // An edit made in source is what saving writes, even from the preview.
    app.workbench
        .active_editor_mut()
        .unwrap()
        .insert_text("Intro\n");
    app.execute_command(Command::MarkdownTogglePreview);
    assert!(previewing(&app));
    app.execute_command(Command::FileSave);
    assert_eq!(
        std::fs::read_to_string(&readme).unwrap(),
        "Intro\n# Title\n\nSome **bold** text\n"
    );

    // A command that works on the text shows the source first.
    app.execute_command(Command::EditSelectAll);
    assert!(!previewing(&app));

    // Other files have no preview.
    let notes = app.workspace.join("notes.txt");
    std::fs::write(&notes, "plain").unwrap();
    app.open_file(&notes);
    app.execute_command(Command::MarkdownTogglePreview);
    assert!(!previewing(&app));
}

#[test]
fn markdown_open_mode_edit_opens_the_source() {
    let (_home, mut app) = test_app();
    app.settings
        .set("editor.markdown.openMode", serde_json::json!("edit"));
    let readme = app.workspace.join("README.md");
    std::fs::write(&readme, "# Title\n").unwrap();
    app.open_file(&readme);
    assert!(!app.workbench.active_editor().unwrap().is_previewing());
}
