//! Grouped Git rows with stable file/section identity and shared visible geometry.
use crate::palette::Palette;
use ratatui::{
    Frame,
    layout::Rect,
    style::Style,
    text::{Line, Span},
    widgets::Paragraph,
};
use std::collections::HashSet;
use tide_services::{GitSnapshot, GitStatus};
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Group {
    Conflicts,
    Staged,
    Unstaged,
    Untracked,
}
impl Group {
    pub fn label(self) -> &'static str {
        match self {
            Self::Conflicts => "Conflicts",
            Self::Staged => "Staged",
            Self::Unstaged => "Changes",
            Self::Untracked => "Untracked",
        }
    }
    fn includes(self, index: Option<GitStatus>, worktree: Option<GitStatus>) -> bool {
        match self {
            Self::Conflicts => {
                index == Some(GitStatus::Conflict) || worktree == Some(GitStatus::Conflict)
            }
            Self::Staged => index.is_some() && index != Some(GitStatus::Conflict),
            Self::Unstaged => {
                worktree.is_some()
                    && !matches!(worktree, Some(GitStatus::Untracked | GitStatus::Conflict))
            }
            Self::Untracked => worktree == Some(GitStatus::Untracked),
        }
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Row {
    Header(Group, usize),
    File(Group, usize),
}
#[derive(Debug, Default)]
pub struct GitPanel {
    pub selected: usize,
    pub scroll: usize,
    pub rows: Vec<Row>,
    pub collapsed: HashSet<Group>,
    pub revision: u64,
    height: usize,
    paths: Vec<String>,
    last_click: Option<(Row, std::time::Instant)>,
}
impl GitPanel {
    pub fn rebuild(&mut self, snapshot: Option<&GitSnapshot>, revision: u64) {
        if self.revision != revision {
            self.last_click = None;
        }
        let identity = self.rows.get(self.selected).copied();
        let old_path = match identity {
            Some(Row::File(_, index)) => self.paths.get(index).cloned(),
            _ => None,
        };
        let initial = self.rows.is_empty();
        self.paths = snapshot.map_or(Vec::new(), |s| s.changed_paths.clone());
        self.rows.clear();
        if let Some(snapshot) = snapshot {
            for group in [
                Group::Conflicts,
                Group::Staged,
                Group::Unstaged,
                Group::Untracked,
            ] {
                let files = snapshot
                    .changes
                    .iter()
                    .enumerate()
                    .filter(|(_, change)| group.includes(change.index, change.worktree))
                    .map(|(index, _)| index)
                    .collect::<Vec<_>>();
                if files.is_empty() {
                    continue;
                }
                self.rows.push(Row::Header(group, files.len()));
                if !self.collapsed.contains(&group) {
                    self.rows
                        .extend(files.into_iter().map(|index| Row::File(group, index)));
                }
            }
            if snapshot.changes.is_empty() && !snapshot.changed_paths.is_empty() {
                self.rows
                    .push(Row::Header(Group::Unstaged, snapshot.changed_paths.len()));
                self.rows.extend(
                    (0..snapshot.changed_paths.len())
                        .map(|index| Row::File(Group::Unstaged, index)),
                );
            }
        }
        if let Some(identity) = identity
            && let Some(index) = self.rows.iter().position(|row| match (row, identity) {
                (Row::Header(group, _), Row::Header(previous, _)) => *group == previous,
                (Row::File(group, index), Row::File(previous, old)) => {
                    *group == previous
                        && (self.paths.get(*index) == old_path.as_ref()
                            || (old_path.is_none() && *index == old))
                }
                _ => false,
            })
        {
            self.selected = index;
        }
        if initial {
            self.selected = self
                .rows
                .iter()
                .position(|r| matches!(r, Row::File(..)))
                .unwrap_or(0);
        }
        self.selected = self.selected.min(self.rows.len().saturating_sub(1));
        self.revision = revision;
    }
    pub fn toggle(&mut self, snapshot: Option<&GitSnapshot>, revision: u64) {
        let group = match self.rows.get(self.selected) {
            Some(Row::Header(group, _) | Row::File(group, _)) => *group,
            None => return,
        };
        if !self.collapsed.remove(&group) {
            self.collapsed.insert(group);
        }
        self.rebuild(snapshot, revision);
    }
    pub fn move_by(&mut self, delta: isize) {
        self.selected = self
            .selected
            .saturating_add_signed(delta)
            .min(self.rows.len().saturating_sub(1));
    }
    pub fn file(&self) -> Option<(Group, usize)> {
        match self.rows.get(self.selected) {
            Some(Row::File(group, index)) => Some((*group, *index)),
            _ => None,
        }
    }
    pub fn select_at(&mut self, row: usize) -> Option<Row> {
        if row >= self.height {
            return None;
        }
        let index = self.scroll + row;
        let selected = self.rows.get(index).copied()?;
        self.selected = index;
        Some(selected)
    }
    pub fn click_at(&mut self, row: usize) -> Option<(Row, bool)> {
        let selected = self.select_at(row)?;
        let double = self.last_click.is_some_and(|(previous, at)| {
            previous == selected && at.elapsed() < std::time::Duration::from_millis(400)
        });
        self.last_click = Some((selected, std::time::Instant::now()));
        Some((selected, double))
    }
    /// Fits the scroll position to `area`, where the next render draws.
    pub fn layout(&mut self, area: Rect) {
        self.height = area.height as usize;
        if self.selected < self.scroll {
            self.scroll = self.selected;
        } else if self.selected >= self.scroll + self.height.max(1) {
            self.scroll = self.selected + 1 - self.height.max(1);
        }
    }
    pub fn render(
        &self,
        frame: &mut Frame,
        area: Rect,
        snapshot: Option<&GitSnapshot>,
        palette: Palette,
        focused: bool,
    ) {
        let lines = self
            .rows
            .iter()
            .enumerate()
            .skip(self.scroll)
            .take(self.height)
            .map(|(index, row)| {
                let status_color = match row {
                    Row::File(group, index) => snapshot
                        .and_then(|s| s.changes.get(*index))
                        .and_then(|c| {
                            if *group == Group::Staged {
                                c.index
                            } else {
                                c.worktree
                            }
                        })
                        .map_or(palette.foreground, |status| match status {
                            GitStatus::Added => palette.diff_add,
                            GitStatus::Deleted => palette.diff_delete,
                            GitStatus::Untracked => palette.git_untracked,
                            GitStatus::Conflict => palette.error,
                            _ => palette.git_modified,
                        }),
                    _ => palette.foreground,
                };
                let style = if focused && index == self.selected {
                    Style::default()
                        .fg(palette.selection_fg)
                        .bg(palette.selection_bg)
                } else {
                    Style::default().fg(status_color).bg(palette.sidebar)
                };
                let text = match row {
                    Row::Header(group, count) => format!(
                        " {} {} ({count})",
                        if self.collapsed.contains(group) {
                            "▸"
                        } else {
                            "▾"
                        },
                        group.label()
                    ),
                    Row::File(group, index) => snapshot
                        .and_then(|snapshot| snapshot.changes.get(*index))
                        .map(|change| {
                            format!(
                                "   {} {}{}",
                                if *group == Group::Staged {
                                    change.index
                                } else {
                                    change.worktree
                                }
                                .map_or("?", GitStatus::label),
                                change.path,
                                change
                                    .previous_path
                                    .as_ref()
                                    .map_or(String::new(), |previous| format!(" ← {previous}"))
                            )
                        })
                        .or_else(|| {
                            snapshot
                                .and_then(|snapshot| snapshot.changed_paths.get(*index))
                                .map(|path| format!("   M {path}"))
                        })
                        .unwrap_or_default(),
                };
                Line::from(Span::styled(text, style))
            })
            .collect::<Vec<_>>();
        frame.render_widget(
            Paragraph::new(lines).style(Style::default().bg(palette.sidebar)),
            area,
        );
        crate::scrollbar::paint(
            frame,
            area,
            self.rows.len(),
            self.height,
            self.scroll,
            palette,
        );
    }
}
