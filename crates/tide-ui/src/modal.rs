use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use nucleo::{Config, Matcher, Utf32Str};
use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Clear, List, ListItem, Paragraph};

use crate::command::Command;
use crate::connection_form::Submission as ConnectionSubmission;
use crate::palette::{ActionRole, Palette};
use std::path::PathBuf;
use tide_ai::cli::CliProvider;
use tide_core::num::saturating_u16;
use tide_core::{display_width, grapheme_boundaries, truncate_to_width};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PromptAction {
    SaveAs,
    GotoLine,
    Commit,
    Find,
    FindInFiles,
    ReplaceFind,
    ReplaceCurrentWith(String),
    ReplaceWith(String),
    CreateBranch,
    RenameBranch(String),
    StashMessage,
    Rename,
    UnlockSecrets,
    UnlockConnectionSecrets,
    NewConnection,
    EditConnection(String),
    NewFile,
    NewFolder,
    RenameFile(std::path::PathBuf),
    ExportResults,
    EditRowCell(String, String),
    SaveSessionName,
    WorkspaceSymbol,
    EditSetting(String),
    EditKeybinding(String),
    RenameAgent(String, String),
    CreateTag,
    /// The new tag's name.
    TagMessage(String),
    /// Rewrites the last commit; `message` is its current message and `upstream` names the
    /// upstream branch that already contains it.
    CommitAmend {
        commit: String,
        message: String,
        upstream: Option<String>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConfirmAction {
    CloseDirty,
    CloseTab {
        pane: usize,
        index: usize,
    },
    CloseAllDirty,
    ClosePane(usize),
    SaveAs(std::path::PathBuf),
    RenamePath(std::path::PathBuf, std::path::PathBuf),
    SwitchSession(String),
    QuitDirty,
    RunSql(String),
    DeleteRow(Box<tide_db::RowMutation>),
    ClearQueryHistory(Option<String>),
    DeleteConnection(String),
    DeletePath(std::path::PathBuf),
    DiscardGitChange(String),
    /// Unstaged changes in these tracked paths.
    DiscardAllGitChanges(Vec<String>),
    GitCommand(Vec<String>),
    DeleteAiSession {
        provider: String,
        session: String,
        archive: bool,
    },
    ReloadFile,
    OverwriteFile,
    ExportResults(std::path::PathBuf),
    /// The terminal's unique serial.
    KillTerminal(u64),
    CloseAllTerminals,
}

impl ConfirmAction {
    pub fn offers_save(&self) -> bool {
        matches!(
            self,
            Self::CloseDirty
                | Self::CloseTab { .. }
                | Self::CloseAllDirty
                | Self::ClosePane(_)
                | Self::SwitchSession(_)
                | Self::QuitDirty
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ModalKind {
    CommandPalette,
    Tabs(usize),
    AllTabs,
    MoveTab,
    AddPanel,
    StatusHistory,
    QuickOpen,
    SearchResults,
    ThemePicker,
    BranchPicker,
    GitChoice(String),
    Settings,
    Keybindings,
    Completion,
    AgentSessions,
    AgentWorkspaces,
    AiCliPicker,
    ConnectionPicker,
    ConnectionForm,
    RowInsert,
    SchemaTables,
    QueryHistory,
    SessionPicker,
    CodeActions,
    Info,
    SaveAs,
    Prompt(PromptAction),
    Find,
    Confirm(ConfirmAction),
}

#[derive(Debug, Clone)]
pub struct ModalItem {
    pub label: String,
    pub detail: String,
    pub value: String,
    pub current: bool,
}

impl ModalItem {
    #[must_use]
    pub fn new(
        label: impl Into<String>,
        detail: impl Into<String>,
        value: impl Into<String>,
    ) -> Self {
        Self {
            label: label.into(),
            detail: detail.into(),
            value: value.into(),
            current: false,
        }
    }

    #[must_use]
    pub fn current(mut self, current: bool) -> Self {
        self.current = current;
        self
    }
}

#[derive(Debug, Clone)]
pub struct ModalBase {
    pub anchor: Option<(u16, u16)>,
    pub checked_actions: Vec<usize>,
    pub loading: bool,
    pub pending_submit: bool,
    pub kind: ModalKind,
    pub title: String,
    pub input: String,
    pub cursor: usize,
    pub selected: usize,
    pub items: Vec<ModalItem>,
    pub filtered: Vec<usize>,
    pub hints: Option<String>,
    pub message: Option<String>,
    pub actions_focused: bool,
    pub action_index: usize,
    pub message_scroll: usize,
    pub find_host: Option<Rect>,
    pub field_selected: bool,
    pub replacement_selected: bool,
    pub search_matches: Option<usize>,
    pub replace_mode: bool,
    pub replacement: String,
    pub replacement_cursor: usize,
    pub replacement_focused: bool,
}

impl ModalBase {
    #[must_use]
    pub fn new(kind: ModalKind, title: impl Into<String>, items: Vec<ModalItem>) -> Self {
        let filtered = (0..items.len()).collect();
        Self {
            anchor: None,
            checked_actions: Vec::new(),
            loading: false,
            pending_submit: false,
            kind,
            title: title.into(),
            input: String::new(),
            cursor: 0,
            selected: 0,
            items,
            filtered,
            hints: None,
            message: None,
            actions_focused: false,
            action_index: 0,
            message_scroll: 0,
            find_host: None,
            field_selected: false,
            replacement_selected: false,
            search_matches: None,
            replace_mode: false,
            replacement: String::new(),
            replacement_cursor: 0,
            replacement_focused: false,
        }
    }

    #[must_use]
    pub fn prompt(kind: ModalKind, title: impl Into<String>, initial: &str) -> Self {
        Self {
            anchor: None,
            checked_actions: Vec::new(),
            loading: false,
            pending_submit: false,
            kind,
            title: title.into(),
            input: initial.to_owned(),
            cursor: initial.chars().count(),
            selected: 0,
            items: Vec::new(),
            filtered: Vec::new(),
            hints: None,
            message: None,
            actions_focused: false,
            action_index: 0,
            message_scroll: 0,
            find_host: None,
            field_selected: false,
            replacement_selected: false,
            search_matches: None,
            replace_mode: false,
            replacement: String::new(),
            replacement_cursor: 0,
            replacement_focused: false,
        }
    }

    #[must_use]
    pub fn confirm(
        action: ConfirmAction,
        title: impl Into<String>,
        message: impl Into<String>,
    ) -> Self {
        let cancel_index = if action.offers_save() { 2 } else { 1 };
        Self {
            anchor: None,
            checked_actions: Vec::new(),
            loading: false,
            pending_submit: false,
            kind: ModalKind::Confirm(action),
            title: title.into(),
            input: String::new(),
            cursor: 0,
            selected: 0,
            items: Vec::new(),
            filtered: Vec::new(),
            hints: Some("Enter confirm   Esc cancel".to_owned()),
            message: Some(message.into()),
            actions_focused: true,
            action_index: cancel_index,
            message_scroll: 0,
            find_host: None,
            field_selected: false,
            replacement_selected: false,
            search_matches: None,
            replace_mode: false,
            replacement: String::new(),
            replacement_cursor: 0,
            replacement_focused: false,
        }
    }

    pub fn with_hints(mut self, hints: impl Into<String>) -> Self {
        self.hints = Some(hints.into());
        self
    }

    pub fn push_char(&mut self, ch: char) {
        self.push_str(ch.encode_utf8(&mut [0; 4]));
    }

    /// Inserts `text` at the cursor and filters once, however long it is.
    pub fn push_str(&mut self, text: &str) {
        if text.is_empty() {
            return;
        }
        self.pending_submit = false;
        self.input
            .insert_str(char_byte(&self.input, self.cursor), text);
        self.cursor += text.chars().count();
        self.refilter();
    }

    pub fn backspace(&mut self) {
        self.pending_submit = false;
        let end = self.cursor;
        self.move_left();
        if end > self.cursor {
            let start_byte = char_byte(&self.input, self.cursor);
            let end_byte = char_byte(&self.input, end);
            self.input.replace_range(start_byte..end_byte, "");
            self.refilter();
        }
    }

    pub fn delete(&mut self) {
        self.pending_submit = false;
        let start = self.cursor;
        self.move_right();
        let end = self.cursor;
        self.cursor = start;
        if end > start {
            self.input.replace_range(
                char_byte(&self.input, start)..char_byte(&self.input, end),
                "",
            );
            self.refilter();
        }
    }

    pub fn move_left(&mut self) {
        self.cursor = grapheme_boundaries(&self.input)
            .into_iter()
            .rev()
            .find(|position| *position < self.cursor)
            .unwrap_or(0);
    }

    pub fn move_right(&mut self) {
        self.cursor = grapheme_boundaries(&self.input)
            .into_iter()
            .find(|position| *position > self.cursor)
            .unwrap_or(self.input.chars().count());
    }

    pub fn move_up(&mut self) {
        if self.selected > 0 {
            self.selected -= 1;
        }
    }

    pub fn move_down(&mut self) {
        if self.selected + 1 < self.filtered.len() {
            self.selected += 1;
        }
    }

    pub fn select_first(&mut self) {
        self.selected = 0;
    }

    pub fn select_last(&mut self) {
        self.selected = self.filtered.len().saturating_sub(1);
    }

    pub fn refilter(&mut self) {
        #[cfg(test)]
        REFILTERS.set(REFILTERS.get() + 1);
        if self.input.is_empty() || !self.is_searchable() {
            self.filtered = (0..self.items.len()).collect();
            self.selected = self.selected.min(self.filtered.len().saturating_sub(1));
            return;
        }
        let mut matcher = Matcher::new(Config::DEFAULT);
        let mut needle_buffer = Vec::new();
        let needle = Utf32Str::new(&self.input, &mut needle_buffer);
        let mut results = Vec::new();
        let mut haystack_buffer = Vec::new();
        let mut search = String::new();
        for (index, item) in self.items.iter().enumerate() {
            let haystack = if self.kind == ModalKind::CommandPalette {
                use std::fmt::Write as _;
                search.clear();
                let _ = write!(search, "{}: {} {}", item.detail, item.label, item.value);
                Utf32Str::new(&search, &mut haystack_buffer)
            } else {
                Utf32Str::new(&item.label, &mut haystack_buffer)
            };
            if let Some(score) = matcher.fuzzy_match_greedy(haystack, needle) {
                results.push((score, index));
            }
        }
        results.sort_by_key(|entry| std::cmp::Reverse(entry.0));
        self.filtered = results.into_iter().map(|entry| entry.1).collect();
        self.selected = self.selected.min(self.filtered.len().saturating_sub(1));
    }

    #[must_use]
    pub fn selected_item(&self) -> Option<&ModalItem> {
        self.filtered
            .get(self.selected)
            .and_then(|index| self.items.get(*index))
    }

    #[must_use]
    pub fn is_searchable(&self) -> bool {
        matches!(
            self.kind,
            ModalKind::CommandPalette
                | ModalKind::Tabs(_)
                | ModalKind::AllTabs
                | ModalKind::MoveTab
                | ModalKind::AddPanel
                | ModalKind::StatusHistory
                | ModalKind::QuickOpen
                | ModalKind::SearchResults
                | ModalKind::ThemePicker
                | ModalKind::BranchPicker
                | ModalKind::GitChoice(_)
                | ModalKind::Settings
                | ModalKind::Keybindings
                | ModalKind::Completion
                | ModalKind::AgentSessions
                | ModalKind::AgentWorkspaces
                | ModalKind::AiCliPicker
                | ModalKind::ConnectionPicker
                | ModalKind::SchemaTables
                | ModalKind::QueryHistory
                | ModalKind::SessionPicker
                | ModalKind::CodeActions
        )
    }

    #[must_use]
    pub fn is_prompt(&self) -> bool {
        matches!(self.kind, ModalKind::Prompt(_) | ModalKind::Find)
    }

    /// Keep explicit decisions and unsaved database/file forms open.
    pub fn dismiss_on_outside_click(&self) -> bool {
        !matches!(
            self.kind,
            ModalKind::Confirm(_)
                | ModalKind::SaveAs
                | ModalKind::ConnectionForm
                | ModalKind::RowInsert
                | ModalKind::Prompt(PromptAction::SaveAs | PromptAction::EditRowCell(_, _))
        )
    }

    #[must_use]
    pub fn geometry(&self, area: Rect) -> Rect {
        if self.kind == ModalKind::Find {
            let host = self
                .find_host
                .filter(|host| !host.is_empty())
                .unwrap_or(area);
            let width = host.width.saturating_sub(6).clamp(12, 56).min(area.width);
            let errors = saturating_u16(self.message.as_ref().map_or(0, |message| {
                wrap_message(message, width.saturating_sub(2) as usize)
                    .len()
                    .clamp(1, 3)
            }));
            let height = (2
                + if self.replace_mode { 2 } else { 1 }
                + self.find_action_rows(width.saturating_sub(2))
                + 1
                + errors)
                .min(area.height);
            let x = host
                .right()
                .saturating_sub(width + 9)
                .max(area.x)
                .min(area.right().saturating_sub(width));
            let y = host.y.max(area.y).min(area.bottom().saturating_sub(height));
            return Rect::new(x, y, width, height);
        }
        let (width, height) = match &self.kind {
            ModalKind::CommandPalette
            | ModalKind::Tabs(_)
            | ModalKind::AllTabs
            | ModalKind::MoveTab
            | ModalKind::AddPanel
            | ModalKind::StatusHistory => (60, 20),
            ModalKind::QuickOpen => (70, 20),
            ModalKind::SearchResults => (70, 20),
            ModalKind::ThemePicker => (60, 20),
            ModalKind::BranchPicker | ModalKind::GitChoice(_) => (60, 20),
            ModalKind::Settings => (70, 25),
            ModalKind::Keybindings => (70, 25),
            ModalKind::Completion => (
                saturating_u16(
                    self.items
                        .iter()
                        .map(|item| display_width(&item.label) + display_width(&item.detail) + 5)
                        .max()
                        .unwrap_or(24)
                        .clamp(24, 80),
                ),
                saturating_u16(self.filtered.len().clamp(1, 10)) + 2,
            ),
            ModalKind::AgentSessions | ModalKind::AgentWorkspaces => (70, 20),
            ModalKind::AiCliPicker => (60, 15),
            ModalKind::ConnectionPicker => (60, 20),
            ModalKind::ConnectionForm => (78, 19),
            ModalKind::RowInsert => (96, 24),
            ModalKind::SchemaTables => (70, 22),
            ModalKind::QueryHistory => (70, 22),
            ModalKind::SessionPicker => (60, 20),
            ModalKind::CodeActions => (60, 20),
            ModalKind::Info => (60, 8),
            ModalKind::SaveAs => (70, 25),
            ModalKind::Prompt(PromptAction::GotoLine) => (40, 7),
            ModalKind::Prompt(_) => (50, 7),
            ModalKind::Find => (88, if self.replace_mode { 11 } else { 10 }),
            ModalKind::Confirm(_) => (50, 9),
        };
        let width = width.min(area.width.saturating_sub(2).max(1));
        let message_height = self.message.as_ref().map_or(0, |text| {
            saturating_u16(
                wrap_message(text, width.saturating_sub(2) as usize)
                    .len()
                    .saturating_add(5),
            )
        });
        let height = if self.kind == ModalKind::Info && self.anchor.is_some() {
            saturating_u16(
                wrap_message(
                    self.message.as_deref().unwrap_or_default(),
                    width.saturating_sub(2) as usize,
                )
                .len()
                .clamp(1, 14),
            ) + 2
        } else {
            height.max(message_height)
        };
        let height = height
            .min(area.height.saturating_sub(2).max(1))
            .min(area.height);
        let width = width.min(area.width);
        let (x, y) = if let Some((x, y)) = self.anchor {
            (
                x.clamp(area.x, area.right().saturating_sub(width).max(area.x)),
                if y + 1 + height <= area.bottom() {
                    y + 1
                } else {
                    y.saturating_sub(height).max(area.y)
                },
            )
        } else {
            (
                area.x + area.width.saturating_sub(width) / 2,
                area.y + (area.height / 6).min(area.height.saturating_sub(height)),
            )
        };
        Rect::new(x, y, width, height)
    }

    fn content_layout(&self, inner: Rect) -> [Rect; 5] {
        if self.kind == ModalKind::Completion
            || (self.kind == ModalKind::Info && self.anchor.is_some())
        {
            return [
                Rect::default(),
                Rect::default(),
                inner,
                Rect::default(),
                Rect::default(),
            ];
        }
        let input = self.is_searchable() || self.is_prompt() || self.kind == ModalKind::SaveAs;
        let list = self.is_searchable() || self.kind == ModalKind::SaveAs;
        Layout::vertical([
            Constraint::Length(if self.kind == ModalKind::Find && self.replace_mode {
                2
            } else {
                u16::from(input)
            }),
            Constraint::Length(u16::from(list)),
            Constraint::Min(0),
            Constraint::Length(if self.kind == ModalKind::Find {
                self.find_action_rows(inner.width)
            } else {
                let labels = self.action_labels();
                let total = labels.iter().map(|s| display_width(s)).sum::<usize>()
                    + labels.len().saturating_sub(1);
                if total > inner.width as usize {
                    saturating_u16(labels.len())
                } else {
                    1
                }
            }),
            Constraint::Length(u16::from(inner.height > 3)),
        ])
        .areas(inner)
    }

    pub fn action_bounds(&self, area: Rect) -> Vec<(Rect, usize, String)> {
        if self.kind == ModalKind::Completion
            || (self.kind == ModalKind::Info && self.anchor.is_some())
        {
            return Vec::new();
        }
        let inner = Block::bordered().inner(self.geometry(area));
        let bounds = self.content_layout(inner)[3];
        self.actions_in(bounds)
    }

    pub fn scroll_message(&mut self, area: Rect, amount: isize) {
        let body = self.content_layout(Block::bordered().inner(self.geometry(area)))[2];
        let count = wrap_message(
            self.message.as_deref().unwrap_or_default(),
            body.width as usize,
        )
        .len();
        self.message_scroll = self
            .message_scroll
            .saturating_add_signed(amount)
            .min(count.saturating_sub(body.height as usize));
    }

    fn offers_save(&self) -> bool {
        matches!(&self.kind,ModalKind::Confirm(action) if action.offers_save())
    }
    fn action_labels(&self) -> Vec<String> {
        let mut labels = if self.kind == ModalKind::Info {
            vec![" Close [Esc] ".to_owned()]
        } else {
            let label = match self.kind {
                ModalKind::Confirm(ref action) if action.offers_save() => "Discard [N]",
                ModalKind::Confirm(
                    ConfirmAction::DeleteConnection(_)
                    | ConfirmAction::DeleteRow(_)
                    | ConfirmAction::DeletePath(_)
                    | ConfirmAction::DeleteAiSession { archive: false, .. },
                ) => "Delete [Y]",
                ModalKind::Confirm(ConfirmAction::DeleteAiSession { archive: true, .. }) => {
                    "Archive [Y]"
                }
                ModalKind::Confirm(
                    ConfirmAction::SaveAs(_)
                    | ConfirmAction::RenamePath(_, _)
                    | ConfirmAction::OverwriteFile
                    | ConfirmAction::ExportResults(_),
                ) => "Overwrite [Y]",
                ModalKind::Confirm(ConfirmAction::ReloadFile) => "Reload [Y]",
                ModalKind::Confirm(ConfirmAction::RunSql(_)) => "Run [Y]",
                ModalKind::Confirm(ConfirmAction::ClearQueryHistory(_)) => "Clear [Y]",
                ModalKind::Confirm(ConfirmAction::SwitchSession(_)) => "Switch [Y]",
                ModalKind::Confirm(ConfirmAction::KillTerminal(_)) => "Kill [Y]",
                ModalKind::Confirm(ConfirmAction::GitCommand(ref args)) => {
                    match args.iter().map(String::as_str).collect::<Vec<_>>()[..] {
                        ["push", ..] => "Push [Y]",
                        ["reset", ..] => "Reset [Y]",
                        ["clean", ..] => "Delete [Y]",
                        ["rebase", "--abort"] => "Abort [Y]",
                        ["rebase", ..] => "Rebase [Y]",
                        ["commit", "--amend", ..] => "Amend [Y]",
                        _ => "Discard [Y]",
                    }
                }
                ModalKind::Confirm(ConfirmAction::CloseAllTerminals) => "Close [Y]",
                ModalKind::Confirm(_) => "Discard [Y]",
                ModalKind::SaveAs => "Save",
                ModalKind::Settings | ModalKind::Keybindings => "Edit",
                ModalKind::Prompt(_) | ModalKind::Find => "Submit",
                _ => "Open",
            };
            vec![format!(" {label} "), " Cancel [Esc] ".to_owned()]
        };
        if self.offers_save() {
            labels.insert(0, " Save [Y] ".to_owned());
        }
        labels
    }
    fn find_action_rows(&self, width: u16) -> u16 {
        let mut rows = 1;
        let mut used = 0;
        for (index, label) in [
            "Next",
            "Previous",
            "Case",
            "Whole word",
            "Regex",
            "Selection",
            if self.replace_mode {
                "Replace"
            } else {
                "Replace…"
            },
            "Replace all",
            "Close",
        ]
        .iter()
        .enumerate()
        {
            let size = (display_width(label)
                + 2
                + if self.checked_actions.contains(&index) {
                    2
                } else {
                    0
                })
            .min(width as usize);
            if used + size > width as usize && used > 0 {
                rows += 1;
                used = 0;
            }
            used += size + 1;
        }
        rows
    }

    fn actions_in(&self, bounds: Rect) -> Vec<(Rect, usize, String)> {
        if self.kind == ModalKind::Find {
            let labels = [
                "Next",
                "Previous",
                "Case",
                "Whole word",
                "Regex",
                "Selection",
                if self.replace_mode {
                    "Replace"
                } else {
                    "Replace…"
                },
                "Replace all",
                "Close",
            ];
            let mut result = Vec::new();
            let mut rows: Vec<Vec<(usize, String)>> = vec![Vec::new()];
            let mut used = 0;
            for (index, label) in labels.iter().enumerate() {
                let label = format!(
                    " {label}{} ",
                    if self.checked_actions.contains(&index) {
                        " ✓"
                    } else {
                        ""
                    }
                );
                let width = display_width(&label).min(bounds.width as usize);
                if used + width > bounds.width as usize && !rows.last().unwrap().is_empty() {
                    rows.push(Vec::new());
                    used = 0;
                }
                rows.last_mut().unwrap().push((index, label));
                used += width + 1;
            }
            for (row, labels) in rows.into_iter().enumerate().take(bounds.height as usize) {
                let width = labels
                    .iter()
                    .map(|(_, label)| display_width(label).min(bounds.width as usize))
                    .sum::<usize>()
                    + labels.len().saturating_sub(1);
                let mut x = bounds.x + bounds.width.saturating_sub(saturating_u16(width)) / 2;
                for (index, label) in labels {
                    let width =
                        saturating_u16(display_width(&label)).min(bounds.right().saturating_sub(x));
                    let y = bounds.y + saturating_u16(row);
                    result.push((Rect::new(x, y, width, 1), index, label));
                    x += width + 1;
                }
            }
            return result;
        }
        let labels = self.action_labels();
        let total = labels
            .iter()
            .map(|label| display_width(label))
            .sum::<usize>()
            + labels.len().saturating_sub(1);
        let stacked = total > bounds.width as usize;
        let mut labels = labels.into_iter().enumerate().collect::<Vec<_>>();
        if stacked && (bounds.height as usize) < labels.len() {
            labels.rotate_right(1);
        }
        let mut x = bounds.x + bounds.width.saturating_sub(saturating_u16(total)) / 2;
        labels
            .into_iter()
            .enumerate()
            .filter_map(|(row, (index, label))| {
                let y = bounds.y + if stacked { saturating_u16(row) } else { 0 };
                if y >= bounds.bottom() {
                    return None;
                }
                let width = saturating_u16(display_width(&label)).min(bounds.width);
                if stacked {
                    x = bounds.x + bounds.width.saturating_sub(width) / 2;
                }
                let width = width.min(bounds.right().saturating_sub(x));
                let rect = Rect::new(x, y, width, 1);
                x += width.saturating_add(1);
                Some((rect, index, label))
            })
            .collect()
    }

    fn action_role(&self, index: usize) -> ActionRole {
        if self.kind == ModalKind::Find {
            return if index == 0 {
                ActionRole::Primary
            } else if self.replace_mode && matches!(index, 6 | 7) {
                ActionRole::Confirmation
            } else {
                ActionRole::Neutral
            };
        }
        if self.kind == ModalKind::Info || index + 1 == self.action_labels().len() {
            return ActionRole::Neutral;
        }
        if self.offers_save() {
            return if index == 0 {
                ActionRole::Confirmation
            } else {
                ActionRole::Destructive
            };
        }
        match &self.kind {
            ModalKind::SaveAs | ModalKind::Prompt(PromptAction::EditRowCell(_, _)) => {
                ActionRole::Confirmation
            }
            ModalKind::Confirm(
                ConfirmAction::RunSql(_) | ConfirmAction::DeleteAiSession { archive: true, .. },
            ) => ActionRole::Primary,
            ModalKind::Confirm(_) => ActionRole::Destructive,
            _ => ActionRole::Primary,
        }
    }

    fn action_enabled(&self, index: usize) -> bool {
        if self.kind == ModalKind::Find && matches!(index, 0 | 1 | 6 | 7) {
            if !self.replace_mode && matches!(index, 6 | 7) {
                return true;
            }
            return !self.input.is_empty()
                && self.message.is_none()
                && self.search_matches != Some(0);
        }
        index != 0 || !self.is_searchable() || !self.filtered.is_empty()
    }

    fn render_actions(&self, frame: &mut Frame, bounds: Rect, palette: Palette) {
        for (rect, index, label) in self.actions_in(bounds) {
            let enabled = self.action_enabled(index);
            let focused = enabled && self.actions_focused && self.action_index == index;
            let style = palette.action(self.action_role(index), focused, enabled);
            frame.render_widget(
                Paragraph::new(truncate_to_width(&label, rect.width as usize)).style(style),
                rect,
            );
        }
    }

    fn secret_input(&self) -> bool {
        matches!(
            self.kind,
            ModalKind::Prompt(PromptAction::UnlockSecrets | PromptAction::UnlockConnectionSecrets)
        )
    }
    fn input_view(&self, width: usize) -> (String, usize, usize) {
        let available = width.saturating_sub(2).max(1);
        if self.secret_input() {
            let boundaries = grapheme_boundaries(&self.input);
            let total = boundaries.len().saturating_sub(1);
            let cursor = boundaries
                .iter()
                .take_while(|n| **n < self.cursor)
                .count()
                .min(total);
            let start = cursor.saturating_sub(available.saturating_sub(1));
            return (
                "•".repeat(total.saturating_sub(start).min(available)),
                cursor - start,
                boundaries.get(start).copied().unwrap_or(0),
            );
        }
        let prefix: String = self.input.chars().take(self.cursor).collect();
        let mut start = 0;
        for boundary in grapheme_boundaries(&prefix) {
            if display_width(&prefix.chars().skip(boundary).collect::<String>()) < available {
                start = boundary;
                break;
            }
        }
        let visible = self.input.chars().skip(start).collect::<String>();
        let cursor = display_width(&prefix.chars().skip(start).collect::<String>());
        (
            truncate_to_width(&visible, available).to_owned(),
            cursor,
            start,
        )
    }

    fn render_input(&self, frame: &mut Frame, area: Rect, palette: Palette) {
        self.render_input_prefix(frame, area, palette, "> ");
    }
    fn render_input_prefix(&self, frame: &mut Frame, area: Rect, palette: Palette, prefix: &str) {
        if area.is_empty() {
            return;
        }
        let (text, cursor, _) = self.input_view(area.width as usize);
        let style = Style::default().fg(palette.input_fg).bg(palette.input_bg);
        frame.render_widget(Paragraph::new(format!("{prefix}{text}")).style(style), area);
        if !self.actions_focused {
            frame.set_cursor_position((
                area.x
                    .saturating_add(
                        saturating_u16(display_width(prefix))
                            .saturating_add(saturating_u16(cursor)),
                    )
                    .min(area.right().saturating_sub(1)),
                area.y,
            ));
        }
    }

    #[must_use]
    pub fn list_bounds(&self, area: Rect) -> Option<Rect> {
        if self.is_searchable() || self.kind == ModalKind::SaveAs {
            Some(self.content_layout(Block::bordered().inner(self.geometry(area)))[2])
        } else {
            None
        }
    }

    /// The same action and field geometry is used for rendering and clicking.
    pub fn click_key(&mut self, area: Rect, x: u16, y: u16) -> Option<KeyEvent> {
        self.pending_submit = false;
        for (rect, index, _) in self.action_bounds(area) {
            if rect.contains((x, y).into()) {
                if !self.action_enabled(index) {
                    return None;
                }
                self.actions_focused = true;
                self.action_index = index;
                return Some(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
            }
        }
        let popup = self.geometry(area);
        let input = self.content_layout(Block::bordered().inner(popup))[0];
        if !input.is_empty() && input.contains((x, y).into()) {
            self.actions_focused = false;
            let (_, _, start) = self.input_view(input.width as usize);
            let target = x.saturating_sub(input.x + 2) as usize;
            let value: String = self.input.chars().skip(start).collect();
            if self.secret_input() {
                let boundaries = grapheme_boundaries(&value);
                self.cursor = start
                    + boundaries
                        .get(target)
                        .copied()
                        .unwrap_or(value.chars().count());
                return None;
            }
            self.cursor = start;
            for boundary in grapheme_boundaries(&value) {
                let prefix: String = value.chars().take(boundary).collect();
                if display_width(&prefix) > target {
                    break;
                }
                self.cursor = start + boundary;
            }
            return None;
        }
        if self.handle_mouse(area, x, y) {
            self.actions_focused = false;
            return Some(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        }
        if !popup.contains((x, y).into())
            && matches!(self.kind, ModalKind::Completion | ModalKind::Info)
        {
            return Some(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
        }
        None
    }

    pub fn handle_mouse(&mut self, area: Rect, x: u16, y: u16) -> bool {
        let Some(list) = self.list_bounds(area) else {
            return false;
        };
        if x < list.x
            || x >= list.x + list.width
            || y < list.y
            || y >= list.y + list.height
            || self.filtered.is_empty()
        {
            return false;
        }
        let visible = list.height as usize;
        let start = self
            .selected
            .saturating_sub(visible.saturating_sub(1))
            .min(self.filtered.len().saturating_sub(1));
        let index = start + (y - list.y) as usize;
        if index < self.filtered.len() {
            self.selected = index;
            true
        } else {
            false
        }
    }

    pub fn render(&self, frame: &mut Frame, area: Rect, palette: Palette) {
        let popup = self.geometry(area);
        frame.render_widget(Clear, popup);
        let block = Block::bordered()
            .title(format!(" {} ", self.title))
            .border_style(Style::default().fg(palette.modal_border))
            .style(Style::default().bg(palette.modal_bg));
        let inner = block.inner(popup);
        frame.render_widget(block, popup);

        if self.kind == ModalKind::Completion {
            let visible = inner.height as usize;
            let start = self.selected.saturating_sub(visible.saturating_sub(1));
            for (row, index) in self.filtered.iter().skip(start).take(visible).enumerate() {
                let Some(item) = self.items.get(*index) else {
                    continue;
                };
                let selected = start + row == self.selected;
                let style = if selected {
                    Style::default()
                        .fg(palette.selection_fg)
                        .bg(palette.selection_bg)
                } else {
                    Style::default().fg(palette.modal_fg).bg(palette.modal_bg)
                };
                let text = format!(" {}  {}", item.label, item.detail);
                frame.render_widget(
                    Paragraph::new(truncate_to_width(&text, inner.width as usize)).style(style),
                    Rect::new(inner.x, inner.y + saturating_u16(row), inner.width, 1),
                );
            }
        } else if self.kind == ModalKind::Info && self.anchor.is_some() {
            let text = wrap_message(
                self.message.as_deref().unwrap_or_default(),
                inner.width as usize,
            )
            .into_iter()
            .skip(self.message_scroll)
            .collect::<Vec<_>>()
            .join("\n");
            frame.render_widget(
                Paragraph::new(text)
                    .style(Style::default().fg(palette.modal_fg).bg(palette.modal_bg)),
                inner,
            );
        } else if self.is_searchable() || self.kind == ModalKind::SaveAs {
            self.render_searchable(frame, inner, palette);
        } else if self.is_prompt() {
            self.render_prompt(frame, inner, palette);
        } else {
            self.render_confirm(frame, inner, palette);
        }
    }

    fn render_searchable(&self, frame: &mut Frame, inner: Rect, palette: Palette) {
        let [
            input_area,
            separator_area,
            list_area,
            actions_area,
            footer_area,
        ] = self.content_layout(inner);

        self.render_input(frame, input_area, palette);
        frame.render_widget(
            Paragraph::new("─".repeat(inner.width as usize))
                .style(Style::default().fg(palette.modal_border)),
            separator_area,
        );

        if self.filtered.is_empty() {
            let message = if self.input.is_empty() {
                "No items available"
            } else {
                "No matching items"
            };
            frame.render_widget(
                Paragraph::new(format!(" {message}"))
                    .style(Style::default().fg(palette.description)),
                list_area,
            );
        } else {
            let visible = list_area.height as usize;
            let start = self
                .selected
                .saturating_sub(visible.saturating_sub(1))
                .min(self.filtered.len().saturating_sub(1));
            let items = self
                .filtered
                .iter()
                .skip(start)
                .take(visible)
                .enumerate()
                .filter_map(|(offset, index)| {
                    let item = self.items.get(*index)?;
                    let selected = !self.actions_focused && start + offset == self.selected;
                    let background = if selected {
                        palette.selection_bg
                    } else {
                        palette.modal_bg
                    };
                    let foreground = if selected {
                        palette.selection_fg
                    } else {
                        palette.modal_fg
                    };
                    let mut label = String::new();
                    if item.current {
                        label.push_str("● ");
                    }
                    label.push_str(&item.label);
                    let label = format!(" {label}");
                    let detail = item.detail.clone();
                    let used = display_width(&label) + display_width(&detail);
                    let width = list_area.width as usize;
                    let padding = width.saturating_sub(used);
                    Some(ListItem::new(Line::from(vec![
                        Span::styled(
                            label,
                            Style::default().fg(foreground).bg(background).add_modifier(
                                if selected {
                                    Modifier::BOLD
                                } else {
                                    Modifier::empty()
                                },
                            ),
                        ),
                        Span::styled(" ".repeat(padding), Style::default().bg(background)),
                        Span::styled(
                            detail,
                            Style::default().fg(palette.description).bg(background),
                        ),
                    ])))
                })
                .collect::<Vec<_>>();
            frame.render_widget(List::new(items), list_area);
        }

        let count = if self.input.is_empty() {
            format!("{} items", self.items.len())
        } else {
            format!("{}/{}", self.filtered.len(), self.items.len())
        };
        let hints = self
            .hints
            .clone()
            .unwrap_or_else(|| "Enter open   Esc cancel".to_owned());
        self.render_footer(frame, footer_area, &hints, &count, palette);

        self.render_actions(frame, actions_area, palette);
    }

    fn find_fields(&self, area: Rect) -> Vec<(bool, Rect, Rect)> {
        let inner = Block::bordered().inner(self.geometry(area));
        let input = self.content_layout(inner)[0];
        let label = 9.min(input.width / 2);
        (0..if self.replace_mode { 2 } else { 1 })
            .filter(|row| *row < input.height)
            .map(|row| {
                (
                    row == 1,
                    Rect::new(input.x, input.y + row, label, 1),
                    Rect::new(input.x + label, input.y + row, input.width - label, 1),
                )
            })
            .collect()
    }

    fn render_prompt(&self, frame: &mut Frame, inner: Rect, palette: Palette) {
        let [input, _, body, actions, help] = self.content_layout(inner);
        if self.kind == ModalKind::Find {
            // Derive input rows from the same inner layout used by find_fields.
            let label_width = 9.min(input.width / 2);
            for row in 0..if self.replace_mode { 2 } else { 1 } {
                if row >= input.height {
                    break;
                }
                let label = Rect::new(input.x, input.y + row, label_width, 1);
                let field = Rect::new(
                    input.x + label_width,
                    input.y + row,
                    input.width - label_width,
                    1,
                );
                frame.render_widget(
                    Paragraph::new(if row == 0 { "Find:" } else { "Replace:" })
                        .style(Style::default().fg(palette.modal_fg)),
                    label,
                );
                let mut state = ModalBase::prompt(
                    ModalKind::Prompt(PromptAction::Find),
                    "",
                    if row == 0 {
                        &self.input
                    } else {
                        &self.replacement
                    },
                );
                state.cursor = if row == 0 {
                    self.cursor
                } else {
                    self.replacement_cursor
                };
                state.actions_focused =
                    self.actions_focused || self.replacement_focused != (row == 1);
                let mut field_palette = palette;
                if if row == 0 {
                    self.field_selected
                } else {
                    self.replacement_selected
                } {
                    field_palette.input_bg = palette.selection_bg;
                    field_palette.input_fg = palette.selection_fg;
                }
                state.render_input_prefix(frame, field, field_palette, "");
            }
        } else {
            self.render_input(frame, input, palette);
        }
        if let Some(message) = &self.message {
            let lines = wrap_message(message, body.width as usize);
            frame.render_widget(
                Paragraph::new(
                    lines
                        .into_iter()
                        .skip(self.message_scroll)
                        .map(Line::raw)
                        .collect::<Vec<_>>(),
                )
                .style(Style::default().fg(if self.kind == ModalKind::Find {
                    palette.error
                } else {
                    palette.description
                })),
                body,
            );
        }
        self.render_actions(frame, actions, palette);
        self.render_footer(frame, help, "Tab focus · Enter · Esc", "", palette);
    }

    fn render_confirm(&self, frame: &mut Frame, inner: Rect, palette: Palette) {
        let [_, _, body, actions, help] = self.content_layout(inner);
        let lines = wrap_message(
            self.message.as_deref().unwrap_or_default(),
            body.width as usize,
        );
        let scroll = self
            .message_scroll
            .min(lines.len().saturating_sub(body.height as usize));
        frame.render_widget(
            Paragraph::new(
                lines
                    .into_iter()
                    .skip(scroll)
                    .map(Line::raw)
                    .collect::<Vec<_>>(),
            )
            .style(Style::default().fg(palette.modal_fg)),
            body,
        );
        self.render_actions(frame, actions, palette);
        self.render_footer(
            frame,
            help,
            "Tab/Arrows · Enter · Esc · PgDn scroll",
            "",
            palette,
        );
    }

    fn render_footer(
        &self,
        frame: &mut Frame,
        area: Rect,
        hints: &str,
        count: &str,
        palette: Palette,
    ) {
        let hint_width = display_width(hints);
        let count_width = display_width(count);
        let gap = (area.width as usize)
            .saturating_sub(hint_width)
            .saturating_sub(count_width);
        let line = Line::from(vec![
            Span::styled(hints.to_owned(), Style::default().fg(palette.description)),
            Span::styled(" ".repeat(gap), Style::default().bg(palette.modal_bg)),
            Span::styled(count.to_owned(), Style::default().fg(palette.description)),
        ]);
        frame.render_widget(
            Paragraph::new(line).style(Style::default().bg(palette.modal_bg)),
            area,
        );
    }
}

/// What a key did inside a modal, before the modal turns a submit into its typed result.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum KeyOutcome {
    None,
    Close,
    Submit,
    SaveBeforeConfirm,
    FindNext,
    SearchAction(usize),
}

/// What a key did to a modal. A submit carries the modal's typed result.
#[derive(Debug)]
pub enum ModalOutcome {
    None,
    Close,
    Submit(Submitted),
    SaveBeforeConfirm,
    FindNext,
    SearchAction(usize),
}

/// The typed result of submitting a modal.
#[derive(Debug)]
pub enum Submitted {
    /// Nothing usable was selected; the modal just closes.
    Nothing,
    Command(Command),
    OpenFile(PathBuf),
    SearchResult {
        path: String,
        line: Option<usize>,
    },
    Theme(String),
    GitChoice {
        action: String,
        value: String,
    },
    Branch(String),
    AgentWorkspace(String),
    AgentSession {
        provider: CliProvider,
        session: String,
    },
    AiCli(CliProvider),
    RowInsert(Box<tide_db::RowInsert>),
    Connection(Box<ConnectionSubmission>),
    DatabaseConnection(String),
    CatalogItem(Box<tide_db::CatalogItem>),
    HistoryEntry(String),
    HistoryControl {
        control: HistoryControl,
        filter: String,
    },
    Session(String),
    CodeAction(usize),
    Setting(String),
    Keybinding(String),
    LspCompletion(Box<tide_lsp::CompletionItem>),
    CompletionText(String),
    SaveAs(PathBuf),
    Prompt(PromptAction, String),
    /// `save` writes the edited row at once instead of only staging the cell.
    RowCell {
        query: String,
        column: String,
        input: String,
        save: bool,
    },
    Find(String),
    Confirm(ConfirmAction),
    ActivateTab {
        pane: usize,
        index: usize,
    },
    MoveTab(usize),
    StatusEntry(String),
}

/// A query history control, applied before the history list reopens with the same filter.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HistoryControl {
    Favorite(Option<String>),
    Favorites,
    Connection,
    Clear,
}

impl Submitted {
    /// The result of a modal whose state lives entirely in its base.
    fn from_base(base: &ModalBase) -> Self {
        let value = base.selected_item().map(|item| item.value.as_str());
        let owned = || value.map(str::to_owned);
        let submitted = match &base.kind {
            ModalKind::CommandPalette | ModalKind::AddPanel => value
                .and_then(|value| value.parse().ok())
                .map(Self::Command),
            ModalKind::QuickOpen => value.map(|value| Self::OpenFile(PathBuf::from(value))),
            ModalKind::SearchResults => value.and_then(|value| {
                let (path, line) = value.rsplit_once('\t')?;
                Some(Self::SearchResult {
                    path: path.to_owned(),
                    line: line.parse().ok(),
                })
            }),
            ModalKind::ThemePicker => owned().map(Self::Theme),
            ModalKind::GitChoice(action) => owned().map(|value| Self::GitChoice {
                action: action.clone(),
                value,
            }),
            ModalKind::BranchPicker => owned().map(Self::Branch),
            ModalKind::AgentWorkspaces => owned().map(Self::AgentWorkspace),
            ModalKind::AgentSessions => value.and_then(|value| {
                let (provider, session) = value.split_once('\t')?;
                Some(Self::AgentSession {
                    provider: CliProvider::parse(provider)?,
                    session: session.to_owned(),
                })
            }),
            ModalKind::AiCliPicker => value.and_then(CliProvider::parse).map(Self::AiCli),
            ModalKind::ConnectionPicker => owned().map(Self::DatabaseConnection),
            ModalKind::QueryHistory => owned().map(Self::HistoryEntry),
            ModalKind::SessionPicker => owned().map(Self::Session),
            ModalKind::CodeActions => value
                .and_then(|value| value.parse().ok())
                .map(Self::CodeAction),
            ModalKind::Settings => owned().map(Self::Setting),
            ModalKind::Keybindings => owned().map(Self::Keybinding),
            ModalKind::Completion => owned().map(Self::CompletionText),
            ModalKind::Prompt(PromptAction::EditRowCell(query, column)) => Some(Self::RowCell {
                query: query.clone(),
                column: column.clone(),
                input: base.input.clone(),
                save: false,
            }),
            ModalKind::Prompt(action) => Some(Self::Prompt(action.clone(), base.input.clone())),
            ModalKind::Find => Some(Self::Find(base.input.clone())),
            ModalKind::Confirm(action) => Some(Self::Confirm(action.clone())),
            ModalKind::AllTabs => value.and_then(|value| {
                let (pane, index) = value.split_once(':')?;
                Some(Self::ActivateTab {
                    pane: pane.parse().ok()?,
                    index: index.parse().ok()?,
                })
            }),
            ModalKind::Tabs(pane) => value
                .and_then(|value| value.parse().ok())
                .map(|index| Self::ActivateTab { pane: *pane, index }),
            ModalKind::MoveTab => value
                .and_then(|value| value.parse().ok())
                .map(Self::MoveTab),
            ModalKind::StatusHistory => base
                .selected_item()
                .map(|item| Self::StatusEntry(item.detail.clone())),
            ModalKind::RowInsert
            | ModalKind::ConnectionForm
            | ModalKind::SchemaTables
            | ModalKind::SaveAs
            | ModalKind::Info => None,
        };
        submitted.unwrap_or(Self::Nothing)
    }
}

#[cfg(test)]
impl ModalOutcome {
    /// The outcome without its payload, for assertions.
    pub(crate) fn key(&self) -> KeyOutcome {
        match self {
            Self::None => KeyOutcome::None,
            Self::Close => KeyOutcome::Close,
            Self::Submit(_) => KeyOutcome::Submit,
            Self::SaveBeforeConfirm => KeyOutcome::SaveBeforeConfirm,
            Self::FindNext => KeyOutcome::FindNext,
            Self::SearchAction(index) => KeyOutcome::SearchAction(*index),
        }
    }
}

/// Turns a key outcome into a modal outcome, asking the modal for its typed result on submit.
pub(crate) fn resolve<M: Modal + ?Sized>(modal: &M, outcome: KeyOutcome) -> ModalOutcome {
    match outcome {
        KeyOutcome::None => ModalOutcome::None,
        KeyOutcome::Close => ModalOutcome::Close,
        KeyOutcome::Submit => ModalOutcome::Submit(modal.submission()),
        KeyOutcome::SaveBeforeConfirm => ModalOutcome::SaveBeforeConfirm,
        KeyOutcome::FindNext => ModalOutcome::FindNext,
        KeyOutcome::SearchAction(index) => ModalOutcome::SearchAction(index),
    }
}

pub trait Modal {
    fn base(&self) -> &ModalBase;
    fn base_mut(&mut self) -> &mut ModalBase;

    fn kind(&self) -> &ModalKind {
        &self.base().kind
    }

    fn input(&self) -> &str {
        &self.base().input
    }

    fn selected_item(&self) -> Option<&ModalItem> {
        self.base().selected_item()
    }

    fn handle_key(&mut self, key: KeyEvent) -> ModalOutcome {
        let outcome = default_key(self.base_mut(), key);
        resolve(self, outcome)
    }

    /// The typed result handed to the app when this modal submits.
    fn submission(&self) -> Submitted {
        Submitted::from_base(self.base())
    }

    /// Completion lists and anchored popups give way to any other input.
    fn is_popup(&self) -> bool {
        *self.kind() == ModalKind::Completion || self.base().anchor.is_some()
    }

    /// The insert request this modal has submitted and is waiting on.
    fn submitted_insert(&self) -> Option<&tide_db::RowInsert> {
        None
    }

    fn paste(&mut self, text: &str) {
        self.base_mut().push_str(text);
    }

    fn handle_mouse(&mut self, area: Rect, x: u16, y: u16) -> bool {
        self.base_mut().handle_mouse(area, x, y)
    }

    fn click_key(&mut self, area: Rect, x: u16, y: u16) -> Option<KeyEvent> {
        self.base_mut().click_key(area, x, y)
    }

    fn render(&self, frame: &mut Frame, area: Rect, palette: Palette) {
        self.base().render(frame, area, palette);
    }
}

fn char_byte(text: &str, position: usize) -> usize {
    text.char_indices()
        .nth(position)
        .map_or(text.len(), |(byte, _)| byte)
}

fn wrap_message(text: &str, width: usize) -> Vec<String> {
    let width = width.max(1);
    let mut lines = Vec::new();
    for paragraph in text.split('\n') {
        let mut current = String::new();
        for word in paragraph.split_whitespace() {
            if !current.is_empty() && display_width(&current) + 1 + display_width(word) > width {
                lines.push(std::mem::take(&mut current));
            }
            if !current.is_empty() {
                current.push(' ');
            }
            for window in grapheme_boundaries(word).windows(2) {
                let part: String = word
                    .chars()
                    .skip(window[0])
                    .take(window[1] - window[0])
                    .collect();
                if !current.is_empty() && display_width(&current) + display_width(&part) > width {
                    lines.push(std::mem::take(&mut current));
                }
                current.push_str(&part);
            }
        }
        lines.push(current);
    }
    lines
}

fn action_key(base: &mut ModalBase, key: KeyEvent) -> Option<KeyOutcome> {
    if key.code == KeyCode::Esc {
        return Some(KeyOutcome::Close);
    }
    let confirm = matches!(base.kind, ModalKind::Confirm(_));
    let info = base.kind == ModalKind::Info;
    let count = if base.kind == ModalKind::Find {
        9
    } else if info {
        1
    } else if base.offers_save() {
        3
    } else {
        2
    };
    if key.code == KeyCode::Tab || key.code == KeyCode::BackTab {
        if confirm || info {
            base.action_index = (base.action_index
                + if key.code == KeyCode::BackTab {
                    count - 1
                } else {
                    1
                })
                % count;
        } else {
            let zone = if base.actions_focused {
                base.action_index + 1
            } else {
                0
            };
            let next = if key.code == KeyCode::BackTab {
                (zone + count) % (count + 1)
            } else {
                (zone + 1) % (count + 1)
            };
            base.actions_focused = next != 0;
            base.action_index = next.saturating_sub(1);
        }
        return Some(KeyOutcome::None);
    }
    if confirm || info {
        if matches!(key.code, KeyCode::PageDown | KeyCode::PageUp) {
            base.message_scroll = if key.code == KeyCode::PageUp {
                base.message_scroll.saturating_sub(5)
            } else {
                base.message_scroll.saturating_add(5)
            };
            return Some(KeyOutcome::None);
        }
        if let KeyCode::Char(ch) = key.code
            && key.modifiers.difference(KeyModifiers::SHIFT).is_empty()
        {
            if base.offers_save() {
                match ch {
                    'y' | 'Y' | 's' | 'S' => return Some(KeyOutcome::SaveBeforeConfirm),
                    'n' | 'N' => return Some(KeyOutcome::Submit),
                    _ => {}
                }
            }
            if confirm && matches!(ch, 'y' | 'Y') {
                return Some(KeyOutcome::Submit);
            }
            if matches!(ch, 'n' | 'N') {
                return Some(KeyOutcome::Close);
            }
        }
    }
    if base.actions_focused {
        if matches!(
            key.code,
            KeyCode::Left | KeyCode::Right | KeyCode::Up | KeyCode::Down
        ) {
            base.action_index = (base.action_index
                + if matches!(key.code, KeyCode::Left | KeyCode::Up) {
                    count - 1
                } else {
                    1
                })
                % count;
            return Some(KeyOutcome::None);
        }
        if matches!(key.code, KeyCode::Enter | KeyCode::Char(' ')) {
            if base.kind == ModalKind::Find {
                return Some(if base.action_enabled(base.action_index) {
                    KeyOutcome::SearchAction(base.action_index)
                } else {
                    KeyOutcome::None
                });
            }
            if info || base.action_index == count - 1 {
                return Some(KeyOutcome::Close);
            }
            if base.offers_save() && base.action_index == 0 {
                return Some(KeyOutcome::SaveBeforeConfirm);
            }
            if base.is_searchable() && base.filtered.is_empty() {
                return Some(KeyOutcome::None);
            }
            return Some(if base.kind == ModalKind::Find {
                KeyOutcome::FindNext
            } else {
                KeyOutcome::Submit
            });
        }
    }
    None
}

pub(crate) fn default_key(base: &mut ModalBase, key: KeyEvent) -> KeyOutcome {
    if base.kind == ModalKind::Completion && key.code == KeyCode::Tab && key.modifiers.is_empty() {
        return KeyOutcome::Submit;
    }
    if let Some(outcome) = action_key(base, key) {
        return outcome;
    }
    if matches!(key.code, KeyCode::Home | KeyCode::End) && !base.actions_focused {
        base.cursor = if key.code == KeyCode::Home {
            0
        } else {
            base.input.chars().count()
        };
        return KeyOutcome::None;
    }
    match key.code {
        KeyCode::Esc => return KeyOutcome::Close,
        KeyCode::Enter => {
            return if matches!(base.kind, ModalKind::Find) {
                KeyOutcome::FindNext
            } else {
                KeyOutcome::Submit
            };
        }
        KeyCode::Up => base.move_up(),
        KeyCode::Down => base.move_down(),
        KeyCode::PageUp => {
            for _ in 0..10 {
                base.move_up();
            }
        }
        KeyCode::PageDown => {
            for _ in 0..10 {
                base.move_down();
            }
        }
        KeyCode::Home => base.select_first(),
        KeyCode::End => base.select_last(),
        KeyCode::Left => base.move_left(),
        KeyCode::Right => base.move_right(),
        KeyCode::Backspace => base.backspace(),
        KeyCode::Delete => base.delete(),
        KeyCode::Char(ch)
            if !key.modifiers.intersects(
                KeyModifiers::CONTROL
                    | KeyModifiers::ALT
                    | KeyModifiers::SUPER
                    | KeyModifiers::META,
            ) =>
        {
            if let ModalKind::Confirm(_) = base.kind {
                return match ch {
                    'y' | 'Y' => KeyOutcome::Submit,
                    'n' | 'N' => KeyOutcome::Close,
                    _ => KeyOutcome::None,
                };
            }
            base.push_char(ch);
        }
        _ => {}
    }
    KeyOutcome::None
}

macro_rules! list_modal {
    ($name:ident, $kind:expr, $title:expr) => {
        pub struct $name {
            base: ModalBase,
        }

        impl $name {
            #[must_use]
            pub fn new(items: Vec<ModalItem>) -> Self {
                Self {
                    base: ModalBase::new($kind, $title, items),
                }
            }
        }

        impl Modal for $name {
            fn base(&self) -> &ModalBase {
                &self.base
            }

            fn base_mut(&mut self) -> &mut ModalBase {
                &mut self.base
            }
        }
    };
}

list_modal!(
    CommandPaletteModal,
    ModalKind::CommandPalette,
    "Command Palette"
);
pub struct QuickOpenModal {
    base: ModalBase,
}

impl QuickOpenModal {
    #[must_use]
    pub fn new(items: Vec<ModalItem>) -> Self {
        Self {
            base: ModalBase::new(ModalKind::QuickOpen, "Quick Open", items),
        }
    }
}

impl Modal for QuickOpenModal {
    fn base(&self) -> &ModalBase {
        &self.base
    }

    fn base_mut(&mut self) -> &mut ModalBase {
        &mut self.base
    }

    /// Enter while files are still loading submits once they arrive.
    fn handle_key(&mut self, key: KeyEvent) -> ModalOutcome {
        if self.base.loading {
            if key.code == KeyCode::Enter && !self.base.actions_focused {
                self.base.pending_submit = true;
                return ModalOutcome::None;
            }
            self.base.pending_submit = false;
        }
        let outcome = default_key(&mut self.base, key);
        resolve(self, outcome)
    }
}
list_modal!(
    SearchResultsModal,
    ModalKind::SearchResults,
    "Search Results"
);
list_modal!(
    ThemePickerModal,
    ModalKind::ThemePicker,
    "Select Color Theme"
);
list_modal!(BranchPickerModal, ModalKind::BranchPicker, "Switch Branch");
list_modal!(SettingsModal, ModalKind::Settings, "Settings");
list_modal!(
    KeybindingsModal,
    ModalKind::Keybindings,
    "Keyboard Shortcuts"
);
pub struct CompletionModal {
    base: ModalBase,
    lsp: Vec<tide_lsp::CompletionItem>,
}

impl CompletionModal {
    #[must_use]
    pub fn new(items: Vec<ModalItem>) -> Self {
        Self {
            base: ModalBase::new(ModalKind::Completion, "Completions", items),
            lsp: Vec::new(),
        }
    }
}

impl Modal for CompletionModal {
    fn base(&self) -> &ModalBase {
        &self.base
    }

    fn base_mut(&mut self) -> &mut ModalBase {
        &mut self.base
    }

    fn submission(&self) -> Submitted {
        let index = self.base.filtered.get(self.base.selected);
        match index.and_then(|index| self.lsp.get(*index)) {
            Some(item) => Submitted::LspCompletion(Box::new(item.clone())),
            None => Submitted::from_base(&self.base),
        }
    }
}
list_modal!(
    AgentSessionsModal,
    ModalKind::AgentSessions,
    "Agent Sessions"
);
list_modal!(
    AgentWorkspacesModal,
    ModalKind::AgentWorkspaces,
    "Associate Previous Workspace"
);
pub fn agent_workspaces(items: Vec<ModalItem>) -> Box<dyn Modal> {
    Box::new(AgentWorkspacesModal::new(items))
}

list_modal!(
    ConnectionPickerModal,
    ModalKind::ConnectionPicker,
    "Database Connections"
);
pub struct QueryHistoryModal {
    base: ModalBase,
    last_click: Option<(usize, std::time::Instant)>,
}
impl QueryHistoryModal {
    pub fn new(items: Vec<ModalItem>) -> Self {
        Self {
            base: ModalBase::new(ModalKind::QueryHistory, "Query History", items),
            last_click: None,
        }
    }
    fn controls(&self, bounds: Rect) -> Vec<(Rect, char, String)> {
        let area = self
            .base
            .content_layout(Block::bordered().inner(self.base.geometry(bounds)))[4];
        let mut x = area.x;
        let mut controls = Vec::new();
        for (key, label) in [
            ('f', "Favorite [F]"),
            ('o', "Favorites [O]"),
            ('c', "Connection [C]"),
            ('d', "Clear [D]"),
        ] {
            let width = saturating_u16(display_width(label)).min(area.right().saturating_sub(x));
            if width > 0 && area.height > 0 {
                controls.push((Rect::new(x, area.y, width, 1), key, label.into()));
                x += width + 1;
            }
        }
        controls
    }
}
impl Modal for QueryHistoryModal {
    fn base(&self) -> &ModalBase {
        &self.base
    }
    fn base_mut(&mut self) -> &mut ModalBase {
        &mut self.base
    }
    fn handle_key(&mut self, key: KeyEvent) -> ModalOutcome {
        if key.modifiers.contains(KeyModifiers::ALT)
            && let KeyCode::Char(control) = key.code
        {
            let control = match control {
                'f' => HistoryControl::Favorite(self.selected_item().map(|i| i.value.clone())),
                'o' => HistoryControl::Favorites,
                'c' => HistoryControl::Connection,
                'd' => HistoryControl::Clear,
                _ => return ModalOutcome::None,
            };
            return ModalOutcome::Submit(Submitted::HistoryControl {
                control,
                filter: self.base.input.clone(),
            });
        }
        let outcome = default_key(&mut self.base, key);
        resolve(self, outcome)
    }
    fn click_key(&mut self, bounds: Rect, x: u16, y: u16) -> Option<KeyEvent> {
        for (rect, key, _) in self.controls(bounds) {
            if rect.contains((x, y).into()) {
                return Some(KeyEvent::new(KeyCode::Char(key), KeyModifiers::ALT));
            }
        }
        if self
            .base
            .list_bounds(bounds)
            .is_some_and(|r| r.contains((x, y).into()))
            && self.base.handle_mouse(bounds, x, y)
        {
            let now = std::time::Instant::now();
            let double = self.last_click.is_some_and(|(row, at)| {
                row == self.base.selected
                    && now.duration_since(at) < std::time::Duration::from_millis(400)
            });
            self.last_click = Some((self.base.selected, now));
            return double.then(|| KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        }
        self.base.click_key(bounds, x, y)
    }
    fn render(&self, frame: &mut Frame, bounds: Rect, palette: Palette) {
        self.base.render(frame, bounds, palette);
        for (rect, _, label) in self.controls(bounds) {
            frame.render_widget(
                Paragraph::new(truncate_to_width(&label, rect.width as usize).to_owned())
                    .style(Style::default().fg(palette.accent).bg(palette.background)),
                rect,
            );
        }
    }
}
list_modal!(SessionPickerModal, ModalKind::SessionPicker, "Open Session");
list_modal!(CodeActionsModal, ModalKind::CodeActions, "Code Actions");
list_modal!(AiCliPickerModal, ModalKind::AiCliPicker, "Open AI CLI");

pub struct FileBrowserModal {
    base: ModalBase,
    directory: std::path::PathBuf,
    row_selected: bool,
}

impl FileBrowserModal {
    #[must_use]
    pub fn new(initial: &str) -> Self {
        let path = std::path::PathBuf::from(initial);
        let (directory, filename) = if path.is_dir() {
            (path, String::new())
        } else {
            (
                path.parent().map_or_else(
                    || std::path::PathBuf::from("."),
                    std::path::Path::to_path_buf,
                ),
                path.file_name()
                    .and_then(|name| name.to_str())
                    .unwrap_or_default()
                    .to_owned(),
            )
        };
        let mut base = ModalBase::new(ModalKind::SaveAs, "Save As", Vec::new());
        base.input = filename;
        base.cursor = base.input.chars().count();
        base.hints = Some("Enter save   ← parent   Esc cancel".to_owned());
        let mut modal = Self {
            base,
            directory,
            row_selected: false,
        };
        modal.reload();
        modal
    }

    fn reload(&mut self) {
        let mut items = Vec::new();
        if let Some(parent) = self.directory.parent() {
            items.push(
                ModalItem::new("..", "parent directory", parent.display().to_string())
                    .current(false),
            );
        }
        let Ok(read_dir) = std::fs::read_dir(&self.directory) else {
            self.base.items = items;
            self.base.filtered = (0..self.base.items.len()).collect();
            return;
        };
        let mut entries = read_dir.flatten().collect::<Vec<_>>();
        entries.sort_by(|left, right| {
            let left_dir = left.file_type().map(|kind| kind.is_dir()).unwrap_or(false);
            let right_dir = right.file_type().map(|kind| kind.is_dir()).unwrap_or(false);
            right_dir.cmp(&left_dir).then_with(|| {
                left.file_name()
                    .to_string_lossy()
                    .to_lowercase()
                    .cmp(&right.file_name().to_string_lossy().to_lowercase())
            })
        });
        for entry in entries {
            let name = entry.file_name().to_string_lossy().to_string();
            if name.starts_with('.') {
                continue;
            }
            let is_dir = entry.file_type().map(|kind| kind.is_dir()).unwrap_or(false);
            items.push(ModalItem::new(
                if is_dir { format!("{name}/") } else { name },
                if is_dir { "directory" } else { "file" },
                entry.path().display().to_string(),
            ));
        }
        self.base.items = items;
        self.base.filtered = (0..self.base.items.len()).collect();
        self.base.selected = self
            .base
            .selected
            .min(self.base.filtered.len().saturating_sub(1));
    }

    fn navigate(&mut self, path: &std::path::Path) {
        if path.is_dir() {
            self.directory = path.to_path_buf();
            self.base.input.clear();
            self.base.cursor = 0;
            self.base.selected = 0;
            self.reload();
        }
    }

    #[must_use]
    pub fn directory(&self) -> &std::path::Path {
        &self.directory
    }
}

impl Modal for FileBrowserModal {
    fn base(&self) -> &ModalBase {
        &self.base
    }

    fn base_mut(&mut self) -> &mut ModalBase {
        &mut self.base
    }

    fn handle_key(&mut self, key: KeyEvent) -> ModalOutcome {
        let outcome = self.key(key);
        resolve(self, outcome)
    }

    fn submission(&self) -> Submitted {
        self.target_path()
            .map_or(Submitted::Nothing, Submitted::SaveAs)
    }

    fn click_key(&mut self, area: Rect, x: u16, y: u16) -> Option<KeyEvent> {
        self.row_selected = self
            .base
            .list_bounds(area)
            .is_some_and(|bounds| bounds.contains((x, y).into()));
        self.base.click_key(area, x, y)
    }

    fn render(&self, frame: &mut Frame, area: Rect, palette: Palette) {
        self.base.render(frame, area, palette);
    }
}

impl FileBrowserModal {
    fn key(&mut self, key: KeyEvent) -> KeyOutcome {
        if let Some(outcome) = action_key(&mut self.base, key) {
            return outcome;
        }
        match key.code {
            KeyCode::Esc => KeyOutcome::Close,
            KeyCode::Enter => {
                if self.row_selected
                    && let Some(item) = self.base.selected_item()
                {
                    let path = std::path::PathBuf::from(&item.value);
                    if path.is_dir() {
                        self.navigate(&path);
                        return KeyOutcome::None;
                    }
                    if let Some(name) = path.file_name().and_then(|name| name.to_str()) {
                        self.base.input = name.to_owned();
                        self.base.cursor = self.base.input.chars().count();
                    }
                }
                KeyOutcome::Submit
            }
            KeyCode::Up => {
                self.row_selected = true;
                self.base.move_up();
                KeyOutcome::None
            }
            KeyCode::Down => {
                self.row_selected = true;
                self.base.move_down();
                KeyOutcome::None
            }
            KeyCode::Left => {
                if !self.row_selected {
                    self.base.move_left();
                } else if let Some(parent) =
                    self.directory.parent().map(std::path::Path::to_path_buf)
                {
                    self.navigate(&parent);
                }
                KeyOutcome::None
            }
            KeyCode::Right => {
                if !self.row_selected {
                    self.base.move_right();
                } else if let Some(item) = self.base.selected_item() {
                    let path = std::path::PathBuf::from(&item.value);
                    self.navigate(&path);
                }
                KeyOutcome::None
            }
            KeyCode::Backspace => {
                self.row_selected = false;
                self.base.backspace();
                KeyOutcome::None
            }
            KeyCode::Delete => {
                self.row_selected = false;
                self.base.delete();
                KeyOutcome::None
            }
            KeyCode::Char(ch)
                if !key.modifiers.intersects(
                    KeyModifiers::CONTROL
                        | KeyModifiers::ALT
                        | KeyModifiers::SUPER
                        | KeyModifiers::META,
                ) =>
            {
                self.row_selected = false;
                self.base.push_char(ch);
                KeyOutcome::None
            }
            _ => default_key(&mut self.base, key),
        }
    }

    #[must_use]
    pub fn target_path(&self) -> Option<PathBuf> {
        let name = self.base.input.trim();
        if name.is_empty() {
            None
        } else {
            Some(self.directory.join(name))
        }
    }
}

pub struct PromptModal {
    base: ModalBase,
}

impl PromptModal {
    #[must_use]
    pub fn new(action: PromptAction, title: &str, initial: &str) -> Self {
        Self {
            base: ModalBase::prompt(ModalKind::Prompt(action), title, initial),
        }
    }
}

impl Modal for PromptModal {
    fn base(&self) -> &ModalBase {
        &self.base
    }

    fn base_mut(&mut self) -> &mut ModalBase {
        &mut self.base
    }

    /// Ctrl+S or Ctrl+Enter in a row cell editor saves the row immediately.
    fn handle_key(&mut self, key: KeyEvent) -> ModalOutcome {
        if let ModalKind::Prompt(PromptAction::EditRowCell(query, column)) = &self.base.kind
            && matches!(key.code, KeyCode::Char('s') | KeyCode::Enter)
            && key.modifiers.contains(KeyModifiers::CONTROL)
        {
            return ModalOutcome::Submit(Submitted::RowCell {
                query: query.clone(),
                column: column.clone(),
                input: self.base.input.clone(),
                save: true,
            });
        }
        let outcome = default_key(&mut self.base, key);
        resolve(self, outcome)
    }
}

pub struct FindModal {
    base: ModalBase,
}

impl FindModal {
    #[must_use]
    pub fn new(initial: &str) -> Self {
        let mut base = ModalBase::prompt(ModalKind::Find, "Find", initial);
        base.hints = Some(
            "Enter next · Alt+C case · Alt+W word · Alt+R regex · Alt+S selection · Esc close"
                .to_owned(),
        );
        Self { base }
    }
}

impl FindModal {
    fn clear_field_selection(&mut self) -> bool {
        if self.base.replacement_focused && self.base.replacement_selected {
            self.base.replacement.clear();
            self.base.replacement_cursor = 0;
            self.base.replacement_selected = false;
            true
        } else if !self.base.replacement_focused && self.base.field_selected {
            self.base.input.clear();
            self.base.cursor = 0;
            self.base.field_selected = false;
            true
        } else {
            false
        }
    }
}

impl Modal for FindModal {
    fn base(&self) -> &ModalBase {
        &self.base
    }

    fn base_mut(&mut self) -> &mut ModalBase {
        &mut self.base
    }
    fn paste(&mut self, text: &str) {
        if self.base.actions_focused {
            return;
        }
        self.clear_field_selection();
        if self.base.replacement_focused {
            let mut field = ModalBase::prompt(
                ModalKind::Prompt(PromptAction::Find),
                "",
                &self.base.replacement,
            );
            field.cursor = self.base.replacement_cursor;
            field.push_str(text);
            self.base.replacement = field.input;
            self.base.replacement_cursor = field.cursor;
        } else {
            self.base.push_str(text);
        }
    }
    fn handle_key(&mut self, key: KeyEvent) -> ModalOutcome {
        let outcome = self.key(key);
        resolve(self, outcome)
    }
    fn click_key(&mut self, area: Rect, x: u16, y: u16) -> Option<KeyEvent> {
        self.find_click(area, x, y)
    }
}

impl FindModal {
    fn key(&mut self, key: KeyEvent) -> KeyOutcome {
        if !self.base.actions_focused
            && matches!(
                key.code,
                KeyCode::Backspace | KeyCode::Delete | KeyCode::Char(_)
            )
            && key.modifiers.difference(KeyModifiers::SHIFT).is_empty()
        {
            let cleared = self.clear_field_selection();
            if cleared && matches!(key.code, KeyCode::Backspace | KeyCode::Delete) {
                return KeyOutcome::None;
            }
        }
        if !self.base.actions_focused
            && matches!(
                key.code,
                KeyCode::Left | KeyCode::Right | KeyCode::Home | KeyCode::End
            )
        {
            let selected = if self.base.replacement_focused {
                self.base.replacement_selected
            } else {
                self.base.field_selected
            };
            if selected {
                let start = matches!(key.code, KeyCode::Left | KeyCode::Home);
                if self.base.replacement_focused {
                    self.base.replacement_selected = false;
                    self.base.replacement_cursor = if start {
                        0
                    } else {
                        self.base.replacement.chars().count()
                    };
                } else {
                    self.base.field_selected = false;
                    self.base.cursor = if start {
                        0
                    } else {
                        self.base.input.chars().count()
                    };
                }
                return KeyOutcome::None;
            }
        }
        if key.code == KeyCode::Esc {
            return KeyOutcome::Close;
        }
        if matches!(key.code, KeyCode::Tab | KeyCode::BackTab) {
            let fields = if self.base.replace_mode { 2 } else { 1 };
            let total = fields + 9;
            let zone = if self.base.actions_focused {
                fields + self.base.action_index
            } else {
                usize::from(self.base.replacement_focused)
            };
            let next = (zone
                + if key.code == KeyCode::BackTab {
                    total - 1
                } else {
                    1
                })
                % total;
            self.base.actions_focused = next >= fields;
            self.base.replacement_focused = next == 1 && fields == 2;
            self.base.action_index = next.saturating_sub(fields);
            return KeyOutcome::None;
        }
        if !self.base.actions_focused {
            if key.code == KeyCode::Enter {
                return if self.base.replacement_focused {
                    if self.base.action_enabled(6) {
                        KeyOutcome::SearchAction(6)
                    } else {
                        KeyOutcome::None
                    }
                } else if key.modifiers.contains(KeyModifiers::SHIFT) {
                    KeyOutcome::SearchAction(1)
                } else {
                    KeyOutcome::FindNext
                };
            }
            if self.base.replacement_focused {
                let mut field = ModalBase::prompt(
                    ModalKind::Prompt(PromptAction::Find),
                    "",
                    &self.base.replacement,
                );
                field.cursor = self.base.replacement_cursor;
                if key.code == KeyCode::Char('u') && key.modifiers.contains(KeyModifiers::CONTROL) {
                    field.input.clear();
                    field.cursor = 0;
                } else {
                    default_key(&mut field, key);
                }
                self.base.replacement = field.input;
                self.base.replacement_cursor = field.cursor;
                return KeyOutcome::None;
            }
        }
        if key.code == KeyCode::Char('u')
            && key.modifiers.contains(KeyModifiers::CONTROL)
            && !self.base.actions_focused
        {
            self.base.input.clear();
            self.base.cursor = 0;
            return KeyOutcome::None;
        }
        default_key(&mut self.base, key)
    }
    fn find_click(&mut self, area: Rect, x: u16, y: u16) -> Option<KeyEvent> {
        for (replacement, label, rect) in self.base.find_fields(area) {
            if rect.contains((x, y).into()) || label.contains((x, y).into()) {
                self.base.actions_focused = false;
                self.base.replacement_focused = replacement;
                self.base.field_selected = false;
                self.base.replacement_selected = false;
                let (text, cursor) = if replacement {
                    (&self.base.replacement, self.base.replacement_cursor)
                } else {
                    (&self.base.input, self.base.cursor)
                };
                let mut field = ModalBase::prompt(ModalKind::Prompt(PromptAction::Find), "", text);
                field.cursor = cursor;
                let (_, _, start) = field.input_view(rect.width as usize);
                let mut cells = 0;
                let target = x.saturating_sub(rect.x) as usize;
                let position = start
                    + text
                        .chars()
                        .skip(start)
                        .take_while(|c| {
                            cells += display_width(&c.to_string());
                            cells <= target
                        })
                        .count();
                if replacement {
                    self.base.replacement_cursor = position;
                } else {
                    self.base.cursor = position;
                }
                return None;
            }
        }
        self.base.click_key(area, x, y)
    }
}

pub struct ConfirmModal {
    base: ModalBase,
}

impl ConfirmModal {
    #[must_use]
    pub fn new(action: ConfirmAction, title: &str, message: &str) -> Self {
        Self {
            base: ModalBase::confirm(action, title, message),
        }
    }
}

impl Modal for ConfirmModal {
    fn base(&self) -> &ModalBase {
        &self.base
    }

    fn base_mut(&mut self) -> &mut ModalBase {
        &mut self.base
    }
}

pub struct InfoModal {
    base: ModalBase,
}

impl InfoModal {
    #[must_use]
    pub fn new(title: &str, message: &str) -> Self {
        let mut base = ModalBase::new(ModalKind::Info, title, Vec::new());
        base.actions_focused = true;
        base.message = Some(message.to_owned());
        base.hints = Some("Esc close".to_owned());
        Self { base }
    }
}

impl Modal for InfoModal {
    fn base(&self) -> &ModalBase {
        &self.base
    }

    fn base_mut(&mut self) -> &mut ModalBase {
        &mut self.base
    }
}

#[must_use]
pub fn completion(items: Vec<ModalItem>) -> Box<dyn Modal> {
    Box::new(CompletionModal::new(items))
}

#[must_use]
pub fn lsp_completion(items: Vec<tide_lsp::CompletionItem>) -> Box<dyn Modal> {
    let rows = items
        .iter()
        .map(|item| {
            ModalItem::new(
                item.label.clone(),
                item.detail.clone().unwrap_or_default(),
                String::new(),
            )
        })
        .collect();
    let mut modal = CompletionModal::new(rows);
    modal.lsp = items;
    Box::new(modal)
}

#[must_use]
pub fn info(title: &str, message: &str) -> Box<dyn Modal> {
    Box::new(InfoModal::new(title, message))
}

#[must_use]
pub fn command_palette(items: Vec<ModalItem>) -> Box<dyn Modal> {
    Box::new(CommandPaletteModal::new(items))
}

pub fn tabs(pane: usize, items: Vec<ModalItem>) -> Box<dyn Modal> {
    struct TabsModal {
        base: ModalBase,
    }
    impl Modal for TabsModal {
        fn base(&self) -> &ModalBase {
            &self.base
        }
        fn base_mut(&mut self) -> &mut ModalBase {
            &mut self.base
        }
    }
    let selected = items.iter().position(|item| item.current).unwrap_or(0);
    let mut base = ModalBase::new(ModalKind::Tabs(pane), "Open Tabs", items);
    base.selected = selected;
    Box::new(TabsModal { base })
}

pub fn picker(kind: ModalKind, title: &str, items: Vec<ModalItem>) -> Box<dyn Modal> {
    struct Picker(ModalBase);
    impl Modal for Picker {
        fn base(&self) -> &ModalBase {
            &self.0
        }
        fn base_mut(&mut self) -> &mut ModalBase {
            &mut self.0
        }
    }
    Box::new(Picker(ModalBase::new(kind, title, items)))
}

#[cfg(test)]
thread_local! {
    static REFILTERS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

#[must_use]
pub fn quick_open(items: Vec<ModalItem>) -> Box<dyn Modal> {
    Box::new(QuickOpenModal::new(items))
}

#[must_use]
pub fn search_results(items: Vec<ModalItem>) -> Box<dyn Modal> {
    Box::new(SearchResultsModal::new(items))
}

pub fn agent_sessions(items: Vec<ModalItem>) -> Box<dyn Modal> {
    Box::new(AgentSessionsModal::new(items))
}

pub fn connection_picker(items: Vec<ModalItem>) -> Box<dyn Modal> {
    Box::new(ConnectionPickerModal::new(items))
}

pub fn code_actions(items: Vec<ModalItem>) -> Box<dyn Modal> {
    Box::new(CodeActionsModal::new(items))
}

pub fn session_picker(items: Vec<ModalItem>) -> Box<dyn Modal> {
    Box::new(SessionPickerModal::new(items))
}

pub fn query_history(items: Vec<ModalItem>) -> Box<dyn Modal> {
    Box::new(QueryHistoryModal::new(items))
}

pub fn ai_cli_picker(items: Vec<ModalItem>) -> Box<dyn Modal> {
    Box::new(AiCliPickerModal::new(items))
}

pub fn theme_picker(items: Vec<ModalItem>) -> Box<dyn Modal> {
    Box::new(ThemePickerModal::new(items))
}

#[must_use]
pub fn branch_picker(items: Vec<ModalItem>) -> Box<dyn Modal> {
    Box::new(BranchPickerModal::new(items))
}

#[must_use]
pub fn settings(items: Vec<ModalItem>) -> Box<dyn Modal> {
    Box::new(SettingsModal::new(items))
}

#[must_use]
pub fn keybindings(items: Vec<ModalItem>) -> Box<dyn Modal> {
    Box::new(KeybindingsModal::new(items))
}

#[must_use]
pub fn save_as(initial: &str) -> Box<dyn Modal> {
    Box::new(FileBrowserModal::new(initial))
}

#[must_use]
pub fn prompt(action: PromptAction, title: &str, initial: &str) -> Box<dyn Modal> {
    Box::new(PromptModal::new(action, title, initial))
}

#[must_use]
pub fn find(initial: &str) -> Box<dyn Modal> {
    Box::new(FindModal::new(initial))
}

#[must_use]
pub fn confirm(action: ConfirmAction, title: &str, message: &str) -> Box<dyn Modal> {
    Box::new(ConfirmModal::new(action, title, message))
}

#[must_use]
pub fn command_items(commands: &[(String, tide_config::CommandInfo)]) -> Vec<ModalItem> {
    commands
        .iter()
        .map(|(id, info)| ModalItem::new(info.label.clone(), info.category.clone(), id.clone()))
        .collect()
}

#[must_use]
pub fn file_items(files: &[std::path::PathBuf], root: &std::path::Path) -> Vec<ModalItem> {
    files
        .iter()
        .map(|path| {
            let relative = path.strip_prefix(root).unwrap_or(path);
            ModalItem::new(
                relative.display().to_string(),
                String::new(),
                path.display().to_string(),
            )
        })
        .collect()
}

struct GitChoiceModal {
    base: ModalBase,
}
impl Modal for GitChoiceModal {
    fn base(&self) -> &ModalBase {
        &self.base
    }
    fn base_mut(&mut self) -> &mut ModalBase {
        &mut self.base
    }
}
pub fn git_choice(action: &str, items: Vec<ModalItem>) -> Box<dyn Modal> {
    let title = match action {
        "rebase" => "Rebase Onto Branch".to_owned(),
        "cherryPick" => "Cherry-Pick Commit".to_owned(),
        "revert" => "Revert Commit".to_owned(),
        "reset" => "Reset Current Branch To".to_owned(),
        "resetMode" => "Reset Mode".to_owned(),
        "conflicts" => "Conflicted Files".to_owned(),
        _ => format!("Git: {action}"),
    };
    Box::new(GitChoiceModal {
        base: ModalBase::new(ModalKind::GitChoice(action.to_owned()), title, items),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pasting_into_a_large_quick_open_filters_once() {
        let items = (0..10_000)
            .map(|index| {
                let path = format!("src/module_{index}/file_{index}.rs");
                ModalItem::new(path.clone(), String::new(), path)
            })
            .collect();
        let mut modal = quick_open(items);
        REFILTERS.set(0);
        modal.paste("module_42/file_42");
        assert_eq!(REFILTERS.get(), 1);
        assert_eq!(modal.base().input, "module_42/file_42");
        assert_eq!(modal.base().cursor, 17);
        assert_eq!(
            modal.selected_item().map(|item| item.label.as_str()),
            Some("src/module_42/file_42.rs")
        );
        modal.base_mut().cursor = 0;
        modal.paste("src/");
        assert_eq!(modal.base().input, "src/module_42/file_42");
        assert_eq!(modal.base().cursor, 4);
        assert_eq!(REFILTERS.get(), 2);
    }

    #[test]
    fn save_discard_cancel_and_overwrite_have_explicit_roles_independent_of_focus() {
        let modal = ModalBase::confirm(ConfirmAction::QuitDirty, "Unsaved", "Save changes?");
        assert_eq!(modal.action_role(0), ActionRole::Confirmation);
        assert_eq!(modal.action_role(1), ActionRole::Destructive);
        assert_eq!(modal.action_role(2), ActionRole::Neutral);
        assert_eq!(modal.action_index, 2);
        let overwrite = ModalBase::confirm(ConfirmAction::OverwriteFile, "Overwrite", "Replace?");
        assert_eq!(overwrite.action_role(0), ActionRole::Destructive);
        assert_eq!(overwrite.action_role(1), ActionRole::Neutral);
    }

    #[test]
    fn completion_and_anchored_info_fit_their_content_and_share_click_geometry() {
        let area = Rect::new(0, 0, 120, 40);
        let mut popup = ModalBase::new(
            ModalKind::Completion,
            "Completions",
            vec![ModalItem::new("mock_fn", "fn()", "accept")],
        );
        popup.anchor = Some((50, 5));
        assert_eq!(popup.geometry(area), Rect::new(50, 6, 24, 3));
        assert!(popup.action_bounds(area).is_empty());
        assert!(matches!(
            default_key(&mut popup, KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE)),
            KeyOutcome::Submit
        ));
        let list = popup.list_bounds(area).unwrap();
        assert_eq!(list, Rect::new(51, 7, 22, 1));
        assert_eq!(
            popup.click_key(area, list.x, list.y).unwrap().code,
            KeyCode::Enter
        );
        assert_eq!(popup.click_key(area, 0, 0).unwrap().code, KeyCode::Esc);
        let mut info = ModalBase::new(ModalKind::Info, "Hover", Vec::new());
        info.message = Some("one line".into());
        info.anchor = Some((50, 5));
        assert_eq!(info.geometry(area).height, 3);
        assert!(info.action_bounds(area).is_empty());
    }

    fn items() -> Vec<ModalItem> {
        vec![
            ModalItem::new("Save", "File", "file.save"),
            ModalItem::new("Quick Open", "View", "workbench.quickOpen"),
            ModalItem::new("Toggle Sidebar", "View", "workbench.toggleSidebar"),
        ]
    }

    #[test]
    fn fuzzy_filter_finds_items() {
        let mut modal = CommandPaletteModal::new(items());
        modal.base_mut().push_char('q');
        modal.base_mut().push_char('o');
        assert!(!modal.base().filtered.is_empty());
        assert_eq!(modal.selected_item().unwrap().value, "workbench.quickOpen");
    }

    #[test]
    fn command_palette_filters_real_catalog_without_panicking() {
        let commands = tide_config::builtin::commands()
            .into_iter()
            .collect::<Vec<_>>();
        let items = command_items(&commands);
        for query in ["New Query", "Agents: Toggle Sidebar", "Database", "AI CLI"] {
            for length in 0..=query.chars().count() {
                let prefix = query.chars().take(length).collect::<String>();
                let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    let mut modal = CommandPaletteModal::new(items.clone());
                    for ch in prefix.chars() {
                        modal.base_mut().push_char(ch);
                    }
                    modal.base().filtered.len()
                }));
                assert!(result.is_ok(), "panic while filtering {prefix:?}");
            }
        }
    }

    #[test]
    fn default_modal_geometry_is_centred_and_bounded() {
        let modal = CommandPaletteModal::new(items());
        let area = modal.base().geometry(Rect::new(0, 0, 120, 40));
        assert_eq!(area.width, 60);
        assert_eq!(area.height, 20);
        assert_eq!(area.y, 6);
        let quick = QuickOpenModal::new(items());
        let area = quick.base().geometry(Rect::new(0, 0, 120, 40));
        assert_eq!(area.width, 70);
    }

    #[test]
    fn prompt_editing_tracks_cursor() {
        let mut modal = PromptModal::new(PromptAction::GotoLine, "Go to Line", "");
        modal.base_mut().push_char('1');
        modal.base_mut().push_char('2');
        modal.base_mut().backspace();
        assert_eq!(modal.input(), "1");
        assert_eq!(modal.base().cursor, 1);
    }

    #[test]
    fn file_browser_targets_a_filename_in_the_current_directory() {
        let dir = tempfile::tempdir().unwrap();
        let modal = FileBrowserModal::new(&dir.path().join("draft.txt").display().to_string());
        assert_eq!(modal.directory(), dir.path());
        assert_eq!(modal.target_path().unwrap(), dir.path().join("draft.txt"));
    }

    #[test]
    fn mouse_rows_select_without_submitting() {
        let mut modal = CommandPaletteModal::new(items());
        let area = Rect::new(0, 0, 120, 40);
        let list = modal.base().list_bounds(area).unwrap();
        assert!(modal.handle_mouse(area, list.x + 1, list.y + 1));
        assert_eq!(modal.base().selected, 1);
    }

    #[test]
    fn save_discard_cancel_actions_share_mouse_keyboard_and_tiny_geometry() {
        for area in [Rect::new(0, 0, 82, 26), Rect::new(0, 0, 22, 14)] {
            let mut modal = ConfirmModal::new(
                ConfirmAction::CloseDirty,
                "Save Changes",
                "Save this file before closing?",
            );
            assert_eq!(
                modal
                    .handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE))
                    .key(),
                KeyOutcome::Close
            );
            let actions = modal.base().action_bounds(area);
            assert_eq!(actions.len(), 3);
            for (rect, index, _) in actions {
                let key = modal.click_key(area, rect.x, rect.y).unwrap();
                assert_eq!(
                    modal.handle_key(key).key(),
                    [
                        KeyOutcome::SaveBeforeConfirm,
                        KeyOutcome::Submit,
                        KeyOutcome::Close
                    ][index]
                );
            }
            assert_eq!(
                modal
                    .handle_key(KeyEvent::new(KeyCode::Char('s'), KeyModifiers::NONE))
                    .key(),
                KeyOutcome::SaveBeforeConfirm
            );
            assert_eq!(
                modal
                    .handle_key(KeyEvent::new(KeyCode::Char('y'), KeyModifiers::NONE))
                    .key(),
                KeyOutcome::SaveBeforeConfirm
            );
            assert_eq!(
                modal
                    .handle_key(KeyEvent::new(KeyCode::Char('n'), KeyModifiers::NONE))
                    .key(),
                KeyOutcome::Submit
            );
            assert_eq!(
                modal
                    .handle_key(KeyEvent::new(KeyCode::Char('y'), KeyModifiers::CONTROL))
                    .key(),
                KeyOutcome::None
            );
            assert_eq!(
                modal
                    .handle_key(KeyEvent::new(KeyCode::Char('n'), KeyModifiers::CONTROL))
                    .key(),
                KeyOutcome::None
            );
        }
        let modal = ConfirmModal::new(ConfirmAction::CloseDirty, "Save Changes", "Save?");
        let actions = modal.base().action_bounds(Rect::new(0, 0, 8, 5));
        assert!(
            actions.iter().any(|(_, index, _)| *index == 2),
            "Cancel must stay visible when other actions cannot fit"
        );
    }

    #[test]
    fn confirmation_keys_are_explicit() {
        let mut modal = ConfirmModal::new(
            ConfirmAction::DeletePath("fixture".into()),
            "Delete",
            "Delete?",
        );
        let outcome = modal
            .handle_key(KeyEvent::new(KeyCode::Char('y'), KeyModifiers::NONE))
            .key();
        assert_eq!(outcome, KeyOutcome::Submit);
        let outcome = modal
            .handle_key(KeyEvent::new(KeyCode::Char('n'), KeyModifiers::NONE))
            .key();
        assert_eq!(outcome, KeyOutcome::Close);
    }

    #[test]
    fn destructive_confirmation_defaults_to_cancel_and_rendered_actions_activate() {
        let area = Rect::new(0, 0, 82, 26);
        let mut modal = ConfirmModal::new(
            ConfirmAction::DeletePath("fixture".into()),
            "Delete",
            "Permanently delete this file?",
        );
        assert_eq!(
            modal
                .handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE))
                .key(),
            KeyOutcome::Close
        );
        let actions = modal.base().action_bounds(area);
        assert_eq!(actions.len(), 2);
        let first = actions[0].0;
        let last = actions[1].0;
        let popup = modal.base().geometry(area);
        assert!(
            (first.x.saturating_sub(popup.x) as i32 - (popup.right() - last.right()) as i32).abs()
                <= 1
        );
        let key = modal.click_key(area, first.x + 1, first.y).unwrap();
        assert_eq!(modal.handle_key(key).key(), KeyOutcome::Submit);
        let key = modal.click_key(area, last.x + 1, last.y).unwrap();
        assert_eq!(modal.handle_key(key).key(), KeyOutcome::Close);
    }

    #[test]
    fn dialog_wrap_resize_and_unicode_fields_preserve_content_and_targets() {
        let message = "Quit with unsaved buffers? The session snapshot preserves the draft. Cancel returns to editing.";
        let modal = ConfirmModal::new(ConfirmAction::QuitDirty, "Unsaved Changes", message);
        for (width, height) in [
            (82, 26),
            (120, 40),
            (160, 50),
            (22, 14),
            (8, 5),
            (1, 1),
            (0, 0),
        ] {
            let area = Rect::new(0, 0, width, height);
            let popup = modal.base().geometry(area);
            assert!(popup.right() <= area.right() && popup.bottom() <= area.bottom());
            for (rect, _, _) in modal.base().action_bounds(area) {
                assert!(rect.right() <= area.right() && rect.bottom() <= area.bottom());
            }
        }
        let lines = wrap_message(message, 20);
        assert!(lines.iter().all(|line| display_width(line) <= 20));
        assert_eq!(lines.join(" "), message);
        let mut field = PromptModal::new(PromptAction::SaveSessionName, "Name", "a👩‍💻界b");
        field.base_mut().move_left();
        field.base_mut().backspace();
        assert_eq!(field.input(), "a👩‍💻b");
        field.base_mut().backspace();
        assert_eq!(field.input(), "ab");
        let area = Rect::new(0, 0, 82, 26);
        let input = field
            .base()
            .content_layout(Block::bordered().inner(field.base().geometry(area)))[0];
        field.click_key(area, input.x + 2, input.y);
        assert_eq!(field.base().cursor, 0);
    }

    #[test]
    fn picker_clicks_activate_rows_and_disabled_empty_actions_do_not_submit() {
        let mut modal = QuickOpenModal::new(items());
        let area = Rect::new(0, 0, 120, 40);
        let list = modal.base().list_bounds(area).unwrap();
        let key = modal.click_key(area, list.x + 1, list.y + 1).unwrap();
        assert_eq!(modal.base().selected, 1);
        assert_eq!(modal.handle_key(key).key(), KeyOutcome::Submit);
        let mut empty = QuickOpenModal::new(Vec::new());
        let first = empty.base().action_bounds(area)[0].0;
        assert!(empty.click_key(area, first.x, first.y).is_none());
        empty.base_mut().actions_focused = true;
        assert_eq!(
            empty
                .handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE))
                .key(),
            KeyOutcome::None
        );
    }

    #[test]
    fn save_as_typing_saves_the_name_and_row_click_navigates_directory() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join("child")).unwrap();
        let mut modal = FileBrowserModal::new(&dir.path().join("new.txt").display().to_string());
        assert_eq!(
            modal
                .handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE))
                .key(),
            KeyOutcome::Submit
        );
        assert_eq!(modal.target_path().unwrap(), dir.path().join("new.txt"));
        let index = modal
            .base()
            .items
            .iter()
            .position(|item| item.label == "child/")
            .unwrap();
        let area = Rect::new(0, 0, 120, 40);
        let list = modal.base().list_bounds(area).unwrap();
        let key = modal
            .click_key(area, list.x + 1, list.y + u16::try_from(index).unwrap())
            .unwrap();
        assert_eq!(modal.handle_key(key).key(), KeyOutcome::None);
        assert_eq!(modal.directory(), dir.path().join("child"));
    }
}
