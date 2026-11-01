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
    app.toggle_comment();
    assert_eq!(
        app.workbench.active_editor().unwrap().document.text(),
        "// let value = 1;"
    );
    app.toggle_comment();
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
    app.toggle_comment();
    assert_eq!(
        app.workbench.active_editor().unwrap().document.text(),
        "\t\t// build: all\n\u{3000}// value"
    );
    app.workbench
        .active_editor_mut()
        .unwrap()
        .document
        .select_all();
    app.toggle_comment();
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
