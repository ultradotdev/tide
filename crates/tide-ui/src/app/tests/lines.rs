use super::*;

fn edited(text: &str) -> (tempfile::TempDir, App) {
    let (home, mut app) = test_app();
    app.new_file();
    let editor = app.workbench.active_editor_mut().unwrap();
    editor.insert_text(text);
    editor.document.set_cursor(Position::new(0, 0));
    (home, app)
}

fn text(app: &App) -> String {
    app.workbench.active_editor().unwrap().document.text()
}

/// A line and column.
type At = (usize, usize);

/// Places cursors given as `(anchor, active)` line/column pairs; the first is primary.
fn place(app: &mut App, cursors: &[(At, At)]) {
    let cursors = cursors
        .iter()
        .map(|((al, ac), (l, c))| serde_json::json!({"position": [l, c], "anchor": [al, ac]}))
        .collect::<Vec<_>>();
    app.workbench
        .active_editor_mut()
        .unwrap()
        .restore_view_state(&serde_json::json!({ "cursors": cursors }));
}

fn cursor(app: &App) -> tide_core::Cursor {
    app.workbench.active_editor().unwrap().document.cursor()
}

/// Runs `command`, checks the result, then checks one undo restores the text and redo
/// repeats it. Undo history keeps only the primary cursor, so the cursors the command
/// placed are put back afterwards.
fn run_and_undo(app: &mut App, command: Command, expected: &str) {
    let before = text(app);
    app.execute_command(command);
    assert_eq!(text(app), expected, "{command}");
    let view = app.workbench.active_editor().unwrap().view_state();
    app.execute_command(Command::EditUndo);
    assert_eq!(text(app), before, "{command} is not one undo step");
    app.execute_command(Command::EditRedo);
    assert_eq!(text(app), expected, "{command} redo");
    app.workbench
        .active_editor_mut()
        .unwrap()
        .restore_view_state(&view);
}

#[test]
fn moving_lines_swaps_blocks_and_keeps_cursors_and_selections_on_them() {
    let (_home, mut app) = edited("a\nb\nc\nd\ne");
    place(&mut app, &[((1, 0), (1, 0))]);
    run_and_undo(&mut app, Command::EditMoveLinesDown, "a\nc\nb\nd\ne");
    assert_eq!(cursor(&app).position, Position::new(2, 0));
    app.execute_command(Command::EditMoveLinesUp);
    assert_eq!(text(&app), "a\nb\nc\nd\ne");

    // A selection ending at column 0 leaves that line where it is.
    place(&mut app, &[((1, 1), (3, 0))]);
    run_and_undo(&mut app, Command::EditMoveLinesUp, "b\nc\na\nd\ne");
    let selection = cursor(&app).selection.unwrap();
    assert_eq!(
        (selection.anchor, selection.active),
        (Position::new(0, 1), Position::new(2, 0))
    );

    // Two cursors move together; the top line cannot move further up.
    place(&mut app, &[((1, 0), (1, 0)), ((3, 0), (3, 0))]);
    run_and_undo(&mut app, Command::EditMoveLinesDown, "b\na\nc\ne\nd");
    assert_eq!(
        app.workbench.active_editor().unwrap().extra_cursors()[0].position,
        Position::new(4, 0)
    );
    place(&mut app, &[((0, 0), (0, 0))]);
    let before = text(&app);
    app.execute_command(Command::EditMoveLinesUp);
    assert_eq!(text(&app), before);
}

#[test]
fn deleting_lines_removes_every_cursor_line_including_the_last() {
    let (_home, mut app) = edited("one\ntwo\nthree\nfour");
    place(&mut app, &[((0, 2), (0, 2)), ((2, 1), (2, 1))]);
    run_and_undo(&mut app, Command::EditDeleteLine, "two\nfour");
    place(&mut app, &[((1, 2), (1, 2))]);
    app.execute_command(Command::EditDeleteLine);
    assert_eq!(text(&app), "two");
    assert_eq!(cursor(&app).position, Position::new(0, 2));
}

#[test]
fn indenting_follows_the_settings_and_tab_keys_indent_selected_lines() {
    let (_home, mut app) = edited("a\n\nb\n\tc");
    app.settings.set("editor.tabSize", serde_json::json!(4));
    // The tab-indented last line would otherwise make detection choose tabs.
    app.settings
        .set("editor.detectIndentation", serde_json::Value::Bool(false));
    place(&mut app, &[((0, 0), (2, 1))]);
    run_and_undo(&mut app, Command::EditIndentLine, "    a\n\n    b\n\tc");
    let selection = cursor(&app).selection.unwrap();
    assert_eq!(selection.active, Position::new(2, 5));
    app.execute_command(Command::EditOutdentLine);
    assert_eq!(text(&app), "a\n\nb\n\tc");
    place(&mut app, &[((3, 1), (3, 1))]);
    app.execute_command(Command::EditOutdentLine);
    assert_eq!(text(&app), "a\n\nb\nc");

    app.focus = Focus::Main;
    place(&mut app, &[((0, 0), (1, 0)), ((2, 0), (3, 1))]);
    app.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
    assert_eq!(text(&app), "    a\n\n    b\n    c");
    app.handle_key(KeyEvent::new(KeyCode::BackTab, KeyModifiers::SHIFT));
    assert_eq!(text(&app), "a\n\nb\nc");
    app.execute_command(Command::EditUndo);
    assert_eq!(text(&app), "    a\n\n    b\n    c");
}

#[test]
fn joining_lines_trims_the_seam_and_joins_selected_lines() {
    let (_home, mut app) = edited("let a =  \n    1 +\n  2;\nend");
    place(&mut app, &[((0, 0), (0, 0))]);
    run_and_undo(&mut app, Command::EditJoinLines, "let a = 1 +\n  2;\nend");
    assert_eq!(cursor(&app).position, Position::new(0, 8));
    place(&mut app, &[((0, 0), (1, 4))]);
    run_and_undo(&mut app, Command::EditJoinLines, "let a = 1 + 2;\nend");
    place(&mut app, &[((1, 0), (1, 0))]);
    let before = text(&app);
    app.execute_command(Command::EditJoinLines);
    assert_eq!(text(&app), before, "the last line has nothing to join");
}

#[test]
fn case_transforms_cover_selections_and_the_word_at_each_cursor() {
    let (_home, mut app) = edited("hello wide world\nstraße x");
    place(&mut app, &[((0, 6), (0, 16)), ((1, 2), (1, 2))]);
    run_and_undo(
        &mut app,
        Command::EditTransformToUppercase,
        "hello WIDE WORLD\nSTRASSE x",
    );
    let selection = cursor(&app).selection.unwrap();
    assert_eq!(
        (selection.anchor, selection.active),
        (Position::new(0, 6), Position::new(0, 16))
    );
    app.execute_command(Command::EditTransformToLowercase);
    assert_eq!(text(&app), "hello wide world\nstrasse x");
    place(&mut app, &[((0, 0), (0, 16))]);
    run_and_undo(
        &mut app,
        Command::EditTransformToTitleCase,
        "Hello Wide World\nstrasse x",
    );
    place(&mut app, &[((1, 7), (1, 7))]);
    app.status.clear();
    app.execute_command(Command::EditTransformToUppercase);
    assert!(app.status.contains("Select text"), "{}", app.status);
}

#[test]
fn sorting_uses_the_selection_or_the_whole_file_and_trimming_counts_lines() {
    let (_home, mut app) = edited("pear\nApple\nbanana\n");
    run_and_undo(
        &mut app,
        Command::EditorSortLinesAscending,
        "Apple\nbanana\npear\n",
    );
    assert!(app.status.contains("Sorted 3 line"), "{}", app.status);
    place(&mut app, &[((0, 0), (1, 6))]);
    run_and_undo(
        &mut app,
        Command::EditorSortLinesDescending,
        "banana\nApple\npear\n",
    );

    let (_home, mut app) = edited("a  \n\tb\t\nc");
    place(&mut app, &[((1, 3), (1, 3))]);
    run_and_undo(&mut app, Command::EditorTrimTrailingWhitespace, "a\n\tb\nc");
    assert_eq!(cursor(&app).position, Position::new(1, 2));
    assert!(app.status.contains("2 line"), "{}", app.status);
    app.execute_command(Command::EditorTrimTrailingWhitespace);
    assert!(
        app.status.contains("No trailing whitespace"),
        "{}",
        app.status
    );
}

#[test]
fn block_comments_use_the_language_delimiters_and_line_comments_add_or_remove() {
    let (home, mut app) = test_app();
    let path = home.path().join("workspace/lib.rs");
    std::fs::write(&path, "  let x = 1;\nlet y = 2;\n").unwrap();
    app.open_file(&path);
    run_and_undo(
        &mut app,
        Command::EditorToggleBlockComment,
        "  /* let x = 1; */\nlet y = 2;\n",
    );
    app.execute_command(Command::EditorToggleBlockComment);
    assert_eq!(text(&app), "  let x = 1;\nlet y = 2;\n");
    place(&mut app, &[((1, 4), (1, 5))]);
    app.execute_command(Command::EditorToggleBlockComment);
    assert_eq!(text(&app), "  let x = 1;\nlet /* y */ = 2;\n");
    let selection = cursor(&app).selection.unwrap();
    assert_eq!(selection.range().start, Position::new(1, 7));
    assert_eq!(selection.range().end, Position::new(1, 8));
    app.execute_command(Command::EditUndo);

    place(&mut app, &[((0, 0), (1, 3))]);
    run_and_undo(
        &mut app,
        Command::EditorCommentLine,
        "  // let x = 1;\n// let y = 2;\n",
    );
    app.execute_command(Command::EditorCommentLine);
    assert_eq!(text(&app), "  // // let x = 1;\n// // let y = 2;\n");
    app.execute_command(Command::EditorUncommentLine);
    app.execute_command(Command::EditorUncommentLine);
    assert_eq!(text(&app), "  let x = 1;\nlet y = 2;\n");
    app.execute_command(Command::EditorUncommentLine);
    assert_eq!(text(&app), "  let x = 1;\nlet y = 2;\n");

    let path = home.path().join("workspace/tool.py");
    std::fs::write(&path, "x = 1\n").unwrap();
    app.open_file(&path);
    app.execute_command(Command::EditorToggleBlockComment);
    assert_eq!(text(&app), "x = 1\n");
    assert!(
        app.status.contains(".py has no block comments"),
        "{}",
        app.status
    );
}

#[test]
fn duplicate_line_copies_lines_and_duplicate_selection_copies_the_selection() {
    let (_home, mut app) = edited("ab\ncd");
    place(&mut app, &[((0, 1), (0, 1))]);
    run_and_undo(&mut app, Command::EditorDuplicateLine, "ab\nab\ncd");
    assert_eq!(cursor(&app).position, Position::new(1, 1));
    place(&mut app, &[((2, 0), (2, 1))]);
    run_and_undo(&mut app, Command::EditorDuplicateSelection, "ab\nab\nccd");
    let selection = cursor(&app).selection.unwrap();
    assert_eq!(
        (selection.anchor, selection.active),
        (Position::new(2, 1), Position::new(2, 2))
    );
    place(&mut app, &[((0, 0), (0, 0))]);
    app.execute_command(Command::EditorDuplicateSelection);
    assert_eq!(text(&app), "ab\nab\nab\nccd");
}

#[test]
fn line_edits_refuse_read_only_help_pages() {
    let (_home, mut app) = test_app();
    app.execute_command(Command::HelpEditor);
    let before = text(&app);
    for command in [
        Command::EditMoveLinesDown,
        Command::EditDeleteLine,
        Command::EditIndentLine,
        Command::EditOutdentLine,
        Command::EditJoinLines,
        Command::EditTransformToUppercase,
        Command::EditorSortLinesAscending,
        Command::EditorTrimTrailingWhitespace,
        Command::EditorToggleBlockComment,
        Command::EditorToggleComment,
        Command::EditorDuplicateLine,
        Command::EditorDuplicateSelection,
    ] {
        app.status.clear();
        app.execute_command(command);
        assert_eq!(text(&app), before, "{command}");
        assert!(
            app.status.contains("is read-only"),
            "{command}: {}",
            app.status
        );
    }
}

#[test]
fn fold_levels_fold_regions_by_nesting_depth() {
    let (home, mut app) = test_app();
    let path = home.path().join("workspace/fold.rs");
    std::fs::write(
        &path,
        "fn a() {\n    if x {\n        y();\n    }\n}\nfn b() {\n    z();\n}\n",
    )
    .unwrap();
    app.open_file(&path);
    app.execute_command(Command::EditorFoldLevel1);
    assert_eq!(
        app.workbench.active_editor().unwrap().folded_ranges().len(),
        2
    );
    assert!(app.status.contains("Folded 2"), "{}", app.status);
    app.execute_command(Command::EditorUnfoldAll);
    app.execute_command(Command::EditorFoldLevel2);
    assert_eq!(
        app.workbench.active_editor().unwrap().folded_ranges(),
        [(1, 3)]
    );
    app.execute_command(Command::EditorFoldLevel3);
    assert!(app.status.contains("No regions"), "{}", app.status);
}

#[test]
fn minimap_and_zen_mode_toggle_and_restore_the_layout() {
    let (_home, mut app) = test_app();
    let shown = app.setting_bool("editor.minimap.enabled", true);
    app.execute_command(Command::WorkbenchToggleMinimap);
    assert_eq!(app.setting_bool("editor.minimap.enabled", true), !shown);
    assert_eq!(app.build_session().ui.minimap_enabled, !shown);
    app.execute_command(Command::WorkbenchToggleMinimap);
    assert_eq!(app.setting_bool("editor.minimap.enabled", true), shown);

    app.sidebar_visible = true;
    app.ai.agents_visible = false;
    app.open_terminal(false);
    app.focus = Focus::Dock;
    app.execute_command(Command::WorkbenchToggleZenMode);
    assert!(!app.sidebar_visible && !app.dock.visible && !app.ai.agents_visible);
    assert_eq!(app.focus, Focus::Main);
    let saved = app.build_session().ui;
    assert!(saved.sidebar_visible && saved.terminal_visible && !saved.agents_sidebar_visible);
    app.execute_command(Command::WorkbenchToggleZenMode);
    assert!(app.sidebar_visible && app.dock.visible && !app.ai.agents_visible);
    assert_eq!(app.focus, Focus::Dock);
}
