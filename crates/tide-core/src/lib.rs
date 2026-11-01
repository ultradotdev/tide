//! Core text-editing primitives for Tide.
//!
//! The TypeScript implementation used a custom piece table, cursor manager,
//! and undo stack. The Rust implementation starts from a rope and keeps the
//! same externally observable editing semantics.

mod cursor;
mod document;
pub mod fs;
pub mod num;
pub mod process;
mod undo;
mod width;

pub use cursor::{Cursor, Position, Selection, compare_positions, get_selection_range, min_max};
pub use document::{Document, OverlappingEdits, TextSnapshot};
pub use undo::{Edit, UndoManager};
pub use width::{display_width, grapheme_boundaries, grapheme_width, graphemes, truncate_to_width};

/// A half-open character range.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Range {
    pub start: Position,
    pub end: Position,
}

impl Range {
    #[must_use]
    pub const fn new(start: Position, end: Position) -> Self {
        Self { start, end }
    }

    #[must_use]
    pub fn normalized(self) -> Self {
        if compare_positions(self.start, self.end).is_le() {
            self
        } else {
            Self::new(self.end, self.start)
        }
    }

    #[must_use]
    pub fn is_empty(self) -> bool {
        self.start == self.end
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn position_ordering_is_line_then_column() {
        let a = Position::new(1, 5);
        let b = Position::new(2, 0);
        assert!(compare_positions(a, b).is_lt());
        assert_eq!(min_max(a, b), (a, b));
    }

    #[test]
    fn range_normalizes_reversed_selection() {
        let range = Range::new(Position::new(3, 2), Position::new(1, 4)).normalized();
        assert_eq!(range.start, Position::new(1, 4));
        assert_eq!(range.end, Position::new(3, 2));
    }
}
