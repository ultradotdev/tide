//! Large-document sanity: editing stays correct at production file sizes.

use tide_core::{Document, Position};

#[test]
fn edits_and_undo_at_fifty_thousand_lines() {
    let mut text = String::new();
    for index in 0..50_000 {
        text.push_str(&format!("line {index} with some representative content\n"));
    }
    let mut document = Document::new(&text);
    assert_eq!(document.len_lines(), 50_001);
    document.insert(Position::new(25_000, 0), "inserted\n");
    assert_eq!(document.len_lines(), 50_002);
    assert!(document.line(25_000).unwrap().starts_with("inserted"));
    assert!(document.undo());
    assert_eq!(document.len_lines(), 50_001);
    document.insert(Position::new(50_000, 0), "tail\n");
    assert!(document.text().ends_with("tail\n"));
}
