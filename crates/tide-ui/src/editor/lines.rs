//! Line and selection edits for every cursor. Each edit is one undo step and leaves
//! read-only buffers untouched.
use tide_core::{Cursor, Document, Position, Range};

use super::EditorState;

/// How a case transform rewrites text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CaseTransform {
    Upper,
    Lower,
    /// Upper-cases the first letter of each word and lower-cases the rest.
    Title,
}

/// What a line-comment command does to the selected lines.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommentMode {
    /// Removes the comment when every non-blank line has one, and adds it otherwise.
    Toggle,
    Add,
    Remove,
}

/// One replacement in original character offsets, for moving positions across it.
#[derive(Debug, Clone, Copy)]
struct Change {
    start: usize,
    end: usize,
    len: usize,
    /// Positions at an insertion point move after the inserted text.
    after: bool,
}

/// Where `offset` lands once `changes` (sorted, non-overlapping) are applied. A position
/// inside a replaced range keeps its distance from the start, clamped to the new text.
fn remap(changes: &[Change], offset: usize) -> usize {
    let (mut added, mut removed) = (0, 0);
    for change in changes {
        let past = if change.end > change.start {
            offset >= change.end
        } else {
            offset > change.start
        };
        if offset < change.start {
            break;
        }
        if past {
            added += change.len;
            removed += change.end - change.start;
            continue;
        }
        let inside = if change.after {
            change.len
        } else {
            (offset - change.start).min(change.len)
        };
        return change.start + added - removed + inside;
    }
    offset + added - removed
}

/// The lines a cursor covers: its first through its last line, leaving out a last line
/// that a multi-line selection only reaches at column 0.
fn cursor_lines(cursor: &Cursor) -> (usize, usize) {
    match cursor.range() {
        Some(range) if !range.is_empty() => {
            let last = if range.end.column == 0 && range.end.line > range.start.line {
                range.end.line - 1
            } else {
                range.end.line
            };
            (range.start.line, last)
        }
        _ => (cursor.position.line, cursor.position.line),
    }
}

/// Sorted line blocks, merged where they overlap or, with `touching`, where they meet.
fn merge_blocks(mut blocks: Vec<(usize, usize)>, touching: bool) -> Vec<(usize, usize)> {
    blocks.sort_unstable();
    let mut merged: Vec<(usize, usize)> = Vec::with_capacity(blocks.len());
    for (start, end) in blocks {
        match merged.last_mut() {
            Some(last) if start <= last.1 || (touching && start == last.1 + 1) => {
                last.1 = last.1.max(end);
            }
            _ => merged.push((start, end)),
        }
    }
    merged
}

fn block_of(blocks: &[(usize, usize)], line: usize) -> usize {
    blocks
        .iter()
        .position(|(start, end)| (*start..=*end).contains(&line))
        .unwrap_or(0)
}

fn map_cursor(cursor: &Cursor, map: impl Fn(Position) -> Position) -> Cursor {
    match cursor.selection {
        Some(selection) => Cursor::with_selection(map(selection.anchor), map(selection.active)),
        None => {
            let position = map(cursor.position);
            Cursor::at(position.line, position.column)
        }
    }
}

fn line_range(document: &Document, first: usize, last: usize) -> Range {
    Range::new(
        Position::new(first, 0),
        Position::new(last, document.line_len_chars(last)),
    )
}

fn leading_whitespace(text: &str) -> usize {
    text.chars().take_while(|ch| ch.is_whitespace()).count()
}

fn trailing_whitespace(text: &str) -> usize {
    text.chars()
        .rev()
        .take_while(|ch| ch.is_whitespace())
        .count()
}

/// `text` in title case: each word's first letter upper-cased and the rest lower-cased.
fn title_case(text: &str) -> String {
    let mut result = String::with_capacity(text.len());
    let mut in_word = false;
    for ch in text.chars() {
        if in_word {
            result.extend(ch.to_lowercase());
        } else {
            result.extend(ch.to_uppercase());
        }
        in_word = ch.is_alphanumeric() || matches!(ch, '_' | '\'' | '’');
    }
    result
}

/// The line-comment marker for a file extension.
pub(crate) fn line_comment(extension: &str) -> &'static str {
    match extension {
        "py" | "sh" | "bash" | "zsh" | "yaml" | "yml" | "toml" | "rb" | "r" => "#",
        "sql" | "lua" | "hs" => "--",
        _ => "//",
    }
}

/// The block-comment delimiters for a file extension, if the language has them.
pub(crate) fn block_comment(extension: &str) -> Option<(&'static str, &'static str)> {
    match extension {
        "rs" | "c" | "h" | "cc" | "cpp" | "cxx" | "hpp" | "hh" | "js" | "jsx" | "mjs" | "cjs"
        | "ts" | "tsx" | "mts" | "cts" | "java" | "go" | "cs" | "kt" | "kts" | "swift"
        | "scala" | "dart" | "php" | "css" | "scss" | "less" | "sql" | "jsonc" | "zig" => {
            Some(("/*", "*/"))
        }
        "html" | "htm" | "xml" | "svg" | "md" | "markdown" | "vue" | "svelte" => {
            Some(("<!--", "-->"))
        }
        "lua" => Some(("--[[", "]]")),
        "hs" => Some(("{-", "-}")),
        "ml" | "mli" => Some(("(*", "*)")),
        _ => None,
    }
}

impl EditorState {
    /// Whether the buffer refuses edits, as help pages and undecodable files do.
    #[must_use]
    pub fn is_read_only(&self) -> bool {
        self.document.with(Document::is_read_only)
    }

    /// Whether any cursor's selection spans more than one line.
    #[must_use]
    pub fn selection_spans_lines(&self) -> bool {
        self.cursor_states()
            .iter()
            .any(|cursor| cursor.range().is_some_and(|r| r.start.line != r.end.line))
    }

    fn line_blocks(&self, touching: bool) -> Vec<(usize, usize)> {
        merge_blocks(
            self.cursor_states().iter().map(cursor_lines).collect(),
            touching,
        )
    }

    /// Applies `edits` as one undo step and places `cursors`. False when the buffer is
    /// read-only or nothing would change.
    fn apply_with_cursors(&mut self, edits: &[(Range, String)], cursors: &[Cursor]) -> bool {
        if self.is_read_only() {
            return false;
        }
        let edits = self.document.with(|document| {
            edits
                .iter()
                .filter(|(range, text)| document.text_in_range(*range) != *text)
                .cloned()
                .collect::<Vec<_>>()
        });
        if edits.is_empty() || self.document.apply_edits(&edits, 0).is_err() {
            return false;
        }
        let mut placed = self.document.with(|document| {
            cursors
                .iter()
                .map(|cursor| map_cursor(cursor, |position| document.clamp_position(position)))
                .collect::<Vec<_>>()
        });
        let mut seen = Vec::with_capacity(placed.len());
        placed.retain(|cursor| {
            let fresh = !seen.contains(cursor);
            seen.push(*cursor);
            fresh
        });
        self.set_cursor_states(&placed);
        self.folded.clear();
        self.follow_cursor = true;
        true
    }

    /// Applies `edits` (range, replacement, and whether positions at an insertion point
    /// move after it) as one undo step, moving every cursor with the text around it.
    fn apply_mapped(&mut self, edits: Vec<(Range, String, bool)>) -> bool {
        let cursors = self.cursor_states();
        let (edits, changes, offsets) = self.document.with(|document| {
            let edits = edits
                .into_iter()
                .filter(|(range, text, _)| document.text_in_range(*range) != *text)
                .collect::<Vec<_>>();
            let mut changes = edits
                .iter()
                .map(|(range, text, after)| {
                    let range = range.normalized();
                    Change {
                        start: document.position_to_char(range.start),
                        end: document.position_to_char(range.end),
                        len: text.chars().count(),
                        after: *after,
                    }
                })
                .collect::<Vec<_>>();
            changes.sort_by_key(|change| (change.start, change.end));
            let offsets = cursors
                .iter()
                .map(|cursor| {
                    let anchor = cursor.selection.map_or(cursor.position, |s| s.anchor);
                    (
                        document.position_to_char(anchor),
                        document.position_to_char(cursor.position),
                    )
                })
                .collect::<Vec<_>>();
            let edits = edits
                .into_iter()
                .map(|(range, text, _)| (range, text))
                .collect::<Vec<_>>();
            (edits, changes, offsets)
        });
        if self.is_read_only() || edits.is_empty() {
            return false;
        }
        let before = self.revision();
        if self.document.apply_edits(&edits, 0).is_err() || self.revision() == before {
            return false;
        }
        let placed = self.document.with(|document| {
            cursors
                .iter()
                .zip(offsets)
                .map(|(cursor, (anchor, active))| {
                    let anchor = document.char_to_position(remap(&changes, anchor));
                    let active = document.char_to_position(remap(&changes, active));
                    if cursor.selection.is_some() {
                        Cursor::with_selection(anchor, active)
                    } else {
                        Cursor::at(active.line, active.column)
                    }
                })
                .collect::<Vec<_>>()
        });
        self.set_cursor_states(&placed);
        self.folded.clear();
        self.follow_cursor = true;
        true
    }

    /// Swaps the lines under each cursor with the line above or below them. Nothing
    /// moves when a block is already at the edge of the document.
    pub fn move_lines(&mut self, up: bool) -> bool {
        let cursors = self.cursor_states();
        let blocks = self.line_blocks(true);
        let edits = self.document.with(|document| {
            let last = document.len_lines().saturating_sub(1);
            if blocks
                .iter()
                .any(|(start, end)| if up { *start == 0 } else { *end >= last })
            {
                return None;
            }
            let line_break = document.line_break();
            Some(
                blocks
                    .iter()
                    .map(|&(start, end)| {
                        let (first, last) = if up {
                            (start - 1, end)
                        } else {
                            (start, end + 1)
                        };
                        let mut lines = (first..=last)
                            .map(|line| document.line(line).unwrap_or_default())
                            .collect::<Vec<_>>();
                        if up {
                            lines.rotate_left(1);
                        } else {
                            lines.rotate_right(1);
                        }
                        (line_range(document, first, last), lines.join(line_break))
                    })
                    .collect::<Vec<_>>(),
            )
        });
        let Some(edits) = edits else {
            return false;
        };
        let moved = cursors
            .iter()
            .map(|cursor| {
                map_cursor(cursor, |position| {
                    let line = if up {
                        position.line.saturating_sub(1)
                    } else {
                        position.line + 1
                    };
                    Position::new(line, position.column)
                })
            })
            .collect::<Vec<_>>();
        self.apply_with_cursors(&edits, &moved)
    }

    /// Deletes every line under a cursor, keeping each cursor's column.
    pub fn delete_lines(&mut self) -> bool {
        let cursors = self.cursor_states();
        let blocks = self.line_blocks(true);
        let edits = self.document.with(|document| {
            let count = document.len_lines();
            blocks
                .iter()
                .map(|&(start, end)| {
                    let range = if end + 1 < count {
                        Range::new(Position::new(start, 0), Position::new(end + 1, 0))
                    } else if start > 0 {
                        Range::new(
                            Position::new(start - 1, document.line_len_chars(start - 1)),
                            Position::new(end, document.line_len_chars(end)),
                        )
                    } else {
                        line_range(document, start, end)
                    };
                    (range, String::new())
                })
                .collect::<Vec<_>>()
        });
        let placed = cursors
            .iter()
            .map(|cursor| {
                let index = block_of(&blocks, cursor.position.line);
                let removed = blocks[..index]
                    .iter()
                    .map(|(start, end)| end - start + 1)
                    .sum::<usize>();
                Cursor::at(blocks[index].0 - removed, cursor.position.column)
            })
            .collect::<Vec<_>>();
        self.apply_with_cursors(&edits, &placed)
    }

    /// Copies the lines under each cursor below them and moves the cursors to the copy.
    pub fn duplicate_lines(&mut self) -> bool {
        let cursors = self.cursor_states();
        let blocks = self.line_blocks(true);
        let edits = self.document.with(|document| {
            let line_break = document.line_break();
            blocks
                .iter()
                .map(|&(start, end)| {
                    let lines = (start..=end)
                        .map(|line| document.line(line).unwrap_or_default())
                        .collect::<Vec<_>>();
                    let at = Position::new(end, document.line_len_chars(end));
                    (
                        Range::new(at, at),
                        format!("{line_break}{}", lines.join(line_break)),
                    )
                })
                .collect::<Vec<_>>()
        });
        let placed = cursors
            .iter()
            .map(|cursor| {
                let index = block_of(&blocks, cursor.position.line);
                let below = blocks[..=index]
                    .iter()
                    .map(|(start, end)| end - start + 1)
                    .sum::<usize>();
                map_cursor(cursor, |position| {
                    Position::new(position.line + below, position.column)
                })
            })
            .collect::<Vec<_>>();
        self.apply_with_cursors(&edits, &placed)
    }

    /// Copies each selection in front of itself, leaving the original text selected after
    /// the copy. Without any selection, duplicates the lines instead.
    pub fn duplicate_selections(&mut self) -> bool {
        let cursors = self.cursor_states();
        if !cursors
            .iter()
            .any(|cursor| cursor.range().is_some_and(|range| !range.is_empty()))
        {
            return self.duplicate_lines();
        }
        let edits = self.document.with(|document| {
            let mut ranges = cursors
                .iter()
                .filter_map(|cursor| cursor.range().filter(|range| !range.is_empty()))
                .collect::<Vec<_>>();
            ranges.sort_by_key(|range| range.start);
            ranges.dedup();
            ranges
                .into_iter()
                .map(|range| {
                    (
                        Range::new(range.start, range.start),
                        document.text_in_range(range),
                        true,
                    )
                })
                .collect::<Vec<_>>()
        });
        self.apply_mapped(edits)
    }

    /// Indents, or with `outdent` removes one level of indentation from, every line under
    /// a cursor. Blank lines in a multi-line block are left alone when indenting.
    pub fn indent_lines(&mut self, outdent: bool, tab_size: usize) -> bool {
        let unit = self.indent_unit.clone();
        let width = if unit == "\t" {
            tab_size.max(1)
        } else {
            unit.chars().count()
        };
        let blocks = self.line_blocks(false);
        let edits = self.document.with(|document| {
            let mut edits = Vec::new();
            for &(start, end) in &blocks {
                for line in start..=end {
                    let text = document.line(line).unwrap_or_default();
                    if outdent {
                        let remove = if text.starts_with('\t') {
                            1
                        } else {
                            text.chars().take(width).take_while(|ch| *ch == ' ').count()
                        };
                        if remove > 0 {
                            edits.push((
                                Range::new(Position::new(line, 0), Position::new(line, remove)),
                                String::new(),
                                false,
                            ));
                        }
                    } else if start == end || !text.trim().is_empty() {
                        let at = Position::new(line, 0);
                        edits.push((Range::new(at, at), unit.clone(), true));
                    }
                }
            }
            edits
        });
        self.apply_mapped(edits)
    }

    /// Joins each cursor's line with the next one, or a multi-line selection's lines
    /// into one, separating the joined text with single spaces.
    pub fn join_lines(&mut self) -> bool {
        let cursors = self.cursor_states();
        let blocks = self.line_blocks(false);
        let (edits, placed) = self.document.with(|document| {
            let count = document.len_lines();
            let mut edits = Vec::new();
            let mut placed = Vec::new();
            let mut removed = 0;
            for &(start, end) in &blocks {
                let last = if start == end { end + 1 } else { end };
                if last >= count {
                    continue;
                }
                let mut joined = document.line(start).unwrap_or_default();
                joined.truncate(joined.trim_end().len());
                let mut join_point = joined.chars().count();
                for line in start + 1..=last {
                    let text = document.line(line).unwrap_or_default();
                    let text = if line == last {
                        text.trim_start()
                    } else {
                        text.trim()
                    };
                    if text.is_empty() {
                        continue;
                    }
                    if !joined.is_empty() {
                        joined.push(' ');
                    }
                    if line == start + 1 {
                        join_point = joined.chars().count();
                    }
                    joined.push_str(text);
                }
                let line = start - removed;
                let selected = cursors
                    .iter()
                    .find(|cursor| (start..=end).contains(&cursor.position.line))
                    .and_then(|cursor| cursor.range())
                    .filter(|range| !range.is_empty());
                placed.push(match selected {
                    Some(range) if start != end => Cursor::with_selection(
                        Position::new(line, range.start.column),
                        Position::new(line, joined.chars().count()),
                    ),
                    _ => Cursor::at(line, join_point),
                });
                edits.push((line_range(document, start, last), joined));
                removed += last - start;
            }
            (edits, placed)
        });
        self.apply_with_cursors(&edits, &placed)
    }

    /// Rewrites each selection's case, or the word at a cursor without one.
    pub fn transform_case(&mut self, transform: CaseTransform) -> bool {
        let cursors = self.cursor_states();
        let edits = self.document.with(|document| {
            let mut ranges = cursors
                .iter()
                .filter_map(|cursor| {
                    cursor
                        .range()
                        .filter(|range| !range.is_empty())
                        .or_else(|| document.word_at(cursor.position))
                })
                .collect::<Vec<_>>();
            ranges.sort_by_key(|range| range.start);
            let mut kept: Vec<Range> = Vec::with_capacity(ranges.len());
            for range in ranges {
                if kept.last().is_none_or(|last| range.start >= last.end) {
                    kept.push(range);
                }
            }
            kept.into_iter()
                .map(|range| {
                    let text = document.text_in_range(range);
                    let text = match transform {
                        CaseTransform::Upper => text.to_uppercase(),
                        CaseTransform::Lower => text.to_lowercase(),
                        CaseTransform::Title => title_case(&text),
                    };
                    (range, text, false)
                })
                .collect::<Vec<_>>()
        });
        self.apply_mapped(edits)
    }

    /// Sorts the lines of each multi-line selection, or of the whole document when no
    /// selection spans lines. Returns how many lines were sorted.
    pub fn sort_lines(&mut self, descending: bool) -> Option<usize> {
        let spanning = self
            .cursor_states()
            .iter()
            .map(cursor_lines)
            .filter(|(start, end)| end > start)
            .collect::<Vec<_>>();
        let blocks = self.document.with(|document| {
            if spanning.is_empty() {
                let mut last = document.len_lines().saturating_sub(1);
                if last > 0 && document.line_len_chars(last) == 0 {
                    last -= 1;
                }
                vec![(0, last)]
            } else {
                merge_blocks(spanning, false)
            }
        });
        let (edits, sorted) = self.document.with(|document| {
            let line_break = document.line_break();
            let mut sorted = 0;
            let edits = blocks
                .iter()
                .map(|&(start, end)| {
                    let mut lines = (start..=end)
                        .map(|line| document.line(line).unwrap_or_default())
                        .collect::<Vec<_>>();
                    lines.sort_by(|a, b| a.to_lowercase().cmp(&b.to_lowercase()).then(a.cmp(b)));
                    if descending {
                        lines.reverse();
                    }
                    sorted += lines.len();
                    (
                        line_range(document, start, end),
                        lines.join(line_break),
                        false,
                    )
                })
                .collect::<Vec<_>>();
            (edits, sorted)
        });
        if self.is_read_only() {
            return None;
        }
        self.apply_mapped(edits);
        Some(sorted)
    }

    /// Removes trailing whitespace from every line. Returns how many lines changed.
    pub fn trim_trailing_whitespace(&mut self) -> Option<usize> {
        let edits = self.document.with(|document| {
            (0..document.len_lines())
                .filter_map(|line| {
                    let text = document.line_str(line)?;
                    let trailing = trailing_whitespace(&text);
                    (trailing > 0).then(|| {
                        let length = text.chars().count();
                        (
                            Range::new(
                                Position::new(line, length - trailing),
                                Position::new(line, length),
                            ),
                            String::new(),
                            false,
                        )
                    })
                })
                .collect::<Vec<_>>()
        });
        if self.is_read_only() {
            return None;
        }
        let count = edits.len();
        self.apply_mapped(edits);
        Some(count)
    }

    /// Adds or removes `marker` line comments on the lines under each cursor.
    pub fn comment_lines(&mut self, marker: &str, mode: CommentMode) -> bool {
        let blocks = self.line_blocks(false);
        let edits = self.document.with(|document| {
            let lines = blocks
                .iter()
                .flat_map(|&(start, end)| start..=end)
                .filter_map(|line| {
                    let text = document.line(line)?;
                    (!text.trim().is_empty()).then_some((line, text))
                })
                .collect::<Vec<_>>();
            let commented = |text: &str| text.trim_start().starts_with(marker);
            let remove = match mode {
                CommentMode::Toggle => {
                    !lines.is_empty() && lines.iter().all(|(_, text)| commented(text))
                }
                CommentMode::Add => false,
                CommentMode::Remove => true,
            };
            lines
                .iter()
                .filter_map(|(line, text)| {
                    let indent = leading_whitespace(text);
                    let at = Position::new(*line, indent);
                    if remove {
                        let rest = text.chars().skip(indent).collect::<String>();
                        let rest = rest.strip_prefix(marker)?;
                        let length = marker.chars().count() + usize::from(rest.starts_with(' '));
                        Some((
                            Range::new(at, Position::new(*line, indent + length)),
                            String::new(),
                            false,
                        ))
                    } else {
                        Some((Range::new(at, at), format!("{marker} "), true))
                    }
                })
                .collect::<Vec<_>>()
        });
        self.apply_mapped(edits)
    }

    /// Wraps each selection, or each cursor's line, in `open` and `close`, or removes
    /// them when the text is already wrapped. The selection keeps the text inside.
    pub fn toggle_block_comment(&mut self, open: &str, close: &str) -> bool {
        let cursors = self.cursor_states();
        let edits = self.document.with(|document| {
            let mut ranges = cursors
                .iter()
                .map(
                    |cursor| match cursor.range().filter(|range| !range.is_empty()) {
                        Some(range) => range,
                        None => line_range(document, cursor.position.line, cursor.position.line),
                    },
                )
                .collect::<Vec<_>>();
            ranges.sort_by_key(|range| range.start);
            ranges.dedup();
            let mut edits = Vec::new();
            let mut previous_end = None;
            for range in ranges {
                if previous_end.is_some_and(|end| range.start < end) {
                    continue;
                }
                previous_end = Some(range.end);
                let text = document.text_in_range(range);
                let (lead, trail) = (leading_whitespace(&text), trailing_whitespace(&text));
                let start = document.position_to_char(range.start) + lead;
                let end = (document.position_to_char(range.end) - trail).max(start);
                let inner = text.trim();
                let at = |offset| document.char_to_position(offset);
                if inner.len() >= open.len() + close.len()
                    && inner.starts_with(open)
                    && inner.ends_with(close)
                {
                    let body = &inner[open.len()..inner.len() - close.len()];
                    let opening = open.chars().count() + usize::from(body.starts_with(' '));
                    let closing =
                        close.chars().count() + usize::from(body.len() > 1 && body.ends_with(' '));
                    edits.push((
                        Range::new(at(start), at(start + opening)),
                        String::new(),
                        false,
                    ));
                    edits.push((Range::new(at(end - closing), at(end)), String::new(), false));
                } else {
                    edits.push((Range::new(at(start), at(start)), format!("{open} "), true));
                    edits.push((Range::new(at(end), at(end)), format!(" {close}"), false));
                }
            }
            edits
        });
        self.apply_mapped(edits)
    }

    /// Folds every region nested `level` deep (1 is outermost). Returns how many.
    pub fn fold_level(&mut self, level: usize) -> usize {
        self.ensure_fold_regions();
        let mut regions = self.fold_regions.clone();
        regions.sort_by_key(|(start, end)| (*start, std::cmp::Reverse(*end)));
        let mut open: Vec<usize> = Vec::new();
        let mut folded = 0;
        for (start, end) in regions {
            while open.last().is_some_and(|last| *last < start) {
                open.pop();
            }
            if open.len() + 1 == level {
                if !self.folded.contains(&(start, end)) {
                    self.folded.push((start, end));
                }
                folded += 1;
            }
            open.push(end);
        }
        self.unfold_for_cursor();
        folded
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn change(start: usize, end: usize, len: usize, after: bool) -> Change {
        Change {
            start,
            end,
            len,
            after,
        }
    }

    #[test]
    fn remap_moves_positions_across_replacements_and_insertions() {
        let changes = [change(2, 4, 3, false), change(6, 6, 2, true)];
        assert_eq!(remap(&changes, 1), 1);
        assert_eq!(remap(&changes, 2), 2);
        assert_eq!(remap(&changes, 3), 3);
        assert_eq!(remap(&changes, 4), 5);
        assert_eq!(remap(&changes, 6), 9);
        assert_eq!(remap(&changes, 7), 10);
        assert_eq!(remap(&[change(2, 2, 2, false)], 2), 2);
    }

    #[test]
    fn blocks_merge_overlapping_and_optionally_touching_lines() {
        assert_eq!(
            merge_blocks(vec![(4, 5), (0, 1), (1, 2)], false),
            [(0, 2), (4, 5)]
        );
        assert_eq!(merge_blocks(vec![(3, 3), (0, 2)], false), [(0, 2), (3, 3)]);
        assert_eq!(merge_blocks(vec![(3, 3), (0, 2)], true), [(0, 3)]);
        let cursor = Cursor::with_selection(Position::new(1, 2), Position::new(3, 0));
        assert_eq!(cursor_lines(&cursor), (1, 2));
    }

    #[test]
    fn title_case_capitalizes_words() {
        assert_eq!(title_case("hello WORLD it's x_y"), "Hello World It's X_y");
        assert_eq!(block_comment("py"), None);
        assert_eq!(block_comment("rs"), Some(("/*", "*/")));
    }
}
