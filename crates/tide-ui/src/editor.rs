use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};

pub(crate) use tide_core::fs::write_atomic;
use tide_core::fs::{canonical_target as document_path, write_atomic_new};
use tide_core::num::{saturating_u16, saturating_usize};
use tide_core::{Document, OverlappingEdits, Position, Range, display_width};
use tide_services::{HighlightSpan, SyntaxHighlighter};

use crate::palette::Palette;

/// Stable identity for an authoritative file-backed document.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct DocumentId(String);

impl DocumentId {
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct FileStamp {
    modified: Option<std::time::SystemTime>,
    len: u64,
}

impl FileStamp {
    fn from_metadata(metadata: &std::fs::Metadata) -> Self {
        Self {
            modified: metadata.modified().ok(),
            len: metadata.len(),
        }
    }
}

/// Shared handle to the one authoritative buffer for a document.
#[derive(Clone, Debug)]
pub struct DocumentHandle {
    shared: Arc<Mutex<SharedDocument>>,
    view: Arc<Mutex<ViewState>>,
}

#[derive(Debug, Clone)]
struct ViewState {
    primary: tide_core::Cursor,
    extra: Vec<tide_core::Cursor>,
}

#[derive(Debug)]
struct SharedDocument {
    document: Document,
    syntax_id: u64,
    syntax_changes: std::collections::VecDeque<(u64, usize)>,
    views: Vec<std::sync::Weak<Mutex<ViewState>>>,
    disk: DiskState,
    /// The status bar's line ending for a document version; detecting it
    /// scans up to 256 KiB.
    line_ending: Option<(u64, &'static str)>,
}

#[derive(Debug, Default)]
struct DiskState {
    stamp: Option<FileStamp>,
    /// A stamp already known to hold contents other than the baseline.
    differing: Option<FileStamp>,
    baseline_hash: Option<String>,
    recovery_conflict: bool,
    initialized: bool,
}

impl DocumentHandle {
    #[must_use]
    pub fn new(mut document: Document) -> Self {
        let view = Arc::new(Mutex::new(ViewState {
            primary: document.cursor(),
            extra: Vec::new(),
        }));
        document.track_changes();
        Self {
            shared: Arc::new(Mutex::new(SharedDocument {
                document,
                syntax_id: {
                    static ID: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
                    ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
                },
                syntax_changes: Default::default(),
                views: vec![Arc::downgrade(&view)],
                disk: DiskState::default(),
                line_ending: None,
            })),
            view,
        }
    }

    /// A new viewport shares text/history/disk state, never selection or cursor.
    pub fn fork_view(&self) -> Self {
        let view = Arc::new(Mutex::new(crate::lock(&self.view).clone()));
        crate::lock(&self.shared).views.push(Arc::downgrade(&view));
        Self {
            shared: self.shared.clone(),
            view,
        }
    }

    pub fn syntax_id(&self) -> u64 {
        crate::lock(&self.shared).syntax_id
    }

    /// The first line edited after `revision`, or 0 when that history is gone.
    fn first_change_since(&self, revision: u64) -> usize {
        let shared = crate::lock(&self.shared);
        if revision == 0
            || shared
                .syntax_changes
                .front()
                .is_some_and(|(v, _)| revision < v.saturating_sub(1))
        {
            return 0;
        }
        shared
            .syntax_changes
            .iter()
            .filter(|(v, _)| *v > revision)
            .map(|(_, line)| *line)
            .min()
            .unwrap_or(usize::MAX)
    }
    fn request_syntax(&self, syntax: &SyntaxHighlighter, extension: &str, lines: Vec<usize>) {
        let shared = crate::lock(&self.shared);
        let revision = syntax.document_revision(shared.syntax_id);
        let changed = if revision == 0
            || shared
                .syntax_changes
                .front()
                .is_some_and(|(v, _)| revision < v.saturating_sub(1))
        {
            0
        } else {
            shared
                .syntax_changes
                .iter()
                .filter(|(v, _)| *v > revision)
                .map(|(_, line)| *line)
                .min()
                .unwrap_or(usize::MAX)
        };
        syntax.request_document(
            shared.syntax_id,
            shared.document.version(),
            changed,
            shared.document.text_snapshot(),
            extension,
            lines,
        );
    }
    pub fn same_document(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.shared, &other.shared)
    }

    pub fn same_view(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.view, &other.view)
    }

    fn install_cursor(document: &mut Document, cursor: tide_core::Cursor) {
        if let Some(selection) = cursor.selection {
            document.set_selection(selection);
        } else {
            document.set_cursor(cursor.position);
        }
    }

    pub fn with<R>(&self, f: impl FnOnce(&Document) -> R) -> R {
        let mut shared = crate::lock(&self.shared);
        Self::install_cursor(&mut shared.document, crate::lock(&self.view).primary);
        f(&shared.document)
    }

    pub fn with_mut<R>(&self, f: impl FnOnce(&mut Document) -> R) -> R {
        let mut shared = crate::lock(&self.shared);
        shared.views.retain(|view| view.strong_count() > 0);
        let others = shared
            .views
            .iter()
            .filter_map(std::sync::Weak::upgrade)
            .map(|view| {
                let state = crate::lock(&view);
                let own = Arc::ptr_eq(&view, &self.view);
                let cursors = std::iter::once(state.primary)
                    .chain(state.extra.iter().copied())
                    .skip(usize::from(own))
                    .map(|cursor| {
                        (
                            shared.document.position_to_char(cursor.position),
                            cursor
                                .selection
                                .map(|s| shared.document.position_to_char(s.anchor)),
                        )
                    })
                    .collect::<Vec<_>>();
                drop(state);
                (view, own, cursors)
            })
            .collect::<Vec<_>>();
        Self::install_cursor(&mut shared.document, crate::lock(&self.view).primary);
        let result = f(&mut shared.document);
        crate::lock(&self.view).primary = shared.document.cursor();
        let changes = shared.document.take_changes();
        if !changes.is_empty() {
            let first = changes
                .iter()
                .map(|(start, _, _)| shared.document.char_to_position(*start).line)
                .min()
                .unwrap_or(0);
            let revision = shared.document.version();
            shared.syntax_changes.push_back((revision, first));
            if shared.syntax_changes.len() > 256 {
                shared.syntax_changes.pop_front();
            }
            for (view, own, cursors) in others {
                let cursors = cursors
                    .into_iter()
                    .map(|(mut position, mut anchor)| {
                        for &(start, removed, inserted) in &changes {
                            position = translate_offset(position, start, removed, inserted);
                            anchor = anchor
                                .map(|offset| translate_offset(offset, start, removed, inserted));
                        }
                        let active = shared.document.char_to_position(position);
                        anchor.map_or_else(
                            || tide_core::Cursor::at(active.line, active.column),
                            |anchor| {
                                tide_core::Cursor::with_selection(
                                    shared.document.char_to_position(anchor),
                                    active,
                                )
                            },
                        )
                    })
                    .collect::<Vec<_>>();
                let mut state = crate::lock(&view);
                if own {
                    state.extra = cursors;
                } else if let Some(primary) = cursors.first() {
                    state.primary = *primary;
                    state.extra = cursors.into_iter().skip(1).collect();
                }
            }
        }

        result
    }

    fn initialize_disk(&self, path: Option<&Path>, stamp: Option<FileStamp>) {
        let mut shared = crate::lock(&self.shared);
        if !shared.disk.initialized {
            shared.disk = DiskState {
                stamp,
                baseline_hash: path.and_then(hash_file),
                initialized: true,
                ..DiskState::default()
            };
        }
    }

    /// Whether the file at `path` differs from the last loaded or saved
    /// contents. The file is hashed only when its stamp moves, without
    /// holding the document lock.
    pub(crate) fn disk_changed(&self, path: &Path) -> bool {
        let Ok(metadata) = std::fs::metadata(path) else {
            return true;
        };
        let stamp = FileStamp::from_metadata(&metadata);
        let baseline = {
            let shared = crate::lock(&self.shared);
            let disk = &shared.disk;
            if disk.recovery_conflict || disk.differing == Some(stamp) {
                return true;
            }
            if disk.stamp == Some(stamp) {
                return false;
            }
            disk.baseline_hash.clone()
        };
        let changed = baseline != hash_file(path);
        let mut shared = crate::lock(&self.shared);
        if shared.disk.baseline_hash == baseline {
            if changed {
                shared.disk.differing = Some(stamp);
            } else {
                shared.disk.stamp = Some(stamp);
            }
        }
        changed
    }

    /// Like `disk_changed`, but always compares contents: a same-length write that
    /// kept the stamp must still block an overwrite.
    pub(crate) fn disk_contents_changed(&self, path: &Path) -> bool {
        let baseline = {
            let shared = crate::lock(&self.shared);
            if shared.disk.recovery_conflict {
                return true;
            }
            shared.disk.baseline_hash.clone()
        };
        baseline != hash_file(path)
    }

    fn update_disk(&self, path: &Path) {
        crate::lock(&self.shared).disk = DiskState {
            stamp: file_stamp(path),
            differing: None,
            baseline_hash: hash_file(path),
            recovery_conflict: false,
            initialized: true,
        };
    }

    #[must_use]
    pub fn text(&self) -> String {
        self.with(Document::text)
    }

    #[must_use]
    pub fn line(&self, line: usize) -> Option<String> {
        self.with(|document| document.line(line))
    }

    #[must_use]
    pub fn lines(&self) -> Vec<String> {
        self.with(Document::lines)
    }

    #[must_use]
    pub fn len_lines(&self) -> usize {
        self.with(Document::len_lines)
    }

    #[must_use]
    pub fn status_metadata(&self) -> (&'static str, usize) {
        let mut shared = crate::lock(&self.shared);
        Self::install_cursor(&mut shared.document, crate::lock(&self.view).primary);
        let document = &shared.document;
        let selected = document.cursor().range().map_or(0, |range| {
            document
                .position_to_char(range.end)
                .abs_diff(document.position_to_char(range.start))
        });
        let version = document.version();
        let ending = match shared.line_ending {
            Some((cached, ending)) if cached == version => ending,
            _ => {
                let ending = shared.document.line_ending();
                shared.line_ending = Some((version, ending));
                ending
            }
        };
        (ending, selected)
    }

    pub fn line_len_chars(&self, line: usize) -> usize {
        self.with(|document| document.line_len_chars(line))
    }

    #[must_use]
    pub fn cursor(&self) -> tide_core::Cursor {
        self.with(Document::cursor)
    }

    pub fn set_cursor(&self, position: Position) {
        self.with_mut(|document| document.set_cursor(position));
    }

    pub fn set_selection(&self, selection: tide_core::Selection) {
        self.with_mut(|document| document.set_selection(selection));
    }

    pub fn apply_edits(
        &self,
        edits: &[(Range, String)],
        primary_index: usize,
    ) -> Result<Vec<tide_core::Cursor>, OverlappingEdits> {
        self.with_mut(|document| document.apply_edits(edits, primary_index))
    }

    pub fn apply_edits_preserving_cursor(
        &self,
        edits: &[(Range, String)],
    ) -> Result<(), OverlappingEdits> {
        self.with_mut(|document| {
            let before = document.cursor();
            let mut position = document.position_to_char(before.position);
            let mut anchor = before
                .selection
                .map(|s| document.position_to_char(s.anchor));
            document.apply_edits(edits, 0)?;
            for &(start, removed, inserted) in document.edit_deltas() {
                position = translate_offset(position, start, removed, inserted);
                anchor = anchor.map(|offset| translate_offset(offset, start, removed, inserted));
            }
            let active = document.char_to_position(position);
            if let Some(anchor) = anchor {
                document.set_selection(tide_core::Selection {
                    anchor: document.char_to_position(anchor),
                    active,
                });
            } else {
                document.set_cursor(active);
            }
            Ok(())
        })
    }
    pub fn set_file_path(&self, path: Option<PathBuf>) {
        self.with_mut(|document| document.set_file_path(path));
    }

    pub fn move_cursor_to(&self, position: Position, extend: bool) {
        self.with_mut(|document| document.move_cursor_to(position, extend));
    }

    pub fn move_left(&self, extend: bool) {
        self.with_mut(|document| document.move_left(extend));
    }

    pub fn move_right(&self, extend: bool) {
        self.with_mut(|document| document.move_right(extend));
    }

    pub fn move_vertical(&self, delta: isize, extend: bool) {
        self.with_mut(|document| document.move_vertical(delta, extend));
    }

    pub fn insert(&self, position: Position, text: &str) -> Range {
        self.with_mut(|document| document.insert(position, text))
    }

    pub fn delete(&self, range: Range) -> Range {
        self.with_mut(|document| document.delete(range))
    }

    pub fn replace(&self, range: Range, text: &str) -> Range {
        self.with_mut(|document| document.replace(range, text))
    }

    pub fn undo(&self) {
        self.with_mut(Document::undo);
    }

    pub fn redo(&self) {
        self.with_mut(Document::redo);
    }

    pub fn select_all(&self) {
        self.with_mut(Document::select_all);
    }

    pub fn set_content(&self, content: &str) {
        self.with_mut(|document| document.set_content(content));
    }

    pub fn clear_selection(&self) {
        self.with_mut(Document::clear_selection);
    }

    #[must_use]
    pub fn selection_positions(&self) -> Option<(Position, Position)> {
        self.with(Document::selection_positions)
    }

    #[must_use]
    pub fn selected_text(&self) -> Option<String> {
        self.with(Document::selected_text)
    }

    #[must_use]
    pub fn is_dirty(&self) -> bool {
        self.with(Document::is_dirty)
    }

    pub fn mark_clean(&self) {
        self.with_mut(Document::mark_clean);
    }

    pub fn mark_dirty(&self) {
        self.with_mut(Document::mark_dirty);
    }

    #[must_use]
    pub fn version(&self) -> u64 {
        self.with(Document::version)
    }
}

#[derive(Debug, Clone)]
struct SearchCache {
    revision: u64,
    query: String,
    options: crate::search::SearchOptions,
    ranges: Vec<Range>,
}

/// How an editor is drawn this frame.
#[derive(Clone, Copy)]
pub struct EditorView<'a> {
    pub palette: Palette,
    pub syntax: &'a SyntaxHighlighter,
    pub show_line_numbers: bool,
    pub tab_size: usize,
    pub focused: bool,
}

#[derive(Debug, Clone)]
pub struct HunkEdit {
    pub root: PathBuf,
    pub diff: Arc<tide_services::git::diff::FileDiff>,
    pub hunk: usize,
}
#[derive(Debug)]
pub struct EditorState {
    pub inline: Option<crate::diff_browser::DiffBrowser>,
    pub inline_focus: bool,
    pub inline_max_height: u16,
    pub minimap: bool,
    pub auto_indent: String,
    minimap_area: Rect,
    scrollbar_area: Rect,
    drag_minimap: bool,
    pub inline_signature: Option<String>,
    pub inline_area: Rect,
    pub hunk_edit: Option<HunkEdit>,
    pub historical: Option<(PathBuf, String)>,
    pub document: DocumentHandle,
    pub scroll_top: usize,
    pub scroll_left: usize,
    language_extension: String,
    path: Option<PathBuf>,
    word_wrap: bool,
    wrap_column: Option<usize>,
    wrap_offset: usize,
    rendered_rows: Vec<(usize, usize)>,
    /// Where each wrapped row in `rendered_rows` ends.
    wrapped_ends: Vec<usize>,
    /// The text area the last layout chose, inside the gutter-less scrollbar and minimap.
    text_area: Rect,
    signature_area: Rect,
    diagnostic_lines: Vec<(usize, u8)>,
    diagnostic_ranges: Vec<(Range, u8)>,
    pub diagnostics_enabled: bool,
    pub show_diagnostic_gutter: bool,
    pub underline_diagnostics: bool,
    follow_cursor: bool,
    folded: Vec<(usize, usize)>,
    fold_regions: Vec<(usize, usize)>,
    delimiters: crate::delimiters::Delimiters,
    /// The document (syntax id) and revision the fold index describes.
    fold_revision: (u64, u64),
    fold_indents: Vec<Option<usize>>,
    fold_deferred: Option<(u64, Instant)>,
    relative_numbers: bool,
    indent_unit: String,
    drag_anchor: Option<Position>,
    drag_scrollbar: bool,
    search_cache: std::cell::RefCell<Option<SearchCache>>,
    cursor_screen: Option<(u16, u16)>,
}

impl Clone for EditorState {
    fn clone(&self) -> Self {
        Self {
            inline: self.inline.clone(),
            inline_focus: false,
            inline_max_height: 15,
            minimap: false,
            auto_indent: "full".into(),
            minimap_area: Rect::default(),
            scrollbar_area: Rect::default(),
            drag_minimap: false,
            inline_signature: None,
            inline_area: Rect::default(),
            hunk_edit: self.hunk_edit.clone(),
            historical: self.historical.clone(),
            document: self.document.fork_view(),
            scroll_top: self.scroll_top,
            scroll_left: self.scroll_left,
            language_extension: self.language_extension.clone(),
            path: self.path.clone(),
            word_wrap: self.word_wrap,
            wrap_column: self.wrap_column,
            wrap_offset: self.wrap_offset,
            rendered_rows: self.rendered_rows.clone(),
            wrapped_ends: self.wrapped_ends.clone(),
            text_area: self.text_area,
            signature_area: self.signature_area,
            diagnostic_lines: self.diagnostic_lines.clone(),
            diagnostic_ranges: self.diagnostic_ranges.clone(),
            diagnostics_enabled: true,
            show_diagnostic_gutter: true,
            underline_diagnostics: true,
            follow_cursor: self.follow_cursor,
            folded: self.folded.clone(),
            fold_regions: self.fold_regions.clone(),
            delimiters: self.delimiters.clone(),
            fold_revision: self.fold_revision,
            fold_indents: self.fold_indents.clone(),
            fold_deferred: self.fold_deferred,
            relative_numbers: self.relative_numbers,
            indent_unit: self.indent_unit.clone(),
            drag_anchor: None,
            drag_scrollbar: false,
            search_cache: std::cell::RefCell::new(None),
            cursor_screen: None,
        }
    }
}

impl EditorState {
    pub fn from_path(path: impl AsRef<Path>) -> std::io::Result<Self> {
        let path = path.as_ref();
        let document = Document::from_path(path)?;
        let language_extension = path
            .extension()
            .and_then(|ext| ext.to_str())
            .unwrap_or("txt")
            .to_ascii_lowercase();
        let stamp = std::fs::metadata(path)
            .ok()
            .map(|metadata| FileStamp::from_metadata(&metadata));
        Ok(Self::from_handle(
            DocumentHandle::new(document),
            Some(path.to_path_buf()),
            language_extension,
            stamp,
        ))
    }

    #[must_use]
    pub fn untitled() -> Self {
        Self {
            inline: None,
            inline_focus: false,
            inline_max_height: 15,
            minimap: false,
            auto_indent: "full".into(),
            minimap_area: Rect::default(),
            scrollbar_area: Rect::default(),
            drag_minimap: false,
            inline_signature: None,
            inline_area: Rect::default(),
            hunk_edit: None,
            historical: None,
            document: DocumentHandle::new(Document::new("")),
            scroll_top: 0,
            scroll_left: 0,
            language_extension: "txt".to_owned(),
            path: None,
            word_wrap: false,
            wrap_column: None,
            wrap_offset: 0,
            rendered_rows: Vec::new(),
            wrapped_ends: Vec::new(),
            text_area: Rect::default(),
            signature_area: Rect::default(),
            diagnostic_lines: Vec::new(),
            diagnostic_ranges: Vec::new(),
            diagnostics_enabled: true,
            show_diagnostic_gutter: true,
            underline_diagnostics: true,
            follow_cursor: true,
            folded: Vec::new(),
            fold_regions: Vec::new(),
            delimiters: Default::default(),
            fold_revision: (0, 0),
            fold_indents: Vec::new(),
            fold_deferred: None,
            relative_numbers: false,
            indent_unit: "  ".into(),
            drag_anchor: None,
            drag_scrollbar: false,
            search_cache: std::cell::RefCell::new(None),
            cursor_screen: None,
        }
    }

    #[must_use]
    /// Scratch buffer with a forced language, used by SQL editors.
    pub fn scratch(content: &str, language: &str) -> Self {
        let mut state = Self::from_content(None, content);
        state.language_extension = language.to_owned();
        state
    }

    pub fn from_content(path: Option<PathBuf>, content: &str) -> Self {
        let mut document = Document::new(content);
        let language_extension = path
            .as_deref()
            .and_then(Path::extension)
            .and_then(|ext| ext.to_str())
            .unwrap_or("txt")
            .to_ascii_lowercase();
        document.set_file_path(path.clone());
        Self::from_handle(
            DocumentHandle::new(document),
            path,
            language_extension,
            None,
        )
    }

    #[must_use]
    pub(crate) fn from_handle(
        document: DocumentHandle,
        path: Option<PathBuf>,
        language_extension: String,
        stamp: Option<FileStamp>,
    ) -> Self {
        document.initialize_disk(path.as_deref(), stamp);
        Self {
            document,
            inline: None,
            inline_focus: false,
            inline_max_height: 15,
            minimap: false,
            auto_indent: "full".into(),
            minimap_area: Rect::default(),
            scrollbar_area: Rect::default(),
            drag_minimap: false,
            inline_signature: None,
            inline_area: Rect::default(),
            hunk_edit: None,
            historical: None,
            scroll_top: 0,
            scroll_left: 0,
            language_extension,
            word_wrap: false,
            wrap_column: None,
            wrap_offset: 0,
            rendered_rows: Vec::new(),
            wrapped_ends: Vec::new(),
            text_area: Rect::default(),
            signature_area: Rect::default(),
            path,
            diagnostic_lines: Vec::new(),
            diagnostic_ranges: Vec::new(),
            diagnostics_enabled: true,
            show_diagnostic_gutter: true,
            underline_diagnostics: true,
            follow_cursor: true,
            folded: Vec::new(),
            fold_regions: Vec::new(),
            delimiters: Default::default(),
            fold_revision: (0, 0),
            fold_indents: Vec::new(),
            fold_deferred: None,
            relative_numbers: false,
            indent_unit: "  ".into(),
            drag_anchor: None,
            drag_scrollbar: false,
            search_cache: std::cell::RefCell::new(None),
            cursor_screen: None,
        }
    }

    #[must_use]
    pub fn path(&self) -> Option<&Path> {
        self.path.as_deref()
    }

    #[must_use]
    pub fn cursor_screen(&self) -> Option<(u16, u16)> {
        self.cursor_screen
    }
    pub fn title(&self) -> String {
        if let Some(edit) = &self.hunk_edit {
            return format!("Edit hunk: {}", edit.diff.path);
        }
        if let Some((path, hash)) = &self.historical {
            return format!(
                "{} @ {} [read-only]",
                path.display(),
                crate::display::short_id(hash)
            );
        }
        self.path()
            .and_then(|path| path.file_name())
            .and_then(|name| name.to_str())
            .map_or_else(|| "Untitled".to_owned(), ToOwned::to_owned)
    }

    #[must_use]
    pub fn document_id(&self) -> Option<DocumentId> {
        self.path()
            .map(|path| DocumentId(path.display().to_string()))
    }

    #[must_use]
    pub fn revision(&self) -> u64 {
        self.document.version()
    }

    #[must_use]
    pub fn is_dirty(&self) -> bool {
        self.document.is_dirty()
    }

    pub fn follow_cursor(&mut self) {
        self.follow_cursor = true;
    }

    pub fn scroll_by(&mut self, lines: isize) {
        self.follow_cursor = false;
        let max_scroll = self.document.len_lines().saturating_sub(1);
        self.scroll_top = if lines < 0 {
            self.scroll_top.saturating_sub(lines.unsigned_abs())
        } else {
            self.scroll_top
                .saturating_add(lines.unsigned_abs())
                .min(max_scroll)
        };
    }

    pub fn set_diagnostic_ranges(&mut self, ranges: Vec<(Range, u8)>) {
        self.diagnostic_ranges = ranges;
    }
    pub fn set_diagnostics(&mut self, diagnostics: Vec<(usize, u8)>) {
        self.diagnostic_lines = diagnostics;
    }

    #[must_use]
    pub fn diagnostic_severity(&self, line: usize) -> Option<u8> {
        if !self.diagnostics_enabled || !self.show_diagnostic_gutter {
            return None;
        }
        self.diagnostic_lines
            .iter()
            .filter(|(candidate, _)| *candidate == line)
            .map(|(_, severity)| *severity)
            .min()
    }

    #[must_use]
    pub fn is_externally_changed(&self) -> bool {
        self.path()
            .is_some_and(|path| self.document.disk_changed(path))
    }

    pub fn save(&mut self) -> std::io::Result<()> {
        if self.historical.is_some() {
            return Err(std::io::Error::other(
                "Historical file is read-only; use Save As to create a copy",
            ));
        }
        if self.hunk_edit.is_some() {
            return Err(std::io::Error::other(
                "Save the edited hunk through its Git action",
            ));
        }
        refuse_lossy_overwrite(&self.document)?;
        let Some(path) = self.path().map(Path::to_path_buf) else {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "document has no path",
            ));
        };
        if self.document.disk_contents_changed(&path) {
            return Err(std::io::Error::other(
                "file changed on disk; reload or explicitly overwrite before saving",
            ));
        }
        write_atomic(&path, self.document.text().as_bytes())?;
        self.document.update_disk(&path);
        self.document.mark_clean();
        Ok(())
    }

    /// Replace the buffer with the current disk contents and refresh the stamp.
    pub fn saved_disk_hash(&self) -> Option<String> {
        crate::lock(&self.document.shared)
            .disk
            .baseline_hash
            .clone()
    }

    pub fn restore_recovery(&mut self, saved_hash: Option<&str>) {
        self.document.mark_dirty();
        let mut shared = crate::lock(&self.document.shared);
        shared.disk.recovery_conflict = self.path.is_some() && saved_hash.is_none();
        shared.disk.baseline_hash = saved_hash.map(str::to_owned);
        // The stamp describes the file as reopened, not the recovered baseline.
        shared.disk.stamp = None;
        shared.disk.differing = None;
    }

    pub fn reload(&mut self) -> std::io::Result<()> {
        let Some(path) = self.path().map(Path::to_path_buf) else {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "document has no path",
            ));
        };
        let bytes = std::fs::read(&path)?;
        self.document
            .with_mut(|document| document.reload_bytes(bytes));
        self.document.update_disk(&path);
        self.document.mark_clean();
        Ok(())
    }

    /// Save even if the file changed on disk. Used only after explicit user
    /// confirmation in the reload/overwrite flow.
    pub fn save_force(&mut self) -> std::io::Result<()> {
        if self.historical.is_some() {
            return Err(std::io::Error::other(
                "Historical files are read-only; use Save As",
            ));
        }
        refuse_lossy_overwrite(&self.document)?;
        let Some(path) = self.path().map(Path::to_path_buf) else {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "document has no path",
            ));
        };
        write_atomic(&path, self.document.text().as_bytes())?;
        self.document.update_disk(&path);
        self.document.mark_clean();
        Ok(())
    }

    pub fn save_as(&mut self, path: PathBuf) -> std::io::Result<()> {
        self.save_as_inner(path, false)
    }

    pub fn save_as_overwrite(&mut self, path: PathBuf) -> std::io::Result<()> {
        self.save_as_inner(path, true)
    }

    pub fn retarget(&mut self, path: PathBuf) {
        self.language_extension = language_extension(&path);
        self.fold_revision = (0, 0);
        self.document.set_file_path(Some(path.clone()));
        self.path = Some(path);
    }

    fn save_as_inner(&mut self, path: PathBuf, overwrite: bool) -> std::io::Result<()> {
        let path = document_path(&path);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        if overwrite {
            write_atomic(&path, self.document.text().as_bytes())?;
        } else {
            write_atomic_new(&path, self.document.text().as_bytes())?;
        }
        if self.historical.is_some() || self.hunk_edit.is_some() {
            self.document = DocumentHandle::new(Document::new(self.document.text()));
            self.historical = None;
            self.hunk_edit = None;
        }
        self.path = Some(path.clone());
        self.document.set_file_path(Some(path.clone()));
        self.document.update_disk(&path);
        self.document.mark_clean();
        self.language_extension = path
            .extension()
            .and_then(|ext| ext.to_str())
            .unwrap_or("txt")
            .to_ascii_lowercase();
        self.fold_revision = (0, 0);
        Ok(())
    }

    #[must_use]
    pub fn language_extension(&self) -> &str {
        &self.language_extension
    }

    pub fn word_wrap_enabled(&self) -> bool {
        self.word_wrap
    }

    pub fn set_wrap_column(&mut self, column: Option<usize>) {
        if self.wrap_column != column {
            self.wrap_column = column;
            self.wrap_offset = 0;
            self.follow_cursor = true;
        }
    }

    pub fn set_word_wrap(&mut self, enabled: bool) {
        if self.word_wrap != enabled {
            self.word_wrap = enabled;
            self.wrap_offset = 0;
            self.scroll_left = 0;
        }
    }

    pub fn ensure_visible(&mut self, view_height: usize, view_width: usize, tab_size: usize) {
        if !self.follow_cursor {
            let max_scroll = self.document.len_lines().saturating_sub(view_height.max(1));
            self.scroll_top = self.scroll_top.min(max_scroll);
            return;
        }
        let cursor = self.document.cursor().position;
        if cursor.line < self.scroll_top {
            self.scroll_top = cursor.line;
        } else if view_height > 0 && cursor.line >= self.scroll_top + view_height {
            self.scroll_top = cursor.line + 1 - view_height;
        }

        let line = self.document.line(cursor.line).unwrap_or_default();
        let cursor_width = column_cells(&line, cursor.column, tab_size);
        let gutter = gutter_width(self.document.len_lines()) + 3;
        let usable = view_width.saturating_sub(gutter);
        if cursor_width < self.scroll_left {
            self.scroll_left = cursor_width;
        } else if usable > 0 && cursor_width >= self.scroll_left + usable {
            self.scroll_left = cursor_width + 1 - usable;
        }
    }

    /// Carves `area` into text, gutter, minimap, scrollbar and inline diff, scrolls to the
    /// cursor, and records the rows the next [`Self::render`] draws.
    pub fn layout(&mut self, area: Rect, view: &EditorView<'_>) {
        let mut area = area;
        self.inline_area = Rect::default();
        self.minimap_area = Rect::default();
        self.signature_area = Rect::default();
        self.scrollbar_area = if !area.is_empty() {
            let bar = Rect::new(area.right() - 1, area.y, 1, area.height);
            area.width = area.width.saturating_sub(1);
            bar
        } else {
            Rect::default()
        };
        if self.minimap && area.width >= 30 && area.height > 0 {
            let width = 10;
            self.minimap_area = Rect::new(area.right() - width, area.y, width, area.height);
            area.width -= width;
        }
        if self.inline_signature.is_some() && area.height > 2 {
            self.signature_area = Rect::new(area.x, area.bottom() - 1, area.width, 1);
            area.height -= 1;
        }
        if self.inline_focus
            && area.height >= 5
            && let Some(browser) = &mut self.inline
        {
            let height = (area.height / 2).clamp(3, self.inline_max_height.clamp(3, 100));
            self.inline_area = Rect::new(area.x, area.bottom() - height, area.width, height);
            area.height -= height;
            browser.layout(self.inline_area);
        }
        self.text_area = area;
        self.cursor_screen = None;
        self.refresh_fold_regions(true);
        if self.word_wrap {
            self.layout_wrapped(area, view);
            return;
        }
        let tab_size = view.tab_size;
        let view_height = area.height as usize;
        let view_width = area.width as usize;
        self.ensure_visible(view_height, view_width, tab_size);

        self.unfold_for_cursor();
        let line_count = self.document.len_lines();
        let gutter = if view.show_line_numbers {
            gutter_width(line_count) + 2
        } else {
            2
        };
        let cursor = self.document.cursor().position;
        let mut visible = Vec::with_capacity(view_height);
        for line in self.scroll_top..line_count {
            if !self.is_hidden(line) {
                visible.push(line);
                if visible.len() >= view_height {
                    break;
                }
            }
        }

        self.document.request_syntax(
            view.syntax,
            &self.language_extension,
            self.syntax_lines(visible.clone()),
        );
        self.rendered_rows = visible.iter().map(|line| (*line, 0)).collect();
        if view.focused
            && let Some(row) = visible.iter().position(|line| *line == cursor.line)
        {
            let line = self.document.line(cursor.line).unwrap_or_default();
            let prefix: String = expand_tabs(
                &line.chars().take(cursor.column).collect::<String>(),
                tab_size,
            );
            let x = area
                .x
                .saturating_add(saturating_u16(gutter))
                .saturating_add(saturating_u16(display_width(&prefix)))
                .saturating_sub(saturating_u16(self.scroll_left))
                .min(area.x + area.width.saturating_sub(1));
            let y = area
                .y
                .saturating_add(saturating_u16(row))
                .min(area.y + area.height.saturating_sub(1));
            self.cursor_screen = Some((x, y));
        }
    }

    /// Lays out and draws the editor into `area`.
    pub fn draw(&mut self, frame: &mut Frame, area: Rect, view: &EditorView<'_>) {
        self.layout(area, view);
        self.render(frame, view);
    }

    /// Draws the editor where the last [`Self::layout`] placed it.
    pub fn render(&self, frame: &mut Frame, view: &EditorView<'_>) {
        let EditorView {
            palette,
            syntax,
            show_line_numbers,
            tab_size,
            focused,
        } = *view;
        if let Some(signature) = &self.inline_signature
            && !self.signature_area.is_empty()
        {
            frame.render_widget(
                ratatui::widgets::Paragraph::new(signature.clone()).style(
                    Style::default()
                        .fg(palette.description)
                        .bg(palette.input_bg),
                ),
                self.signature_area,
            );
        }
        if !self.inline_area.is_empty()
            && let Some(browser) = &self.inline
        {
            browser.render(frame, self.inline_area, palette, focused);
        }
        let area = self.text_area;
        if self.word_wrap {
            self.render_wrapped(frame, view);
            self.render_minimap(frame, palette, syntax, tab_size);
            self.render_scrollbar(frame, palette);
            return;
        }
        let view_height = area.height as usize;
        let view_width = area.width as usize;
        let line_count = self.document.len_lines();
        let gutter = if show_line_numbers {
            gutter_width(line_count) + 2
        } else {
            2
        };
        let cursor = self.document.cursor().position;
        let visible = self
            .rendered_rows
            .iter()
            .map(|(line, _)| *line)
            .collect::<Vec<_>>();
        let mut lines = Vec::with_capacity(view_height);
        for row in 0..view_height {
            let Some(line_index) = visible.get(row).copied() else {
                lines.push(Line::raw(""));
                continue;
            };
            let raw = self.document.line(line_index).unwrap_or_default();
            let highlighted = syntax
                .document_line(self.document.syntax_id(), self.revision(), line_index)
                .unwrap_or_else(|| {
                    vec![HighlightSpan {
                        text: raw.clone(),
                        fg: None,
                        bg: None,
                        bold: false,
                        italic: false,
                        underline: false,
                    }]
                });
            let mut spans = Vec::new();
            if show_line_numbers {
                let active = line_index == cursor.line;
                let severity = self.diagnostic_severity(line_index);
                let line_number_color = match severity {
                    Some(1) => palette.error,
                    Some(2) => palette.warning,
                    _ if active => palette.line_number_active,
                    _ => palette.line_number,
                };
                spans.push(Span::styled(
                    format!(
                        "{:>gutter$} ",
                        if self.relative_numbers && line_index != cursor.line {
                            line_index.abs_diff(cursor.line)
                        } else {
                            line_index + 1
                        },
                        gutter = gutter.saturating_sub(3)
                    ),
                    Style::default().fg(line_number_color),
                ));
            }
            spans.push(Span::styled(
                self.fold_marker(line_index),
                Style::default().fg(palette.description),
            ));
            let style = if line_index == cursor.line {
                Style::default().bg(palette.line_highlight)
            } else {
                Style::default()
            };
            let spans_for_line = styled_slice(
                &highlighted,
                palette,
                tab_size,
                0,
                usize::MAX,
                self.scroll_left,
                view_width.saturating_sub(gutter),
            );
            if let Some((first, rest)) = spans_for_line.split_first() {
                spans.push(Span::styled(first.0.clone(), first.1.patch(style)));
                for (text, span_style) in rest {
                    spans.push(Span::styled(text.clone(), span_style.patch(style)));
                }
            }
            if self.folded.iter().any(|(start, _)| *start == line_index) {
                spans.push(Span::styled(" …", Style::default().fg(palette.description)));
            }
            lines.push(Line::from(spans).style(style));
        }

        frame.render_widget(
            ratatui::widgets::Paragraph::new(lines).style(
                Style::default()
                    .fg(palette.foreground)
                    .bg(palette.background),
            ),
            area,
        );

        if let Some(position) = self.cursor_screen {
            frame.set_cursor_position(position);
        }
        self.paint_overlays(frame, area, palette, gutter, tab_size);
        self.render_minimap(frame, palette, syntax, tab_size);
        self.render_scrollbar(frame, palette);
        if let Some(browser) = &self.inline
            && !self.is_dirty()
        {
            for (row, (line, _)) in self
                .rendered_rows
                .iter()
                .enumerate()
                .take(area.height as usize)
            {
                if browser.diff.hunks.iter().any(|h| {
                    *line + 1 >= h.new_start.max(1)
                        && *line + 1 < h.new_start.max(1) + h.new_count.max(1)
                }) {
                    frame.buffer_mut().set_string(
                        area.x,
                        area.y + saturating_u16(row),
                        "▎",
                        Style::default().fg(palette.diff_add),
                    );
                }
            }
        }
    }

    fn scrollbar_hidden(&self) -> Vec<(usize, usize)> {
        let count = self.document.len_lines();
        let mut ranges = self
            .folded
            .iter()
            .map(|(a, b)| {
                (
                    a.saturating_add(1).min(count),
                    b.saturating_add(1).min(count),
                )
            })
            .filter(|(a, b)| a < b)
            .collect::<Vec<_>>();
        ranges.sort_unstable();
        let mut merged: Vec<(usize, usize)> = Vec::new();
        for (a, b) in ranges {
            if let Some(last) = merged.last_mut()
                && a <= last.1
            {
                last.1 = last.1.max(b);
            } else {
                merged.push((a, b));
            }
        }
        merged
    }
    fn scrollbar_metrics(&self) -> (usize, usize, usize) {
        let hidden = self.scrollbar_hidden();
        let total = self
            .document
            .len_lines()
            .saturating_sub(hidden.iter().map(|(a, b)| b - a).sum::<usize>());
        let top = self.scroll_top.saturating_sub(
            hidden
                .iter()
                .map(|(a, b)| self.scroll_top.min(*b).saturating_sub(*a))
                .sum::<usize>(),
        );
        let visible = self
            .rendered_rows
            .iter()
            .map(|(line, _)| *line)
            .collect::<std::collections::BTreeSet<_>>()
            .len()
            .min(self.scrollbar_area.height as usize);
        (total, visible, top)
    }
    fn render_scrollbar(&self, frame: &mut Frame, palette: Palette) {
        let (total, visible, top) = self.scrollbar_metrics();
        crate::scrollbar::paint(frame, self.scrollbar_area, total, visible, top, palette);
    }
    fn minimap_top(&self) -> usize {
        let count = self.document.len_lines();
        let height = self.minimap_area.height as usize;
        self.scroll_top
            .min(count.saturating_sub(height))
            .saturating_mul(count.div_ceil(3).saturating_sub(height))
            / count.saturating_sub(height).max(1)
    }
    fn syntax_lines(&self, mut lines: Vec<usize>) -> Vec<usize> {
        if !self.minimap_area.is_empty() {
            let start = self.minimap_top() * 3;
            lines.extend(
                start
                    ..(start + self.minimap_area.height as usize * 3)
                        .min(self.document.len_lines()),
            );
        }
        lines.sort_unstable();
        lines.dedup();
        lines
    }
    fn render_minimap(
        &self,
        frame: &mut Frame,
        palette: Palette,
        syntax: &SyntaxHighlighter,
        tab_size: usize,
    ) {
        let area = self.minimap_area;
        if area.is_empty() {
            return;
        }
        let width = area.width as usize;
        let columns_per_cell = 120usize.div_ceil(width);
        let top = self.minimap_top();
        let viewport_end = (self.scroll_top + self.rendered_rows.len()).div_ceil(3);
        for row in 0..area.height as usize {
            let source = (top + row) * 3;
            let end = (source + 3).min(self.document.len_lines());
            let in_view = top + row >= self.scroll_top / 3 && top + row < viewport_end;
            let bg = if in_view {
                palette.selection_bg
            } else {
                palette.sidebar
            };
            let mut density = vec![0usize; width];
            let mut colors = vec![None; width];
            for line in source..end {
                let cached = syntax.document_line(self.document.syntax_id(), self.revision(), line);
                let spans = cached.unwrap_or_else(|| {
                    vec![HighlightSpan {
                        text: self.document.with(|d| d.line_prefix(line, 384)),
                        fg: None,
                        bg: None,
                        bold: false,
                        italic: false,
                        underline: false,
                    }]
                });
                let mut column = 0;
                for span in spans {
                    let color = span.fg.as_deref().and_then(crate::palette::hex_color);
                    for ch in span.text.chars().take(384) {
                        if column >= 120 {
                            break;
                        }
                        if ch == '\t' {
                            column += tab_size.max(1) - column % tab_size.max(1);
                            continue;
                        }
                        let cell = (column / columns_per_cell).min(width - 1);
                        if !ch.is_whitespace() {
                            density[cell] += 1;
                            if colors[cell].is_none() {
                                colors[cell] = color;
                            }
                        }
                        column += tide_core::display_width(&ch.to_string());
                    }
                    if column >= 120 {
                        break;
                    }
                }
            }
            for col in 0..width {
                let total = columns_per_cell * end.saturating_sub(source).max(1);
                let glyph = match density[col] * 4 {
                    0 => " ",
                    n if n > total * 3 => "█",
                    n if n > total * 2 => "▓",
                    n if n > total => "▒",
                    _ => "░",
                };
                frame.buffer_mut()[(area.x + saturating_u16(col), area.y + saturating_u16(row))]
                    .set_symbol(glyph)
                    .set_style(
                        Style::default()
                            .fg(colors[col].unwrap_or(palette.foreground))
                            .bg(bg),
                    );
            }
        }
    }

    fn layout_wrapped(&mut self, area: Rect, view: &EditorView<'_>) {
        let (show_numbers, tab_size) = (view.show_line_numbers, view.tab_size);
        let gutter = if show_numbers {
            gutter_width(self.document.len_lines()) + 2
        } else {
            2
        };
        let width = (area.width as usize)
            .saturating_sub(gutter)
            .min(self.wrap_column.unwrap_or(usize::MAX))
            .max(1);
        let height = (area.height as usize).max(1);
        let cursor = self.document.cursor().position;
        let collect = |top: usize, offset: usize| {
            let mut rows = Vec::new();
            for line in top..self.document.len_lines() {
                if self.is_hidden(line) {
                    continue;
                }
                for (index, (start, end, text)) in wrap_segments(
                    &self.document.line(line).unwrap_or_default(),
                    width,
                    tab_size,
                )
                .into_iter()
                .enumerate()
                {
                    if line == top && index < offset {
                        continue;
                    }
                    rows.push((line, start, end, text));
                    if rows.len() >= height {
                        return rows;
                    }
                }
            }
            rows
        };
        let mut rows = collect(self.scroll_top, self.wrap_offset);
        let locate = |rows: &[(usize, usize, usize, String)]| {
            rows.iter().position(|(line, start, end, _)| {
                *line == cursor.line
                    && cursor.column >= *start
                    && (cursor.column < *end
                        || cursor.column == *end && *end == self.document.line_len_chars(*line))
            })
        };
        if self.follow_cursor && locate(&rows).is_none() {
            let segments = wrap_segments(
                &self.document.line(cursor.line).unwrap_or_default(),
                width,
                tab_size,
            );
            let index = segments
                .iter()
                .position(|(start, end, _)| {
                    cursor.column >= *start
                        && (cursor.column < *end
                            || cursor.column == *end
                                && *end == self.document.line_len_chars(cursor.line))
                })
                .unwrap_or(0);
            let top = cursor.line;
            let offset = index.saturating_sub(height - 1);
            rows = collect(top, offset);
            self.scroll_top = top;
            self.wrap_offset = offset;
        }
        self.document.request_syntax(
            view.syntax,
            &self.language_extension,
            self.syntax_lines(rows.iter().map(|(line, _, _, _)| *line).collect()),
        );
        self.rendered_rows = rows
            .iter()
            .map(|(line, start, _, _)| (*line, *start))
            .collect();
        self.wrapped_ends = rows.iter().map(|(_, _, end, _)| *end).collect();
        if view.focused
            && let Some(row) = locate(&rows)
        {
            let raw = self.document.line(cursor.line).unwrap_or_default();
            let x = column_cells(&raw, cursor.column, tab_size).saturating_sub(column_cells(
                &raw,
                rows[row].1,
                tab_size,
            ));
            let point = (
                area.x
                    .saturating_add(saturating_u16(gutter + x))
                    .min(area.right().saturating_sub(1)),
                area.y.saturating_add(saturating_u16(row)),
            );
            self.cursor_screen = Some(point);
        }
    }

    fn render_wrapped(&self, frame: &mut Frame, view: &EditorView<'_>) {
        let EditorView {
            palette,
            syntax,
            show_line_numbers: show_numbers,
            tab_size,
            ..
        } = *view;
        let area = self.text_area;
        let gutter = if show_numbers {
            gutter_width(self.document.len_lines()) + 2
        } else {
            2
        };
        let width = (area.width as usize)
            .saturating_sub(gutter)
            .min(self.wrap_column.unwrap_or(usize::MAX))
            .max(1);
        let cursor = self.document.cursor().position;
        let rows = self
            .rendered_rows
            .iter()
            .zip(&self.wrapped_ends)
            .map(|((line, start), end)| (*line, *start, *end, ()))
            .collect::<Vec<_>>();
        let lines = rows
            .iter()
            .map(|(line, start, end, _text)| {
                let mut spans = Vec::new();
                if show_numbers {
                    spans.push(Span::styled(
                        if *start == 0 {
                            format!(
                                "{:>width$} ",
                                if self.relative_numbers && *line != cursor.line {
                                    line.abs_diff(cursor.line)
                                } else {
                                    line + 1
                                },
                                width = gutter - 3
                            )
                        } else {
                            " ".repeat(gutter - 2)
                        },
                        Style::default().fg(palette.line_number),
                    ));
                }
                spans.push(Span::styled(
                    if *start == 0 {
                        self.fold_marker(*line)
                    } else {
                        "  "
                    },
                    Style::default().fg(palette.description),
                ));
                let highlighted = syntax
                    .document_line(self.document.syntax_id(), self.revision(), *line)
                    .unwrap_or_else(|| {
                        vec![HighlightSpan {
                            text: self.document.line(*line).unwrap_or_default(),
                            fg: None,
                            bg: None,
                            bold: false,
                            italic: false,
                            underline: false,
                        }]
                    });
                // Every visual row of the cursor line gets the current-line
                // background, as in the unwrapped renderer.
                let line_style = if *line == cursor.line {
                    Style::default().bg(palette.line_highlight)
                } else {
                    Style::default()
                };
                spans.extend(
                    styled_slice(&highlighted, palette, tab_size, *start, *end, 0, width)
                        .into_iter()
                        .map(|(text, style)| Span::styled(text, style.patch(line_style))),
                );
                Line::from(spans).style(line_style)
            })
            .collect::<Vec<_>>();
        frame.render_widget(
            ratatui::widgets::Paragraph::new(lines).style(
                Style::default()
                    .fg(palette.foreground)
                    .bg(palette.background),
            ),
            area,
        );
        if let Some(point) = self.cursor_screen {
            frame.set_cursor_position(point);
        }
        self.paint_overlays(frame, area, palette, gutter, tab_size);
        if let Some(browser) = &self.inline
            && !self.is_dirty()
        {
            for (row, (line, _)) in self
                .rendered_rows
                .iter()
                .enumerate()
                .take(area.height as usize)
            {
                if browser.diff.hunks.iter().any(|h| {
                    *line + 1 >= h.new_start.max(1)
                        && *line + 1 < h.new_start.max(1) + h.new_count.max(1)
                }) {
                    frame.buffer_mut().set_string(
                        area.x,
                        area.y + saturating_u16(row),
                        "▎",
                        Style::default().fg(palette.diff_add),
                    );
                }
            }
        }
    }

    #[must_use]
    pub fn extra_cursors(&self) -> Vec<tide_core::Cursor> {
        crate::lock(&self.document.view).extra.clone()
    }

    pub fn clear_extra_cursors(&mut self) {
        crate::lock(&self.document.view).extra.clear();
    }

    pub fn clear_selection(&mut self) {
        crate::lock(&self.document.view).extra.clear();
        self.document.clear_selection();
    }

    pub fn view_state(&self) -> serde_json::Value {
        serde_json::json!({"cursors":self.cursor_states().iter().map(|c|serde_json::json!({"position":[c.position.line,c.position.column],"anchor":c.selection.map(|s|[s.anchor.line,s.anchor.column])})).collect::<Vec<_>>(),"folds":self.folded.iter().filter_map(|(a,_)|u32::try_from(*a).ok()).collect::<Vec<_>>(),"top":self.scroll_top,"left":self.scroll_left,"wrapOffset":self.wrap_offset})
    }
    pub fn restore_view_state(&mut self, value: &serde_json::Value) {
        let number = |key: &str, limit: usize| {
            value
                .get(key)
                .and_then(|v| v.as_u64())
                .map_or(0, saturating_usize)
                .min(limit)
        };
        let position = |v: &serde_json::Value| {
            let line = v
                .get(0)
                .and_then(|v| v.as_u64())
                .map_or(0, saturating_usize)
                .min(self.document.len_lines().saturating_sub(1));
            let column = v
                .get(1)
                .and_then(|v| v.as_u64())
                .map_or(0, saturating_usize)
                .min(self.document.line_len_chars(line));
            Position::new(line, column)
        };
        if let Some(cursors) = value.get("cursors").and_then(|v| v.as_array()) {
            let cursors = cursors
                .iter()
                .take(1000)
                .filter_map(|v| {
                    let p = position(v.get("position")?);
                    Some(
                        if let Some(anchor) = v.get("anchor").filter(|a| a.is_array()) {
                            tide_core::Cursor::with_selection(position(anchor), p)
                        } else {
                            tide_core::Cursor::at(p.line, p.column)
                        },
                    )
                })
                .collect::<Vec<_>>();
            self.set_cursor_states(&cursors);
        }
        if let Some(folds) = value.get("folds").and_then(|v| v.as_array()) {
            self.restore_folds(
                &folds
                    .iter()
                    .take(50000)
                    .filter_map(|v| v.as_u64().and_then(|n| u32::try_from(n).ok()))
                    .collect::<Vec<_>>(),
            );
        }
        self.scroll_top = number("top", self.document.len_lines().saturating_sub(1));
        self.scroll_left = number("left", 1_000_000);
        self.wrap_offset = number("wrapOffset", self.document.line_len_chars(self.scroll_top));
        self.follow_cursor = false;
    }

    fn cursor_states(&self) -> Vec<tide_core::Cursor> {
        let mut cursors = Vec::with_capacity(self.extra_cursors().len() + 1);
        cursors.push(self.document.cursor());
        cursors.extend(self.extra_cursors().iter().copied());
        cursors
    }

    fn set_cursor_states(&mut self, cursors: &[tide_core::Cursor]) {
        if let Some(primary) = cursors.first() {
            if let Some(selection) = primary.selection {
                self.document.set_selection(selection);
            } else {
                self.document.set_cursor(primary.position);
            }
        }
        crate::lock(&self.document.view).extra = cursors.iter().skip(1).copied().collect();
    }

    fn apply_cursor_edits(&mut self, edits: Vec<(Range, String)>) {
        if let Ok(results) = self.document.apply_edits(&edits, 0) {
            self.set_cursor_states(&results);
        }
    }

    /// Add a cursor on the nearest line above all existing cursors.
    pub fn add_cursor_above(&mut self) {
        let cursors = self.cursor_states();
        let Some(min_line) = cursors.iter().map(|cursor| cursor.position.line).min() else {
            return;
        };
        if min_line == 0 {
            return;
        }
        let column = cursors[0].position.column;
        let mut cursors = cursors;
        cursors.push(tide_core::Cursor::at(min_line - 1, column));
        self.set_cursor_states(&cursors);
    }

    /// Add a cursor on the nearest line below all existing cursors.
    pub fn add_cursor_below(&mut self) {
        let cursors = self.cursor_states();
        let Some(max_line) = cursors.iter().map(|cursor| cursor.position.line).max() else {
            return;
        };
        if max_line + 1 >= self.document.len_lines() {
            return;
        }
        let column = cursors[0].position.column;
        let mut cursors = cursors;
        cursors.push(tide_core::Cursor::at(max_line + 1, column));
        self.set_cursor_states(&cursors);
    }

    fn move_cursors<F>(&mut self, extend: bool, action: F)
    where
        F: Fn(&Document, Position) -> Position,
    {
        if self.extra_cursors().is_empty() {
            return;
        }
        let cursors = self.cursor_states();
        let positions = self.document.with(|document| {
            cursors
                .iter()
                .map(|cursor| action(document, cursor.position))
                .collect::<Vec<_>>()
        });
        let next = cursors
            .iter()
            .zip(positions)
            .map(|(cursor, position)| {
                if extend {
                    let anchor = cursor
                        .selection
                        .map_or(cursor.position, |selection| selection.anchor);
                    tide_core::Cursor::with_selection(anchor, position)
                } else {
                    tide_core::Cursor::at(position.line, position.column)
                }
            })
            .collect::<Vec<_>>();
        self.set_cursor_states(&next);
    }

    fn word_at_cursor(&self) -> Option<String> {
        if let Some((start, end)) = self.document.selection_positions()
            && start != end
        {
            return self.document.with(|document| {
                let text = document.text();
                let start = document.position_to_char(start);
                let end = document.position_to_char(end);
                Some(
                    text.chars()
                        .skip(start)
                        .take(end - start)
                        .collect::<String>(),
                )
            });
        }
        let position = self.document.cursor().position;
        let line = self.document.line(position.line)?;
        let chars = line.chars().collect::<Vec<_>>();
        if chars.is_empty() {
            return None;
        }
        let is_word = |ch: char| ch.is_alphanumeric() || ch == '_';
        let mut start = position.column.min(chars.len());
        if start == chars.len() || !is_word(chars[start]) {
            start = start.saturating_sub(1);
        }
        if start >= chars.len() || !is_word(chars[start]) {
            return None;
        }
        let mut left = start;
        while left > 0 && is_word(chars[left - 1]) {
            left -= 1;
        }
        let mut right = start;
        while right < chars.len() && is_word(chars[right]) {
            right += 1;
        }
        Some(chars[left..right].iter().collect())
    }

    fn occurrences(&self, word: &str) -> Vec<(Position, Position)> {
        if word.is_empty() {
            return Vec::new();
        }
        let needle = word.to_lowercase();
        let lines = self.document.lines();
        let mut found = Vec::new();
        for (line_index, line) in lines.iter().enumerate() {
            let case_insensitive = line.is_ascii() && needle.is_ascii();
            let haystack = if case_insensitive {
                line.to_lowercase()
            } else {
                line.clone()
            };
            let search = if case_insensitive {
                needle.as_str()
            } else {
                word
            };
            let mut offset = 0;
            while let Some(index) = haystack[offset..].find(search) {
                let byte_start = offset + index;
                let byte_end = byte_start + search.len();
                let start_column = line[..byte_start].chars().count();
                let end_column = line[..byte_end].chars().count();
                found.push((
                    Position::new(line_index, start_column),
                    Position::new(line_index, end_column),
                ));
                offset = byte_end.max(byte_start + 1);
            }
        }
        found
    }

    pub fn select_all_occurrences(&mut self) -> bool {
        let Some(word) = self.word_at_cursor() else {
            return false;
        };
        let occurrences = self.occurrences(&word);
        if occurrences.is_empty() {
            return false;
        }
        let cursors = occurrences
            .into_iter()
            .map(|(start, end)| tide_core::Cursor::with_selection(start, end))
            .collect::<Vec<_>>();
        self.set_cursor_states(&cursors);
        self.follow_cursor = true;
        true
    }

    pub fn select_next_match(&mut self) -> bool {
        let Some(word) = self.word_at_cursor() else {
            return false;
        };
        let occurrences = self.occurrences(&word);
        if occurrences.is_empty() {
            return false;
        }
        let mut cursors = self.cursor_states();
        let selected = cursors
            .iter()
            .filter_map(|cursor| cursor.range())
            .collect::<Vec<_>>();
        let from = cursors[0].position;
        let next = occurrences
            .iter()
            .copied()
            .find(|(start, end)| {
                let range = Range::new(*start, *end);
                !selected.contains(&range) && (start.line, start.column) >= (from.line, from.column)
            })
            .or_else(|| {
                occurrences
                    .iter()
                    .copied()
                    .find(|(start, end)| !selected.contains(&Range::new(*start, *end)))
            });
        let Some((start, end)) = next else {
            return false;
        };
        if cursors[0].selection.is_some() {
            cursors.push(tide_core::Cursor::with_selection(start, end));
        } else {
            cursors[0] = tide_core::Cursor::with_selection(start, end);
        }
        self.set_cursor_states(&cursors);
        self.follow_cursor = true;
        true
    }

    #[must_use]
    pub fn folded_ranges(&self) -> &[(usize, usize)] {
        &self.folded
    }

    fn is_hidden(&self, line: usize) -> bool {
        self.folded
            .iter()
            .any(|(start, end)| line > *start && line <= *end)
    }

    fn unfold_for_cursor(&mut self) {
        let line = self.document.cursor().position.line;
        let hidden = self
            .folded
            .iter()
            .find(|(start, end)| line > *start && line <= *end)
            .copied();
        if let Some((start, _)) = hidden {
            self.document.set_cursor(Position::new(start, 0));
        }
    }

    fn ensure_fold_regions(&mut self) {
        self.refresh_fold_regions(false);
    }

    /// Whether a large file's deferred fold/delimiter reparse is due.
    #[must_use]
    pub fn fold_refresh_due(&self) -> bool {
        self.fold_deferred
            .is_some_and(|(_, since)| since.elapsed() >= FOLD_IDLE)
    }

    /// Re-lexes delimiters from the first line edited since the last parse.
    /// With `defer`, files over [`DEFERRED_FOLD_CHARS`] wait until edits
    /// pause for [`FOLD_IDLE`], keeping the previous index meanwhile.
    fn refresh_fold_regions(&mut self, defer: bool) {
        let key = (self.document.syntax_id(), self.revision());
        if self.fold_revision == key {
            self.fold_deferred = None;
            return;
        }
        if defer && self.document.with(Document::len_chars) > DEFERRED_FOLD_CHARS {
            match self.fold_deferred {
                Some((pending, since)) if pending == key.1 && since.elapsed() >= FOLD_IDLE => {}
                Some((pending, _)) if pending == key.1 => return,
                _ => {
                    self.fold_deferred = Some((key.1, Instant::now()));
                    return;
                }
            }
        }
        self.fold_deferred = None;
        let first = if self.fold_revision.0 == key.0 {
            self.document
                .first_change_since(self.fold_revision.1)
                .min(self.fold_indents.len())
        } else {
            0
        };
        let (delimiters, indents) = (&mut self.delimiters, &mut self.fold_indents);
        let extension = &self.language_extension;
        self.document.with(|document| {
            let count = document.len_lines();
            let text = |line| document.line_str(line).unwrap_or_default();
            delimiters.reparse(first, count, text, extension);
            indents.truncate(first);
            indents.extend((first..count).map(|line| {
                let text = text(line);
                (!text.trim().is_empty())
                    .then(|| text.chars().take_while(|ch| ch.is_whitespace()).count())
            }));
        });
        self.fold_regions = compute_fold_regions(&self.fold_indents, &self.delimiters);
        self.folded
            .retain(|range| self.fold_regions.contains(range));
        self.fold_revision = key;
    }

    pub fn restore_folds(&mut self, starts: &[u32]) {
        self.ensure_fold_regions();
        self.folded = self
            .fold_regions
            .iter()
            .copied()
            .filter(|(start, _)| u32::try_from(*start).is_ok_and(|start| starts.contains(&start)))
            .collect();
    }

    pub fn toggle_fold_at(&mut self, line: usize) -> bool {
        self.ensure_fold_regions();
        if self.folded.iter().any(|(start, _)| *start == line) {
            self.folded.retain(|(start, _)| *start != line);
            return true;
        }
        if let Some(range) = self
            .fold_regions
            .iter()
            .find(|(start, _)| *start == line)
            .copied()
        {
            self.folded.push(range);
            if self.document.cursor().position.line > range.0
                && self.document.cursor().position.line <= range.1
            {
                self.document.set_cursor(Position::new(line, 0));
            }
            return true;
        }
        false
    }

    pub fn fold(&mut self) {
        self.ensure_fold_regions();
        let cursor = self.document.cursor().position.line;
        let region = self
            .fold_regions
            .iter()
            .copied()
            .filter(|(start, end)| *start <= cursor && *end >= cursor)
            .max_by_key(|(start, _)| *start)
            .or_else(|| {
                self.fold_regions
                    .iter()
                    .copied()
                    .find(|(start, _)| *start >= cursor)
            });
        if let Some((start, end)) = region {
            if !self.folded.contains(&(start, end)) {
                self.folded.push((start, end));
            }
            self.document.set_cursor(Position::new(start, 0));
        }
    }

    pub fn fold_all(&mut self) {
        self.ensure_fold_regions();
        self.folded.clone_from(&self.fold_regions);
        self.unfold_for_cursor();
    }

    pub fn unfold(&mut self) {
        let line = self.document.cursor().position.line;
        self.folded
            .retain(|(start, end)| !(line >= *start && line <= *end));
    }

    pub fn unfold_all(&mut self) {
        self.folded.clear();
    }

    fn paint_overlays(
        &self,
        frame: &mut Frame,
        area: Rect,
        palette: Palette,
        gutter: usize,
        tab_size: usize,
    ) {
        let cache = self.search_cache.borrow();
        let matches = cache
            .as_ref()
            .filter(|cache| cache.revision == self.revision());
        let cursors = self.cursor_states();
        let matching = self.delimiters.enclosing(self.document.cursor().position);
        let boundary = matching
            .map(|(open, close)| (open.line, close.line))
            .or_else(|| {
                matches!(
                    self.language_extension.as_str(),
                    "py" | "pyw" | "yaml" | "yml" | "rb" | "sh"
                )
                .then(|| {
                    let line = self.document.cursor().position.line;
                    self.fold_regions
                        .iter()
                        .filter(|(start, end)| *start <= line && line <= *end)
                        .max_by_key(|(start, _)| *start)
                        .copied()
                })
                .flatten()
            });
        for (row, (line, start)) in self
            .rendered_rows
            .iter()
            .enumerate()
            .take(area.height as usize)
        {
            let raw = self.document.line(*line).unwrap_or_default();
            let origin = if self.word_wrap {
                column_cells(&raw, *start, tab_size)
            } else {
                self.scroll_left
            };
            let left = (area.x as usize + gutter).min(area.right() as usize);
            let right = (area.right() as usize).min(if self.word_wrap {
                left + self.wrap_column.unwrap_or(area.width as usize)
            } else {
                usize::MAX
            });
            let mut paint = |range: Range, style: Style| {
                let range = range.normalized();
                if *line < range.start.line || *line > range.end.line || range.is_empty() {
                    return;
                }
                let start = if *line == range.start.line {
                    range.start.column
                } else {
                    0
                };
                let end = if *line == range.end.line {
                    range.end.column
                } else {
                    raw.chars().count() + 1
                };
                let start = column_cells(&raw, start, tab_size);
                let end = column_cells(&raw, end.min(raw.chars().count()), tab_size)
                    + usize::from(end > raw.chars().count());
                let x = left + start.saturating_sub(origin);
                let stop = (left + end.saturating_sub(origin)).min(right);
                if stop > x && x < right {
                    frame.buffer_mut().set_style(
                        Rect::new(
                            saturating_u16(x),
                            area.y + saturating_u16(row),
                            saturating_u16(stop - x),
                            1,
                        ),
                        style,
                    );
                }
            };
            if let Some((open, close)) = boundary
                && open != close
                && (*line == open || *line == close)
            {
                paint(
                    Range::new(
                        Position::new(*line, 0),
                        Position::new(*line, raw.chars().count()),
                    ),
                    Style::default().bg(palette.line_highlight),
                );
            }
            if let Some((open, close)) = matching {
                for point in [open, close] {
                    if matches.is_some_and(|cache| {
                        let i = cache.ranges.partition_point(|range| range.end <= point);
                        cache
                            .ranges
                            .get(i)
                            .is_some_and(|range| range.start <= point && point < range.end)
                    }) {
                        continue;
                    }
                    if cursors.iter().any(|cursor| {
                        cursor.selection.is_some_and(|selection| {
                            let range = selection.range();
                            range.start <= point && point < range.end
                        })
                    }) {
                        continue;
                    }
                    paint(
                        Range::new(point, Position::new(point.line, point.column + 1)),
                        Style::default()
                            .bg(palette.bracket_match_bg)
                            .fg(palette.bracket_match_fg)
                            .add_modifier(Modifier::BOLD | Modifier::UNDERLINED),
                    );
                }
            }
            if let Some(matches) = matches {
                let first = matches
                    .ranges
                    .partition_point(|range| range.end.line < *line);
                for range in matches.ranges[first..]
                    .iter()
                    .take_while(|range| range.start.line <= *line)
                {
                    paint(
                        *range,
                        Style::default()
                            .bg(palette.line_highlight)
                            .add_modifier(Modifier::UNDERLINED),
                    );
                }
            }
            if self.diagnostics_enabled && self.underline_diagnostics {
                for (range, severity) in &self.diagnostic_ranges {
                    let mut range = *range;
                    if range.is_empty() {
                        range.end.column = range.end.column.saturating_add(1);
                    }
                    paint(
                        range,
                        Style::default()
                            .add_modifier(Modifier::UNDERLINED)
                            .underline_color(if *severity == 1 {
                                palette.error
                            } else {
                                palette.warning
                            }),
                    );
                }
            }
            for cursor in &cursors {
                if let Some(selection) = cursor.selection {
                    paint(selection.range(), Style::default().bg(palette.selection));
                }
            }
            for cursor in self
                .extra_cursors()
                .iter()
                .filter(|cursor| cursor.position.line == *line)
            {
                let col = column_cells(&raw, cursor.position.column, tab_size);
                let x = left + col.saturating_sub(origin);
                if col >= origin && x < right {
                    frame.buffer_mut().set_style(
                        Rect::new(saturating_u16(x), area.y + saturating_u16(row), 1, 1),
                        Style::default().bg(palette.accent),
                    );
                }
            }
        }
    }

    pub fn insert_char(&mut self, ch: char) {
        if !self.extra_cursors().is_empty() {
            self.insert_text(&ch.to_string());
            return;
        }
        let has_selection = self
            .document
            .selection_positions()
            .is_some_and(|(start, end)| start != end);
        if !has_selection {
            let cursor = self.document.cursor().position;
            if is_closing_pair(ch) && self.character_at(cursor) == Some(ch) {
                self.document.move_right(false);
                return;
            }
            if let Some(close) = closing_pair(ch) {
                self.insert_text(&format!("{ch}{close}"));
                self.document.move_left(false);
                return;
            }
        }
        self.insert_text(&ch.to_string());
    }

    #[must_use]
    pub fn character_at(&self, position: Position) -> Option<char> {
        self.document
            .line(position.line)?
            .chars()
            .nth(position.column)
    }

    pub fn insert_text(&mut self, text: &str) {
        self.follow_cursor = true;
        self.folded.clear();
        if !self.extra_cursors().is_empty() {
            let cursors = self.cursor_states();
            let edits = cursors
                .iter()
                .map(|cursor| {
                    (
                        cursor
                            .range()
                            .unwrap_or(Range::new(cursor.position, cursor.position)),
                        text.to_owned(),
                    )
                })
                .collect();
            self.apply_cursor_edits(edits);
            return;
        }
        if let Some((start, end)) = self.document.selection_positions()
            && start != end
        {
            self.document.replace(Range::new(start, end), text);
            return;
        }
        let position = self.document.cursor().position;
        self.document.insert(position, text);
    }

    pub fn newline(&mut self) {
        self.follow_cursor = true;
        if !self.extra_cursors().is_empty() {
            let cursors = self.cursor_states();
            let edits = self.document.with(|document| {
                let line_break = document.line_break();
                cursors
                    .iter()
                    .map(|cursor| {
                        let line = document.line(cursor.position.line).unwrap_or_default();
                        let before: String = line.chars().take(cursor.position.column).collect();
                        let indent = before
                            .chars()
                            .take_while(|ch| ch.is_whitespace())
                            .collect::<String>();
                        let opens = matches!(
                            before.trim_end().chars().last(),
                            Some('{' | '[' | '(' | ':')
                        );
                        let extra = if opens && self.auto_indent == "full" {
                            self.indent_unit.as_str()
                        } else {
                            ""
                        };
                        (
                            cursor
                                .range()
                                .unwrap_or(Range::new(cursor.position, cursor.position)),
                            format!(
                                "{line_break}{}{extra}",
                                if self.auto_indent == "none" {
                                    ""
                                } else {
                                    indent.as_str()
                                }
                            ),
                        )
                    })
                    .collect::<Vec<_>>()
            });
            self.apply_cursor_edits(edits);
            return;
        }
        let position = self.document.cursor().position;
        let line = self.document.line(position.line).unwrap_or_default();
        let before: String = line.chars().take(position.column).collect();
        let indent = before
            .chars()
            .take_while(|ch| ch.is_whitespace())
            .collect::<String>();
        let opens = matches!(
            before.trim_end().chars().last(),
            Some('{' | '[' | '(' | ':')
        );
        let extra = if opens && self.auto_indent == "full" {
            self.indent_unit.as_str()
        } else {
            ""
        };
        let line_break = self.document.with(Document::line_break);
        self.insert_text(&format!(
            "{line_break}{}{extra}",
            if self.auto_indent == "none" {
                ""
            } else {
                indent.as_str()
            }
        ));
    }

    pub fn backspace(&mut self) {
        self.follow_cursor = true;
        self.folded.clear();
        if !self.extra_cursors().is_empty() {
            let cursors = self.cursor_states();
            let edits = self.document.with(|document| {
                cursors
                    .iter()
                    .map(|cursor| {
                        let range = if let Some(selection) = cursor.selection {
                            let range = selection.range();
                            (!range.is_empty()).then_some(range)
                        } else {
                            None
                        };
                        let range = range.or_else(|| {
                            let position = cursor.position;
                            if position.line == 0 && position.column == 0 {
                                None
                            } else if position.column > 0 {
                                Some(Range::new(
                                    Position::new(
                                        position.line,
                                        document.prev_grapheme_column(position),
                                    ),
                                    position,
                                ))
                            } else {
                                Some(Range::new(
                                    Position::new(
                                        position.line - 1,
                                        document.line_len_chars(position.line - 1),
                                    ),
                                    position,
                                ))
                            }
                        });
                        (
                            range.unwrap_or(Range::new(cursor.position, cursor.position)),
                            String::new(),
                        )
                    })
                    .collect::<Vec<_>>()
            });
            self.apply_cursor_edits(edits);
            return;
        }
        if let Some((start, end)) = self.document.selection_positions()
            && start != end
        {
            self.document.delete(Range::new(start, end));
            self.document.set_cursor(start);
            return;
        }
        let cursor = self.document.cursor().position;
        if cursor.line == 0 && cursor.column == 0 {
            return;
        }
        let previous = if cursor.column > 0 {
            Position::new(
                cursor.line,
                self.document
                    .with(|document| document.prev_grapheme_column(cursor)),
            )
        } else {
            Position::new(
                cursor.line - 1,
                self.document.line_len_chars(cursor.line - 1),
            )
        };
        self.document.delete(Range::new(previous, cursor));
        self.document.set_cursor(previous);
    }

    pub fn delete_forward(&mut self) {
        self.follow_cursor = true;
        self.folded.clear();
        if !self.extra_cursors().is_empty() {
            let cursors = self.cursor_states();
            let edits = self.document.with(|document| {
                cursors
                    .iter()
                    .map(|cursor| {
                        let range = if let Some(selection) = cursor.selection {
                            let range = selection.range();
                            (!range.is_empty()).then_some(range)
                        } else {
                            None
                        };
                        let range = range.or_else(|| {
                            let position = cursor.position;
                            let line_len = document.line_len_chars(position.line);
                            if position.column < line_len {
                                Some(Range::new(
                                    position,
                                    Position::new(
                                        position.line,
                                        document.next_grapheme_column(position),
                                    ),
                                ))
                            } else if position.line + 1 < document.len_lines() {
                                Some(Range::new(position, Position::new(position.line + 1, 0)))
                            } else {
                                None
                            }
                        });
                        (
                            range.unwrap_or(Range::new(cursor.position, cursor.position)),
                            String::new(),
                        )
                    })
                    .collect::<Vec<_>>()
            });
            self.apply_cursor_edits(edits);
            return;
        }
        if let Some((start, end)) = self.document.selection_positions()
            && start != end
        {
            self.document.delete(Range::new(start, end));
            self.document.set_cursor(start);
            return;
        }
        let cursor = self.document.cursor().position;
        let line_len = self.document.line_len_chars(cursor.line);
        let next = if cursor.column < line_len {
            Position::new(
                cursor.line,
                self.document
                    .with(|document| document.next_grapheme_column(cursor)),
            )
        } else if cursor.line + 1 < self.document.len_lines() {
            Position::new(cursor.line + 1, 0)
        } else {
            return;
        };
        self.document.delete(Range::new(cursor, next));
        self.document.set_cursor(cursor);
    }

    pub fn move_left(&mut self, extend: bool) {
        self.follow_cursor = true;
        if self.extra_cursors().is_empty() {
            self.document.move_left(extend);
            return;
        }
        self.move_cursors(extend, |document, position| {
            if position.column > 0 {
                Position::new(position.line, document.prev_grapheme_column(position))
            } else if position.line > 0 {
                Position::new(
                    position.line - 1,
                    document.line_len_chars(position.line - 1),
                )
            } else {
                position
            }
        });
    }

    pub fn move_right(&mut self, extend: bool) {
        self.follow_cursor = true;
        if self.extra_cursors().is_empty() {
            self.document.move_right(extend);
            return;
        }
        self.move_cursors(extend, |document, position| {
            if position.column < document.line_len_chars(position.line) {
                Position::new(position.line, document.next_grapheme_column(position))
            } else if position.line + 1 < document.len_lines() {
                Position::new(position.line + 1, 0)
            } else {
                position
            }
        });
    }

    pub fn move_up(&mut self, extend: bool) {
        self.follow_cursor = true;
        if self.extra_cursors().is_empty() {
            self.document.move_vertical(-1, extend);
            return;
        }
        self.move_cursors(extend, |document, position| {
            let line = position.line.saturating_sub(1);
            Position::new(line, position.column.min(document.line_len_chars(line)))
        });
    }

    pub fn move_down(&mut self, extend: bool) {
        self.follow_cursor = true;
        if self.extra_cursors().is_empty() {
            self.document.move_vertical(1, extend);
            return;
        }
        self.move_cursors(extend, |document, position| {
            let line = (position.line + 1).min(document.len_lines().saturating_sub(1));
            Position::new(line, position.column.min(document.line_len_chars(line)))
        });
    }

    pub fn home(&mut self, extend: bool) {
        self.follow_cursor = true;
        if self.extra_cursors().is_empty() {
            let cursor = self.document.cursor().position;
            self.document
                .move_cursor_to(Position::new(cursor.line, 0), extend);
            return;
        }
        self.move_cursors(extend, |_, position| Position::new(position.line, 0));
    }

    pub fn end(&mut self, extend: bool) {
        self.follow_cursor = true;
        if self.extra_cursors().is_empty() {
            let cursor = self.document.cursor().position;
            let column = self.document.line_len_chars(cursor.line);
            self.document
                .move_cursor_to(Position::new(cursor.line, column), extend);
            return;
        }
        self.move_cursors(extend, |document, position| {
            Position::new(position.line, document.line_len_chars(position.line))
        });
    }

    pub fn page_up(&mut self, page: usize, extend: bool) {
        self.follow_cursor = true;
        if self.extra_cursors().is_empty() {
            let cursor = self.document.cursor().position;
            let line = cursor.line.saturating_sub(page);
            let column = cursor.column.min(self.document.line_len_chars(line));
            self.document
                .move_cursor_to(Position::new(line, column), extend);
            return;
        }
        self.move_cursors(extend, |document, position| {
            let line = position.line.saturating_sub(page);
            Position::new(line, position.column.min(document.line_len_chars(line)))
        });
    }

    pub fn page_down(&mut self, page: usize, extend: bool) {
        self.follow_cursor = true;
        if self.extra_cursors().is_empty() {
            let cursor = self.document.cursor().position;
            let line = (cursor.line + page).min(self.document.len_lines().saturating_sub(1));
            let column = cursor.column.min(self.document.line_len_chars(line));
            self.document
                .move_cursor_to(Position::new(line, column), extend);
            return;
        }
        self.move_cursors(extend, |document, position| {
            let line = (position.line + page).min(document.len_lines().saturating_sub(1));
            Position::new(line, position.column.min(document.line_len_chars(line)))
        });
    }

    pub fn undo(&mut self) {
        crate::lock(&self.document.view).extra.clear();
        self.folded.clear();
        self.document.undo();
    }

    pub fn redo(&mut self) {
        crate::lock(&self.document.view).extra.clear();
        self.folded.clear();
        self.document.redo();
    }

    pub fn select_all(&mut self) {
        self.document.select_all();
    }

    pub fn selected_text(&self) -> Option<String> {
        self.document.selected_text()
    }

    /// Select the cursor's current line, matching the editor.selectLine command.
    pub fn select_line(&mut self) {
        if !self.extra_cursors().is_empty() {
            let cursors = self.cursor_states();
            let selections = self.document.with(|document| {
                cursors
                    .iter()
                    .map(|cursor| {
                        let line = cursor.position.line;
                        let length = document
                            .line(line)
                            .map_or(0, |text| text.trim_end_matches('\n').chars().count());
                        tide_core::Cursor::with_selection(
                            Position::new(line, 0),
                            Position::new(line, length),
                        )
                    })
                    .collect::<Vec<_>>()
            });
            self.set_cursor_states(&selections);
            return;
        }
        let line = self
            .document
            .with(|document| document.cursor().position.line);
        let length = self.document.with(|document| {
            document
                .line(line)
                .map_or(0, |text| text.trim_end_matches('\n').chars().count())
        });
        self.document.with_mut(|document| {
            document.set_selection(tide_core::Selection {
                anchor: tide_core::Position::new(line, 0),
                active: tide_core::Position::new(line, length),
            });
        });
    }

    pub fn delete_selection(&mut self) -> bool {
        if let Some((start, end)) = self.document.selection_positions()
            && start != end
        {
            self.document.delete(Range::new(start, end));
            self.document.set_cursor(start);
            true
        } else {
            false
        }
    }

    pub fn goto_line(&mut self, line: usize) {
        self.follow_cursor = true;
        let line = line
            .saturating_sub(1)
            .min(self.document.len_lines().saturating_sub(1));
        self.document.move_cursor_to(Position::new(line, 0), false);
        self.scroll_top = line;
    }

    pub fn search_ranges(
        &self,
        query: &str,
        options: crate::search::SearchOptions,
    ) -> Result<Vec<Range>, String> {
        let revision = self.revision();
        let mut cache = self.search_cache.borrow_mut();
        if cache
            .as_ref()
            .is_none_or(|c| c.revision != revision || c.query != query || c.options != options)
        {
            let matches = match self
                .document
                .with(|doc| crate::search::matches(doc, query, options, None))
            {
                Ok(matches) => matches,
                Err(error) => {
                    *cache = None;
                    return Err(error);
                }
            };
            let ranges = matches.into_iter().map(|(range, _)| range).collect();
            *cache = Some(SearchCache {
                revision,
                query: query.into(),
                options,
                ranges,
            });
        }
        Ok(cache.as_ref().unwrap().ranges.clone())
    }

    pub fn find_range(
        &self,
        query: &str,
        from: Position,
        forward: bool,
        options: crate::search::SearchOptions,
    ) -> Result<Option<Range>, String> {
        let ranges = self.search_ranges(query, options)?;
        let key = (from.line, from.column);
        Ok(if forward {
            ranges
                .iter()
                .find(|range| (range.start.line, range.start.column) >= key)
                .or_else(|| ranges.first())
        } else {
            ranges
                .iter()
                .rev()
                .find(|range| (range.start.line, range.start.column) < key)
                .or_else(|| ranges.last())
        }
        .copied())
    }

    #[must_use]
    pub fn find(&self, query: &str, from: Position, forward: bool) -> Option<Position> {
        self.find_range(
            query,
            from,
            forward,
            crate::search::SearchOptions::default(),
        )
        .ok()
        .flatten()
        .map(|range| range.start)
    }

    pub fn replace_matches(
        &mut self,
        query: &str,
        replacement: &str,
        options: crate::search::SearchOptions,
        all: bool,
    ) -> Result<usize, String> {
        let mut edits = self
            .document
            .with(|doc| crate::search::matches(doc, query, options, Some(replacement)))?;
        if !all {
            let cursor = self.document.cursor();
            let selected = cursor.range();
            let found = edits
                .iter()
                .position(|(range, _)| Some(*range) == selected)
                .or_else(|| {
                    edits.iter().position(|(range, _)| {
                        (range.start.line, range.start.column)
                            >= (cursor.position.line, cursor.position.column)
                    })
                })
                .unwrap_or(0);
            edits = edits.into_iter().nth(found).into_iter().collect();
        }
        let count = edits.len();
        if count > 0 {
            self.document
                .apply_edits(&edits, 0)
                .map_err(|error| error.to_string())?;
            self.folded.clear();
            self.follow_cursor = true;
        }
        Ok(count)
    }

    pub fn configure_editing(
        &mut self,
        tab_size: usize,
        insert_spaces: bool,
        relative_numbers: bool,
    ) {
        self.indent_unit = if insert_spaces {
            " ".repeat(tab_size.clamp(1, 32))
        } else {
            "\t".into()
        };
        self.relative_numbers = relative_numbers;
    }

    pub fn insert_tab(&mut self) {
        let unit = self.indent_unit.clone();
        self.insert_text(&unit);
    }

    fn fold_marker(&self, line: usize) -> &'static str {
        if self.folded.iter().any(|(start, _)| *start == line) {
            "▸ "
        } else if self.fold_regions.iter().any(|(start, _)| *start == line) {
            "▾ "
        } else {
            "  "
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub fn mouse_press(
        &mut self,
        area: Rect,
        x: u16,
        y: u16,
        numbers: bool,
        tab_size: usize,
        modifiers: crossterm::event::KeyModifiers,
        clicks: u8,
    ) {
        use crossterm::event::KeyModifiers;
        if self
            .minimap_area
            .contains(ratatui::layout::Position::new(x, y))
        {
            self.drag_minimap = true;
            self.minimap_to(y);
            return;
        }
        if x == area.right().saturating_sub(1) && self.document.len_lines() > area.height as usize {
            self.drag_scrollbar = true;
            self.mouse_drag(area, x, y, numbers, tab_size);
            return;
        }
        let previous = self.document.cursor();
        let row = y.saturating_sub(area.y) as usize;
        let line = self
            .rendered_rows
            .get(row)
            .map_or(self.scroll_top + row, |(line, _)| *line);
        let gutter = if numbers {
            gutter_width(self.document.len_lines())
        } else {
            0
        };
        if x == area.x
            && !self.is_dirty()
            && let Some(browser) = &mut self.inline
            && let Some(index) = browser.diff.hunks.iter().position(|h| {
                line + 1 >= h.new_start.max(1) && line + 1 < h.new_start.max(1) + h.new_count.max(1)
            })
        {
            browser.show_only_hunk(index);
            self.inline_focus = !self.inline_focus;
            return;
        }
        self.inline_focus = false;
        if x.saturating_sub(area.x) as usize == gutter && self.toggle_fold_at(line) {
            return;
        }
        self.follow_cursor = true;
        self.set_cursor_from_screen(area, x, y, numbers, tab_size);
        let position = self.document.cursor().position;
        if modifiers.intersects(KeyModifiers::ALT | KeyModifiers::CONTROL | KeyModifiers::SUPER) {
            let mut cursors = self.cursor_states();
            cursors[0] = previous;
            if let Some(index) = cursors
                .iter()
                .position(|cursor| cursor.position == position)
            {
                if index > 0 {
                    cursors.remove(index);
                }
            } else {
                cursors.push(tide_core::Cursor::at(position.line, position.column));
            }
            self.set_cursor_states(&cursors);
        } else if modifiers.contains(KeyModifiers::SHIFT) {
            let anchor = previous.selection.map_or(previous.position, |s| s.anchor);
            self.document.set_selection(tide_core::Selection {
                anchor,
                active: position,
            });
        } else if clicks >= 3 || (numbers && x.saturating_sub(area.x) < saturating_u16(gutter)) {
            self.select_line();
        } else if clicks == 2 {
            if let Some(range) = self.document.with(|d| d.word_at(position)) {
                self.document.set_selection(tide_core::Selection {
                    anchor: range.start,
                    active: range.end,
                });
            }
        } else {
            self.clear_extra_cursors();
        }
        self.drag_anchor = Some(
            self.document
                .cursor()
                .selection
                .map_or(position, |s| s.anchor),
        );
    }

    fn minimap_to(&mut self, y: u16) {
        let row = y
            .saturating_sub(self.minimap_area.y)
            .min(self.minimap_area.height.saturating_sub(1)) as usize;
        let line = (self.minimap_top() + row) * 3;
        let height = self.minimap_area.height as usize;
        self.scroll_top = line
            .saturating_sub(height / 2)
            .min(self.document.len_lines().saturating_sub(height));
        self.wrap_offset = 0;
        self.follow_cursor = false;
    }
    pub fn mouse_drag(&mut self, area: Rect, x: u16, y: u16, numbers: bool, tab_size: usize) {
        if self.drag_minimap {
            self.minimap_to(y);
            return;
        }
        if self.drag_scrollbar {
            self.follow_cursor = false;
            let bar = self.scrollbar_area;
            let (total, visible, _) = self.scrollbar_metrics();
            let target = crate::scrollbar::offset(
                bar.height as usize,
                total,
                visible,
                y.saturating_sub(bar.y) as usize,
            );
            let mut source = target;
            for (a, b) in self.scrollbar_hidden() {
                if source >= a {
                    source += b - a;
                } else {
                    break;
                }
            }
            self.scroll_top = source.min(self.document.len_lines().saturating_sub(1));
            self.wrap_offset = 0;
            return;
        }
        let Some(anchor) = self.drag_anchor else {
            return;
        };
        if y < area.y {
            self.scroll_by(-1);
        } else if y >= area.bottom() {
            self.scroll_by(1);
        }
        self.set_cursor_from_screen(
            area,
            x,
            y.clamp(area.y, area.bottom().saturating_sub(1).max(area.y)),
            numbers,
            tab_size,
        );
        self.document.set_selection(tide_core::Selection {
            anchor,
            active: self.document.cursor().position,
        });
    }

    pub fn mouse_release(&mut self) {
        self.drag_minimap = false;
        self.drag_anchor = None;
        self.drag_scrollbar = false;
    }

    pub fn set_cursor_from_screen(
        &mut self,
        area: Rect,
        x: u16,
        y: u16,
        show_line_numbers: bool,
        tab_size: usize,
    ) {
        let gutter = if show_line_numbers {
            gutter_width(self.document.len_lines()) + 2
        } else {
            2
        };
        let row = y.saturating_sub(area.y) as usize;
        let (line_index, start_column) = self
            .rendered_rows
            .get(row)
            .copied()
            .unwrap_or((self.scroll_top + row, 0));
        if line_index >= self.document.len_lines() {
            return;
        }
        let col = x.saturating_sub(area.x) as usize;
        let line = self.document.line(line_index).unwrap_or_default();
        let target = col.saturating_sub(gutter)
            + if self.word_wrap {
                column_cells(&line, start_column, tab_size)
            } else {
                self.scroll_left
            };
        let chars = line.chars().collect::<Vec<_>>();
        let mut width = 0;
        let mut column = 0;
        for pair in tide_core::grapheme_boundaries(&line).windows(2) {
            let cluster = chars[pair[0]..pair[1]].iter().collect::<String>();
            let next = if cluster == "\t" {
                tab_size.max(1) - width % tab_size.max(1)
            } else {
                display_width(&cluster)
            };
            if width + next > target {
                break;
            }
            width += next;
            column = pair[1];
        }
        self.document.set_cursor(Position::new(line_index, column));
    }
}

fn translate_offset(offset: usize, start: usize, removed: usize, inserted: usize) -> usize {
    if offset < start {
        offset
    } else if offset <= start + removed {
        start + inserted
    } else {
        // `offset` is past `start + removed`, so the subtraction cannot underflow.
        (offset - removed).saturating_add(inserted)
    }
}

/// A lossily decoded buffer would replace the original bytes with U+FFFD.
fn refuse_lossy_overwrite(document: &DocumentHandle) -> std::io::Result<()> {
    if document.with(Document::is_lossy) {
        return Err(std::io::Error::other(
            "File is not valid UTF-8 and opened read-only; use Save As to write a copy",
        ));
    }
    Ok(())
}

/// `line_indents` holds each line's indentation, or `None` for blank lines.
fn compute_fold_regions(
    line_indents: &[Option<usize>],
    delimiters: &crate::delimiters::Delimiters,
) -> Vec<(usize, usize)> {
    let mut ranges = Vec::new();
    let mut indents: Vec<(usize, usize)> = Vec::new();
    let nonempty = line_indents
        .iter()
        .enumerate()
        .filter_map(|(line, indent)| indent.map(|indent| (line, indent)))
        .collect::<Vec<_>>();
    for (index, &(line, indent)) in nonempty.iter().enumerate() {
        while indents.last().is_some_and(|(_, prior)| *prior >= indent) {
            let (start, _) = indents.pop().unwrap();
            if line > start + 1 {
                ranges.push((start, line - 1));
            }
        }
        let next_indent = nonempty.get(index + 1).map(|(_, next)| *next);
        if next_indent.is_some_and(|next| next > indent) {
            indents.push((line, indent));
        }
    }
    ranges.extend(delimiters.multiline_regions());
    for (start, _) in indents {
        if line_indents.len() > start + 1 {
            ranges.push((start, line_indents.len() - 1));
        }
    }
    ranges.sort_by_key(|(start, end)| (*start, std::cmp::Reverse(*end)));
    ranges.dedup_by_key(|(start, _)| *start);
    ranges
}

#[must_use]
fn closing_pair(ch: char) -> Option<char> {
    match ch {
        '(' => Some(')'),
        '[' => Some(']'),
        '{' => Some('}'),
        '"' => Some('"'),
        '\'' => Some('\''),
        '`' => Some('`'),
        _ => None,
    }
}

fn is_closing_pair(ch: char) -> bool {
    matches!(ch, ')' | ']' | '}' | '"' | '\'' | '`')
}

/// One authoritative buffer per file path.
#[derive(Debug, Default)]
pub struct DocumentRegistry {
    documents: HashMap<PathBuf, DocumentHandle>,
}

impl DocumentRegistry {
    pub fn is_dirty(&self, path: &Path) -> bool {
        self.documents
            .get(&document_path(path))
            .is_some_and(DocumentHandle::is_dirty)
    }
    pub fn open(&mut self, path: impl AsRef<Path>) -> std::io::Result<EditorState> {
        let normalized = document_path(path.as_ref());
        let path = normalized.as_path();
        if let Some(handle) = self.documents.get(path).cloned() {
            let language_extension = language_extension(path);
            let stamp = file_stamp(path);
            return Ok(EditorState::from_handle(
                handle.fork_view(),
                Some(path.to_path_buf()),
                language_extension,
                stamp,
            ));
        }
        let document = Document::from_path(path)?;
        let handle = DocumentHandle::new(document);
        self.documents.insert(path.to_path_buf(), handle.clone());
        Ok(EditorState::from_handle(
            handle,
            Some(path.to_path_buf()),
            language_extension(path),
            file_stamp(path),
        ))
    }

    pub fn from_content(&mut self, path: Option<PathBuf>, content: &str) -> EditorState {
        let path = path.map(|path| document_path(&path));
        if let Some(path) = &path
            && let Some(handle) = self.documents.get(path).cloned()
        {
            if handle.text() != content {
                handle.set_content(content);
            }
            return EditorState::from_handle(
                handle.fork_view(),
                Some(path.clone()),
                language_extension(path),
                file_stamp(path),
            );
        }
        let mut document = Document::new(content);
        document.set_file_path(path.clone());
        let handle = DocumentHandle::new(document);
        if let Some(path) = &path {
            self.documents.insert(path.clone(), handle.clone());
        }
        EditorState::from_handle(
            handle,
            path.clone(),
            path.as_deref()
                .map_or_else(|| "txt".to_owned(), language_extension),
            path.as_deref().and_then(file_stamp),
        )
    }

    pub fn forget(&mut self, path: &Path) {
        self.documents.remove(&document_path(path));
    }

    pub fn register(&mut self, path: PathBuf, handle: DocumentHandle) {
        self.documents.insert(document_path(&path), handle);
    }

    #[must_use]
    pub fn get(&self, path: &Path) -> Option<DocumentHandle> {
        self.documents.get(&document_path(path)).cloned()
    }
}

fn column_cells(text: &str, count: usize, tab_size: usize) -> usize {
    let prefix = text.chars().take(count).collect::<String>();
    display_width(&expand_tabs(&prefix, tab_size.max(1)))
}

fn grapheme_chunks(text: &str) -> impl Iterator<Item = (usize, usize, &str)> {
    let mut offset = 0;
    tide_core::graphemes(text).map(move |cluster| {
        let start = offset;
        offset += cluster.chars().count();
        (start, offset, cluster)
    })
}

fn wrap_segments(text: &str, width: usize, tab_size: usize) -> Vec<(usize, usize, String)> {
    let width = width.max(1);
    let tab_size = tab_size.max(1);
    let mut result = Vec::new();
    let mut start = 0;
    let mut segment = String::new();
    let mut cells = 0;
    let mut logical = 0;
    for (index, _, cluster) in grapheme_chunks(text) {
        let size = if cluster == "\t" {
            tab_size - logical % tab_size
        } else {
            display_width(cluster)
        };
        if cells + size > width && !segment.is_empty() {
            result.push((start, index, std::mem::take(&mut segment)));
            start = index;
            cells = 0;
        }
        if cluster == "\t" {
            segment.push_str(&" ".repeat(size));
        } else {
            segment.push_str(cluster);
        }
        cells += size;
        logical += size;
    }
    result.push((start, text.chars().count(), segment));
    result
}

fn language_extension(path: &Path) -> String {
    path.extension()
        .and_then(|ext| ext.to_str())
        .unwrap_or("txt")
        .to_ascii_lowercase()
}

#[cfg(test)]
thread_local! {
    pub(crate) static HASHES: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// Files larger than this reparse delimiters only once edits pause.
const DEFERRED_FOLD_CHARS: usize = 512 * 1024;
const FOLD_IDLE: Duration = Duration::from_millis(100);

fn hash_file(path: &Path) -> Option<String> {
    use sha2::{Digest, Sha256};
    #[cfg(test)]
    HASHES.set(HASHES.get() + 1);
    std::fs::read(path)
        .ok()
        .map(|bytes| format!("{:x}", Sha256::digest(bytes)))
}

fn file_stamp(path: &Path) -> Option<FileStamp> {
    std::fs::metadata(path)
        .ok()
        .map(|metadata| FileStamp::from_metadata(&metadata))
}

pub fn gutter_width(line_count: usize) -> usize {
    line_count.max(1).to_string().len() + 2
}

fn expand_tabs(line: &str, tab_size: usize) -> String {
    let tab_size = tab_size.max(1);
    let mut output = String::with_capacity(line.len());
    let mut width = 0;
    for (_, _, cluster) in grapheme_chunks(line) {
        if cluster == "\t" {
            let spaces = tab_size - width % tab_size;
            output.push_str(&" ".repeat(spaces));
            width += spaces;
        } else {
            width += display_width(cluster);
            output.push_str(cluster);
        }
    }
    output
}

fn spans_from_highlight(spans: &[HighlightSpan], palette: Palette) -> Vec<(String, Style)> {
    if spans.is_empty() {
        return vec![(String::new(), Style::default().fg(palette.foreground))];
    }
    let styled = spans
        .iter()
        .map(|span| {
            let color = span
                .fg
                .as_deref()
                .and_then(crate::palette::hex_color)
                .unwrap_or(palette.foreground);
            let mut style = Style::default().fg(color);
            if span.bold {
                style = style.add_modifier(Modifier::BOLD);
            }
            if span.italic {
                style = style.add_modifier(Modifier::ITALIC);
            }
            if span.underline {
                style = style.add_modifier(Modifier::UNDERLINED);
            }
            (span.text.clone(), style)
        })
        .collect::<Vec<(String, Style)>>();
    // A grammar token boundary must never split an extended grapheme. Ratatui
    // measures each Span independently, so split ZWJ sequences corrupt cell maps.
    let text = styled
        .iter()
        .map(|(text, _)| text.as_str())
        .collect::<String>();
    let mut result: Vec<(String, Style)> = Vec::new();
    let mut index = 0;
    let mut end = styled[0].0.chars().count();
    for (start, _, cluster) in grapheme_chunks(&text) {
        while start >= end && index + 1 < styled.len() {
            index += 1;
            end += styled[index].0.chars().count();
        }
        let style = styled[index].1;
        if let Some((text, previous)) = result.last_mut()
            && *previous == style
        {
            text.push_str(cluster);
        } else {
            result.push((cluster.to_owned(), style));
        }
    }
    result
}

#[allow(clippy::too_many_arguments)]
fn styled_slice(
    spans: &[HighlightSpan],
    palette: Palette,
    tab_size: usize,
    start: usize,
    end: usize,
    left: usize,
    width: usize,
) -> Vec<(String, Style)> {
    let mut result: Vec<(String, Style)> = Vec::new();
    let mut chars = 0;
    let mut cells = 0;
    let mut base = None;
    let tab_size = tab_size.max(1);
    for (text, style) in spans_from_highlight(spans, palette) {
        for (_, _, cluster) in grapheme_chunks(&text) {
            let position = chars;
            chars += cluster.chars().count();
            if matches!(cluster, "\n" | "\r" | "\r\n") {
                continue;
            }
            let size = if cluster == "\t" {
                tab_size - cells % tab_size
            } else {
                display_width(cluster)
            };
            let cell = cells;
            cells += size;
            if position < start {
                continue;
            }
            if position >= end {
                return result;
            }
            let local = cell - *base.get_or_insert(cell);
            if local >= left + width {
                return result;
            }
            let visible_start = local.max(left);
            let visible_end = (local + size).min(left + width);
            if visible_end <= visible_start {
                continue;
            }
            let value = if cluster == "\t" || visible_start > local || visible_end < local + size {
                " ".repeat(visible_end - visible_start)
            } else {
                cluster.to_owned()
            };
            if let Some((text, previous)) = result.last_mut()
                && *previous == style
            {
                text.push_str(&value);
            } else {
                result.push((value, style));
            }
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
    use tide_config::Theme;

    fn syntax() -> SyntaxHighlighter {
        let theme: Theme = tide_config::builtin::themes()
            .get("ultra-dusk")
            .cloned()
            .unwrap();
        SyntaxHighlighter::new(&theme)
    }

    #[test]
    fn wrapping_and_syntax_spans_keep_grapheme_cell_boundaries() {
        let text = "👩‍💻漢\tend";
        assert_eq!(expand_tabs(text, 4), "👩‍💻漢    end");
        let rows = wrap_segments(text, 4, 4);
        assert_eq!(rows[0], (0, 4, "👩‍💻漢".into()));
        assert_eq!(rows[1], (4, 5, "    ".into()));
        let spans = syntax().highlight_line("👩‍💻 end", "py");
        let styled = spans_from_highlight(&spans, Palette::ultra_dusk());
        assert_eq!(
            styled
                .iter()
                .map(|(text, _)| text.as_str())
                .collect::<String>(),
            "👩‍💻 end"
        );
        assert!(styled.iter().any(|(text, _)| text.contains("👩‍💻")));
        let mut editor = EditorState::from_content(None, "abcdefghijklmno");
        editor.set_word_wrap(true);
        editor.set_wrap_column(Some(6));
        let mut terminal = Terminal::new(TestBackend::new(40, 5)).unwrap();
        terminal
            .draw(|frame| {
                editor.draw(
                    frame,
                    frame.area(),
                    &EditorView {
                        palette: Palette::ultra_dusk(),
                        syntax: &syntax(),
                        show_line_numbers: false,
                        tab_size: 4,
                        focused: true,
                    },
                )
            })
            .unwrap();
        assert_eq!(editor.rendered_rows, vec![(0, 0), (0, 6), (0, 12)]);
    }

    #[test]
    fn independent_views_translate_selections_share_undo_and_disk_baseline() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("shared.rs");
        std::fs::write(&path, "alpha\nbeta\n").unwrap();
        let mut registry = DocumentRegistry::default();
        let mut first = registry.open(&path).unwrap();
        let mut second = registry.open(&path).unwrap();
        first.document.set_cursor(Position::new(0, 1));
        second.document.set_selection(tide_core::Selection {
            anchor: Position::new(1, 1),
            active: Position::new(1, 3),
        });
        assert_eq!(first.document.cursor().position, Position::new(0, 1));
        first.insert_text("X\n");
        assert_eq!(
            second.document.cursor().selection.unwrap().anchor,
            Position::new(2, 1)
        );
        assert_eq!(second.selected_text().as_deref(), Some("et"));
        assert_eq!(first.document.cursor().position, Position::new(1, 0));
        second.undo();
        assert_eq!(first.document.text(), "alpha\nbeta\n");
        first.document.set_cursor(Position::new(0, 0));
        first.insert_text("!");
        first.save().unwrap();
        assert!(!second.is_externally_changed());
        second.document.set_cursor(Position::new(1, 0));
        second.insert_text("?");
        second.save().unwrap();
        assert!(!first.is_externally_changed());
        let clone = first.clone();
        clone.document.set_cursor(Position::new(1, 1));
        assert_ne!(clone.document.cursor(), first.document.cursor());
        std::fs::write(&path, "external").unwrap();
        assert!(first.save().is_err());
        assert!(second.save().is_err());
    }

    #[test]
    fn folding_indent_gutter_and_screen_mapping_work_with_selection_and_scrollbars() {
        use crossterm::event::KeyModifiers;
        let mut editor =
            EditorState::from_content(None, "if ready:\n    first\n    second\nafter\nlast");
        editor.restore_folds(&[0]);
        assert_eq!(editor.folded_ranges(), &[(0, 2)]);
        let mut terminal = Terminal::new(TestBackend::new(40, 3)).unwrap();
        terminal
            .draw(|frame| {
                editor.draw(
                    frame,
                    frame.area(),
                    &EditorView {
                        palette: Palette::ultra_dusk(),
                        syntax: &syntax(),
                        show_line_numbers: true,
                        tab_size: 4,
                        focused: true,
                    },
                )
            })
            .unwrap();
        let area = Rect::new(0, 0, 40, 3);
        editor.mouse_press(area, 5, 1, true, 4, KeyModifiers::NONE, 1);
        assert_eq!(editor.document.cursor().position.line, 3);
        editor.mouse_press(area, 6, 2, true, 4, KeyModifiers::SHIFT, 1);
        assert!(editor.selected_text().unwrap().contains("after"));
        editor.mouse_release();
        editor.mouse_press(
            area,
            u16::try_from(gutter_width(5)).unwrap(),
            0,
            true,
            4,
            KeyModifiers::NONE,
            1,
        );
        assert!(editor.folded_ranges().is_empty());
        editor.mouse_press(area, 39, 2, true, 4, KeyModifiers::NONE, 1);
        assert!(editor.scroll_top > 0);
        editor.mouse_release();
        editor.document.set_cursor(Position::new(0, 0));
        editor.scroll_top = 0;
        editor.follow_cursor();
        terminal
            .draw(|frame| {
                editor.draw(
                    frame,
                    frame.area(),
                    &EditorView {
                        palette: Palette::ultra_dusk(),
                        syntax: &syntax(),
                        show_line_numbers: true,
                        tab_size: 4,
                        focused: true,
                    },
                )
            })
            .unwrap();
        editor.mouse_press(area, 6, 0, true, 4, KeyModifiers::NONE, 2);
        assert_eq!(editor.selected_text().as_deref(), Some("if"));
        editor.mouse_drag(area, 8, 1, true, 4);
        assert!(editor.selected_text().unwrap().contains('\n'));
        editor.mouse_release();
        editor.mouse_press(area, 6, 0, true, 4, KeyModifiers::NONE, 3);
        assert_eq!(editor.selected_text().as_deref(), Some("if ready:"));
        editor.mouse_press(area, 6, 1, true, 4, KeyModifiers::ALT, 1);
        assert_eq!(editor.extra_cursors().len(), 1);
    }

    #[test]
    fn search_uses_all_matches_wrap_unicode_options_and_grouped_replacement() {
        use crate::search::SearchOptions;
        let mut editor = EditorState::from_content(None, "foo FOO food foo\nİ😀 foo42 foo7");
        let options = SearchOptions::default();
        assert_eq!(editor.search_ranges("foo", options).unwrap().len(), 6);
        assert_eq!(
            editor.find("foo", Position::new(0, 1), true),
            Some(Position::new(0, 4))
        );
        assert_eq!(
            editor.find("foo", Position::new(1, 20), true),
            Some(Position::new(0, 0))
        );
        assert_eq!(
            editor.find("foo", Position::new(0, 0), false),
            Some(Position::new(1, 9))
        );
        assert_eq!(
            editor
                .search_ranges(
                    "foo",
                    SearchOptions {
                        whole_word: true,
                        ..options
                    }
                )
                .unwrap()
                .len(),
            3
        );
        assert_eq!(
            editor
                .search_ranges(
                    "foo",
                    SearchOptions {
                        case_sensitive: true,
                        ..options
                    }
                )
                .unwrap()
                .len(),
            5
        );
        assert!(
            editor
                .search_ranges(
                    "[",
                    SearchOptions {
                        regex: true,
                        ..options
                    }
                )
                .is_err()
        );
        let regex = SearchOptions {
            regex: true,
            ..options
        };
        assert_eq!(
            editor
                .replace_matches("foo(\\d+)", "bar$1", regex, true)
                .unwrap(),
            2
        );
        assert!(editor.document.text().contains("İ😀 bar42 bar7"));
        editor.undo();
        assert!(editor.document.text().contains("foo42 foo7"));
        editor.document.set_cursor(Position::new(0, 4));
        assert_eq!(
            editor
                .replace_matches("foo", "one", options, false)
                .unwrap(),
            1
        );
        assert!(editor.document.text().starts_with("foo one food foo"));
        let selected = SearchOptions {
            selection: Some(Range::new(Position::new(1, 0), Position::new(1, 20))),
            ..options
        };
        assert_eq!(editor.search_ranges("foo", selected).unwrap().len(), 2);
    }

    #[test]
    fn nondefault_indentation_relative_numbers_and_grapheme_mouse_geometry() {
        let mut editor = EditorState::from_content(None, "if ready:\n👩‍💻漢 text");
        editor.configure_editing(4, false, true);
        editor.document.set_cursor(Position::new(0, 9));
        editor.newline();
        assert!(editor.document.text().starts_with("if ready:\n\t"));
        editor.undo();
        editor.document.set_cursor(Position::new(0, 9));
        editor.configure_editing(4, true, true);
        editor.newline();
        assert!(editor.document.text().starts_with("if ready:\n    "));
        editor.undo();
        let mut terminal = Terminal::new(TestBackend::new(40, 5)).unwrap();
        terminal
            .draw(|frame| {
                editor.draw(
                    frame,
                    frame.area(),
                    &EditorView {
                        palette: Palette::ultra_dusk(),
                        syntax: &syntax(),
                        show_line_numbers: false,
                        tab_size: 4,
                        focused: true,
                    },
                )
            })
            .unwrap();
        editor.set_cursor_from_screen(Rect::new(0, 0, 40, 5), 4, 1, false, 4);
        assert_eq!(editor.document.cursor().position, Position::new(1, 3));
    }

    #[test]
    fn non_utf8_files_open_read_only_and_are_never_overwritten() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("latin1.txt");
        std::fs::write(&path, b"caf\xe9\n").unwrap();
        let mut editor = EditorState::from_path(&path).unwrap();
        assert!(editor.document.with(Document::is_lossy));
        assert_eq!(editor.document.text(), "caf\u{FFFD}\n");
        editor.insert_text("x");
        assert_eq!(editor.document.text(), "caf\u{FFFD}\n");
        assert!(editor.save().is_err());
        assert!(editor.save_force().is_err());
        assert_eq!(std::fs::read(&path).unwrap(), b"caf\xe9\n");
    }

    #[test]
    fn save_as_never_replaces_an_existing_destination_implicitly() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("keep.txt");
        std::fs::write(&path, "keep").unwrap();
        let mut editor = EditorState::from_content(None, "new");
        assert!(editor.save_as(path.clone()).is_err());
        assert_eq!(std::fs::read_to_string(path).unwrap(), "keep");
    }

    #[test]
    fn inserts_text_and_undoes() {
        let mut editor = EditorState::untitled();
        editor.insert_text("hello");
        editor.newline();
        editor.insert_text("world");
        assert_eq!(editor.document.text(), "hello\nworld");
        editor.undo();
        assert_eq!(editor.document.text(), "hello\n");
        editor.undo();
        assert_eq!(editor.document.text(), "hello");
    }

    #[test]
    fn newline_keeps_the_document_line_ending() {
        let mut editor = EditorState::from_content(None, "one\r\ntwo\r\n");
        editor.document.set_cursor(Position::new(0, 3));
        editor.newline();
        assert_eq!(editor.document.text(), "one\r\n\r\ntwo\r\n");
        editor.add_cursor_below();
        editor.newline();
        assert_eq!(editor.document.text(), "one\r\n\r\n\r\n\r\ntwo\r\n");
    }

    #[test]
    fn backspace_joins_lines() {
        let mut editor = EditorState::untitled();
        editor.insert_text("one\ntwo");
        editor.document.set_cursor(Position::new(1, 0));
        editor.backspace();
        assert_eq!(editor.document.text(), "onetwo");
    }

    #[test]
    fn find_wraps_line_boundaries() {
        let mut editor = EditorState::untitled();
        editor.insert_text("alpha\nbeta\nalpha");
        let found = editor.find("alpha", Position::new(0, 1), true).unwrap();
        assert_eq!(found, Position::new(2, 0));
    }

    #[test]
    fn auto_pairs_and_skips_closing_characters() {
        let mut editor = EditorState::untitled();
        editor.insert_char('(');
        assert_eq!(editor.document.text(), "()");
        assert_eq!(editor.document.cursor().position.column, 1);
        editor.insert_char(')');
        assert_eq!(editor.document.text(), "()");
        assert_eq!(editor.document.cursor().position.column, 2);
    }

    #[test]
    fn nondefault_indentation_undo_and_minimap_controls_change_behavior() {
        for (mode, expected) in [
            ("none", "  {\n"),
            ("keep", "  {\n  "),
            ("full", "  {\n      "),
        ] {
            let mut editor = EditorState::from_content(None, "  {");
            editor.document.set_cursor(Position::new(0, 3));
            editor.indent_unit = "    ".into();
            editor.auto_indent = mode.into();
            editor.newline();
            assert_eq!(editor.document.text(), expected, "{mode}");
        }
        let mut editor = EditorState::from_content(None, "alpha\nbeta\ngamma");
        editor.document.with_mut(|d| d.set_undo_limit(2));
        for (line, c) in ['x', 'y', 'z'].into_iter().enumerate() {
            editor.document.set_cursor(Position::new(line, 0));
            editor.insert_char(c);
        }
        assert!(editor.document.with_mut(|d| d.undo()));
        assert!(editor.document.with_mut(|d| d.undo()));
        assert!(!editor.document.with_mut(|d| d.undo()));
        editor.minimap = true;
        let mut terminal = Terminal::new(TestBackend::new(40, 5)).unwrap();
        terminal
            .draw(|frame| {
                editor.draw(
                    frame,
                    frame.area(),
                    &EditorView {
                        palette: Palette::ultra_dusk(),
                        syntax: &syntax(),
                        show_line_numbers: false,
                        tab_size: 4,
                        focused: true,
                    },
                )
            })
            .unwrap();
        assert_eq!(editor.minimap_area.width, 10);
        let original_cursor = editor.document.cursor();
        let mini = editor.minimap_area;
        for y in mini.y..mini.bottom() {
            for x in mini.x..mini.right() {
                assert!(matches!(
                    terminal.backend().buffer()[(x, y)].symbol(),
                    " " | "░" | "▒" | "▓" | "█"
                ));
            }
        }
        editor.minimap_to(1);
        assert_eq!(editor.document.cursor(), original_cursor);
        assert!(!editor.follow_cursor);
        editor.minimap = false;
        terminal
            .draw(|frame| {
                editor.draw(
                    frame,
                    frame.area(),
                    &EditorView {
                        palette: Palette::ultra_dusk(),
                        syntax: &syntax(),
                        show_line_numbers: false,
                        tab_size: 4,
                        focused: true,
                    },
                )
            })
            .unwrap();
        assert_eq!(editor.minimap_area, Rect::default());
    }

    #[test]
    fn diagnostic_severity_uses_the_most_severe_line_marker() {
        let mut editor = EditorState::untitled();
        editor.set_diagnostics(vec![(3, 2), (3, 1), (7, 2)]);
        assert_eq!(editor.diagnostic_severity(3), Some(1));
        assert_eq!(editor.diagnostic_severity(7), Some(2));
        assert_eq!(editor.diagnostic_severity(1), None);
    }

    #[test]
    fn save_refuses_a_same_length_external_write_that_kept_the_stamp() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("file.txt");
        std::fs::write(&path, "one\n").unwrap();
        let mut editor = EditorState::from_path(&path).unwrap();
        let modified = std::fs::metadata(&path).unwrap().modified().unwrap();
        std::fs::write(&path, "two\n").unwrap();
        std::fs::File::options()
            .write(true)
            .open(&path)
            .unwrap()
            .set_modified(modified)
            .unwrap();
        editor.insert_text("x");
        assert!(editor.save().is_err());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "two\n");
    }

    #[test]
    fn external_change_checks_hash_only_when_the_disk_stamp_moves() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("file.txt");
        std::fs::write(&path, "one\n").unwrap();
        let editor = EditorState::from_path(&path).unwrap();
        HASHES.set(0);
        for _ in 0..3 {
            assert!(!editor.is_externally_changed());
        }
        assert_eq!(HASHES.get(), 0);
        std::fs::write(&path, "external\n").unwrap();
        assert!(editor.is_externally_changed());
        assert!(editor.is_externally_changed());
        assert_eq!(HASHES.get(), 1);
        std::fs::write(&path, "one\n").unwrap();
        let file = std::fs::File::options().write(true).open(&path).unwrap();
        file.set_modified(std::time::SystemTime::now() + std::time::Duration::from_secs(5))
            .unwrap();
        assert!(!editor.is_externally_changed());
        assert!(!editor.is_externally_changed());
        assert_eq!(HASHES.get(), 2);
    }

    #[test]
    fn save_refuses_to_clobber_an_external_change() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("file.txt");
        std::fs::write(&path, "one\n").unwrap();
        let mut editor = EditorState::from_path(&path).unwrap();
        editor.insert_text("draft\n");
        std::fs::write(&path, "external\n").unwrap();
        let error = editor.save().unwrap_err().to_string();
        assert!(error.contains("changed on disk"));
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "external\n");
    }

    #[test]
    fn save_is_atomic_and_updates_the_clean_stamp() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("file.txt");
        std::fs::write(&path, "one\n").unwrap();
        let mut editor = EditorState::from_path(&path).unwrap();
        editor.insert_text("two\n");
        editor.save().unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "two\none\n");
        assert!(!editor.is_dirty());
        editor.insert_text("x");
        editor.save().unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "two\nxone\n");
    }

    #[test]
    fn registry_shares_one_buffer_between_views() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("shared.txt");
        std::fs::write(&path, "one\n").unwrap();
        let mut registry = DocumentRegistry::default();
        let mut first = registry.open(&path).unwrap();
        let second = registry.open(&path).unwrap();
        first.insert_text("two\n");
        assert_eq!(second.document.text(), "two\none\n");
        assert_eq!(first.document.version(), second.document.version());
    }

    #[test]
    fn multi_cursor_typing_and_undo_apply_to_all_cursors() {
        let mut editor = EditorState::untitled();
        editor.insert_text("one\ntwo\nthree\n");
        editor.document.set_cursor(Position::new(0, 0));
        editor.add_cursor_below();
        assert_eq!(editor.extra_cursors().len(), 1);
        editor.insert_text("X");
        assert_eq!(editor.document.text(), "Xone\nXtwo\nthree\n");
        assert_eq!(editor.document.cursor(), tide_core::Cursor::at(0, 1));
        assert_eq!(editor.extra_cursors()[0], tide_core::Cursor::at(1, 1));
        editor.undo();
        assert_eq!(editor.document.text(), "one\ntwo\nthree\n");
        assert!(editor.extra_cursors().is_empty());
    }

    #[test]
    fn multi_cursor_backspace_and_vertical_movement() {
        let mut editor = EditorState::untitled();
        editor.insert_text("abc\ndef\n");
        editor.document.set_cursor(Position::new(0, 3));
        editor.add_cursor_below();
        editor.backspace();
        assert_eq!(editor.document.text(), "ab\nde\n");
        assert_eq!(editor.document.cursor(), tide_core::Cursor::at(0, 3 - 1));
        assert_eq!(editor.extra_cursors()[0], tide_core::Cursor::at(1, 2));
        editor.move_up(true);
        assert!(editor.document.cursor().selection.is_some());
        assert!(editor.extra_cursors()[0].selection.is_some());
    }

    #[test]
    fn select_all_occurrences_replaces_every_match() {
        let mut editor = EditorState::untitled();
        editor.insert_text("foo bar foo\nfoo\n");
        editor.document.set_cursor(Position::new(0, 0));
        assert!(editor.select_all_occurrences());
        assert_eq!(editor.extra_cursors().len(), 2);
        editor.insert_text("x");
        assert_eq!(editor.document.text(), "x bar x\nx\n");
        editor.undo();
        assert_eq!(editor.document.text(), "foo bar foo\nfoo\n");
    }

    #[test]
    fn select_next_match_adds_one_occurrence_at_a_time() {
        let mut editor = EditorState::untitled();
        editor.insert_text("foo bar foo\nfoo\n");
        editor.document.set_cursor(Position::new(0, 0));
        assert!(editor.select_next_match());
        assert_eq!(editor.extra_cursors().len(), 0);
        assert!(editor.document.cursor().selection.is_some());
        assert!(editor.select_next_match());
        assert_eq!(editor.extra_cursors().len(), 1);
        assert!(editor.select_next_match());
        assert_eq!(editor.extra_cursors().len(), 2);
        assert!(!editor.select_next_match());
        editor.insert_text("z");
        assert_eq!(editor.document.text(), "z bar z\nz\n");
    }

    #[test]
    fn folding_below_the_initial_view_preserves_a_manually_scrolled_viewport() {
        use crossterm::event::KeyModifiers;
        let content = include_str!("../../../tests/fixtures/folding-context.ts");
        for wrapped in [false, true] {
            for numbers in [false, true] {
                for name in [
                    "firstBelowViewport",
                    "nestedBelowViewport",
                    "lastBelowViewport",
                ] {
                    let mut editor =
                        EditorState::from_content(Some("folding-context.ts".into()), content);
                    editor.set_word_wrap(wrapped);
                    let syntax = syntax();
                    let mut terminal = Terminal::new(TestBackend::new(72, 12)).unwrap();
                    let target = content
                        .lines()
                        .position(|line| line.starts_with(&format!("export function {name}")))
                        .unwrap();
                    let mut draw = |editor: &mut EditorState| {
                        terminal
                            .draw(|frame| {
                                editor.draw(
                                    frame,
                                    frame.area(),
                                    &EditorView {
                                        palette: Palette::ultra_dusk(),
                                        syntax: &syntax,
                                        show_line_numbers: numbers,
                                        tab_size: 4,
                                        focused: true,
                                    },
                                )
                            })
                            .unwrap();
                    };
                    draw(&mut editor);
                    assert!(!editor.rendered_rows.iter().any(|(line, _)| *line == target));
                    editor.scroll_by(isize::try_from(target.saturating_sub(2)).unwrap());
                    draw(&mut editor);
                    assert!(
                        editor.cursor_screen.is_none(),
                        "An offscreen caret must stay hidden after scrolling"
                    );
                    let top = editor.scroll_top;
                    let caret = editor.document.cursor();
                    for folded in [true, false, true, false] {
                        let row = editor
                            .rendered_rows
                            .iter()
                            .position(|(line, start)| *line == target && *start == 0)
                            .unwrap();
                        let gutter = if numbers {
                            gutter_width(editor.document.len_lines())
                        } else {
                            0
                        };
                        editor.mouse_press(
                            Rect::new(0, 0, 72, 12),
                            u16::try_from(gutter).unwrap(),
                            u16::try_from(row).unwrap(),
                            numbers,
                            4,
                            KeyModifiers::NONE,
                            1,
                        );
                        editor.mouse_release();
                        draw(&mut editor);
                        assert_eq!(
                            editor.scroll_top, top,
                            "{name}: wrapped={wrapped}, numbers={numbers}, folded={folded}"
                        );
                        assert_eq!(
                            editor.document.cursor(),
                            caret,
                            "A gutter fold must not move an unrelated caret"
                        );
                        assert_eq!(
                            editor
                                .folded_ranges()
                                .iter()
                                .any(|(line, _)| *line == target),
                            folded
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn the_cursor_line_is_highlighted_with_and_without_word_wrap() {
        // Word wrap is on by default, so the wrapped renderer is the one users see.
        for wrapped in [false, true] {
            let mut editor = EditorState::from_content(None, "first\nsecond line\nthird\n");
            editor.set_word_wrap(wrapped);
            editor.set_wrap_column(Some(6));
            editor.document.set_cursor(Position::new(1, 2));
            let palette = Palette::ultra_dusk();
            assert_ne!(palette.line_highlight, palette.background);
            let mut terminal = Terminal::new(TestBackend::new(30, 8)).unwrap();
            terminal
                .draw(|frame| {
                    editor.draw(
                        frame,
                        frame.area(),
                        &EditorView {
                            palette,
                            syntax: &syntax(),
                            show_line_numbers: true,
                            tab_size: 4,
                            focused: true,
                        },
                    )
                })
                .unwrap();
            let buffer = terminal.backend().buffer();
            let gutter = u16::try_from(gutter_width(editor.document.len_lines()) + 2).unwrap();
            let rows = editor.rendered_rows.clone();
            assert!(rows.iter().filter(|(line, _)| *line == 1).count() >= 1);
            for (row, (line, start)) in rows.iter().enumerate() {
                let y = u16::try_from(row).unwrap();
                let text = editor.document.line(*line).unwrap_or_default();
                let visible = text.chars().count().saturating_sub(*start).min(6);
                for x in gutter..gutter + u16::try_from(visible).unwrap() {
                    let background = buffer[(x, y)].bg;
                    if *line == 1 {
                        assert_eq!(
                            background, palette.line_highlight,
                            "wrapped={wrapped}: cursor line cell ({x}, {y}) is not highlighted"
                        );
                    } else {
                        assert_ne!(
                            background, palette.line_highlight,
                            "wrapped={wrapped}: line {line} cell ({x}, {y}) is highlighted"
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn nesting_boundaries_track_the_cursor_through_wrap_scroll_and_selection() {
        let content = "fn a_very_long_function_name(argument: usize) {\n\tlet text = \"é🦀\";\n}\n";
        for (wrapped, left, expected) in [(false, 0, 1), (true, 0, 2), (false, 21, 1)] {
            let mut editor = EditorState::from_content(Some("scope.rs".into()), content);
            editor.set_word_wrap(wrapped);
            editor.document.set_cursor(Position::new(1, 1));
            editor.scroll_by(0);
            editor.scroll_left = left;
            let syntax = syntax();
            let mut palette = Palette::ultra_dusk();
            palette.bracket_match_bg = ratatui::style::Color::Rgb(40, 20, 70);
            palette.bracket_match_fg = ratatui::style::Color::Black;
            let mut terminal = Terminal::new(TestBackend::new(32, 10)).unwrap();
            let mut draw = |editor: &mut EditorState| {
                terminal
                    .draw(|frame| {
                        editor.draw(
                            frame,
                            frame.area(),
                            &EditorView {
                                palette,
                                syntax: &syntax,
                                show_line_numbers: true,
                                tab_size: 4,
                                focused: true,
                            },
                        )
                    })
                    .unwrap();
                terminal.backend().buffer().clone()
            };
            let buffer = draw(&mut editor);
            let marked: Vec<_> = buffer
                .content()
                .iter()
                .filter(|cell| {
                    matches!(cell.symbol(), "{" | "}")
                        && cell.bg == palette.bracket_match_bg
                        && cell.modifier.contains(Modifier::UNDERLINED)
                })
                .collect();
            assert_eq!(marked.len(), expected, "wrapped={wrapped}, left={left}");
            let revision = editor.fold_revision;
            assert_eq!(editor.document.text(), content);
            editor.document.set_cursor(Position::new(3, 0));
            let buffer = draw(&mut editor);
            assert!(
                buffer
                    .content()
                    .iter()
                    .all(|cell| cell.bg != palette.bracket_match_bg)
            );
            assert_eq!(
                editor.fold_revision, revision,
                "Cursor movement reuses lexical structure"
            );
            editor.document.set_selection(tide_core::Selection::new(
                Position::new(0, 0),
                Position::new(2, 1),
            ));
            let buffer = draw(&mut editor);
            assert!(
                buffer
                    .content()
                    .iter()
                    .filter(|cell| matches!(cell.symbol(), "{" | "}"))
                    .all(|cell| cell.bg == palette.selection)
            );
            editor.set_cursor_states(&[tide_core::Cursor::at(1, 1)]);
            editor
                .search_ranges("}", crate::search::SearchOptions::default())
                .unwrap();
            let buffer = draw(&mut editor);
            assert!(
                buffer
                    .content()
                    .iter()
                    .filter(|cell| cell.symbol() == "}")
                    .all(|cell| cell.bg == palette.line_highlight
                        && cell.fg != palette.bracket_match_fg)
            );
        }
    }

    #[test]
    fn historical_titles_shorten_non_ascii_revisions_on_a_char_boundary() {
        let mut editor = EditorState::untitled();
        editor.historical = Some((PathBuf::from("a.rs"), "abcdefgé12".to_owned()));
        assert_eq!(editor.title(), "a.rs @ abcdefgé [read-only]");
    }

    #[test]
    fn fold_index_reparses_from_the_edit_and_defers_large_files() {
        let source = "fn f() {\n    body();\n}\n".repeat(400);
        let mut editor = EditorState::from_content(None, &source);
        editor.ensure_fold_regions();
        assert_eq!(editor.fold_regions.len(), 400);
        crate::delimiters::PARSED_LINES.set(0);
        editor.document.set_cursor(Position::new(1198, 0));
        editor.insert_text("{");
        editor.ensure_fold_regions();
        assert!(crate::delimiters::PARSED_LINES.get() <= 3);
        let mut full = EditorState::from_content(None, &editor.document.text());
        full.ensure_fold_regions();
        assert_eq!(editor.fold_regions, full.fold_regions);

        let large = "x\n".repeat(DEFERRED_FOLD_CHARS / 2 + 1);
        let mut editor = EditorState::from_content(None, &large);
        editor.refresh_fold_regions(true);
        assert!(editor.fold_deferred.is_some());
        std::thread::sleep(FOLD_IDLE);
        assert!(editor.fold_refresh_due());
        editor.refresh_fold_regions(true);
        assert!(editor.fold_deferred.is_none());
        crate::delimiters::PARSED_LINES.set(0);
        editor.document.set_cursor(Position::new(0, 0));
        editor.insert_text("{");
        editor.refresh_fold_regions(true);
        assert_eq!(crate::delimiters::PARSED_LINES.get(), 0);
        assert!(!editor.fold_refresh_due());
        editor.ensure_fold_regions();
        assert!(crate::delimiters::PARSED_LINES.get() > 0);
    }

    #[test]
    fn backspace_and_delete_remove_whole_grapheme_clusters() {
        let coder = "\u{1F469}\u{200D}\u{1F4BB}";
        let mut editor = EditorState::untitled();
        editor.insert_text(&format!("a{coder}b"));
        editor.document.set_cursor(Position::new(0, 4));
        editor.backspace();
        assert_eq!(editor.document.text(), "ab");
        assert_eq!(editor.document.cursor().position, Position::new(0, 1));
        editor.insert_text(coder);
        editor.document.set_cursor(Position::new(0, 1));
        editor.delete_forward();
        assert_eq!(editor.document.text(), "ab");

        let mut editor = EditorState::untitled();
        editor.insert_text(&format!("x{coder}\ny{coder}"));
        editor.document.set_cursor(Position::new(0, 4));
        editor.add_cursor_below();
        editor.backspace();
        assert_eq!(editor.document.text(), "x\ny");
        editor.insert_text(coder);
        editor.move_left(false);
        editor.delete_forward();
        assert_eq!(editor.document.text(), "x\ny");
    }

    #[test]
    fn reload_decodes_non_utf8_files_lossily_and_recovers() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("file.txt");
        std::fs::write(&path, "one\n").unwrap();
        let mut editor = EditorState::from_path(&path).unwrap();
        std::fs::write(&path, b"caf\xe9\n").unwrap();
        editor.reload().unwrap();
        assert!(editor.document.with(Document::is_lossy));
        assert_eq!(editor.document.text(), "caf\u{FFFD}\n");
        assert!(!editor.is_externally_changed());
        std::fs::write(&path, "two\n").unwrap();
        editor.reload().unwrap();
        assert!(!editor.document.with(Document::is_lossy));
        editor.insert_text("x");
        assert_eq!(editor.document.text(), "xtwo\n");
    }

    #[test]
    fn status_line_ending_is_cached_per_revision() {
        let mut editor = EditorState::untitled();
        editor.insert_text("a\r\nb\r\n");
        assert_eq!(editor.document.status_metadata().0, "CRLF");
        let version = editor.revision();
        editor.document.shared.lock().unwrap().line_ending = Some((version, "cached"));
        assert_eq!(editor.document.status_metadata().0, "cached");
        editor.insert_text("c");
        assert_eq!(editor.document.status_metadata().0, "CRLF");
    }

    #[test]
    fn folds_blocks_and_unfolds_them() {
        let mut editor = EditorState::untitled();
        editor.insert_text("fn main() {\n    let x = 1;\n    let y = 2;\n}\n");
        editor.document.set_cursor(Position::new(0, 0));
        editor.fold();
        assert_eq!(editor.folded_ranges(), &[(0, 3)]);
        let backend = TestBackend::new(60, 8);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|frame| {
                editor.draw(
                    frame,
                    frame.area(),
                    &EditorView {
                        palette: Palette::ultra_dusk(),
                        syntax: &syntax(),
                        show_line_numbers: true,
                        tab_size: 2,
                        focused: true,
                    },
                );
            })
            .unwrap();
        let text = terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .map(|cell| cell.symbol())
            .collect::<String>();
        assert!(text.contains("fn main()"));
        assert!(!text.contains("let x"));
        assert!(text.contains('…'));
        editor.unfold_all();
        assert!(editor.folded_ranges().is_empty());
    }

    #[test]
    fn renders_and_sets_cursor() {
        let mut editor = EditorState::untitled();
        editor.insert_text("fn main() {}\n");
        let backend = TestBackend::new(60, 8);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|frame| {
                editor.draw(
                    frame,
                    frame.area(),
                    &EditorView {
                        palette: Palette::ultra_dusk(),
                        syntax: &syntax(),
                        show_line_numbers: true,
                        tab_size: 2,
                        focused: true,
                    },
                );
            })
            .unwrap();
        let buffer = terminal.backend().buffer();
        let text = buffer
            .content()
            .iter()
            .map(|cell| cell.symbol())
            .collect::<String>();
        assert!(text.contains("fn main"));
    }
}
