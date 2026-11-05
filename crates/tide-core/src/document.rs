use std::path::{Path, PathBuf};
use std::time::Duration;

use ropey::str_utils::byte_to_char_idx;
use ropey::{Rope, RopeSlice};
use similar::{DiffTag, TextDiff};
use unicode_segmentation::{GraphemeCursor, GraphemeIncomplete};

use crate::undo::{Edit, EditGroup, UndoEntry, UndoManager};
use crate::{Cursor, Position, Range, Selection, compare_positions, min_max};

/// Line breaks sampled when detecting a document's line ending.
const LINE_ENDING_SAMPLE_LINES: usize = 1_000;
const LINE_ENDING_SAMPLE_BYTES: usize = 256 * 1024;
const RELOAD_DIFF_TIMEOUT: Duration = Duration::from_millis(500);

/// A batch of edits whose ranges overlap, so no order of application is sound.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OverlappingEdits;

impl std::fmt::Display for OverlappingEdits {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("edits overlap")
    }
}

impl std::error::Error for OverlappingEdits {}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LineEnding {
    Lf,
    Crlf,
    Cr,
}

/// A cheap immutable rope snapshot for bounded background services.
#[derive(Debug, Clone)]
pub struct TextSnapshot(Rope);
impl TextSnapshot {
    pub fn len_lines(&self) -> usize {
        self.0.len_lines()
    }
    pub fn line_with_ending(&self, line: usize) -> Option<String> {
        (line < self.0.len_lines()).then(|| self.0.line(line).to_string())
    }
}

/// A UTF-8 text document backed by a rope.
#[derive(Debug, Clone)]
pub struct Document {
    rope: Rope,
    cursor: Cursor,
    undo: UndoManager,
    version: u64,
    dirty: bool,
    read_only: bool,
    lossy: bool,
    file_path: Option<PathBuf>,
    tracked_changes: Option<Vec<(usize, usize, usize)>>,
    /// The line ending for a document with no line breaks yet to go by.
    default_line_ending: LineEnding,
}

impl Document {
    #[must_use]
    pub fn new(text: impl Into<String>) -> Self {
        Self {
            rope: Rope::from_str(&text.into()),
            cursor: Cursor::default(),
            undo: UndoManager::default(),
            version: 1,
            dirty: false,
            read_only: false,
            lossy: false,
            file_path: None,
            tracked_changes: None,
            default_line_ending: LineEnding::Lf,
        }
    }

    pub fn from_path(path: impl AsRef<Path>) -> std::io::Result<Self> {
        let path = path.as_ref();
        let mut document = Self::from_bytes(std::fs::read(path)?);
        document.file_path = Some(path.to_path_buf());
        Ok(document)
    }

    /// Decode file bytes. Invalid UTF-8 is replaced with U+FFFD and the
    /// document is opened read-only so a save cannot corrupt the original.
    #[must_use]
    pub fn from_bytes(bytes: Vec<u8>) -> Self {
        match String::from_utf8(bytes) {
            Ok(text) => Self::new(text),
            Err(error) => {
                let mut document = Self::new(String::from_utf8_lossy(error.as_bytes()));
                document.lossy = true;
                document.read_only = true;
                document
            }
        }
    }

    #[must_use]
    pub fn is_read_only(&self) -> bool {
        self.read_only
    }

    /// True when the text was decoded lossily from invalid UTF-8.
    #[must_use]
    pub fn is_lossy(&self) -> bool {
        self.lossy
    }
    /// The encoding shown in the status bar: `UTF-8`, `UTF-8 with BOM` when the text
    /// starts with a byte-order mark (kept on save), or `None` for text decoded lossily,
    /// whose real encoding Tide doesn't know.
    #[must_use]
    pub fn encoding_label(&self) -> Option<&'static str> {
        if self.lossy {
            None
        } else if self.rope.get_char(0) == Some('\u{feff}') {
            Some("UTF-8 with BOM")
        } else {
            Some("UTF-8")
        }
    }

    pub fn set_read_only(&mut self, value: bool) {
        self.read_only = value;
    }

    pub fn text_snapshot(&self) -> TextSnapshot {
        TextSnapshot(self.rope.clone())
    }
    pub fn text(&self) -> String {
        self.rope.to_string()
    }

    #[must_use]
    pub fn len_chars(&self) -> usize {
        self.rope.len_chars()
    }

    #[must_use]
    pub fn len_lines(&self) -> usize {
        self.rope.len_lines()
    }

    /// Primary newline convention label (`LF`, `CRLF` or `CR`).
    pub fn line_ending(&self) -> &'static str {
        match self.detect_line_ending() {
            LineEnding::Lf => "LF",
            LineEnding::Crlf => "CRLF",
            LineEnding::Cr => "CR",
        }
    }

    /// The line break to insert so new lines match the document.
    pub fn line_break(&self) -> &'static str {
        match self.detect_line_ending() {
            LineEnding::Lf => "\n",
            LineEnding::Crlf => "\r\n",
            LineEnding::Cr => "\r",
        }
    }

    /// Sets the line break a document uses while it has none of its own, such as a
    /// new file under an `.editorconfig` that asks for CRLF. Unknown values are ignored.
    pub fn set_default_line_break(&mut self, line_break: &str) {
        self.default_line_ending = match line_break {
            "\r\n" => LineEnding::Crlf,
            "\r" => LineEnding::Cr,
            "\n" => LineEnding::Lf,
            _ => self.default_line_ending,
        };
    }

    /// Majority line ending over the first lines, scanning a bounded prefix.
    fn detect_line_ending(&self) -> LineEnding {
        let (mut lf, mut crlf, mut cr) = (0usize, 0usize, 0usize);
        let mut pending_cr = false;
        let mut scanned = 0;
        'chunks: for chunk in self.rope.chunks() {
            for &byte in chunk.as_bytes() {
                if pending_cr {
                    pending_cr = false;
                    if byte == b'\n' {
                        crlf += 1;
                        continue;
                    }
                    cr += 1;
                }
                match byte {
                    b'\r' => pending_cr = true,
                    b'\n' => lf += 1,
                    _ => {}
                }
                scanned += 1;
                if lf + crlf + cr >= LINE_ENDING_SAMPLE_LINES || scanned >= LINE_ENDING_SAMPLE_BYTES
                {
                    break 'chunks;
                }
            }
        }
        if pending_cr {
            cr += 1;
        }
        if lf + crlf + cr == 0 {
            return self.default_line_ending;
        }
        if crlf > lf && crlf >= cr {
            LineEnding::Crlf
        } else if cr > lf && cr > crlf {
            LineEnding::Cr
        } else {
            LineEnding::Lf
        }
    }

    /// A line without its trailing line break.
    fn line_content(&self, line: usize) -> Option<RopeSlice<'_>> {
        if line >= self.rope.len_lines() {
            return None;
        }
        let slice = self.rope.line(line);
        Some(slice.slice(..content_len(slice)))
    }

    /// Return a line without its trailing `\n`, `\r\n` or `\r`.
    #[must_use]
    pub fn line(&self, line: usize) -> Option<String> {
        self.line_content(line).map(|slice| slice.to_string())
    }

    /// Like [`Self::line`], but borrows the text when the rope stores the
    /// line contiguously.
    #[must_use]
    pub fn line_str(&self, line: usize) -> Option<std::borrow::Cow<'_, str>> {
        self.line_content(line).map(Into::into)
    }

    /// Bounded prefix for miniature overviews without copying an oversized line.
    pub fn line_prefix(&self, line: usize, limit: usize) -> String {
        if line >= self.rope.len_lines() {
            return String::new();
        }
        self.rope
            .line(line)
            .chars()
            .take(limit)
            .take_while(|c| !matches!(c, '\r' | '\n'))
            .collect()
    }

    #[must_use]
    pub fn lines(&self) -> Vec<String> {
        (0..self.rope.len_lines())
            .filter_map(|line| self.line(line))
            .collect()
    }

    #[must_use]
    pub fn line_len_chars(&self, line: usize) -> usize {
        self.line_content(line).map_or(0, |slice| slice.len_chars())
    }

    #[must_use]
    pub fn position_to_char(&self, position: Position) -> usize {
        let position = self.clamp_position(position);
        if position.line >= self.rope.len_lines() {
            return self.rope.len_chars();
        }
        let line_start = self.rope.line_to_char(position.line);
        (line_start + position.column).min(self.rope.len_chars())
    }

    #[must_use]
    pub fn char_to_position(&self, offset: usize) -> Position {
        let offset = offset.min(self.rope.len_chars());
        let line = self.rope.char_to_line(offset);
        let line_start = self.rope.line_to_char(line);
        Position::new(line, offset.saturating_sub(line_start))
    }

    #[must_use]
    pub fn clamp_position(&self, position: Position) -> Position {
        if self.rope.len_lines() == 0 {
            return Position::new(0, 0);
        }
        let line = position.line.min(self.rope.len_lines() - 1);
        let column = position.column.min(self.line_len_chars(line));
        Position::new(line, column)
    }

    #[must_use]
    pub fn cursor(&self) -> Cursor {
        self.cursor
    }

    pub fn set_cursor(&mut self, position: Position) {
        self.cursor = Cursor {
            position: self.clamp_position(position),
            selection: None,
        };
    }

    pub fn set_selection(&mut self, selection: Selection) {
        self.cursor = Cursor::with_selection(
            self.clamp_position(selection.anchor),
            self.clamp_position(selection.active),
        );
    }

    pub fn clear_selection(&mut self) {
        self.cursor.selection = None;
    }

    pub fn select_all(&mut self) {
        let start = Position::new(0, 0);
        let end = self.char_to_position(self.rope.len_chars());
        if start == end {
            self.cursor = Cursor::at(0, 0);
        } else {
            self.cursor = Cursor::with_selection(start, end);
        }
    }

    pub fn move_cursor_to(&mut self, position: Position, extend: bool) {
        let position = self.clamp_position(position);
        if extend {
            let anchor = self
                .cursor
                .selection
                .map_or(self.cursor.position, |selection| selection.anchor);
            self.cursor = Cursor::with_selection(anchor, position);
        } else {
            self.cursor = Cursor::at(position.line, position.column);
        }
    }

    #[must_use]
    pub fn selected_text(&self) -> Option<String> {
        let range = self.cursor.range()?;
        if range.is_empty() {
            return Some(String::new());
        }
        let start = self.position_to_char(range.start);
        let end = self.position_to_char(range.end);
        Some(self.rope.slice(start..end).to_string())
    }

    pub fn insert(&mut self, position: Position, text: &str) -> Range {
        let start = self.position_to_char(position);
        self.replace_char_range(start, start, text)
    }

    pub fn delete(&mut self, range: Range) -> Range {
        let range = range.normalized();
        let start = self.position_to_char(range.start);
        let end = self.position_to_char(range.end);
        self.replace_char_range(start, end, "")
    }

    pub fn replace(&mut self, range: Range, text: &str) -> Range {
        let range = range.normalized();
        let start = self.position_to_char(range.start);
        let end = self.position_to_char(range.end);
        self.replace_char_range(start, end, text)
    }

    /// Replace the whole text, recording only the changed lines for undo.
    /// Replace the text with freshly read file bytes as one undoable edit.
    /// Invalid UTF-8 is decoded lossily and leaves the document read-only;
    /// valid UTF-8 lifts the read-only state an earlier lossy load imposed.
    pub fn reload_bytes(&mut self, bytes: Vec<u8>) {
        let (text, lossy) = match String::from_utf8(bytes) {
            Ok(text) => (text, false),
            Err(error) => (String::from_utf8_lossy(error.as_bytes()).into_owned(), true),
        };
        if self.lossy {
            self.read_only = false;
        }
        self.set_content(&text);
        self.lossy = lossy;
        self.read_only |= lossy;
    }

    pub fn set_content(&mut self, text: &str) {
        if self.read_only {
            return;
        }
        let edits = line_diff(&self.rope.to_string(), text);
        if edits.is_empty() {
            return;
        }
        let before = self.cursor;
        let mut offset = self.position_to_char(before.position);
        let (recorded, _) = self.splice_sorted(edits);
        for edit in &recorded {
            let removed = edit.removed.chars().count();
            let inserted = edit.inserted.chars().count();
            if offset >= edit.start + removed {
                offset = offset + inserted - removed;
            } else if offset > edit.start {
                offset = edit.start + inserted;
            }
        }
        let position = self.char_to_position(offset);
        self.cursor = Cursor::at(position.line, position.column);
        self.version = self.version.saturating_add(1);
        self.undo.push_group(EditGroup {
            edits: recorded,
            before,
            after: self.cursor,
        });
    }

    /// Enable edit deltas for independent views. Off for standalone documents.
    pub fn track_changes(&mut self) {
        self.tracked_changes = Some(Vec::new());
    }

    pub fn edit_deltas(&self) -> &[(usize, usize, usize)] {
        self.tracked_changes.as_deref().unwrap_or(&[])
    }
    pub fn take_changes(&mut self) -> Vec<(usize, usize, usize)> {
        self.tracked_changes
            .as_mut()
            .map(std::mem::take)
            .unwrap_or_default()
    }

    fn record_change(&mut self, start: usize, removed: usize, inserted: usize) {
        if let Some(changes) = &mut self.tracked_changes {
            changes.push((start, removed, inserted));
        }
    }

    fn replace_char_range(&mut self, start: usize, end: usize, text: &str) -> Range {
        if self.read_only {
            return Range::new(self.cursor.position, self.cursor.position);
        }
        let start = start.min(self.rope.len_chars());
        let end = end.min(self.rope.len_chars()).max(start);
        let before = self.cursor;
        let removed = self.rope.slice(start..end).to_string();
        let inserted = text.to_owned();

        if start == end && inserted.is_empty() {
            return Range::new(before.position, before.position);
        }

        if end > start {
            self.rope.remove(start..end);
        }
        if !inserted.is_empty() {
            self.rope.insert(start, &inserted);
        }

        self.record_change(start, end - start, inserted.chars().count());
        let after_position = self.char_to_position(start + inserted.chars().count());
        self.cursor = Cursor::at(after_position.line, after_position.column);
        self.version = self.version.saturating_add(1);
        self.undo.push(Edit {
            start,
            removed,
            inserted,
            before,
            after: self.cursor,
        });

        Range::new(before.position, after_position)
    }

    /// Apply a batch of non-overlapping edits and return the resulting cursors
    /// in the same order as the input. The batch is one undo step. Overlapping
    /// ranges reject the whole batch and leave the document unchanged.
    pub fn apply_edits(
        &mut self,
        edits: &[(Range, String)],
        primary_index: usize,
    ) -> Result<Vec<Cursor>, OverlappingEdits> {
        if self.read_only {
            return Ok(vec![self.cursor]);
        }
        let before = self.cursor;
        let mut prepared = edits
            .iter()
            .enumerate()
            .map(|(index, (range, text))| {
                let range = range.normalized();
                (
                    self.position_to_char(range.start),
                    self.position_to_char(range.end),
                    text.clone(),
                    index,
                )
            })
            .collect::<Vec<_>>();
        prepared.sort_by_key(|(start, end, _, index)| (*start, *end, *index));
        let mut previous_end = 0;
        for (start, end, _, _) in &prepared {
            if *start < previous_end {
                return Err(OverlappingEdits);
            }
            previous_end = previous_end.max(*end);
        }
        let order = prepared.iter().map(|edit| edit.3).collect::<Vec<_>>();
        let (recorded, ends) = self.splice_sorted(
            prepared
                .into_iter()
                .map(|(start, end, text, _)| (start, end, text)),
        );
        let mut results = vec![Cursor::default(); edits.len()];
        for (index, end) in order.into_iter().zip(ends) {
            let position = self.char_to_position(end);
            results[index] = Cursor::at(position.line, position.column);
        }
        let primary = results.get(primary_index).copied().unwrap_or(before);
        self.cursor = primary;
        if !recorded.is_empty() {
            self.version = self.version.saturating_add(1);
            self.undo.push_group(EditGroup {
                edits: recorded,
                before,
                after: primary,
            });
        }
        Ok(results)
    }

    /// Apply sorted, non-overlapping character edits given in original
    /// offsets. Returns the recorded edits and each edit's final end offset.
    fn splice_sorted(
        &mut self,
        edits: impl IntoIterator<Item = (usize, usize, String)>,
    ) -> (Vec<Edit>, Vec<usize>) {
        let mut recorded = Vec::new();
        let mut ends = Vec::new();
        // Earlier edits shift later offsets by `inserted_total - removed_total`.
        // Every earlier removal ends at or before `start`, so the subtraction
        // cannot underflow.
        let mut inserted_total = 0usize;
        let mut removed_total = 0usize;
        for (start, end, inserted) in edits {
            let removed_len = end - start;
            let start = start
                .saturating_add(inserted_total)
                .saturating_sub(removed_total);
            let inserted_len = inserted.chars().count();
            ends.push(start + inserted_len);
            if removed_len == 0 && inserted.is_empty() {
                continue;
            }
            let removed = self.rope.slice(start..start + removed_len).to_string();
            if removed_len > 0 {
                self.rope.remove(start..start + removed_len);
            }
            if !inserted.is_empty() {
                self.rope.insert(start, &inserted);
            }
            self.record_change(start, removed_len, inserted_len);
            let position = self.char_to_position(start + inserted_len);
            let cursor = Cursor::at(position.line, position.column);
            recorded.push(Edit {
                start,
                removed,
                inserted,
                before: cursor,
                after: cursor,
            });
            inserted_total += inserted_len;
            removed_total += removed_len;
        }
        (recorded, ends)
    }

    pub fn undo(&mut self) -> bool {
        if self.read_only {
            return false;
        }
        let Some(revision) = self.undo.pop_undo_entry() else {
            return false;
        };
        match &revision.entry {
            UndoEntry::Single(edit) => {
                self.revert_edit(edit);
                self.cursor = edit.before;
            }
            UndoEntry::Group(group) => {
                for edit in group.edits.iter().rev() {
                    self.revert_edit(edit);
                }
                self.cursor = group.before;
            }
        }
        self.undo.push_redo_entry(revision);
        self.version = self.version.saturating_add(1);
        true
    }

    pub fn redo(&mut self) -> bool {
        if self.read_only {
            return false;
        }
        let Some(revision) = self.undo.pop_redo_entry() else {
            return false;
        };
        match &revision.entry {
            UndoEntry::Single(edit) => {
                self.reapply_edit(edit);
                self.cursor = edit.after;
            }
            UndoEntry::Group(group) => {
                for edit in &group.edits {
                    self.reapply_edit(edit);
                }
                self.cursor = group.after;
            }
        }
        self.undo.push_undo_entry(revision);
        self.version = self.version.saturating_add(1);
        true
    }

    fn revert_edit(&mut self, edit: &Edit) {
        self.record_change(
            edit.start,
            edit.inserted.chars().count(),
            edit.removed.chars().count(),
        );
        let inserted_len = edit.inserted.chars().count();
        if inserted_len > 0 {
            self.rope
                .remove(edit.start..(edit.start + inserted_len).min(self.rope.len_chars()));
        }
        if !edit.removed.is_empty() {
            self.rope.insert(edit.start, &edit.removed);
        }
    }

    fn reapply_edit(&mut self, edit: &Edit) {
        self.record_change(
            edit.start,
            edit.removed.chars().count(),
            edit.inserted.chars().count(),
        );
        let inserted_len = edit.inserted.chars().count();
        if !edit.removed.is_empty() {
            self.rope.remove(
                edit.start..(edit.start + edit.removed.chars().count()).min(self.rope.len_chars()),
            );
        }
        if inserted_len > 0 {
            self.rope.insert(edit.start, &edit.inserted);
        }
    }

    pub fn set_undo_limit(&mut self, limit: usize) {
        self.undo.set_limit(limit);
    }
    #[must_use]
    pub fn can_undo(&self) -> bool {
        self.undo.can_undo()
    }

    #[must_use]
    pub fn can_redo(&self) -> bool {
        self.undo.can_redo()
    }

    pub fn clear_undo_history(&mut self) {
        self.undo.clear();
    }

    #[must_use]
    pub fn version(&self) -> u64 {
        self.version
    }

    /// True when the text differs from the last saved revision, or the
    /// document was explicitly marked dirty since.
    #[must_use]
    pub fn is_dirty(&self) -> bool {
        self.dirty || !self.undo.is_at_saved()
    }

    pub fn mark_clean(&mut self) {
        self.dirty = false;
        self.undo.mark_saved();
    }

    pub fn mark_dirty(&mut self) {
        self.dirty = true;
    }

    #[must_use]
    pub fn file_path(&self) -> Option<&Path> {
        self.file_path.as_deref()
    }

    pub fn set_file_path(&mut self, path: Option<PathBuf>) {
        self.file_path = path;
    }

    /// Return the text in a normalized range.
    #[must_use]
    pub fn text_in_range(&self, range: Range) -> String {
        let range = range.normalized();
        let start = self.position_to_char(range.start);
        let end = self.position_to_char(range.end);
        self.rope.slice(start..end).to_string()
    }

    /// Return the word at a position using simple ASCII/Unicode word boundaries.
    #[must_use]
    pub fn word_at(&self, position: Position) -> Option<Range> {
        let line = self.line(position.line)?;
        let chars: Vec<char> = line.chars().collect();
        if chars.is_empty() {
            return None;
        }
        let column = position.column.min(chars.len().saturating_sub(1));
        if !chars[column].is_alphanumeric() && chars[column] != '_' {
            return None;
        }
        let mut start = column;
        while start > 0 && (chars[start - 1].is_alphanumeric() || chars[start - 1] == '_') {
            start -= 1;
        }
        let mut end = column + 1;
        while end < chars.len() && (chars[end].is_alphanumeric() || chars[end] == '_') {
            end += 1;
        }
        Some(Range::new(
            Position::new(position.line, start),
            Position::new(position.line, end),
        ))
    }

    /// The column one grapheme cluster before `position` on its line.
    #[must_use]
    pub fn prev_grapheme_column(&self, position: Position) -> usize {
        self.line_content(position.line).map_or(0, |content| {
            prev_grapheme_boundary(content, position.column.min(content.len_chars()))
        })
    }

    /// The column one grapheme cluster after `position` on its line.
    #[must_use]
    pub fn next_grapheme_column(&self, position: Position) -> usize {
        self.line_content(position.line).map_or(0, |content| {
            next_grapheme_boundary(content, position.column)
        })
    }

    /// Move one grapheme cluster left, collapsing a selection to its start.
    pub fn move_left(&mut self, extend: bool) -> bool {
        if !extend && let Some(range) = self.cursor.range() {
            self.cursor = Cursor::at(range.start.line, range.start.column);
            return true;
        }
        let mut position = self.cursor.position;
        if position.column > 0 {
            let Some(content) = self.line_content(position.line) else {
                return false;
            };
            position.column =
                prev_grapheme_boundary(content, position.column.min(content.len_chars()));
        } else if position.line > 0 {
            position.line -= 1;
            position.column = self.line_len_chars(position.line);
        } else {
            return false;
        }
        self.move_cursor_to(position, extend);
        true
    }

    /// Move one grapheme cluster right, collapsing a selection to its end.
    pub fn move_right(&mut self, extend: bool) -> bool {
        if !extend && let Some(range) = self.cursor.range() {
            self.cursor = Cursor::at(range.end.line, range.end.column);
            return true;
        }
        let mut position = self.cursor.position;
        let Some(content) = self.line_content(position.line) else {
            return false;
        };
        if position.column < content.len_chars() {
            position.column = next_grapheme_boundary(content, position.column);
        } else if position.line + 1 < self.rope.len_lines() {
            position.line += 1;
            position.column = 0;
        } else {
            return false;
        }
        self.move_cursor_to(position, extend);
        true
    }

    /// Move vertically while preserving the desired column where possible,
    /// never landing inside a grapheme cluster.
    pub fn move_vertical(&mut self, delta: isize, extend: bool) -> bool {
        let Some(line) = self.cursor.position.line.checked_add_signed(delta) else {
            return false;
        };
        if line >= self.rope.len_lines() {
            return false;
        }
        let Some(content) = self.line_content(line) else {
            return false;
        };
        let mut column = self.cursor.position.column.min(content.len_chars());
        if column < content.len_chars() {
            column = prev_grapheme_boundary(content, column + 1);
        }
        self.move_cursor_to(Position::new(line, column), extend);
        true
    }

    /// Return start/end positions for the current selection, if any.
    #[must_use]
    pub fn selection_positions(&self) -> Option<(Position, Position)> {
        self.cursor.range().map(|range| (range.start, range.end))
    }

    /// True when `position` is strictly before `other`.
    #[must_use]
    pub fn before(a: Position, b: Position) -> bool {
        compare_positions(a, b).is_lt()
    }

    /// Return the earlier and later of two positions.
    #[must_use]
    pub fn order(a: Position, b: Position) -> (Position, Position) {
        min_max(a, b)
    }
}

/// Characters in a rope line before its line break.
fn content_len(line: RopeSlice<'_>) -> usize {
    let len = line.len_chars();
    if len == 0 {
        return 0;
    }
    match line.char(len - 1) {
        '\n' if len >= 2 && line.char(len - 2) == '\r' => len - 2,
        '\n' | '\r' => len - 1,
        _ => len,
    }
}

fn next_grapheme_boundary(slice: RopeSlice<'_>, char_idx: usize) -> usize {
    if char_idx >= slice.len_chars() {
        return slice.len_chars();
    }
    let byte_idx = slice.char_to_byte(char_idx);
    let (mut chunk, mut chunk_byte, mut chunk_char, _) = slice.chunk_at_byte(byte_idx);
    let mut cursor = GraphemeCursor::new(byte_idx, slice.len_bytes(), true);
    loop {
        match cursor.next_boundary(chunk, chunk_byte) {
            Ok(None) => return slice.len_chars(),
            Ok(Some(byte)) => return chunk_char + byte_to_char_idx(chunk, byte - chunk_byte),
            Err(GraphemeIncomplete::NextChunk) => {
                (chunk, chunk_byte, chunk_char, _) = slice.chunk_at_byte(chunk_byte + chunk.len());
            }
            Err(GraphemeIncomplete::PreContext(byte)) => {
                let (context, context_byte, _, _) = slice.chunk_at_byte(byte - 1);
                cursor.provide_context(context, context_byte);
            }
            Err(_) => return char_idx + 1,
        }
    }
}

fn prev_grapheme_boundary(slice: RopeSlice<'_>, char_idx: usize) -> usize {
    if char_idx == 0 {
        return 0;
    }
    let byte_idx = slice.char_to_byte(char_idx);
    let (mut chunk, mut chunk_byte, mut chunk_char, _) = slice.chunk_at_byte(byte_idx);
    let mut cursor = GraphemeCursor::new(byte_idx, slice.len_bytes(), true);
    loop {
        match cursor.prev_boundary(chunk, chunk_byte) {
            Ok(None) => return 0,
            Ok(Some(byte)) => return chunk_char + byte_to_char_idx(chunk, byte - chunk_byte),
            Err(GraphemeIncomplete::PrevChunk) => {
                (chunk, chunk_byte, chunk_char, _) = slice.chunk_at_byte(chunk_byte - 1);
            }
            Err(GraphemeIncomplete::PreContext(byte)) => {
                let (context, context_byte, _, _) = slice.chunk_at_byte(byte - 1);
                cursor.provide_context(context, context_byte);
            }
            Err(_) => return char_idx - 1,
        }
    }
}

/// Line-level diff of `old` into `new` as sorted `(start, end, text)` char edits.
fn line_diff(old: &str, new: &str) -> Vec<(usize, usize, String)> {
    let diff = TextDiff::configure()
        .timeout(RELOAD_DIFF_TIMEOUT)
        .diff_lines(old, new);
    let old_lines: Vec<&str> = diff.iter_old_slices().collect();
    let new_lines: Vec<&str> = diff.iter_new_slices().collect();
    let mut edits: Vec<(usize, usize, String)> = Vec::new();
    let mut offset = 0;
    for op in diff.ops() {
        let removed: usize = old_lines[op.old_range()]
            .iter()
            .map(|line| line.chars().count())
            .sum();
        if op.tag() != DiffTag::Equal {
            let inserted = new_lines[op.new_range()].concat();
            match edits.last_mut() {
                Some(last) if last.1 == offset => {
                    last.1 += removed;
                    last.2.push_str(&inserted);
                }
                _ => edits.push((offset, offset + removed, inserted)),
            }
        }
        offset += removed;
    }
    edits
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encoding_label_names_a_byte_order_mark_and_gives_none_for_lossy_text() {
        assert_eq!(Document::new("plain").encoding_label(), Some("UTF-8"));
        assert_eq!(Document::new("").encoding_label(), Some("UTF-8"));
        let bom = Document::from_bytes(b"\xEF\xBB\xBFtext".to_vec());
        assert_eq!(bom.encoding_label(), Some("UTF-8 with BOM"));
        assert_eq!(
            bom.text().as_bytes(),
            b"\xEF\xBB\xBFtext",
            "the mark is kept"
        );
        let latin1 = Document::from_bytes(b"caf\xE9".to_vec());
        assert_eq!(latin1.encoding_label(), None);
    }
    use crate::{display_width, grapheme_boundaries};

    #[test]
    fn insert_delete_and_undo_redo() {
        let mut doc = Document::new("hello\nworld\n");
        doc.insert(Position::new(0, 5), "!");
        assert_eq!(doc.text(), "hello!\nworld\n");
        assert_eq!(doc.cursor().position, Position::new(0, 6));
        assert!(doc.can_undo());

        assert!(doc.undo());
        assert_eq!(doc.text(), "hello\nworld\n");
        assert!(doc.can_redo());

        assert!(doc.redo());
        assert_eq!(doc.text(), "hello!\nworld\n");
    }

    #[test]
    fn delete_range_is_one_undo_step() {
        let mut doc = Document::new("abcdef\n");
        doc.delete(Range::new(Position::new(0, 1), Position::new(0, 4)));
        assert_eq!(doc.text(), "aef\n");
        assert!(doc.undo());
        assert_eq!(doc.text(), "abcdef\n");
    }

    #[test]
    fn positions_round_trip() {
        let doc = Document::new("one\ntwo\nthree");
        for line in 0..doc.len_lines() {
            for column in 0..=doc.line_len_chars(line) {
                let position = Position::new(line, column);
                let offset = doc.position_to_char(position);
                assert_eq!(doc.char_to_position(offset), position);
            }
        }
    }

    #[test]
    fn unicode_positions_and_widths_are_preserved() {
        let mut doc = Document::new("a👩‍💻b\n漢字\n");
        assert_eq!(doc.line_len_chars(0), 5);
        assert_eq!(display_width(&doc.line(0).unwrap()), 4);
        assert_eq!(grapheme_boundaries(&doc.line(0).unwrap()), vec![0, 1, 4, 5]);
        doc.insert(Position::new(0, 4), "!");
        assert_eq!(doc.text(), "a👩‍💻!b\n漢字\n");
        doc.undo();
        assert_eq!(doc.text(), "a👩‍💻b\n漢字\n");
    }

    #[test]
    fn select_all_and_selected_text() {
        let mut doc = Document::new("alpha\nbeta\n");
        doc.select_all();
        assert_eq!(doc.selected_text().as_deref(), Some("alpha\nbeta\n"));
        assert!(doc.cursor().selection.is_some());
    }

    #[test]
    fn multi_cursor_edits_are_one_undo_step() {
        let mut doc = Document::new("one\ntwo\nthree\n");
        let cursors = doc
            .apply_edits(
                &[
                    (
                        Range::new(Position::new(0, 0), Position::new(0, 0)),
                        "X".to_owned(),
                    ),
                    (
                        Range::new(Position::new(1, 0), Position::new(1, 0)),
                        "X".to_owned(),
                    ),
                    (
                        Range::new(Position::new(2, 0), Position::new(2, 0)),
                        "X".to_owned(),
                    ),
                ],
                1,
            )
            .expect("disjoint edits");
        assert_eq!(doc.text(), "Xone\nXtwo\nXthree\n");
        assert_eq!(
            cursors,
            vec![Cursor::at(0, 1), Cursor::at(1, 1), Cursor::at(2, 1)]
        );
        assert_eq!(doc.cursor(), Cursor::at(1, 1));
        assert!(doc.undo());
        assert_eq!(doc.text(), "one\ntwo\nthree\n");
        assert!(doc.redo());
        assert_eq!(doc.text(), "Xone\nXtwo\nXthree\n");
        assert!(doc.undo());

        let cursors = doc
            .apply_edits(
                &[
                    (
                        Range::new(Position::new(0, 0), Position::new(0, 3)),
                        "1".to_owned(),
                    ),
                    (
                        Range::new(Position::new(2, 0), Position::new(2, 5)),
                        "3".to_owned(),
                    ),
                ],
                0,
            )
            .expect("disjoint edits");
        assert_eq!(doc.text(), "1\ntwo\n3\n");
        assert_eq!(cursors, vec![Cursor::at(0, 1), Cursor::at(2, 1)]);
        assert!(doc.undo());
        assert_eq!(doc.text(), "one\ntwo\nthree\n");
    }

    #[test]
    fn unicode_separators_do_not_shift_lsp_lines() {
        for separator in ['\u{2028}', '\u{2029}', '\u{85}', '\u{0B}', '\u{0C}'] {
            let doc = Document::new(format!("let s = \"a{separator}b\";\nnext\n"));
            assert_eq!(doc.len_lines(), 3, "{separator:?}");
            assert_eq!(doc.line(1).as_deref(), Some("next"), "{separator:?}");
            assert_eq!(doc.char_to_position(doc.len_chars()), Position::new(2, 0));
        }
    }

    #[test]
    fn lone_carriage_return_is_a_stripped_line_break() {
        let doc = Document::new("a\rb\r\nc\nd");
        assert_eq!(doc.lines(), vec!["a", "b", "c", "d"]);
        assert_eq!(doc.line_len_chars(0), 1);
        assert_eq!(doc.line_len_chars(1), 1);
        assert_eq!(doc.clamp_position(Position::new(0, 9)), Position::new(0, 1));
    }

    #[test]
    fn cursor_movement_on_a_huge_single_line_is_fast() {
        let mut doc = Document::new("x".repeat(5 * 1024 * 1024));
        doc.set_cursor(Position::new(0, 2_500_000));
        let started = std::time::Instant::now();
        for _ in 0..1_000 {
            doc.move_right(false);
        }
        for _ in 0..1_000 {
            doc.move_left(false);
        }
        let elapsed = started.elapsed();
        assert_eq!(doc.cursor().position, Position::new(0, 2_500_000));
        assert!(
            elapsed.as_millis() < 1_000,
            "2k cursor moves on a 5 MB line took {elapsed:?}"
        );
    }

    #[test]
    fn cursor_steps_over_grapheme_clusters() {
        let mut doc = Document::new("a👩‍💻e\u{301}b\nz");
        doc.set_cursor(Position::new(0, 1));
        assert!(doc.move_right(false));
        assert_eq!(doc.cursor().position, Position::new(0, 4));
        assert!(doc.move_right(false));
        assert_eq!(doc.cursor().position, Position::new(0, 6));
        assert!(doc.move_left(false));
        assert_eq!(doc.cursor().position, Position::new(0, 4));
        assert!(doc.move_left(false));
        assert_eq!(doc.cursor().position, Position::new(0, 1));
        doc.set_cursor(Position::new(1, 1));
        assert!(doc.move_left(false));
        assert!(doc.move_left(false));
        assert_eq!(doc.cursor().position, Position::new(0, 7));

        let mut doc = Document::new("abcdef\na👩‍💻b");
        doc.set_cursor(Position::new(0, 2));
        assert!(doc.move_vertical(1, false));
        assert_eq!(doc.cursor().position, Position::new(1, 1));
        doc.set_cursor(Position::new(0, 4));
        assert!(doc.move_vertical(1, false));
        assert_eq!(doc.cursor().position, Position::new(1, 4));
    }

    #[test]
    fn typing_a_word_is_one_undo_step() {
        let mut doc = Document::new("start ");
        doc.set_cursor(Position::new(0, 6));
        for ch in ["h", "e", "l", "l", "o"] {
            let position = doc.cursor().position;
            doc.insert(position, ch);
        }
        assert_eq!(doc.text(), "start hello");
        assert!(doc.undo());
        assert_eq!(doc.text(), "start ");
        assert!(!doc.can_undo());
        assert!(doc.redo());
        assert_eq!(doc.text(), "start hello");
        assert_eq!(doc.cursor().position, Position::new(0, 11));
    }

    #[test]
    fn backspacing_a_word_is_one_undo_step() {
        let mut doc = Document::new("one two");
        doc.set_cursor(Position::new(0, 7));
        for _ in 0..3 {
            let end = doc.cursor().position;
            doc.delete(Range::new(Position::new(0, end.column - 1), end));
        }
        assert_eq!(doc.text(), "one ");
        assert!(doc.undo());
        assert_eq!(doc.text(), "one two");
        assert!(!doc.can_undo());
    }

    #[test]
    fn newline_breaks_a_typing_run() {
        let mut doc = Document::new("");
        for ch in ["a", "b", "\n", "c"] {
            let position = doc.cursor().position;
            doc.insert(position, ch);
        }
        assert!(doc.undo());
        assert_eq!(doc.text(), "ab\n");
        assert!(doc.undo());
        assert_eq!(doc.text(), "ab");
        assert!(doc.undo());
        assert_eq!(doc.text(), "");
    }

    #[test]
    fn undo_back_to_the_saved_point_clears_dirty() {
        let mut doc = Document::new("base");
        doc.insert(Position::new(0, 4), "\n");
        doc.insert(Position::new(1, 0), "a");
        doc.mark_clean();
        doc.insert(Position::new(1, 1), "b");
        assert!(doc.is_dirty());
        assert!(doc.undo());
        assert_eq!(doc.text(), "base\na");
        assert!(!doc.is_dirty());
        assert!(doc.undo());
        assert!(doc.is_dirty());
        assert!(doc.redo());
        assert!(!doc.is_dirty());
        assert!(doc.redo());
        assert!(doc.is_dirty());
        assert!(doc.undo());
        assert!(!doc.is_dirty());
    }

    #[test]
    fn line_ending_is_the_majority_of_the_first_lines() {
        assert_eq!(Document::new("a\nb\r\nc\r\nd\r\n").line_ending(), "CRLF");
        assert_eq!(Document::new("a\r\nb\nc\nd\n").line_ending(), "LF");
        assert_eq!(Document::new("a\rb\rc\n").line_ending(), "CR");
        assert_eq!(Document::new("single").line_ending(), "LF");
    }

    #[test]
    fn crlf_document_round_trips() {
        let original = "one\r\ntwo\r\n";
        let mut doc = Document::new(original);
        assert_eq!(doc.lines(), vec!["one", "two", ""]);
        doc.insert(Position::new(0, 3), "!");
        doc.delete(Range::new(Position::new(1, 0), Position::new(1, 3)));
        assert_eq!(doc.text(), "one!\r\n\r\n");
        assert!(doc.undo());
        assert!(doc.undo());
        assert_eq!(doc.text(), original);
    }

    #[test]
    fn overlapping_edits_reject_the_whole_batch() {
        let mut doc = Document::new("abcdef\n");
        doc.set_cursor(Position::new(0, 6));
        let result = doc.apply_edits(
            &[
                (
                    Range::new(Position::new(0, 0), Position::new(0, 3)),
                    "X".to_owned(),
                ),
                (
                    Range::new(Position::new(0, 2), Position::new(0, 4)),
                    "Y".to_owned(),
                ),
            ],
            0,
        );
        assert_eq!(result, Err(OverlappingEdits));
        assert_eq!(doc.text(), "abcdef\n");
        assert_eq!(doc.cursor(), Cursor::at(0, 6));
        assert!(!doc.can_undo());
        assert!(!doc.is_dirty());

        let nested = doc.apply_edits(
            &[
                (
                    Range::new(Position::new(0, 0), Position::new(0, 5)),
                    String::new(),
                ),
                (
                    Range::new(Position::new(0, 1), Position::new(0, 1)),
                    "!".to_owned(),
                ),
                (
                    Range::new(Position::new(0, 3), Position::new(0, 4)),
                    String::new(),
                ),
            ],
            0,
        );
        assert_eq!(nested, Err(OverlappingEdits));
        assert_eq!(doc.text(), "abcdef\n");
    }

    #[test]
    fn empty_edits_report_positions_after_earlier_edits() {
        let mut doc = Document::new("ab\n");
        let cursors = doc
            .apply_edits(
                &[
                    (
                        Range::new(Position::new(0, 0), Position::new(0, 0)),
                        "\n\n".to_owned(),
                    ),
                    (
                        Range::new(Position::new(0, 1), Position::new(0, 1)),
                        String::new(),
                    ),
                ],
                1,
            )
            .expect("disjoint edits");
        assert_eq!(doc.text(), "\n\nab\n");
        assert_eq!(cursors, vec![Cursor::at(2, 0), Cursor::at(2, 1)]);
    }

    #[test]
    fn a_document_without_line_breaks_uses_the_default() {
        let mut doc = Document::new("one line");
        doc.set_default_line_break("\r\n");
        assert_eq!((doc.line_break(), doc.line_ending()), ("\r\n", "CRLF"));
        doc.set_default_line_break("bogus");
        assert_eq!(doc.line_break(), "\r\n");
        let mut lf = Document::new("a\nb\n");
        lf.set_default_line_break("\r\n");
        assert_eq!(lf.line_break(), "\n", "the file's own line breaks win");
    }

    #[test]
    fn line_break_matches_the_document() {
        assert_eq!(Document::new("a\r\nb\r\n").line_break(), "\r\n");
        assert_eq!(Document::new("a\nb\n").line_break(), "\n");
        assert_eq!(Document::new("a\rb\r").line_break(), "\r");
        let mut doc = Document::new("one\r\ntwo\r\n");
        let line_break = doc.line_break();
        doc.insert(Position::new(0, 3), line_break);
        assert_eq!(doc.text(), "one\r\n\r\ntwo\r\n");
        assert_eq!(doc.lines(), vec!["one", "", "two", ""]);
    }

    #[test]
    fn reloading_bytes_tracks_lossy_state() {
        let mut doc = Document::new("one\n");
        doc.reload_bytes(b"caf\xe9\n".to_vec());
        assert!(doc.is_lossy() && doc.is_read_only());
        assert_eq!(doc.text(), "caf\u{FFFD}\n");
        doc.reload_bytes(b"caf\xc3\xa9\n".to_vec());
        assert!(!doc.is_lossy() && !doc.is_read_only());
        assert_eq!(doc.text(), "caf\u{e9}\n");
        doc.set_read_only(true);
        doc.reload_bytes(b"other\n".to_vec());
        assert_eq!(doc.text(), "caf\u{e9}\n");
    }

    #[test]
    fn grapheme_columns_step_over_whole_clusters() {
        let doc = Document::new("a\u{1F469}\u{200D}\u{1F4BB}b");
        assert_eq!(doc.prev_grapheme_column(Position::new(0, 4)), 1);
        assert_eq!(doc.next_grapheme_column(Position::new(0, 1)), 4);
        assert_eq!(doc.prev_grapheme_column(Position::new(0, 1)), 0);
        assert_eq!(doc.next_grapheme_column(Position::new(0, 5)), 5);
    }

    #[test]
    fn invalid_utf8_opens_lossy_and_read_only() {
        let doc = Document::from_bytes(b"ok\n\xff\xfebad\n".to_vec());
        assert!(doc.is_lossy());
        assert!(doc.is_read_only());
        assert_eq!(doc.line(1).as_deref(), Some("\u{FFFD}\u{FFFD}bad"));

        let valid = Document::from_bytes(b"fine\n".to_vec());
        assert!(!valid.is_lossy());
        assert!(!valid.is_read_only());

        let path = std::env::temp_dir().join(format!(
            "tide-core-lossy-{}-{:?}.txt",
            std::process::id(),
            std::thread::current().id()
        ));
        std::fs::write(&path, b"\xc3(\n").unwrap();
        let loaded = Document::from_path(&path);
        std::fs::remove_file(&path).unwrap();
        let mut loaded = loaded.expect("non-UTF-8 files still open");
        assert!(loaded.is_lossy());
        assert_eq!(loaded.file_path(), Some(path.as_path()));
        loaded.insert(Position::new(0, 0), "x");
        assert_eq!(loaded.text(), "\u{FFFD}(\n");
    }

    #[test]
    fn reload_stores_a_line_diff_and_round_trips() {
        let old = (0..2_000)
            .map(|index| format!("line {index}\r\n"))
            .collect::<String>();
        let new = old.replace("line 1000\r\n", "changed\r\n");
        let mut doc = Document::new(old.clone());
        doc.set_cursor(Position::new(1_500, 2));
        doc.set_content(&new);
        assert_eq!(doc.text(), new);
        assert_eq!(doc.cursor().position, Position::new(1_500, 2));
        assert!(doc.undo.undo_bytes() < 1_024, "{}", doc.undo.undo_bytes());
        doc.mark_clean();
        assert!(doc.undo());
        assert_eq!(doc.text(), old);
        assert!(doc.is_dirty());
        assert!(doc.redo());
        assert_eq!(doc.text(), new);
        assert!(!doc.is_dirty());

        doc.set_content(&new);
        assert!(!doc.is_dirty());
        doc.set_content("");
        assert_eq!(doc.text(), "");
        assert!(doc.undo());
        assert_eq!(doc.text(), new);
    }

    #[test]
    fn large_document_edits_stay_bounded() {
        let mut text = String::new();
        for index in 0..100_000 {
            text.push_str("let value_");
            text.push_str(&index.to_string());
            text.push_str(" = 0;\n");
        }
        let mut doc = Document::new(text);
        assert_eq!(doc.len_lines(), 100_001);

        let started = std::time::Instant::now();
        for index in 0..1_000 {
            doc.insert(Position::new(index, 0), "// spike\n");
        }
        let elapsed = started.elapsed();
        assert_eq!(doc.len_lines(), 101_001);
        assert!(
            elapsed.as_millis() < 5_000,
            "1k edits should complete quickly, took {elapsed:?}"
        );
    }
}
