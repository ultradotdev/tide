//! Editor actions that span the app: clipboard, find navigation, search and line edits.
use super::*;

impl App {
    pub(super) fn insert_completion(&mut self, text: &str) {
        if let Some(editor) = self.workbench.active_editor_mut() {
            editor.insert_text(text);
            self.mark_session_dirty();
            return;
        }
        if let Some(Tab::Sql(tab)) = self.workbench.active_tab_mut() {
            tab.editor.insert_text(text);
            self.mark_session_dirty();
        }
    }

    pub(super) fn copy_selection(&mut self) {
        if let Some(Tab::Sql(sql)) = self.workbench.active_tab()
            && sql.results_focused
        {
            let value = sql
                .result
                .as_ref()
                .and_then(|r| r.fields.get(sql.selected_column))
                .and_then(|f| {
                    sql.row_edits
                        .get(&f.name)
                        .or_else(|| sql.selected_record().and_then(|row| row.get(&f.name)))
                });
            if let Some(value) = value {
                self.clipboard = value
                    .as_str()
                    .map(str::to_owned)
                    .unwrap_or_else(|| value.to_string());
                self.export_clipboard(Some("Copied result value"), Some("Copied inside Tide"));
            }
            return;
        }
        if let Some(editor) = self.workbench.active_editor()
            && let Some(text) = editor.selected_text()
        {
            self.clipboard = text;
            self.export_clipboard(Some("Copied selection"), Some("Copied inside Tide"));
        }
    }

    pub(super) fn cut_selection(&mut self) {
        if matches!(self.workbench.active_tab(),Some(Tab::Sql(sql)) if sql.results_focused) {
            self.copy_selection();
            return;
        }
        let text = self
            .workbench
            .active_editor()
            .and_then(EditorState::selected_text);
        if let Some(text) = text {
            self.clipboard = text;
            if let Some(editor) = self.workbench.active_editor_mut() {
                editor.delete_selection();
            }
            self.mark_session_dirty();
            self.export_clipboard(Some("Cut selection"), Some("Cut inside Tide"));
        }
    }

    pub(super) fn paste_clipboard(&mut self) {
        self.request_paste(self.overlay.as_ref().map(|overlay| overlay.kind().clone()));
    }

    /// Reads the system clipboard off the UI thread and pastes into `target`
    /// (an overlay kind, or `None` for the main area) if it is still there;
    /// the internal clipboard is the fallback.
    pub(super) fn request_paste(&mut self, target: Option<ModalKind>) {
        self.paste_generation = self.paste_generation.wrapping_add(1);
        let generation = self.paste_generation;
        let commands = self.clipboard_commands;
        self.run_clipboard(move || AppEvent::ClipboardRead {
            generation,
            target,
            result: crate::clipboard::read(commands),
        });
    }

    /// Copies the internal clipboard to the system clipboard off the UI
    /// thread, reporting `done` on success and "`fallback`; error" on failure.
    pub(super) fn export_clipboard(&mut self, done: Option<&str>, fallback: Option<&'static str>) {
        let commands = self.clipboard_commands;
        let text = self.clipboard.clone();
        let done = done.map(str::to_owned);
        self.run_clipboard(move || AppEvent::ClipboardWritten {
            result: crate::clipboard::write(commands, &text),
            done,
            fallback,
        });
    }

    /// Queues clipboard work so a paste never overtakes an earlier copy.
    pub(super) fn run_clipboard(&mut self, job: impl FnOnce() -> AppEvent + Send + 'static) {
        let events = &self.events_tx;
        let jobs = self.clipboard_jobs.get_or_insert_with(|| {
            let (jobs, queue) = std::sync::mpsc::channel::<(ClipboardJob, Tracked)>();
            let events = events.clone();
            let _ = std::thread::Builder::new()
                .name("tide-clipboard".into())
                .spawn(move || {
                    for (job, _tracked) in queue {
                        if events.blocking_send(job()).is_err() {
                            break;
                        }
                    }
                });
            jobs
        });
        if let Err(std::sync::mpsc::SendError((job, _))) =
            jobs.send((Box::new(job), self.jobs.track()))
        {
            self.clipboard_jobs = None;
            self.handle_background(job());
        }
    }

    pub(super) fn find_next(&mut self) {
        self.navigate_match(true);
    }

    pub(super) fn find_previous(&mut self) {
        self.navigate_match(false);
    }

    pub(super) fn navigate_match(&mut self, forward: bool) {
        if self.last_find_query.is_empty() {
            self.open_find();
            return;
        }
        let tab_size = self.tab_size();
        if let Some(editor) = self.workbench.active_editor_mut() {
            let cursor = editor.document.cursor();
            let from = if forward {
                cursor.range().map_or(
                    Position::new(cursor.position.line, cursor.position.column + 1),
                    |range| {
                        if range.is_empty() {
                            Position::new(range.end.line, range.end.column + 1)
                        } else {
                            range.end
                        }
                    },
                )
            } else {
                cursor.range().map_or(cursor.position, |range| range.start)
            };
            match editor.find_range(&self.last_find_query, from, forward, self.search_options) {
                Ok(Some(range)) => {
                    editor.document.set_selection(tide_core::Selection {
                        anchor: range.start,
                        active: range.end,
                    });
                    editor.follow_cursor();
                    editor.ensure_visible(20, 80, tab_size);
                    self.set_status(format!(
                        "Found at {}:{}",
                        range.start.line + 1,
                        range.start.column + 1
                    ));
                }
                Ok(None) => self.set_status("No matches"),
                Err(error) => self.set_status(format!("Search: {error}")),
            }
        }
    }

    pub(super) fn spawn_search_in_files(&mut self, query: &str) {
        if query.len() > 4096 {
            self.set_status("Search query exceeds 4096 bytes");
            return;
        }
        if query.trim().is_empty() {
            self.set_status("Find in files needs a query");
            return;
        }
        self.search_cancel
            .store(true, std::sync::atomic::Ordering::Release);
        self.search_cancel = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let cancel = self.search_cancel.clone();
        self.search_generation = self.search_generation.wrapping_add(1);
        let generation = self.search_generation;
        let sender = self.events_tx.clone();
        let root = self.workspace.clone();
        let query = query.to_owned();
        let search_query = query.clone();
        let mut loading = modal::search_results(Vec::new());
        loading.base_mut().hints = Some("Searching workspace…  Esc cancel".into());
        self.overlay = Some(loading);
        self.set_status(format!("Searching for {query}..."));
        self.jobs.spawn(async move {
            let items = tokio::task::spawn_blocking(move || {
                crate::workspace_search::search(&root, &search_query, &cancel)
            })
            .await
            .unwrap_or_default();
            let _ = sender
                .send(AppEvent::SearchResults {
                    generation,
                    query,
                    items,
                })
                .await;
        });
    }

    pub(super) fn toggle_comment(&mut self) {
        let Some(editor) = self.workbench.active_editor_mut() else {
            return;
        };
        let extension = editor.language_extension().to_owned();
        let prefix = comment_prefix(&extension);
        let (start_line, end_line) = editor.document.selection_positions().map_or_else(
            || {
                let line = editor.document.cursor().position.line;
                (line, line)
            },
            |(start, end)| (start.line, end.line),
        );
        let mut all_commented = true;
        for line in start_line..=end_line {
            let text = editor.document.line(line).unwrap_or_default();
            if !text.trim().is_empty() && !text.trim_start().starts_with(prefix) {
                all_commented = false;
                break;
            }
        }
        for line in (start_line..=end_line).rev() {
            let text = editor.document.line(line).unwrap_or_default();
            if text.trim().is_empty() {
                continue;
            }
            let indent = &text[..text.len() - text.trim_start().len()];
            if all_commented {
                let trimmed = text.trim_start();
                if let Some(rest) = trimmed.strip_prefix(prefix) {
                    let rest = rest.strip_prefix(' ').unwrap_or(rest);
                    let new_text = format!("{indent}{rest}");
                    editor.document.replace(
                        tide_core::Range::new(
                            Position::new(line, 0),
                            Position::new(line, text.chars().count()),
                        ),
                        &new_text,
                    );
                }
            } else {
                let new_text = format!("{indent}{prefix} {}", &text[indent.len()..]);
                editor.document.replace(
                    tide_core::Range::new(
                        Position::new(line, 0),
                        Position::new(line, text.chars().count()),
                    ),
                    &new_text,
                );
            }
        }
        self.mark_session_dirty();
    }

    pub(super) fn duplicate_line(&mut self) {
        let Some(editor) = self.workbench.active_editor_mut() else {
            return;
        };
        let line = editor.document.cursor().position.line;
        let text = editor.document.line(line).unwrap_or_default();
        let column = editor.document.line_len_chars(line);
        editor
            .document
            .insert(Position::new(line, column), &format!("\n{text}"));
        editor.document.set_cursor(Position::new(line + 1, 0));
        self.mark_session_dirty();
    }
}

pub(super) fn comment_prefix(extension: &str) -> &'static str {
    match extension {
        "py" | "sh" | "bash" | "zsh" | "yaml" | "yml" | "toml" | "rb" | "r" => "#",
        "sql" | "lua" | "hs" => "--",
        _ => "//",
    }
}
