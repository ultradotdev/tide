use super::*;

fn answer(app: &mut App, input: &str) {
    let base = app.overlay.as_mut().expect("a prompt").base_mut();
    base.input = input.to_owned();
    base.cursor = input.chars().count();
    app.handle_modal_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    app.settle();
}

fn replace_in_files(app: &mut App, query: &str, replacement: &str) {
    app.execute_command(Command::SearchReplaceInFiles);
    answer(app, query);
    answer(app, replacement);
}

#[test]
fn replace_in_files_previews_then_edits_open_buffers_and_writes_closed_files() {
    let (_home, mut app) = test_app();
    let root = app.workspace.clone();
    std::fs::create_dir_all(root.join("src")).unwrap();
    std::fs::write(root.join("src/closed.txt"), "needle and needle\n").unwrap();
    std::fs::write(root.join("open.txt"), "one needle\n").unwrap();
    std::fs::write(root.join(".ignore"), "skipped.txt\n").unwrap();
    std::fs::write(root.join("skipped.txt"), "needle\n").unwrap();
    std::fs::write(root.join("other.txt"), "nothing here\n").unwrap();
    app.open_file(root.join("open.txt"));

    replace_in_files(&mut app, "absent", "x");
    assert!(app.overlay.is_none());
    assert!(app.status.contains("no matches"), "{}", app.status);

    replace_in_files(&mut app, "needle", "pin");
    let message = app
        .overlay
        .as_ref()
        .and_then(|overlay| overlay.base().message.clone())
        .expect("the preview");
    assert!(message.contains("3 match(es) in 2 file(s)"), "{message}");
    assert!(message.contains("src/closed.txt (2)"), "{message}");
    assert!(!message.contains("skipped.txt"), "{message}");

    // Cancelling the preview changes nothing.
    app.handle_modal_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
    app.settle();
    assert_eq!(
        std::fs::read_to_string(root.join("src/closed.txt")).unwrap(),
        "needle and needle\n"
    );

    replace_in_files(&mut app, "needle", "pin");
    app.handle_modal_key(KeyEvent::new(KeyCode::Char('y'), KeyModifiers::NONE));
    app.settle();
    assert_eq!(
        std::fs::read_to_string(root.join("src/closed.txt")).unwrap(),
        "pin and pin\n"
    );
    assert_eq!(
        std::fs::read_to_string(root.join("skipped.txt")).unwrap(),
        "needle\n"
    );
    // The open file changed in its editor, not on disk, and Undo reverts it.
    assert_eq!(
        std::fs::read_to_string(root.join("open.txt")).unwrap(),
        "one needle\n"
    );
    let editor = app.workbench.active_editor_mut().unwrap();
    assert_eq!(editor.document.text(), "one pin\n");
    assert!(editor.is_dirty());
    editor.undo();
    assert_eq!(editor.document.text(), "one needle\n");
    assert!(
        app.status.contains("Replaced 3 match(es) in 2 file(s)"),
        "{}",
        app.status
    );
    assert!(app.status.contains("1 open file(s)"), "{}", app.status);
}

#[test]
fn replace_in_files_uses_the_find_options_and_skips_files_changed_after_the_preview() {
    let (_home, mut app) = test_app();
    let root = app.workspace.clone();
    std::fs::write(root.join("a.txt"), "user@example user@other\n").unwrap();
    std::fs::write(root.join("b.txt"), "User@example\n").unwrap();
    app.search_options.regex = true;
    app.search_options.case_sensitive = true;
    replace_in_files(&mut app, r"(\w+)@example", "$1 at example");
    assert!(app.overlay.as_ref().unwrap().base().title == "Replace in Files");
    std::fs::write(root.join("b.txt"), "changed\n").unwrap();
    app.handle_modal_key(KeyEvent::new(KeyCode::Char('y'), KeyModifiers::NONE));
    app.settle();
    assert_eq!(
        std::fs::read_to_string(root.join("a.txt")).unwrap(),
        "user at example user@other\n"
    );
    assert_eq!(
        std::fs::read_to_string(root.join("b.txt")).unwrap(),
        "changed\n"
    );
    assert!(app.status.contains("skipped"), "{}", app.status);
    assert!(app.status.contains("b.txt"), "{}", app.status);
}
