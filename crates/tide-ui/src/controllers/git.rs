//! Source control: the repository snapshot, the Source Control panel, the timeline, diff views
//! and the ordered Git work queue.
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use anyhow::Result;
use crossterm::event::{KeyCode, KeyEvent};
use tide_lsp::{Diagnostic, path_to_uri};
use tide_services::GitSnapshot;
use tide_services::git::GitStatus;

use super::{Effect, Ui};
use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

use crate::app::AppEvent;
use crate::editor::{DocumentHandle, EditorState};
use crate::git_jobs::GitJobs;
use crate::git_panel::GitPanel;
use crate::jobs::Jobs;
use crate::modal::{self, ConfirmAction, ModalItem, ModalKind, PromptAction, Submitted};
use crate::palette::Palette;
use crate::workbench::{DiffTab, Tab, Workbench};

type FileDiff = tide_services::git::diff::FileDiff;

#[derive(Debug, Clone)]
pub struct GitState {
    pub snapshot: Option<GitSnapshot>,
    pub selected: usize,
    pub revision: u64,
}

impl GitState {
    #[cfg(test)]
    #[must_use]
    pub fn refresh(workspace: &Path) -> Self {
        static REVISION: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
        let snapshot = tide_services::repository_snapshot(workspace).ok();
        Self {
            snapshot,
            selected: 0,
            revision: REVISION.fetch_add(1, std::sync::atomic::Ordering::Relaxed),
        }
    }

    #[must_use]
    pub fn changed(&self) -> &[String] {
        self.snapshot
            .as_ref()
            .map_or(&[], |snapshot| snapshot.changed_paths.as_slice())
    }

    #[must_use]
    pub fn selected_path(&self) -> Option<&str> {
        if self.selected_is_lossy() {
            return None;
        }
        self.changed().get(self.selected).map(String::as_str)
    }

    /// The selected path is not valid UTF-8, so path actions cannot name it.
    #[must_use]
    pub fn selected_is_lossy(&self) -> bool {
        self.snapshot
            .as_ref()
            .and_then(|snapshot| snapshot.changes.get(self.selected))
            .is_some_and(|change| change.lossy_path)
    }
}

#[derive(Debug)]
pub enum GitEvent {
    Refreshed {
        generation: u64,
        git: GitState,
        timeline: Vec<String>,
        message: Option<String>,
    },
    Choices {
        generation: u64,
        action: String,
        result: Result<Vec<ModalItem>, String>,
    },
    InlineDiffLoaded {
        version: u64,
        document: DocumentHandle,
        root: PathBuf,
        result: Result<FileDiff, String>,
    },
    TextLoaded {
        id: u64,
        text: String,
    },
    TimelineLoaded {
        path: Option<String>,
        entries: Vec<String>,
    },
    HistoricalFile {
        path: PathBuf,
        hash: String,
        result: Result<String, String>,
    },
    DiffLoaded {
        id: u64,
        root: PathBuf,
        result: Result<FileDiff, String>,
    },
    HunkSaved {
        document: DocumentHandle,
        version: u64,
        result: Result<(), String>,
    },
    /// A command's background check finished and decided what to show.
    Next {
        generation: u64,
        next: GitNext,
    },
}

/// What a Git command shows after checking the repository in the background.
#[derive(Debug)]
pub enum GitNext {
    Prompt {
        action: PromptAction,
        title: String,
        initial: String,
    },
    Confirm {
        action: ConfirmAction,
        title: String,
        message: String,
    },
    Status(String),
}

/// Commit history for the repository, or for the active file.
#[derive(Default)]
pub(crate) struct Timeline {
    pub entries: Vec<String>,
    pub selected: usize,
    /// Follows the active file instead of the whole repository.
    pub file: bool,
    pub scroll: usize,
    pub path: Option<String>,
}

pub(crate) struct GitController {
    pub state: GitState,
    pub panel: GitPanel,
    pub timeline: Timeline,
    /// The newest status request; older snapshots are still accepted.
    pub generation: u64,
    picker_generation: u64,
    jobs: GitJobs,
}

impl GitController {
    /// Fits the change list to `list`, where the next render draws it.
    pub fn layout_source_control(&mut self, list: Rect) {
        self.sync_panel();
        self.panel.layout(list);
    }

    /// Draws the Source Control section: the branch line and the change list.
    pub fn render_source_control(
        &self,
        frame: &mut Frame,
        area: Rect,
        palette: Palette,
        focused: bool,
    ) {
        let [header, list_area] =
            Layout::vertical([Constraint::Length(1), Constraint::Min(1)]).areas(area);
        let branch = self
            .state
            .snapshot
            .as_ref()
            .and_then(|snapshot| snapshot.branch.as_deref())
            .unwrap_or("no repository");
        frame.render_widget(
            Paragraph::new(Span::styled(
                format!(" {branch}"),
                Style::default().fg(palette.accent).bg(palette.sidebar),
            )),
            header,
        );
        self.panel.render(
            frame,
            list_area,
            self.state.snapshot.as_ref(),
            palette,
            focused,
        );
    }

    /// Keeps the selected commit inside the Timeline section.
    pub fn layout_timeline(&mut self, area: Rect) {
        let height = area.height.saturating_sub(1).max(1) as usize;
        if self.timeline.selected < self.timeline.scroll {
            self.timeline.scroll = self.timeline.selected;
        }
        if self.timeline.selected >= self.timeline.scroll + height {
            self.timeline.scroll = self.timeline.selected + 1 - height;
        }
    }

    /// Draws the Timeline section; `selected` highlights the selected commit.
    pub fn render_timeline(
        &self,
        frame: &mut Frame,
        area: Rect,
        palette: Palette,
        focused: bool,
        selected: bool,
    ) {
        let mode = if self.timeline.file {
            format!(
                "File: {}",
                self.timeline.path.as_deref().unwrap_or("no active file")
            )
        } else {
            "Repository".into()
        };
        let height = area.height.saturating_sub(1).max(1) as usize;
        let mut lines = if self.timeline.entries.is_empty() {
            vec![Line::from(Span::styled(
                " No commits",
                Style::default().fg(palette.description),
            ))]
        } else {
            let content_selected = selected;
            self.timeline
                .entries
                .iter()
                .enumerate()
                .skip(self.timeline.scroll)
                .take(height)
                .map(|(index, line)| {
                    let selected = content_selected && index == self.timeline.selected;
                    Line::from(Span::styled(
                        {
                            let (hash, rest) = line.split_once(' ').unwrap_or((line, ""));
                            format!(" {} {rest}", crate::display::short_id(hash))
                        },
                        Style::default()
                            .fg(if selected {
                                palette.selection_fg
                            } else if focused {
                                palette.foreground
                            } else {
                                palette.muted
                            })
                            .bg(if selected {
                                palette.selection_bg
                            } else {
                                palette.sidebar
                            }),
                    ))
                })
                .collect()
        };
        lines.insert(
            0,
            Line::styled(format!(" {mode} [m]"), Style::default().fg(palette.accent)),
        );
        frame.render_widget(
            Paragraph::new(lines).style(Style::default().bg(palette.sidebar)),
            area,
        );
    }

    pub fn new(jobs: &Jobs, events: tokio::sync::mpsc::Sender<AppEvent>) -> Self {
        Self {
            // The first status arrives from the background queue after the first frame.
            state: GitState {
                snapshot: None,
                selected: 0,
                revision: 0,
            },
            panel: GitPanel::default(),
            timeline: Timeline::default(),
            generation: 0,
            picker_generation: 0,
            jobs: GitJobs::new(jobs, events),
        }
    }

    pub async fn shutdown(&mut self) {
        self.jobs.shutdown().await;
    }

    /// The repository root, or the workspace when there is no repository.
    pub fn root(&self, workspace: &Path) -> PathBuf {
        self.state
            .snapshot
            .as_ref()
            .map_or_else(|| workspace.to_path_buf(), |snapshot| snapshot.root.clone())
    }

    /// The repository-relative path of the active diff or editor.
    pub fn active_path(&self, workspace: &Path, workbench: &Workbench) -> Option<String> {
        if let Some(Tab::Diff(tab)) = workbench.active_tab()
            && let Some(browser) = &tab.browser
        {
            return Some(browser.diff.path.clone());
        }
        workbench
            .active_editor()?
            .path()?
            .strip_prefix(self.root(workspace))
            .ok()?
            .to_str()
            .map(str::to_owned)
    }

    pub fn sync_panel(&mut self) {
        if self.panel.revision != self.state.revision {
            self.panel
                .rebuild(self.state.snapshot.as_ref(), self.state.revision);
        }
        if let Some((_, index)) = self.panel.file() {
            self.state.selected = index;
        }
    }

    /// Whether the Source Control panel has a changed file selected.
    pub fn has_selected_file(&mut self) -> bool {
        self.sync_panel();
        self.panel.file().is_some()
    }

    /// Whether the Source Control panel lists any change.
    pub fn has_changes(&self) -> bool {
        !self.state.changed().is_empty()
    }

    pub fn toggle_panel_group(&mut self) {
        self.panel
            .toggle(self.state.snapshot.as_ref(), self.state.revision);
    }

    pub fn handle_event(
        &mut self,
        event: GitEvent,
        ui: &mut Ui<'_>,
        diagnostics: &HashMap<String, Vec<Diagnostic>>,
    ) {
        match event {
            GitEvent::Refreshed {
                generation,
                git,
                timeline,
                message,
            } => {
                if generation > self.generation {
                    return;
                }
                let mut decorations = git.snapshot.as_ref().map_or(Vec::new(), |snapshot| {
                    snapshot
                        .changed_paths
                        .iter()
                        .map(|path| {
                            let label = snapshot.changes.iter().find(|c| &c.path == path).map_or(
                                "*",
                                |c| {
                                    c.worktree
                                        .or(c.index)
                                        .map_or("*", tide_services::GitStatus::label)
                                },
                            );
                            (snapshot.root.join(path), label.to_owned())
                        })
                        .collect::<Vec<_>>()
                });
                if !ui.setting_bool("tui.fileTree.showGitStatus", true) {
                    decorations.clear();
                }
                ui.effects.push(Effect::FileDecorations(decorations));
                // Request generations and synchronous startup revisions are
                // separate clocks. Every accepted snapshot must invalidate
                // cached rows even when its payload revision equals startup.
                let revision = self.state.revision.wrapping_add(1);
                self.state = git;
                self.state.revision = revision;
                self.load_inline_diff(ui);
                self.refresh_diff_views(ui);
                if !self.timeline.file {
                    self.timeline.entries = timeline.into_iter().take(commit_count(ui)).collect();
                }
                // Quiet background snapshots must not replace a user action's
                // save/stage/error confirmation before it can be read.
                if let Some(message) = message {
                    ui.status(message);
                }
            }
            GitEvent::InlineDiffLoaded {
                version,
                document,
                root,
                result,
            } => {
                if let Ok(diff) = result {
                    for tab in ui.workbench.tabs_mut() {
                        if let Some(editor) = tab.editor_mut()
                            && editor.document.same_document(&document)
                            && editor.document.version() == version
                            && !editor.is_dirty()
                        {
                            editor.inline = Some(crate::diff_browser::DiffBrowser::new(
                                root.clone(),
                                diff.clone(),
                            ));
                        }
                    }
                }
            }
            GitEvent::TextLoaded { id, text } => set_diff_text(ui.workbench, id, &text),
            GitEvent::Choices {
                generation,
                action,
                result,
            } => {
                if generation == self.picker_generation
                    && ui.overlay.as_ref().is_some_and(
                        |o| matches!(o.kind(), ModalKind::GitChoice(open) if *open == action),
                    )
                {
                    *ui.overlay = match result {
                        Ok(items) if items.is_empty() && empty_choice(&action).is_some() => {
                            ui.status(empty_choice(&action).unwrap_or_default());
                            None
                        }
                        Ok(items) => Some(modal::git_choice(&action, items)),
                        Err(error) => Some(modal::info("Git failed", &error)),
                    };
                }
            }
            GitEvent::TimelineLoaded { path, entries } => {
                if path == self.timeline.path {
                    self.timeline.entries = entries.into_iter().take(commit_count(ui)).collect();
                    self.timeline.selected = self
                        .timeline
                        .selected
                        .min(self.timeline.entries.len().saturating_sub(1));
                }
            }
            GitEvent::HistoricalFile { path, hash, result } => match result {
                Ok(content) => {
                    let mut editor = EditorState::scratch(
                        &content,
                        path.extension().and_then(|e| e.to_str()).unwrap_or("txt"),
                    );
                    editor.historical = Some((path, hash));
                    editor.document.with_mut(|d| d.set_read_only(true));
                    ui.open_tab(Tab::Editor(editor));
                }
                Err(error) => ui.status(format!("Historical file: {error}")),
            },
            GitEvent::DiffLoaded { id, root, result } => {
                let diagnostics = if ui.setting_bool("tui.diffViewer.showDiagnostics", true) {
                    result
                        .as_ref()
                        .ok()
                        .and_then(|d| path_to_uri(&root.join(&d.path)))
                        .and_then(|uri| diagnostics.get(&uri))
                        .map(|items| diff_diagnostics(items))
                        .unwrap_or_default()
                } else {
                    Vec::new()
                };
                for tab in ui.workbench.tabs_mut() {
                    if let Tab::Diff(tab) = tab
                        && tab.id == id
                    {
                        match &result {
                            Ok(diff) => {
                                tab.text = diff.patch.clone();
                                if let Some(browser) = &mut tab.browser {
                                    browser.update(diff.clone());
                                } else {
                                    tab.browser = Some(crate::diff_browser::DiffBrowser::new(
                                        root.clone(),
                                        diff.clone(),
                                    ));
                                }
                                tab.browser.as_mut().unwrap().diagnostics = diagnostics.clone();
                            }
                            Err(error) => tab.text = format!("Diff failed: {error}"),
                        }
                    }
                }
            }
            GitEvent::HunkSaved {
                document,
                version,
                result,
            } => {
                let success = result.is_ok();
                match result {
                    Ok(()) => {
                        if document.version() == version {
                            document.mark_clean();
                        }
                        self.refresh(ui);
                        ui.status("Edited hunk staged; working file preserved");
                    }
                    Err(error) => ui.status(format!(
                        "Hunk staging failed: {error}; edit buffer preserved"
                    )),
                }
                ui.effects.push(Effect::HunkSaved {
                    document,
                    version,
                    success,
                });
            }
            GitEvent::Next { generation, next } => {
                if generation != self.picker_generation {
                    return;
                }
                match next {
                    GitNext::Status(message) => ui.status(message),
                    // Never replace a dialog the user opened while the check ran.
                    _ if ui.overlay.is_some() => {
                        ui.status("Git: another dialog is open; run the command again");
                    }
                    GitNext::Prompt {
                        action,
                        title,
                        initial,
                    } => *ui.overlay = Some(modal::prompt(action, &title, &initial)),
                    GitNext::Confirm {
                        action,
                        title,
                        message,
                    } => *ui.overlay = Some(modal::confirm(action, &title, &message)),
                }
            }
        }
    }

    /// Handles the Git parts of a modal submission and hands back anything else.
    pub fn submit(&mut self, submitted: Submitted, ui: &mut Ui<'_>) -> Option<Submitted> {
        match submitted {
            Submitted::GitChoice { action, value } => match action.as_str() {
                "stagedFiles" => {
                    let previous = self
                        .state
                        .snapshot
                        .as_ref()
                        .and_then(|s| s.changes.iter().find(|c| c.path == value))
                        .and_then(|c| c.previous_path.clone());
                    self.open_file_diff(value, previous, true, None, ui);
                }
                "switch" => self.run(&["switch", "--", &value], ui),
                "merge" => confirm(
                    ui,
                    vec!["merge".into(), "--".into(), value],
                    "Merge Branch",
                    "Merge the selected branch into the current branch?",
                ),
                "deleteBranch" => confirm(
                    ui,
                    vec!["branch".into(), "-d".into(), "--".into(), value],
                    "Delete Branch",
                    "Delete this merged local branch? Unmerged branches remain protected.",
                ),
                "renameBranch" => {
                    *ui.overlay = Some(modal::prompt(
                        PromptAction::RenameBranch(value),
                        "Rename Branch",
                        "",
                    ));
                }
                "rebase" => {
                    let current = self
                        .state
                        .snapshot
                        .as_ref()
                        .and_then(|snapshot| snapshot.branch.clone())
                        .unwrap_or_else(|| "the current branch".into());
                    confirm(
                        ui,
                        vec!["rebase".into(), value.clone()],
                        "Rebase",
                        &format!(
                            "Rebase {current} onto {value}? Its commits are replayed on top of {value}; a conflict pauses the rebase for you to resolve."
                        ),
                    );
                }
                "cherryPick" => self.run(&["cherry-pick", &value], ui),
                "revert" => self.run(&["revert", "--no-edit", &value], ui),
                "reset" => {
                    let short = crate::display::short_id(&value).to_owned();
                    let items = [
                        ("Soft", "keep the changes staged", "soft"),
                        ("Mixed", "keep the changes, unstaged", "mixed"),
                        ("Hard", "discard every uncommitted change", "hard"),
                    ]
                    .into_iter()
                    .map(|(label, detail, mode)| {
                        ModalItem::new(
                            format!("{label}: reset to {short}"),
                            detail,
                            format!("{mode}\t{value}"),
                        )
                    })
                    .collect();
                    *ui.overlay = Some(modal::git_choice("resetMode", items));
                }
                "resetMode" => {
                    if let Some((mode, commit)) = value.split_once('\t') {
                        let short = crate::display::short_id(commit).to_owned();
                        match mode {
                            "soft" | "mixed" => self.run_as(
                                &["reset", &format!("--{mode}"), commit],
                                Some(format!("Reset ({mode}) to {short}")),
                                ui,
                            ),
                            _ => confirm(
                                ui,
                                vec!["reset".into(), "--hard".into(), commit.into()],
                                "Hard Reset",
                                &format!(
                                    "Reset the branch and working tree to {short}? Every uncommitted change in tracked files is lost; untracked files are kept. This cannot be undone."
                                ),
                            ),
                        }
                    }
                }
                "conflicts" => ui.effects.push(Effect::OpenFile {
                    path: self.root(ui.workspace).join(value),
                    line: None,
                }),
                "stashApply" | "stashPop" | "stashDrop" => {
                    if let Some((reference, oid)) = value.split_once('\t') {
                        let verb = match action.as_str() {
                            "stashApply" => "apply",
                            "stashPop" => "pop",
                            _ => "drop",
                        };
                        confirm(
                            ui,
                            vec![
                                "__stash_verified".into(),
                                verb.into(),
                                reference.into(),
                                oid.into(),
                            ],
                            "Stash Action",
                            "Apply this stash action? Drop removes the saved stash permanently; conflicting application preserves it.",
                        );
                    }
                }
                _ => {}
            },
            Submitted::Prompt(PromptAction::RenameBranch(previous), input) => {
                let name = input.trim();
                if valid_branch_name(name) {
                    self.run(&["branch", "-m", "--", &previous, name], ui);
                } else {
                    ui.status("Invalid branch name");
                }
            }
            Submitted::Confirm(ConfirmAction::GitCommand(args)) => {
                let refs = args.iter().map(String::as_str).collect::<Vec<_>>();
                self.run(&refs, ui);
            }
            Submitted::Branch(branch) => {
                self.run(&["checkout", &branch], ui);
                self.refresh(ui);
            }
            Submitted::Prompt(PromptAction::Commit, message) => self.commit(&message, ui),
            Submitted::Prompt(PromptAction::CreateBranch, input) => {
                let branch = input.trim().to_owned();
                if valid_branch_name(&branch) {
                    self.run(&["switch", "-c", &branch], ui);
                    self.refresh(ui);
                }
            }
            Submitted::Prompt(PromptAction::StashMessage, input) => {
                let message = input.trim().to_owned();
                if message.is_empty() {
                    self.run(&["stash", "push"], ui);
                } else {
                    self.run(&["stash", "push", "-m", &message], ui);
                }
                self.refresh(ui);
            }
            Submitted::Confirm(ConfirmAction::DiscardGitChange(path)) => {
                if ui.documents.is_dirty(&self.root(ui.workspace).join(&path)) {
                    ui.status("Save or close the dirty working buffer before discarding changes");
                } else {
                    self.run(&["__discard", &path], ui);
                }
            }
            Submitted::Confirm(ConfirmAction::DiscardAllGitChanges(paths)) => {
                let root = self.root(ui.workspace);
                if paths
                    .iter()
                    .any(|path| ui.documents.is_dirty(&root.join(path)))
                {
                    ui.status("Save or close dirty working buffers before discarding changes");
                } else {
                    let count = paths.len();
                    let mut args = vec!["restore", "--worktree", "--"];
                    args.extend(paths.iter().map(String::as_str));
                    self.run_as(
                        &args,
                        Some(format!("Discarded changes in {count} file(s)")),
                        ui,
                    );
                }
            }
            Submitted::Prompt(PromptAction::CreateTag, input) => {
                let name = input.trim();
                if valid_branch_name(name) {
                    *ui.overlay = Some(modal::prompt(
                        PromptAction::TagMessage(name.to_owned()),
                        "Tag Message (empty: lightweight tag)",
                        "",
                    ));
                } else {
                    ui.status("Invalid tag name");
                }
            }
            Submitted::Prompt(PromptAction::TagMessage(name), input) => {
                let message = input.trim();
                if message.is_empty() {
                    self.run_as(&["tag", &name], Some(format!("Created tag {name}")), ui);
                } else {
                    self.run_as(
                        &["tag", "-a", &name, "-m", message],
                        Some(format!("Created annotated tag {name}")),
                        ui,
                    );
                }
            }
            Submitted::Prompt(
                PromptAction::CommitAmend {
                    commit,
                    message,
                    upstream,
                },
                input,
            ) => self.confirm_amend(&commit, &message, upstream.as_deref(), &input, ui),
            other => return Some(other),
        }
        None
    }

    pub fn handle_key(&mut self, key: KeyEvent, ui: &mut Ui<'_>) {
        self.sync_panel();
        let file = self.panel.file().is_some();
        match key.code {
            KeyCode::Char('j') | KeyCode::Down => self.panel.move_by(1),
            KeyCode::Char('k') | KeyCode::Up => self.panel.move_by(-1),
            KeyCode::Left | KeyCode::Right | KeyCode::Char(' ') => self.toggle_panel_group(),
            KeyCode::Char('s') if file => self.stage_selected(ui),
            KeyCode::Char('S') => self.stage_all(ui),
            KeyCode::Char('u') if file => self.unstage_selected(ui),
            KeyCode::Char('d') if file => self.discard_selected(ui),
            KeyCode::Char('c') => open_commit_prompt(ui),
            KeyCode::Char('r') => self.refresh(ui),
            KeyCode::Enter if file => self.open_selected_diff(ui),
            KeyCode::Enter => self.toggle_panel_group(),
            KeyCode::Char('o') if file => self.open_selected_file(ui),
            _ => {}
        }
        self.sync_panel();
    }

    pub fn handle_timeline_key(&mut self, key: KeyEvent, ui: &mut Ui<'_>) {
        let count = self.timeline.entries.len();
        if count == 0 {
            return;
        }
        let selected = &mut self.timeline.selected;
        match key.code {
            KeyCode::Char('j') | KeyCode::Down => *selected = (*selected + 1).min(count - 1),
            KeyCode::Char('k') | KeyCode::Up => *selected = selected.saturating_sub(1),
            KeyCode::Home | KeyCode::Char('g') => *selected = 0,
            KeyCode::End | KeyCode::Char('G') => *selected = count - 1,
            KeyCode::PageDown => *selected = (*selected + 10).min(count - 1),
            KeyCode::PageUp => *selected = selected.saturating_sub(10),
            KeyCode::Enter => self.open_selected_commit(ui),
            KeyCode::Char('y') => self.copy_selected_commit_hash(ui),
            KeyCode::Char('o') => self.open_historical_file(ui),
            KeyCode::Char('m') => self.toggle_timeline_mode(ui),
            KeyCode::Char('r') => self.refresh_timeline(ui),
            _ => {}
        }
    }

    /// Keeps a file timeline on the active file; true when it had to reload.
    pub fn follow_active_file(&mut self, ui: &mut Ui<'_>) -> bool {
        if self.timeline.file && self.active_path(ui.workspace, ui.workbench) != self.timeline.path
        {
            self.refresh_timeline(ui);
            return true;
        }
        false
    }

    pub fn selected_commit_hash(&self) -> Option<&str> {
        self.timeline
            .entries
            .get(self.timeline.selected)
            .and_then(|line| line.split_whitespace().next())
    }

    pub fn copy_selected_commit_hash(&mut self, ui: &mut Ui<'_>) {
        if let Some(hash) = self.selected_commit_hash().map(str::to_owned) {
            ui.effects.push(Effect::Copy {
                done: format!("Copied commit {hash}"),
                text: hash,
            });
        }
    }

    pub fn open_selected_commit(&mut self, ui: &mut Ui<'_>) {
        if let Some(hash) = self.selected_commit_hash().map(str::to_owned) {
            self.open_output_tab("Commit", &["show", "--stat", &hash], ui);
        }
    }

    pub fn toggle_timeline_mode(&mut self, ui: &mut Ui<'_>) {
        self.timeline.file = !self.timeline.file;
        self.refresh_timeline(ui);
    }

    pub fn view_timeline_diff(&mut self, ui: &mut Ui<'_>) {
        if let (Some(hash), Some(path)) = (
            self.selected_commit_hash().map(str::to_owned),
            self.timeline
                .path
                .clone()
                .or_else(|| self.active_path(ui.workspace, ui.workbench)),
        ) {
            self.open_file_diff(path, None, false, Some(hash), ui);
        } else {
            self.open_selected_commit(ui);
        }
    }

    pub fn refresh_timeline(&mut self, ui: &mut Ui<'_>) {
        self.timeline.path = if self.timeline.file {
            self.active_path(ui.workspace, ui.workbench)
        } else {
            None
        };
        if self.timeline.file && self.timeline.path.is_none() {
            self.timeline.entries.clear();
            return;
        }
        let root = self.root(ui.workspace);
        let path = self.timeline.path.clone();
        if !self.jobs.submit(move |_| {
            AppEvent::Git(GitEvent::TimelineLoaded {
                path: path.clone(),
                entries: git_timeline_for(&root, path.as_deref()),
            })
        }) {
            ui.status("Git queue full");
        }
        self.timeline.selected = 0;
        self.timeline.scroll = 0;
    }

    pub fn open_historical_file(&mut self, ui: &mut Ui<'_>) {
        let Some(hash) = self.selected_commit_hash().map(str::to_owned) else {
            return;
        };
        let Some(path) = self
            .timeline
            .path
            .clone()
            .or_else(|| self.active_path(ui.workspace, ui.workbench))
        else {
            ui.status("Select a file before opening a historical version");
            return;
        };
        let root = self.root(ui.workspace);
        let job = move |cancel: &Arc<AtomicBool>| {
            let result = (|| -> Result<String> {
                let path = tide_services::git::diff::historical_path(&root, &path, &hash, cancel)?;
                let output = tide_services::git_process::git_cancellable(
                    &root,
                    &[
                        "show",
                        "--no-ext-diff",
                        "--no-textconv",
                        &format!("{hash}:{path}"),
                    ],
                    None,
                    cancel,
                )?;
                anyhow::ensure!(
                    output.success,
                    "{}",
                    String::from_utf8_lossy(&output.stderr)
                );
                anyhow::ensure!(!output.stdout.contains(&0), "Binary historical file");
                Ok(String::from_utf8(output.stdout)?)
            })();
            AppEvent::Git(GitEvent::HistoricalFile {
                path: PathBuf::from(path),
                hash,
                result: result.map_err(|e| format!("{e:#}")),
            })
        };
        if !self.jobs.submit(job) {
            ui.status("Git queue full");
        }
    }

    pub fn refresh(&mut self, ui: &mut Ui<'_>) {
        self.request_refresh(None, ui);
    }

    pub fn request_refresh(&mut self, message: Option<String>, ui: &mut Ui<'_>) {
        self.generation = self.generation.wrapping_add(1);
        let generation = self.generation;
        let root = ui.workspace.to_path_buf();
        let accepted =
            self.jobs.submit(
                move |cancel| match tide_services::git::repository_snapshot_cancellable(
                    &root,
                    cancel.clone(),
                ) {
                    Ok(snapshot) => AppEvent::Git(GitEvent::Refreshed {
                        generation,
                        git: GitState {
                            snapshot: Some(snapshot),
                            selected: 0,
                            revision: generation,
                        },
                        timeline: git_timeline(&root),
                        message,
                    }),
                    Err(error) => AppEvent::Error(format!("Git status: {error:#}")),
                },
            );
        if !accepted {
            ui.status("Git queue full; try again after pending operations finish");
        }
    }

    /// The selected changed path, or `None` with an explanation when it is
    /// not valid UTF-8 and so cannot be passed to a path action.
    pub fn selected_path(&mut self, ui: &mut Ui<'_>) -> Option<String> {
        if self.state.selected_is_lossy() {
            ui.status(
                "Git: this path is not valid UTF-8; use git in a terminal for path actions on it",
            );
            return None;
        }
        self.state.selected_path().map(str::to_owned)
    }

    pub fn stage_selected(&mut self, ui: &mut Ui<'_>) {
        if let Some(path) = self.selected_path(ui) {
            self.run(&["add", "--", &path], ui);
        }
    }

    pub fn stage_all(&mut self, ui: &mut Ui<'_>) {
        self.run(&["add", "-A"], ui);
    }

    pub fn unstage_selected(&mut self, ui: &mut Ui<'_>) {
        if let Some(path) = self.selected_path(ui) {
            self.run(&["reset", "HEAD", "--", &path], ui);
        }
    }

    pub fn unstage_all(&mut self, ui: &mut Ui<'_>) {
        self.run(&["reset", "HEAD"], ui);
        self.refresh(ui);
    }

    pub fn discard_selected(&mut self, ui: &mut Ui<'_>) {
        let Some(path) = self.selected_path(ui) else {
            return;
        };
        *ui.overlay = Some(modal::confirm(
            ConfirmAction::DiscardGitChange(path),
            "Discard Changes",
            "Discard unstaged changes in the selected file?",
        ));
    }

    pub fn open_selected_file(&mut self, ui: &mut Ui<'_>) {
        if let Some(path) = self.selected_path(ui) {
            ui.effects.push(Effect::OpenFile {
                path: self.root(ui.workspace).join(path),
                line: None,
            });
        }
    }

    pub fn commit(&mut self, message: &str, ui: &mut Ui<'_>) {
        if message.trim().is_empty() {
            ui.status("Commit message is empty");
            return;
        }
        self.run(&["commit", "-m", message], ui);
    }

    pub fn run(&mut self, args: &[&str], ui: &mut Ui<'_>) {
        self.run_as(args, None, ui);
    }

    /// Runs `args`, reporting `done` instead of Git's own output when it succeeds. A command
    /// that stops on conflicts leaves them in place and says how to finish.
    pub fn run_as(&mut self, args: &[&str], done: Option<String>, ui: &mut Ui<'_>) {
        let args = args.iter().map(|arg| (*arg).to_owned()).collect::<Vec<_>>();
        self.run_with(ui, move |root, cancel| {
            let refs = args.iter().map(String::as_str).collect::<Vec<_>>();
            match run_git_checked(root, &refs, cancel) {
                Ok(output) if output.success => done.unwrap_or_else(|| git_message(&refs, &output)),
                Ok(output) => stopped_message(root, &refs, cancel)
                    .unwrap_or_else(|| git_message(&refs, &output)),
                Err(error) => format!("Git failed: {error:#}"),
            }
        });
    }

    /// Queues `work`, then refreshes the repository and reports the message it returns.
    fn run_with(
        &mut self,
        ui: &mut Ui<'_>,
        work: impl FnOnce(&Path, &AtomicBool) -> String + Send + 'static,
    ) {
        self.generation = self.generation.wrapping_add(1);
        let generation = self.generation;
        let root = self.root(ui.workspace);
        let accepted = self.jobs.submit(move |cancel| {
            let message = work(&root, cancel);
            match tide_services::git::repository_snapshot_cancellable(&root, cancel.clone()) {
                Ok(snapshot) => AppEvent::Git(GitEvent::Refreshed {
                    generation,
                    git: GitState {
                        snapshot: Some(snapshot),
                        selected: 0,
                        revision: generation,
                    },
                    timeline: git_timeline(&root),
                    message: Some(message),
                }),
                Err(error) => AppEvent::Error(format!("{message}; refresh failed: {error:#}")),
            }
        });
        if !accepted {
            ui.status("Git queue full; operation was not started");
        }
    }

    /// Queues a read-only check whose result decides the dialog or message that follows.
    fn prepare(
        &mut self,
        ui: &mut Ui<'_>,
        check: impl FnOnce(&Path, &AtomicBool) -> GitNext + Send + 'static,
    ) {
        self.picker_generation = self.picker_generation.wrapping_add(1);
        let generation = self.picker_generation;
        let root = self.root(ui.workspace);
        if !self.jobs.submit(move |cancel| {
            AppEvent::Git(GitEvent::Next {
                generation,
                next: check(&root, cancel),
            })
        }) {
            ui.status("Git queue full; try again after pending operations finish");
        }
    }

    fn changes(&self, ui: &mut Ui<'_>) -> Option<&[tide_services::git::GitChange]> {
        let changes = self
            .state
            .snapshot
            .as_ref()
            .map(|snapshot| snapshot.changes.as_slice());
        if changes.is_none() {
            ui.status("Git: no repository");
        }
        changes
    }

    /// Discards the unstaged changes in the active file, after confirmation.
    pub fn discard_active(&mut self, ui: &mut Ui<'_>) {
        let Some(path) = self.active_path(ui.workspace, ui.workbench) else {
            ui.status("Git: open a file to discard its changes");
            return;
        };
        let Some(changes) = self.changes(ui) else {
            return;
        };
        let message = match changes
            .iter()
            .find(|change| change.path == path)
            .and_then(|change| change.worktree)
        {
            None => {
                ui.status(format!("Git: {path} has no unstaged changes"));
                return;
            }
            Some(GitStatus::Untracked) => {
                format!("Delete the untracked file {path}? This cannot be undone.")
            }
            Some(_) => format!("Discard unstaged changes in {path}? Staged changes are kept."),
        };
        *ui.overlay = Some(modal::confirm(
            ConfirmAction::DiscardGitChange(path),
            "Discard Changes",
            &message,
        ));
    }

    /// Discards unstaged changes in every tracked file. Untracked files are left to Clean.
    pub fn confirm_discard_all(&mut self, ui: &mut Ui<'_>) {
        let Some(changes) = self.changes(ui) else {
            return;
        };
        let paths = changes
            .iter()
            .filter(|change| !change.lossy_path)
            .filter_map(|change| match change.worktree? {
                GitStatus::Untracked | GitStatus::Conflict => None,
                // A rename in the working tree restores its original path.
                GitStatus::Renamed => Some(
                    change
                        .previous_path
                        .clone()
                        .unwrap_or_else(|| change.path.clone()),
                ),
                _ => Some(change.path.clone()),
            })
            .collect::<Vec<_>>();
        if paths.is_empty() {
            ui.status("Git: no unstaged changes in tracked files");
            return;
        }
        let message = format!(
            "Discard unstaged changes in {} tracked file(s)? Staged changes and untracked files are kept. This cannot be undone.",
            paths.len()
        );
        *ui.overlay = Some(modal::confirm(
            ConfirmAction::DiscardAllGitChanges(paths),
            "Discard All Changes",
            &message,
        ));
    }

    /// Deletes untracked files, listing them first. Ignored files are never removed.
    pub fn confirm_clean(&mut self, ui: &mut Ui<'_>) {
        let Some(changes) = self.changes(ui) else {
            return;
        };
        let files = changes
            .iter()
            .filter(|change| change.worktree == Some(GitStatus::Untracked) && !change.lossy_path)
            .map(|change| change.path.clone())
            .collect::<Vec<_>>();
        if files.is_empty() {
            ui.status("Git: no untracked files to clean");
            return;
        }
        const SHOWN: usize = 30;
        let mut list = files
            .iter()
            .take(SHOWN)
            .map(|path| format!("  {path}"))
            .collect::<Vec<_>>()
            .join("\n");
        if files.len() > SHOWN {
            list.push_str(&format!("\n  …and {} more", files.len() - SHOWN));
        }
        let message = format!(
            "Delete {} untracked file(s)? Ignored files are kept. This cannot be undone.\n\n{list}",
            files.len()
        );
        let mut args = vec!["clean".to_owned(), "-f".to_owned(), "--".to_owned()];
        args.extend(files);
        confirm(ui, args, "Clean Untracked Files", &message);
    }

    pub fn confirm_push_force(&mut self, ui: &mut Ui<'_>) {
        let branch = self
            .state
            .snapshot
            .as_ref()
            .and_then(|snapshot| snapshot.branch.clone())
            .unwrap_or_else(|| "the current branch".into());
        confirm(
            ui,
            vec!["push".into(), "--force-with-lease".into()],
            "Push (Force with Lease)",
            &format!(
                "Force-push {branch}? The remote branch is replaced with your local history. Git refuses if the remote has commits you have not fetched."
            ),
        );
    }

    /// Opens a picker of conflicted files.
    pub fn open_conflicts(&mut self, ui: &mut Ui<'_>) {
        let Some(changes) = self.changes(ui) else {
            return;
        };
        let items = changes
            .iter()
            .filter(|change| {
                change.index == Some(GitStatus::Conflict)
                    || change.worktree == Some(GitStatus::Conflict)
            })
            .filter(|change| !change.lossy_path)
            .map(|change| ModalItem::new(&change.path, "Conflict", &change.path))
            .collect::<Vec<_>>();
        if items.is_empty() {
            ui.status("Git: no conflicted files");
            return;
        }
        *ui.overlay = Some(modal::git_choice("conflicts", items));
    }

    /// Opens the amend prompt with the last commit's message once it has been read.
    pub fn prepare_amend(&mut self, ui: &mut Ui<'_>) {
        self.prepare(ui, |root, cancel| {
            let git = |args: &[&str]| {
                tide_services::git_process::git_cancellable(root, args, None, cancel)
                    .ok()
                    .filter(|output| output.success)
                    .map(|output| String::from_utf8_lossy(&output.stdout).into_owned())
            };
            let Some(last) = git(&["log", "-1", "--format=%h%x00%B"]) else {
                return GitNext::Status("Git: nothing to amend; this branch has no commits".into());
            };
            let (commit, message) = last.split_once('\0').unwrap_or(("HEAD", &last));
            let message = message.trim_end().to_owned();
            let upstream = git(&[
                "rev-parse",
                "--abbrev-ref",
                "--symbolic-full-name",
                "@{upstream}",
            ])
            .map(|name| name.trim().to_owned())
            .filter(|_| git(&["merge-base", "--is-ancestor", "HEAD", "@{upstream}"]).is_some());
            let (subject, body) = split_message(&message);
            GitNext::Prompt {
                title: if body.is_some() {
                    "Amend Commit (body is kept)".into()
                } else {
                    "Amend Commit".into()
                },
                initial: subject.to_owned(),
                action: PromptAction::CommitAmend {
                    commit: commit.trim().to_owned(),
                    message,
                    upstream,
                },
            }
        });
    }

    fn confirm_amend(
        &mut self,
        commit: &str,
        message: &str,
        upstream: Option<&str>,
        input: &str,
        ui: &mut Ui<'_>,
    ) {
        let subject = input.trim();
        if subject.is_empty() {
            ui.status("Commit message is empty");
            return;
        }
        // The prompt edits the subject; a body the commit had is kept.
        let amended = match split_message(message).1 {
            Some(body) => format!("{subject}\n\n{body}"),
            None => subject.to_owned(),
        };
        let args = if amended == message {
            vec!["commit".into(), "--amend".into(), "--no-edit".into()]
        } else {
            vec!["commit".into(), "--amend".into(), "-m".into(), amended]
        };
        let mut text = format!("Amend {commit}? Staged changes are added to it.");
        if let Some(upstream) = upstream {
            text.push_str(&format!(
                " {commit} is already on {upstream}: amending rewrites a pushed commit, and the next push needs Push (Force with Lease)."
            ));
        }
        confirm(ui, args, "Amend Commit", &text);
    }

    /// Continues the paused operation, or only a rebase when `only_rebase`.
    pub fn continue_operation(&mut self, only_rebase: bool, ui: &mut Ui<'_>) {
        self.run_with(ui, move |root, cancel| {
            let operation = match paused(root, only_rebase, cancel) {
                Ok(Operation::Merge) => {
                    return "Git: a merge is in progress; commit it to finish, or run Abort Merge, Rebase, Cherry-Pick or Revert".into();
                }
                Ok(operation) => operation,
                Err(message) => return message,
            };
            let args = [operation.command(), "--continue"];
            match tide_services::git_process::git_cancellable(root, &args, None, cancel) {
                Ok(output) if output.success => {
                    if operation_in_progress(root, cancel) == Some(operation) {
                        format!("{} continued", operation.name())
                    } else {
                        format!("{} complete", operation.name())
                    }
                }
                Ok(output) => stopped_message(root, &args, cancel)
                    .unwrap_or_else(|| git_message(&args, &output)),
                Err(error) => format!("Git failed: {error:#}"),
            }
        });
    }

    /// Asks to abort the paused operation, or only a rebase when `only_rebase`.
    pub fn prepare_abort(&mut self, only_rebase: bool, ui: &mut Ui<'_>) {
        self.prepare(ui, move |root, cancel| {
            let operation = match paused(root, only_rebase, cancel) {
                Ok(operation) => operation,
                Err(message) => return GitNext::Status(message),
            };
            let name = operation.name();
            GitNext::Confirm {
                action: ConfirmAction::GitCommand(vec![
                    operation.command().into(),
                    "--abort".into(),
                ]),
                title: format!("Abort {name}"),
                message: format!(
                    "Abort the {} and return the branch and working tree to where they were before it started?",
                    name.to_lowercase()
                ),
            }
        });
    }

    pub fn confirm_abort_merge(&mut self, ui: &mut Ui<'_>) {
        confirm(
            ui,
            vec!["merge".into(), "--abort".into()],
            "Abort Merge",
            "Abort the current merge and restore its pre-merge state?",
        );
    }

    pub fn open_commit_prompt(&mut self, ui: &mut Ui<'_>) {
        open_commit_prompt(ui);
    }

    pub fn open_branch_picker(&mut self, ui: &mut Ui<'_>) {
        self.open_choices("switch", ui);
    }

    pub fn open_choices(&mut self, action: &str, ui: &mut Ui<'_>) {
        self.picker_generation = self.picker_generation.wrapping_add(1);
        let generation = self.picker_generation;
        let action = action.to_owned();
        let root = self.root(ui.workspace);
        *ui.overlay = Some(modal::git_choice(&action, Vec::new()));
        ui.status("Loading Git choices…");
        let job = move |cancel: &Arc<AtomicBool>| {
            let stash = action.starts_with("stash");
            let commits = matches!(action.as_str(), "cherryPick" | "revert" | "reset");
            let log = "--format=%H%x09%h %s%x09%an, %ar";
            let args = if stash {
                vec!["stash", "list", "--format=%gd%x09%H%x09%gs"]
            } else if action == "rebase" {
                vec![
                    "for-each-ref",
                    "--format=%(HEAD)%09%(refname:short)%09%(refname)",
                    "refs/heads/",
                    "refs/remotes/",
                ]
            } else if action == "cherryPick" {
                // Commits on other branches that the current branch does not have yet.
                vec![
                    "log",
                    log,
                    "-n",
                    "500",
                    "--branches",
                    "--remotes",
                    "--not",
                    "HEAD",
                ]
            } else if commits {
                vec!["log", log, "-n", "500", "HEAD"]
            } else {
                vec!["for-each-ref", "--format=%(refname:short)", "refs/heads/"]
            };
            let result = (|| -> Result<Vec<ModalItem>> {
                let output =
                    tide_services::git_process::git_cancellable(&root, &args, None, cancel)?;
                if commits
                    && !output.success
                    && String::from_utf8_lossy(&output.stderr).contains("HEAD")
                {
                    // An unborn branch has no history to pick from.
                    return Ok(Vec::new());
                }
                anyhow::ensure!(
                    output.success,
                    "{}",
                    String::from_utf8_lossy(&output.stderr)
                );
                Ok(String::from_utf8(output.stdout)?
                    .lines()
                    .take(10000)
                    .filter_map(|line| {
                        if commits {
                            let mut fields = line.splitn(3, '\t');
                            let hash = fields.next()?;
                            let label = fields.next()?;
                            let detail = fields.next().unwrap_or("");
                            return Some(ModalItem::new(label, detail, hash));
                        }
                        if action == "rebase" {
                            let mut fields = line.splitn(3, '\t');
                            let current = fields.next()? == "*";
                            let name = fields.next()?;
                            let reference = fields.next()?;
                            if current || reference.ends_with("/HEAD") {
                                return None;
                            }
                            let detail = if reference.starts_with("refs/remotes/") {
                                "Remote branch"
                            } else {
                                "Local branch"
                            };
                            return Some(ModalItem::new(name, detail, name));
                        }
                        if stash {
                            let mut fields = line.splitn(3, '\t');
                            let reference = fields.next()?;
                            let oid = fields.next()?;
                            let subject = fields.next().unwrap_or("");
                            Some(ModalItem::new(
                                format!("{reference} {subject}"),
                                oid,
                                format!("{reference}\t{oid}"),
                            ))
                        } else {
                            Some(ModalItem::new(line, "", line))
                        }
                    })
                    .collect())
            })();
            AppEvent::Git(GitEvent::Choices {
                generation,
                action,
                result: result.map_err(|e| format!("{e:#}")),
            })
        };
        if !self.jobs.submit(job) {
            *ui.overlay = Some(modal::info(
                "Git busy",
                "Queue full; retry after pending operations finish",
            ));
        }
    }

    pub fn open_log(&mut self, path: Option<&str>, ui: &mut Ui<'_>) {
        let mut args = vec!["log", "--oneline", "--graph", "--decorate", "-n", "200"];
        if let Some(path) = path {
            args.push("--");
            args.push(path);
        }
        self.open_output_tab("Git Log", &args, ui);
    }

    pub fn open_selected_log(&mut self, ui: &mut Ui<'_>) {
        let path = self.selected_path(ui);
        if path.is_some() || !self.state.selected_is_lossy() {
            self.open_log(path.as_deref(), ui);
        }
    }

    pub fn open_blame(&mut self, ui: &mut Ui<'_>) {
        let Some(path) = self.selected_path(ui) else {
            if !self.state.selected_is_lossy() {
                ui.status("No changed file selected");
            }
            return;
        };
        self.open_output_tab("Git Blame", &["blame", "--", &path], ui);
    }

    pub fn open_output_tab(&mut self, title: &str, args: &[&str], ui: &mut Ui<'_>) {
        let root = self.root(ui.workspace);
        let args = args.iter().map(|a| (*a).to_owned()).collect::<Vec<_>>();
        let mut tab = DiffTab::new(title, "Loading…");
        let id = tab.id;
        if !self.jobs.submit(move |cancel| {
            let refs = args.iter().map(String::as_str).collect::<Vec<_>>();
            let text = match tide_services::git_process::git_cancellable(&root, &refs, None, cancel)
            {
                Ok(output) if output.success => {
                    String::from_utf8_lossy(&output.stdout).into_owned()
                }
                Ok(output) => {
                    format!("Git failed: {}", String::from_utf8_lossy(&output.stderr))
                }
                Err(error) => format!("Git failed: {error:#}"),
            };
            AppEvent::Git(GitEvent::TextLoaded { id, text })
        }) {
            tab.text = "Git queue full; retry opening this view".into();
        }
        ui.open_tab(Tab::Diff(tab));
    }

    pub fn open_staged_diff(&mut self, ui: &mut Ui<'_>) {
        let items = self.state.snapshot.as_ref().map_or(Vec::new(), |s| {
            s.changes
                .iter()
                .filter(|c| c.index.is_some())
                .map(|c| ModalItem::new(&c.path, "Staged", &c.path))
                .collect()
        });
        *ui.overlay = Some(modal::git_choice("stagedFiles", items));
    }

    pub fn open_active_diff(&mut self, ui: &mut Ui<'_>) {
        if let Some(path) = self.active_path(ui.workspace, ui.workbench) {
            self.open_file_diff(path, None, false, None, ui);
        } else {
            ui.status("No file selected");
        }
    }

    pub fn open_selected_diff(&mut self, ui: &mut Ui<'_>) {
        let Some(path) = self.selected_path(ui) else {
            if !self.state.selected_is_lossy() {
                ui.status("No changed file selected");
            }
            return;
        };
        let staged = self
            .panel
            .file()
            .is_some_and(|(group, _)| group == crate::git_panel::Group::Staged);
        let previous = self
            .state
            .snapshot
            .as_ref()
            .and_then(|s| s.changes.iter().find(|c| c.path == path))
            .and_then(|c| c.previous_path.clone());
        self.open_file_diff(path, previous, staged, None, ui);
    }

    pub fn open_file_diff(
        &mut self,
        path: String,
        previous: Option<String>,
        staged: bool,
        historical: Option<String>,
        ui: &mut Ui<'_>,
    ) {
        let root = self.root(ui.workspace);
        let tab = DiffTab::new(format!("diff: {path}"), "Loading diff…");
        let id = tab.id;
        ui.open_tab(Tab::Diff(tab));
        let failure_root = root.clone();
        let job = move |cancel: &Arc<AtomicBool>| {
            AppEvent::Git(GitEvent::DiffLoaded {
                id,
                result: tide_services::git::diff::load(
                    &root,
                    &path,
                    previous.as_deref(),
                    staged,
                    historical.as_deref(),
                    cancel,
                )
                .map_err(|e| format!("{e:#}")),
                root,
            })
        };
        if !self.jobs.submit_with_failure(job, move || {
            AppEvent::Git(GitEvent::DiffLoaded {
                id,
                root: failure_root,
                result: Err("Git worker failed; retry opening this diff".into()),
            })
        }) {
            set_diff_text(ui.workbench, id, "Git queue full; retry opening this diff");
        }
    }

    /// Loads the inline diff for the active clean editor.
    pub fn load_inline_diff(&mut self, ui: &mut Ui<'_>) {
        let Some(path) = self.active_path(ui.workspace, ui.workbench) else {
            return;
        };
        let Some(editor) = ui.workbench.active_editor() else {
            return;
        };
        if editor.is_dirty() || editor.historical.is_some() || editor.hunk_edit.is_some() {
            return;
        }
        let document = editor.document.clone();
        let version = editor.revision();
        let root = self.root(ui.workspace);
        let context =
            tide_core::num::saturating_usize(ui.setting_u64("git.inlineDiff.contextLines", 3));
        let job = move |cancel: &Arc<AtomicBool>| {
            AppEvent::Git(GitEvent::InlineDiffLoaded {
                version,
                document,
                result: tide_services::git::diff::load_with_context(
                    &root, &path, None, false, None, context, cancel,
                )
                .map_err(|e| format!("{e:#}")),
                root,
            })
        };
        self.jobs.submit(job);
    }

    /// Reloads open working-tree diffs when auto-refresh is on.
    pub fn refresh_diff_views(&mut self, ui: &mut Ui<'_>) {
        if !ui.setting_bool("tui.diffViewer.autoRefresh", true) {
            return;
        }
        let views = ui
            .workbench
            .pane_ids()
            .into_iter()
            .filter_map(|id| ui.workbench.pane(id))
            .flat_map(|p| p.tabs.iter())
            .filter_map(|tab| {
                if let Tab::Diff(tab) = tab
                    && let Some(browser) = &tab.browser
                    && browser.diff.historical.is_none()
                {
                    Some((
                        tab.id,
                        browser.root.clone(),
                        browser.diff.path.clone(),
                        browser.diff.previous_path.clone(),
                        browser.diff.staged,
                    ))
                } else {
                    None
                }
            })
            .collect::<Vec<_>>();
        for (id, root, path, previous, staged) in views {
            let job = move |cancel: &Arc<AtomicBool>| {
                AppEvent::Git(GitEvent::DiffLoaded {
                    id,
                    result: tide_services::git::diff::load(
                        &root,
                        &path,
                        previous.as_deref(),
                        staged,
                        None,
                        cancel,
                    )
                    .map_err(|e| format!("{e:#}")),
                    root,
                })
            };
            self.jobs.submit(job);
        }
    }

    pub fn diff_action(&mut self, action: crate::diff_browser::Action, ui: &mut Ui<'_>) {
        use crate::diff_browser::Action;
        let browser = match ui.workbench.active_tab() {
            Some(Tab::Diff(tab)) => tab.browser.as_ref(),
            Some(Tab::Editor(editor)) => editor.inline.as_ref(),
            Some(Tab::Sql(sql)) => sql.editor.inline.as_ref(),
            _ => None,
        };
        let Some(browser) = browser else {
            return;
        };
        let diagnostics = browser.diagnostics.clone();
        let diff = browser.diff.clone();
        let root = browser.root.clone();
        let hunk = browser.hunk();
        let line = browser.line();
        match action {
            Action::Diagnostics => {
                let text = if diagnostics.is_empty() {
                    "No diagnostics for this diff".into()
                } else {
                    diagnostics
                        .iter()
                        .map(|(line, _, message)| format!("{line}: {message}"))
                        .collect::<Vec<_>>()
                        .join("\n")
                };
                *ui.overlay = Some(modal::info("Diff Diagnostics", &text));
            }
            Action::Open => ui.effects.push(Effect::OpenFile {
                path: root.join(&diff.path),
                line: Some(line.unwrap_or(1)),
            }),
            Action::Edit => {
                if !diff.editable {
                    ui.status("Only an unstaged text hunk can be edited");
                    return;
                }
                let Some(hunk) = hunk else {
                    return;
                };
                match tide_services::git::diff::hunk_text(&diff, hunk) {
                    Ok(text) => {
                        let mut editor = EditorState::scratch(
                            &text,
                            Path::new(&diff.path)
                                .extension()
                                .and_then(|e| e.to_str())
                                .unwrap_or("txt"),
                        );
                        editor.hunk_edit = Some(crate::editor::HunkEdit { root, diff, hunk });
                        ui.workbench.insert_tab_focused(Tab::Editor(editor));
                        ui.mark_session_dirty();
                        ui.status("Edit hunk; Save applies it using tui.diffViewer.editMode; Save As keeps a copy");
                    }
                    Err(error) => ui.status(format!("Cannot edit hunk: {error:#}")),
                }
            }
            Action::Stage | Action::Unstage | Action::Discard if diff.historical.is_some() => {
                ui.status("Historical diffs are read-only")
            }
            Action::Stage => self.run(&["add", "--", &diff.path], ui),
            Action::Unstage => self.run(&["reset", "HEAD", "--", &diff.path], ui),
            Action::Discard => {
                *ui.overlay = Some(modal::confirm(
                    ConfirmAction::DiscardGitChange(diff.path.clone()),
                    "Discard Changes",
                    "Discard unstaged changes in this file? Staged content is retained.",
                ))
            }
        }
    }

    /// Saves the active hunk editor by staging it or, in `direct-write` mode, by writing the
    /// working file.
    pub fn save_hunk_edit(&mut self, ui: &mut Ui<'_>) {
        let Some(editor) = ui.workbench.active_editor() else {
            return;
        };
        let Some(edit) = editor.hunk_edit.clone() else {
            return;
        };
        let document = editor.document.clone();
        let version = document.version();
        let replacement = document.text();
        if ui
            .settings
            .get("tui.diffViewer.editMode")
            .and_then(serde_json::Value::as_str)
            == Some("direct-write")
        {
            let path = edit.root.join(&edit.diff.path);
            let result = (|| -> Result<()> {
                let content = tide_services::git::diff::edited_worktree(
                    &edit.root,
                    &edit.diff,
                    edit.hunk,
                    &replacement,
                )?;
                let mut target = ui.documents.open(&path)?;
                anyhow::ensure!(
                    !target.is_dirty() && target.document.text() == edit.diff.new_text,
                    "Working buffer has changed; save or reload it before applying this hunk"
                );
                // One shared undo step; the registry retains this buffer if saving fails.
                let old = target.document.text();
                let prefix = old
                    .chars()
                    .zip(content.chars())
                    .take_while(|(a, b)| a == b)
                    .count();
                let old_len = old.chars().count();
                let new_len = content.chars().count();
                let suffix = old
                    .chars()
                    .rev()
                    .zip(content.chars().rev())
                    .take(old_len.min(new_len).saturating_sub(prefix))
                    .take_while(|(a, b)| a == b)
                    .count();
                let range = target.document.with(|d| {
                    tide_core::Range::new(
                        d.char_to_position(prefix),
                        d.char_to_position(old_len - suffix),
                    )
                });
                let replacement = content
                    .chars()
                    .skip(prefix)
                    .take(new_len - prefix - suffix)
                    .collect::<String>();
                target.document.replace(range, &replacement);
                if let Err(error) = target.save() {
                    ui.workbench.insert_tab_focused(Tab::Editor(target));
                    return Err(error.into());
                }
                Ok(())
            })();
            let success = result.is_ok();
            match result {
                Ok(()) => {
                    document.mark_clean();
                    self.refresh(ui);
                    ui.status("Edited hunk written to working file");
                }
                Err(error) => ui.status(format!(
                    "Hunk save failed: {error:#}; edit buffer preserved"
                )),
            }
            ui.effects.push(Effect::HunkSaved {
                document,
                version,
                success,
            });
            return;
        }
        let pending_document = document.clone();
        if !self.jobs.submit(move |cancel| {
            AppEvent::Git(GitEvent::HunkSaved {
                result: tide_services::git::diff::stage_edited_hunk(
                    &edit.root,
                    &edit.diff,
                    edit.hunk,
                    &replacement,
                    cancel,
                )
                .map_err(|e| format!("{e:#}")),
                document,
                version,
            })
        }) {
            ui.status("Git queue full; edit buffer preserved");
            ui.effects.push(Effect::HunkSaved {
                document: pending_document,
                version,
                success: false,
            });
        }
    }
}

/// Shows the language server's diagnostics in open diff views, when enabled.
pub(crate) fn sync_diff_diagnostics(
    ui: &mut Ui<'_>,
    diagnostics: &HashMap<String, Vec<Diagnostic>>,
) {
    let enabled = ui.setting_bool("tui.diffViewer.showDiagnostics", true);
    for tab in ui.workbench.tabs_mut() {
        if let Tab::Diff(tab) = tab
            && let Some(browser) = &mut tab.browser
        {
            browser.diagnostics = if enabled {
                path_to_uri(&browser.root.join(&browser.diff.path))
                    .and_then(|uri| diagnostics.get(&uri))
                    .map(|items| diff_diagnostics(items))
                    .unwrap_or_default()
            } else {
                Vec::new()
            };
        }
    }
}

fn diff_diagnostics(items: &[Diagnostic]) -> Vec<(usize, u8, String)> {
    items
        .iter()
        .map(|d| {
            (
                d.range.start.line as usize + 1,
                d.severity.unwrap_or(3),
                d.message.clone(),
            )
        })
        .collect()
}

fn set_diff_text(workbench: &mut Workbench, id: u64, text: &str) {
    for tab in workbench.tabs_mut() {
        if let Tab::Diff(tab) = tab
            && tab.id == id
        {
            tab.text = text.to_owned();
        }
    }
}

fn commit_count(ui: &Ui<'_>) -> usize {
    tide_core::num::saturating_usize(ui.setting_u64("tui.timeline.commitCount", 50))
}

fn confirm(ui: &mut Ui<'_>, args: Vec<String>, title: &str, message: &str) {
    *ui.overlay = Some(modal::confirm(
        ConfirmAction::GitCommand(args),
        title,
        message,
    ));
}

fn open_commit_prompt(ui: &mut Ui<'_>) {
    *ui.overlay = Some(modal::prompt(PromptAction::Commit, "Commit", ""));
}

fn git_message(args: &[&str], output: &tide_services::git_process::GitOutput) -> String {
    if output.success {
        let message = String::from_utf8_lossy(&output.stdout).trim().to_owned();
        if message.is_empty() {
            format!("git {} ok", args.first().copied().unwrap_or(""))
        } else {
            message
        }
    } else {
        format!(
            "git failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        )
    }
}

fn git_timeline(root: &Path) -> Vec<String> {
    git_timeline_for(root, None)
}

fn git_timeline_for(root: &Path, path: Option<&str>) -> Vec<String> {
    let mut args = vec![
        "log",
        "--format=%H %ad %an: %s",
        "--date=short",
        "-n",
        "500",
    ];
    if let Some(path) = path {
        args.extend(["--follow", "--", path]);
    }
    tide_services::git_process::git(root, &args, None)
        .ok()
        .filter(|o| o.success)
        .map(|o| {
            String::from_utf8_lossy(&o.stdout)
                .lines()
                .map(str::to_owned)
                .collect()
        })
        .unwrap_or_default()
}

pub(crate) fn valid_branch_name(name: &str) -> bool {
    !name.is_empty()
        && !name.starts_with(['-', '.'])
        && !name.ends_with(['.', '/'])
        && !name.contains("..")
        && !name.contains("@{")
        && !name.ends_with(".lock")
        && !name
            .chars()
            .any(|c| c.is_control() || c.is_whitespace() || "~^:?*[\\".contains(c))
        && !name
            .split('/')
            .any(|s| s.is_empty() || s.starts_with('.') || s.ends_with(".lock"))
}

pub(crate) fn run_git_checked(
    root: &Path,
    args: &[&str],
    cancel: &AtomicBool,
) -> Result<tide_services::git_process::GitOutput> {
    if args.first() == Some(&"__discard") {
        anyhow::ensure!(args.len() == 2, "Invalid discard target");
        tide_services::git::diff::relative_path(args[1])?;
        let tracked = tide_services::git_process::git_cancellable(
            root,
            &["ls-files", "--error-unmatch", "--", args[1]],
            None,
            cancel,
        )?;
        let argv = if tracked.success {
            vec!["restore", "--worktree", "--", args[1]]
        } else {
            vec!["clean", "-f", "--", args[1]]
        };
        return tide_services::git_process::git_cancellable(root, &argv, None, cancel);
    }
    if args.first() == Some(&"__stash_verified") {
        anyhow::ensure!(
            args.len() == 4 && ["apply", "pop", "drop"].contains(&args[1]),
            "Invalid stash action"
        );
        let current = tide_services::git_process::git_cancellable(
            root,
            &["rev-parse", "--verify", args[2]],
            None,
            cancel,
        )?;
        anyhow::ensure!(
            current.success && String::from_utf8_lossy(&current.stdout).trim() == args[3],
            "Stash list changed; select the stash again"
        );
        return tide_services::git_process::git_cancellable(
            root,
            &["stash", args[1], args[2]],
            None,
            cancel,
        );
    }
    tide_services::git_process::git_cancellable(root, args, None, cancel)
}

/// The status message for a choice list that came back empty, for lists that close instead.
fn empty_choice(action: &str) -> Option<&'static str> {
    match action {
        "rebase" => Some("Git: no other branch to rebase onto"),
        "cherryPick" => Some("Git: no commits on other branches that this branch lacks"),
        "revert" | "reset" => Some("Git: this branch has no commits"),
        _ => None,
    }
}

/// A commit message's subject line and, when it has one, its body.
fn split_message(message: &str) -> (&str, Option<&str>) {
    match message.split_once('\n') {
        Some((subject, body)) if !body.trim().is_empty() => (
            subject.trim_end(),
            Some(body.trim_start_matches(['\n', '\r'])),
        ),
        Some((subject, _)) => (subject.trim_end(), None),
        None => (message, None),
    }
}

/// A multi-step operation that is paused in the repository.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Operation {
    Rebase,
    CherryPick,
    Revert,
    Merge,
}

pub(crate) fn operation_in_progress(root: &Path, cancel: &AtomicBool) -> Option<Operation> {
    let markers = [
        ("rebase-merge", Operation::Rebase),
        ("rebase-apply", Operation::Rebase),
        ("CHERRY_PICK_HEAD", Operation::CherryPick),
        ("REVERT_HEAD", Operation::Revert),
        ("MERGE_HEAD", Operation::Merge),
    ];
    let mut args = vec!["rev-parse"];
    for (marker, _) in markers {
        args.extend(["--git-path", marker]);
    }
    let output = tide_services::git_process::git_cancellable(root, &args, None, cancel)
        .ok()
        .filter(|output| output.success)?;
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .zip(markers)
        .find(|(path, _)| root.join(path).exists())
        .map(|(_, (_, operation))| operation)
}

impl Operation {
    fn command(self) -> &'static str {
        match self {
            Self::Rebase => "rebase",
            Self::CherryPick => "cherry-pick",
            Self::Revert => "revert",
            Self::Merge => "merge",
        }
    }

    fn name(self) -> &'static str {
        match self {
            Self::Rebase => "Rebase",
            Self::CherryPick => "Cherry-pick",
            Self::Revert => "Revert",
            Self::Merge => "Merge",
        }
    }
}

const CONTINUE: &str = "Continue Rebase, Cherry-Pick or Revert";
const ABORT: &str = "Abort Merge, Rebase, Cherry-Pick or Revert";

/// The paused operation Continue or Abort acts on, or why there is none.
fn paused(root: &Path, only_rebase: bool, cancel: &AtomicBool) -> Result<Operation, String> {
    match operation_in_progress(root, cancel) {
        None if only_rebase => Err("Git: no rebase in progress".into()),
        None => Err("Git: no merge, rebase, cherry-pick or revert is in progress".into()),
        Some(Operation::Rebase) => Ok(Operation::Rebase),
        Some(other) if only_rebase => Err(format!(
            "Git: no rebase in progress; a {} is, so run {CONTINUE} or {ABORT}",
            other.name().to_lowercase()
        )),
        Some(other) => Ok(other),
    }
}

/// How to finish a command that stopped on conflicts, which stay in the working tree.
fn stopped_message(root: &Path, args: &[&str], cancel: &AtomicBool) -> Option<String> {
    if !matches!(
        args.first(),
        Some(&("pull" | "rebase" | "merge" | "cherry-pick" | "revert"))
    ) {
        return None;
    }
    let operation = operation_in_progress(root, cancel)?;
    let output = tide_services::git_process::git_cancellable(
        root,
        &["diff", "--name-only", "--diff-filter=U", "-z"],
        None,
        cancel,
    )
    .ok()
    .filter(|output| output.success)?;
    let conflicts = output
        .stdout
        .split(|byte| *byte == 0)
        .filter(|path| !path.is_empty())
        .count();
    if conflicts == 0 {
        return None;
    }
    let name = operation.name();
    let finish = if operation == Operation::Merge {
        format!("commit, or run {ABORT}")
    } else {
        format!("run {CONTINUE}, or {ABORT}")
    };
    Some(format!(
        "{name} stopped with conflicts in {conflicts} file(s): resolve and stage them (Open Conflicted File lists them), then {finish}"
    ))
}
