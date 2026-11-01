use std::collections::VecDeque;
use std::time::{Duration, Instant};

use unicode_segmentation::UnicodeSegmentation;

use crate::Cursor;

/// Typing pauses longer than this start a new undo step.
const COALESCE_WINDOW: Duration = Duration::from_secs(1);
/// Approximate per-edit bookkeeping cost counted against the byte cap.
const EDIT_OVERHEAD_BYTES: usize = 64;
const DEFAULT_BYTE_LIMIT: usize = 64 * 1024 * 1024;

/// One reversible text edit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Edit {
    /// Character offset where the edit starts.
    pub start: usize,
    /// Text removed by the edit.
    pub removed: String,
    /// Text inserted by the edit.
    pub inserted: String,
    /// Cursor before the edit.
    pub before: Cursor,
    /// Cursor after the edit.
    pub after: Cursor,
}

/// One group of edits produced by a multi-cursor operation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EditGroup {
    pub edits: Vec<Edit>,
    pub before: Cursor,
    pub after: Cursor,
}

/// A single- or multi-cursor undo entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UndoEntry {
    Single(Edit),
    Group(EditGroup),
}

impl UndoEntry {
    fn bytes(&self) -> usize {
        match self {
            Self::Single(edit) => edit_bytes(edit),
            Self::Group(group) => {
                group.edits.iter().map(edit_bytes).sum::<usize>() + EDIT_OVERHEAD_BYTES
            }
        }
    }
}

/// An undo entry and the document revision it produces.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Revision {
    id: u64,
    pub entry: UndoEntry,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Run {
    Insert,
    Delete,
}

/// Bounded undo/redo stacks with typing coalescing and a saved-revision marker.
#[derive(Debug, Clone)]
pub struct UndoManager {
    undo_stack: VecDeque<Revision>,
    redo_stack: Vec<Revision>,
    limit: usize,
    byte_limit: usize,
    undo_bytes: usize,
    next_id: u64,
    base_id: u64,
    saved_id: u64,
    run: Option<Run>,
    last_push: Option<Instant>,
}

impl Default for UndoManager {
    fn default() -> Self {
        Self::new(1_000)
    }
}

impl UndoManager {
    #[must_use]
    pub fn new(limit: usize) -> Self {
        Self {
            undo_stack: VecDeque::new(),
            redo_stack: Vec::new(),
            limit: limit.max(1),
            byte_limit: DEFAULT_BYTE_LIMIT,
            undo_bytes: 0,
            next_id: 1,
            base_id: 0,
            saved_id: 0,
            run: None,
            last_push: None,
        }
    }

    pub fn set_limit(&mut self, limit: usize) {
        self.limit = limit.clamp(1, 10000);
        self.enforce_limits();
        if self.redo_stack.len() > self.limit {
            self.redo_stack.drain(..self.redo_stack.len() - self.limit);
        }
    }

    /// Cap the undo history by the size of the text it retains. The newest
    /// entry is always kept so a single large edit stays undoable.
    pub fn set_byte_limit(&mut self, bytes: usize) {
        self.byte_limit = bytes;
        self.enforce_limits();
    }

    pub fn push(&mut self, edit: Edit) {
        self.push_at(edit, Instant::now());
    }

    /// Record an edit made at `now`, merging it into the previous step when it
    /// continues the same typing or deleting run.
    pub fn push_at(&mut self, edit: Edit, now: Instant) {
        self.redo_stack.clear();
        let recent = self
            .last_push
            .is_some_and(|last| now.saturating_duration_since(last) <= COALESCE_WINDOW);
        self.last_push = Some(now);
        let run = run_kind(&edit);
        if recent
            && run.is_some()
            && self.run == run
            && self.current_id() != self.saved_id
            && let Some(top) = self.undo_stack.back_mut()
            && let UndoEntry::Single(previous) = &mut top.entry
        {
            let before = edit_bytes(previous);
            if coalesce(previous, &edit) {
                self.undo_bytes = self.undo_bytes - before + edit_bytes(previous);
                self.enforce_limits();
                return;
            }
        }
        self.run = run;
        self.push_revision(UndoEntry::Single(edit));
    }

    pub fn push_group(&mut self, group: EditGroup) {
        self.redo_stack.clear();
        self.run = None;
        self.last_push = Some(Instant::now());
        self.push_revision(UndoEntry::Group(group));
    }

    fn push_revision(&mut self, entry: UndoEntry) {
        let id = self.next_id;
        self.next_id += 1;
        self.undo_bytes += entry.bytes();
        self.undo_stack.push_back(Revision { id, entry });
        self.enforce_limits();
    }

    fn enforce_limits(&mut self) {
        while self.undo_stack.len() > self.limit
            || (self.undo_bytes > self.byte_limit && self.undo_stack.len() > 1)
        {
            let Some(evicted) = self.undo_stack.pop_front() else {
                break;
            };
            self.undo_bytes -= evicted.entry.bytes();
            self.base_id = evicted.id;
        }
    }

    pub fn pop_undo_entry(&mut self) -> Option<Revision> {
        self.run = None;
        let revision = self.undo_stack.pop_back()?;
        self.undo_bytes -= revision.entry.bytes();
        Some(revision)
    }

    pub fn push_redo_entry(&mut self, revision: Revision) {
        self.redo_stack.push(revision);
    }

    pub fn pop_redo_entry(&mut self) -> Option<Revision> {
        self.run = None;
        self.redo_stack.pop()
    }

    pub fn push_undo_entry(&mut self, revision: Revision) {
        self.undo_bytes += revision.entry.bytes();
        self.undo_stack.push_back(revision);
        self.enforce_limits();
    }

    fn current_id(&self) -> u64 {
        self.undo_stack
            .back()
            .map_or(self.base_id, |revision| revision.id)
    }

    /// Remember the current revision as the one on disk.
    pub fn mark_saved(&mut self) {
        self.saved_id = self.current_id();
        self.run = None;
    }

    /// True when undo/redo has returned the text to the saved revision.
    #[must_use]
    pub fn is_at_saved(&self) -> bool {
        self.current_id() == self.saved_id
    }

    #[must_use]
    pub fn can_undo(&self) -> bool {
        !self.undo_stack.is_empty()
    }

    #[must_use]
    pub fn can_redo(&self) -> bool {
        !self.redo_stack.is_empty()
    }

    pub fn clear(&mut self) {
        self.base_id = self.current_id();
        self.undo_stack.clear();
        self.redo_stack.clear();
        self.undo_bytes = 0;
        self.run = None;
    }

    #[must_use]
    pub fn undo_len(&self) -> usize {
        self.undo_stack.len()
    }

    #[must_use]
    pub fn redo_len(&self) -> usize {
        self.redo_stack.len()
    }

    /// Approximate bytes retained by the undo stack.
    #[must_use]
    pub fn undo_bytes(&self) -> usize {
        self.undo_bytes
    }
}

fn edit_bytes(edit: &Edit) -> usize {
    edit.removed.len() + edit.inserted.len() + EDIT_OVERHEAD_BYTES
}

fn is_keystroke(text: &str) -> bool {
    !text.is_empty() && !text.contains(['\n', '\r']) && text.graphemes(true).take(3).count() <= 2
}

fn run_kind(edit: &Edit) -> Option<Run> {
    if edit.inserted.is_empty() {
        is_keystroke(&edit.removed).then_some(Run::Delete)
    } else {
        is_keystroke(&edit.inserted).then_some(Run::Insert)
    }
}

fn coalesce(previous: &mut Edit, next: &Edit) -> bool {
    if next.inserted.is_empty() {
        if !previous.inserted.is_empty() {
            return false;
        }
        let next_len = next.removed.chars().count();
        if next.start + next_len == previous.start {
            previous.removed.insert_str(0, &next.removed);
            previous.start = next.start;
        } else if next.start == previous.start {
            previous.removed.push_str(&next.removed);
        } else {
            return false;
        }
    } else {
        if !next.removed.is_empty()
            || next.start != previous.start + previous.inserted.chars().count()
        {
            return false;
        }
        previous.inserted.push_str(&next.inserted);
    }
    previous.after = next.after;
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Cursor;

    fn edit(start: usize) -> Edit {
        Edit {
            start,
            removed: String::new(),
            inserted: "x".to_owned(),
            before: Cursor::at(0, 0),
            after: Cursor::at(0, 1),
        }
    }

    fn separated(start: usize) -> Edit {
        edit(start * 2)
    }

    #[test]
    fn stacks_are_bounded() {
        let mut undo = UndoManager::new(2);
        undo.push(separated(0));
        undo.push(separated(1));
        undo.push(separated(2));
        assert_eq!(undo.undo_len(), 2);
    }

    #[test]
    fn new_edit_clears_redo() {
        let mut undo = UndoManager::new(10);
        undo.push(edit(0));
        let first = undo.pop_undo_entry().expect("undo");
        undo.push_redo_entry(first);
        assert!(undo.can_redo());
        undo.push(edit(1));
        assert!(!undo.can_redo());
    }

    #[test]
    fn adjacent_typing_coalesces_until_a_pause() {
        let mut undo = UndoManager::new(10);
        let start = Instant::now();
        undo.push_at(edit(0), start);
        undo.push_at(edit(1), start + Duration::from_millis(400));
        undo.push_at(edit(2), start + Duration::from_millis(800));
        assert_eq!(undo.undo_len(), 1);
        undo.push_at(edit(3), start + Duration::from_millis(2_000));
        assert_eq!(undo.undo_len(), 2);
        undo.push_at(edit(9), start + Duration::from_millis(2_100));
        assert_eq!(undo.undo_len(), 3);
        let Some(Revision {
            entry: UndoEntry::Single(first),
            ..
        }) = undo.undo_stack.front()
        else {
            panic!("expected a single edit");
        };
        assert_eq!(first.inserted, "xxx");
    }

    #[test]
    fn deletions_do_not_merge_into_insertions() {
        let mut undo = UndoManager::new(10);
        let now = Instant::now();
        undo.push_at(edit(0), now);
        undo.push_at(
            Edit {
                start: 0,
                removed: "x".to_owned(),
                inserted: String::new(),
                before: Cursor::at(0, 1),
                after: Cursor::at(0, 0),
            },
            now,
        );
        assert_eq!(undo.undo_len(), 2);
    }

    #[test]
    fn byte_cap_evicts_the_oldest_entries() {
        let mut undo = UndoManager::new(100);
        undo.set_byte_limit(3 * (EDIT_OVERHEAD_BYTES + 1));
        for index in 0..5 {
            undo.push(separated(index));
        }
        assert_eq!(undo.undo_len(), 3);
        assert!(undo.undo_bytes() <= 3 * (EDIT_OVERHEAD_BYTES + 1));
        let mut big = separated(10);
        big.inserted = "y".repeat(10_000);
        undo.push(big);
        assert_eq!(undo.undo_len(), 1);
    }

    #[test]
    fn saved_marker_follows_undo_and_survives_eviction() {
        let mut undo = UndoManager::new(2);
        assert!(undo.is_at_saved());
        undo.push(separated(0));
        undo.push(separated(1));
        undo.mark_saved();
        undo.push(separated(2));
        assert!(!undo.is_at_saved());
        let top = undo.pop_undo_entry().expect("undo");
        assert!(undo.is_at_saved());
        undo.push_redo_entry(top);
        let top = undo.pop_redo_entry().expect("redo");
        undo.push_undo_entry(top);
        assert!(!undo.is_at_saved());
        let top = undo.pop_undo_entry().expect("undo");
        undo.push_redo_entry(top);
        undo.clear();
        assert!(undo.is_at_saved());
    }
}
