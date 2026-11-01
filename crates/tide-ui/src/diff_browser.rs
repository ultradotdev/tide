//! Structured diff viewport shared by keyboard and pointer routes.
use crate::palette::Palette;
use crossterm::event::{KeyCode, KeyEvent};
use ratatui::{
    Frame,
    layout::Rect,
    style::Style,
    text::{Line, Span},
    widgets::Paragraph,
};
use std::{collections::HashSet, path::PathBuf, sync::Arc};
use tide_services::git::diff::{DiffKind, FileDiff};
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    Edit,
    Stage,
    Unstage,
    Discard,
    Open,
    Diagnostics,
}
#[derive(Debug, Clone, Copy)]
struct Row {
    hunk: Option<usize>,
    left: Option<usize>,
    right: Option<usize>,
}
#[derive(Debug, Clone)]
pub struct DiffBrowser {
    pub root: PathBuf,
    pub diff: Arc<FileDiff>,
    pub selected: usize,
    pub scroll: usize,
    pub side_by_side: bool,
    pub diagnostics: Vec<(usize, u8, String)>,
    collapsed: HashSet<usize>,
    file_collapsed: bool,
    only_hunk: Option<usize>,
    rows: Vec<Row>,
    height: usize,
}
impl DiffBrowser {
    pub fn new(root: PathBuf, diff: FileDiff) -> Self {
        let mut value = Self {
            root,
            diff: Arc::new(diff),
            selected: 1,
            scroll: 0,
            side_by_side: false,
            diagnostics: Vec::new(),
            collapsed: HashSet::new(),
            file_collapsed: false,
            only_hunk: None,
            rows: Vec::new(),
            height: 0,
        };
        value.rebuild();
        value
    }
    fn rebuild(&mut self) {
        self.rows = vec![Row {
            hunk: None,
            left: None,
            right: None,
        }];
        if !self.file_collapsed {
            for (hunk, value) in self.diff.hunks.iter().enumerate() {
                if self.only_hunk.is_some_and(|only| only != hunk) {
                    continue;
                }
                self.rows.push(Row {
                    hunk: Some(hunk),
                    left: None,
                    right: None,
                });
                if self.collapsed.contains(&hunk) {
                    continue;
                }
                let mut i = 0;
                while i < value.lines.len() {
                    let line = &value.lines[i];
                    if self.side_by_side && line.kind == DiffKind::Deleted {
                        let start = i;
                        while i < value.lines.len() && value.lines[i].kind == DiffKind::Deleted {
                            i += 1;
                        }
                        let add = i;
                        while i < value.lines.len() && value.lines[i].kind == DiffKind::Added {
                            i += 1;
                        }
                        for offset in 0..(add - start).max(i - add) {
                            self.rows.push(Row {
                                hunk: Some(hunk),
                                left: (start + offset < add).then_some(start + offset),
                                right: (add + offset < i).then_some(add + offset),
                            });
                        }
                    } else {
                        self.rows.push(Row {
                            hunk: Some(hunk),
                            left: (line.kind != DiffKind::Added).then_some(i),
                            right: (line.kind != DiffKind::Deleted).then_some(i),
                        });
                        i += 1;
                    }
                }
            }
        }
        self.selected = self.selected.min(self.rows.len().saturating_sub(1));
    }
    pub fn update(&mut self, diff: FileDiff) {
        let line = self.line();
        let headers = self
            .collapsed
            .iter()
            .filter_map(|index| self.diff.hunks.get(*index))
            .map(|h| h.header.clone())
            .collect::<HashSet<_>>();
        self.diff = Arc::new(diff);
        self.collapsed = self
            .diff
            .hunks
            .iter()
            .enumerate()
            .filter(|(_, h)| headers.contains(&h.header))
            .map(|(index, _)| index)
            .collect();
        self.rebuild();
        if let Some(line) = line
            && let Some(index) = self.rows.iter().position(|row| {
                row.hunk
                    .and_then(|h| row.right.map(|i| self.diff.hunks[h].lines[i].new_line))
                    .flatten()
                    == Some(line)
            })
        {
            self.selected = index;
        }
    }
    pub fn show_only_hunk(&mut self, index: usize) {
        self.only_hunk = Some(index);
        self.collapsed.remove(&index);
        self.selected = 1;
        self.scroll = 0;
        self.rebuild();
    }
    pub fn hunk(&self) -> Option<usize> {
        self.rows.get(self.selected)?.hunk
    }
    pub fn line(&self) -> Option<usize> {
        let row = self.rows.get(self.selected)?;
        let hunk = self.diff.hunks.get(row.hunk?)?;
        row.right
            .and_then(|i| hunk.lines[i].new_line)
            .or_else(|| row.left.and_then(|i| hunk.lines[i].old_line))
    }
    fn toggle(&mut self) {
        match self.rows.get(self.selected).copied() {
            Some(Row { hunk: None, .. }) => self.file_collapsed = !self.file_collapsed,
            Some(Row {
                hunk: Some(index), ..
            }) if !self.collapsed.remove(&index) => {
                self.collapsed.insert(index);
            }
            _ => {}
        }
        self.rebuild();
    }
    pub fn key(&mut self, key: KeyEvent) -> Option<Action> {
        match key.code {
            KeyCode::Up | KeyCode::Char('k') => self.selected = self.selected.saturating_sub(1),
            KeyCode::Down | KeyCode::Char('j') => {
                self.selected = (self.selected + 1).min(self.rows.len().saturating_sub(1))
            }
            KeyCode::PageUp => self.selected = self.selected.saturating_sub(self.height.max(1)),
            KeyCode::PageDown => {
                self.selected =
                    (self.selected + self.height.max(1)).min(self.rows.len().saturating_sub(1))
            }
            KeyCode::Home => self.selected = 0,
            KeyCode::End => self.selected = self.rows.len().saturating_sub(1),
            KeyCode::Left | KeyCode::Right | KeyCode::Char(' ') => self.toggle(),
            KeyCode::Char('v') => {
                self.side_by_side = !self.side_by_side;
                self.rebuild();
            }
            KeyCode::Char('?') => return Some(Action::Diagnostics),
            KeyCode::Char('e') => return Some(Action::Edit),
            KeyCode::Char('s') => return Some(Action::Stage),
            KeyCode::Char('u') => return Some(Action::Unstage),
            KeyCode::Char('d') => return Some(Action::Discard),
            KeyCode::Enter => {
                if self
                    .rows
                    .get(self.selected)
                    .is_some_and(|r| r.left.is_none() && r.right.is_none())
                {
                    self.toggle();
                } else {
                    return Some(Action::Open);
                }
            }
            KeyCode::Char('o') => return Some(Action::Open),
            _ => {}
        }
        None
    }
    pub fn click(&mut self, row: usize, column: usize) -> Option<Action> {
        if row == 0 {
            if column >= 61 {
                return (!self.diagnostics.is_empty()).then_some(Action::Diagnostics);
            }
            if column >= 51 {
                self.side_by_side = !self.side_by_side;
                self.rebuild();
                return None;
            }
            return [
                Action::Open,
                Action::Edit,
                Action::Stage,
                Action::Unstage,
                Action::Discard,
            ]
            .get(column / 10)
            .copied();
        }
        let selected = self.scroll + row - 1;
        if selected >= self.rows.len() {
            return None;
        }
        self.selected = selected;
        if self.rows[selected].left.is_none() && self.rows[selected].right.is_none() {
            self.toggle();
            None
        } else {
            Some(Action::Open)
        }
    }
    /// Fits the scroll position to `area`, where the next render draws.
    pub fn layout(&mut self, area: Rect) {
        self.height = area.height.saturating_sub(1) as usize;
        if self.selected < self.scroll {
            self.scroll = self.selected;
        }
        if self.selected >= self.scroll + self.height.max(1) {
            self.scroll = self.selected + 1 - self.height.max(1);
        }
    }
    pub fn render(&self, frame: &mut Frame, area: Rect, palette: Palette, focused: bool) {
        let mut lines = vec![Line::styled(
            format!(
                " Open [o]  Edit [e]  Stage [s] Unstage[u] Discard[d]  View[v]{}",
                if self.diagnostics.is_empty() {
                    ""
                } else {
                    " Diag[?]"
                }
            ),
            Style::default().fg(palette.accent),
        )];
        for (index, row) in self
            .rows
            .iter()
            .enumerate()
            .skip(self.scroll)
            .take(self.height)
        {
            let base = if focused && index == self.selected {
                Style::default()
                    .bg(palette.selection_bg)
                    .fg(palette.selection_fg)
            } else {
                Style::default()
                    .bg(palette.background)
                    .fg(palette.foreground)
            };
            let Some(hunk) = row.hunk else {
                lines.push(Line::styled(
                    format!(
                        "{} {} [{}]{}",
                        if self.file_collapsed { "▸" } else { "▾" },
                        self.diff.path,
                        if self.diff.historical.is_some() {
                            "history"
                        } else if self.diff.staged {
                            "staged"
                        } else {
                            "working"
                        },
                        if self.diff.binary {
                            " binary / read-only"
                        } else {
                            ""
                        }
                    ),
                    base,
                ));
                continue;
            };
            let value = &self.diff.hunks[hunk];
            let line_number = row.right.and_then(|i| value.lines[i].new_line);
            let diagnostic = line_number
                .and_then(|line| self.diagnostics.iter().find(|(n, _, _)| *n == line))
                .map(|(_, _, message)| format!("  ! {message}"))
                .unwrap_or_default();
            if row.left.is_none() && row.right.is_none() {
                lines.push(Line::styled(
                    format!(
                        " {} {}",
                        if self.collapsed.contains(&hunk) {
                            "▸"
                        } else {
                            "▾"
                        },
                        value.header
                    ),
                    base.fg(palette.accent),
                ));
                continue;
            }
            if self.side_by_side {
                let width = area.width.saturating_sub(1) as usize / 2;
                let cell = |id: Option<usize>, old: bool| -> String {
                    id.map_or(String::new(), |i| {
                        let l = &value.lines[i];
                        format!(
                            "{:>4} {}",
                            if old { l.old_line } else { l.new_line }
                                .map_or(String::new(), |n| n.to_string()),
                            l.text
                        )
                    })
                };
                let left_cell = cell(row.left, true);
                let left = tide_core::truncate_to_width(&left_cell, width);
                let pad = width.saturating_sub(tide_core::display_width(left));
                lines.push(Line::from(vec![
                    Span::styled(
                        format!("{left}{}", " ".repeat(pad)),
                        base.fg(
                            if row
                                .left
                                .is_some_and(|i| value.lines[i].kind == DiffKind::Deleted)
                            {
                                palette.diff_delete
                            } else {
                                palette.foreground
                            },
                        ),
                    ),
                    Span::styled("│", base),
                    Span::styled(
                        cell(row.right, false) + &diagnostic,
                        base.fg(
                            if row
                                .right
                                .is_some_and(|i| value.lines[i].kind == DiffKind::Added)
                            {
                                palette.diff_add
                            } else {
                                palette.foreground
                            },
                        ),
                    ),
                ]));
            } else {
                let value = &value.lines[row.right.or(row.left).unwrap()];
                let (sign, color) = match value.kind {
                    DiffKind::Added => ("+", palette.diff_add),
                    DiffKind::Deleted => ("-", palette.diff_delete),
                    _ => (" ", palette.foreground),
                };
                lines.push(Line::styled(
                    format!(
                        "{:>4} {:>4} {sign}{}{diagnostic}",
                        value.old_line.map_or(String::new(), |n| n.to_string()),
                        value.new_line.map_or(String::new(), |n| n.to_string()),
                        value.text
                    ),
                    base.fg(color),
                ));
            }
        }
        frame.render_widget(
            Paragraph::new(lines).style(Style::default().bg(palette.background)),
            area,
        );
    }
}
