use super::*;

#[test]
fn search_in_files_finds_matches() {
    let (_home, mut app) = test_app();
    std::fs::write(app.workspace.join("search.txt"), "alpha\nneedle here\n").unwrap();
    app.spawn_search_in_files("needle");
    app.settle();
    let overlay = app.overlay.as_ref().unwrap();
    assert_eq!(overlay.base().items.len(), 1);
    assert!(overlay.base().items[0].value.contains("search.txt"));
}

#[test]
fn comments_and_uncomments_the_current_line() {
    let (_home, mut app) = test_app();
    app.new_file();
    app.workbench
        .active_editor_mut()
        .unwrap()
        .insert_text("let value = 1;");
    app.comment_lines(CommentMode::Toggle);
    assert_eq!(
        app.workbench.active_editor().unwrap().document.text(),
        "// let value = 1;"
    );
    app.comment_lines(CommentMode::Toggle);
    assert_eq!(
        app.workbench.active_editor().unwrap().document.text(),
        "let value = 1;"
    );
}

#[test]
fn toggling_a_comment_keeps_tab_and_multibyte_indentation() {
    let (_home, mut app) = test_app();
    app.new_file();
    let original = "\t\tbuild: all\n\u{3000}value";
    app.workbench
        .active_editor_mut()
        .unwrap()
        .insert_text(original);
    app.workbench
        .active_editor_mut()
        .unwrap()
        .document
        .select_all();
    app.comment_lines(CommentMode::Toggle);
    assert_eq!(
        app.workbench.active_editor().unwrap().document.text(),
        "\t\t// build: all\n\u{3000}// value"
    );
    app.workbench
        .active_editor_mut()
        .unwrap()
        .document
        .select_all();
    app.comment_lines(CommentMode::Toggle);
    assert_eq!(
        app.workbench.active_editor().unwrap().document.text(),
        original
    );
}

#[tokio::test]
async fn completed_background_syntax_requests_a_redraw_without_user_input() {
    let (home, mut app) = test_app_async();
    let file = home.path().join("workspace/idle-colors.ts");
    std::fs::write(&file, "// comment\nconst value = 'string';\n").unwrap();
    app.open_file(file);
    let mut terminal = ratatui::Terminal::new(ratatui::backend::TestBackend::new(120, 40)).unwrap();
    terminal.draw(|frame| app.draw(frame)).unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        if terminal.backend().buffer().content.windows(5).any(|cells| {
            cells.iter().map(|cell| cell.symbol()).collect::<String>() == "const"
                && cells[0].fg == ratatui::style::Color::Rgb(192, 171, 245)
        }) {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "syntax did not schedule its own redraw"
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
        if app.on_tick() {
            terminal.draw(|frame| app.draw(frame)).unwrap();
        }
    }
    app.shutdown().await;
}

#[tokio::test]
async fn native_typescript_document_receives_background_token_colors() {
    let (home, mut app) = test_app_async();
    let file = home.path().join("workspace/colors.ts");
    std::fs::write(&file, "// comment\nconst value = 'string';\n").unwrap();
    app.open_file(file);
    let mut terminal = ratatui::Terminal::new(ratatui::backend::TestBackend::new(120, 40)).unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        terminal.draw(|frame| app.draw(frame)).unwrap();
        let editor = app.workbench.active_editor().unwrap();
        let id = editor.document.syntax_id();
        let revision = editor.revision();
        if let Some(spans) = app.syntax.document_line(id, revision, 1) {
            assert!(
                spans
                    .iter()
                    .any(|span| span.text == "const" && span.fg.as_deref() == Some("#c0abf5")),
                "{spans:?}"
            );
            terminal.draw(|frame| app.draw(frame)).unwrap();
            let buffer = terminal.backend().buffer();
            let found = buffer.content.windows(5).any(|cells| {
                cells.iter().map(|c| c.symbol()).collect::<String>() == "const"
                    && cells[0].fg == ratatui::style::Color::Rgb(192, 171, 245)
            });
            assert!(
                found,
                "ready syntax was not painted into the application frame"
            );
            break;
        }
        assert!(
            Instant::now() < deadline,
            "syntax never became ready: id={id} revision={revision} ready={}",
            app.syntax.document_revision(id)
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
        app.on_tick();
    }
    app.shutdown().await;
}

#[test]
fn word_document_and_smart_home_motion_work_from_the_keyboard() {
    let (_home, mut app) = test_app();
    app.new_file();
    app.focus = Focus::Main;
    app.workbench
        .active_editor_mut()
        .unwrap()
        .insert_text("    let total_count = add(1, 2);\nnext line");
    let key = |code, modifiers| KeyEvent::new(code, modifiers);
    let cursor = |app: &App| {
        let position = app
            .workbench
            .active_editor()
            .unwrap()
            .document
            .cursor()
            .position;
        (position.line, position.column)
    };

    app.handle_key(key(KeyCode::Home, KeyModifiers::CONTROL));
    assert_eq!(cursor(&app), (0, 0));
    app.handle_key(key(KeyCode::Right, KeyModifiers::CONTROL));
    assert_eq!(cursor(&app), (0, 7), "skips the indent, then `let`");
    app.handle_key(key(KeyCode::Right, KeyModifiers::CONTROL));
    assert_eq!(cursor(&app), (0, 19), "underscores join a word");
    // Option+Right in iTerm2 and other terminals arrives as Alt+Right.
    app.handle_key(key(KeyCode::Right, KeyModifiers::ALT));
    assert_eq!(cursor(&app), (0, 21), "punctuation is its own word");
    // Option+Left in Terminal.app arrives as Alt+B.
    app.handle_key(key(KeyCode::Char('b'), KeyModifiers::ALT));
    assert_eq!(cursor(&app), (0, 20));
    app.handle_key(key(
        KeyCode::Left,
        KeyModifiers::CONTROL | KeyModifiers::SHIFT,
    ));
    assert_eq!(cursor(&app), (0, 8));
    let selection = app
        .workbench
        .active_editor()
        .unwrap()
        .document
        .selection_positions()
        .unwrap();
    assert_eq!((selection.0.column, selection.1.column), (8, 20));

    app.handle_key(key(KeyCode::End, KeyModifiers::NONE));
    app.handle_key(key(KeyCode::Right, KeyModifiers::CONTROL));
    assert_eq!(cursor(&app), (1, 0), "word motion crosses the line break");
    app.handle_key(key(KeyCode::End, KeyModifiers::CONTROL));
    assert_eq!(cursor(&app), (1, 9));

    app.handle_key(key(KeyCode::Backspace, KeyModifiers::ALT));
    assert_eq!(
        app.workbench.active_editor().unwrap().document.text(),
        "    let total_count = add(1, 2);\nnext "
    );

    app.handle_key(key(KeyCode::Up, KeyModifiers::NONE));
    app.handle_key(key(KeyCode::Home, KeyModifiers::NONE));
    assert_eq!(
        cursor(&app),
        (0, 4),
        "Home goes to the first non-blank first"
    );
    app.handle_key(key(KeyCode::Home, KeyModifiers::NONE));
    assert_eq!(cursor(&app), (0, 0), "and to column 0 on the second press");

    // Outside an editor, Alt+Left still moves pane focus.
    app.focus = Focus::Sidebar;
    assert!(app.resolve_binding("alt+left").is_some());
}
