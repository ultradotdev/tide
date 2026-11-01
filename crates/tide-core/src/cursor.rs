use crate::Range;

/// Zero-based line and character (Unicode scalar) position.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default)]
pub struct Position {
    pub line: usize,
    pub column: usize,
}

impl Position {
    #[must_use]
    pub const fn new(line: usize, column: usize) -> Self {
        Self { line, column }
    }
}

/// Selection anchor and active end. The active end is where the cursor is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Selection {
    pub anchor: Position,
    pub active: Position,
}

impl Selection {
    #[must_use]
    pub const fn new(anchor: Position, active: Position) -> Self {
        Self { anchor, active }
    }

    #[must_use]
    pub fn range(self) -> Range {
        Range::new(self.anchor, self.active).normalized()
    }
}

/// Editor cursor with an optional selection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Cursor {
    pub position: Position,
    pub selection: Option<Selection>,
}

impl Cursor {
    #[must_use]
    pub const fn at(line: usize, column: usize) -> Self {
        Self {
            position: Position::new(line, column),
            selection: None,
        }
    }

    #[must_use]
    pub const fn with_selection(anchor: Position, active: Position) -> Self {
        Self {
            position: active,
            selection: Some(Selection::new(anchor, active)),
        }
    }

    #[must_use]
    pub fn range(self) -> Option<Range> {
        self.selection.map(Selection::range)
    }
}

impl Default for Cursor {
    fn default() -> Self {
        Self::at(0, 0)
    }
}

#[must_use]
pub fn compare_positions(a: Position, b: Position) -> std::cmp::Ordering {
    (a.line, a.column).cmp(&(b.line, b.column))
}

#[must_use]
pub fn min_max(a: Position, b: Position) -> (Position, Position) {
    if compare_positions(a, b).is_le() {
        (a, b)
    } else {
        (b, a)
    }
}

#[must_use]
pub fn get_selection_range(selection: Option<Selection>) -> Option<Range> {
    selection.map(Selection::range)
}
