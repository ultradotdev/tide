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
        if self.focus_in_terminal() {
            self.copy_terminal_selection();
            return;
        }
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
    ///
    /// Following `clipboard.osc52`, the copy also goes to the host terminal with OSC 52: at
    /// once in an SSH session or when set to `always`, and after the system clipboard fails
    /// when set to `auto`.
    pub(super) fn export_clipboard(&mut self, done: Option<&str>, fallback: Option<&'static str>) {
        use crate::clipboard::Osc52;
        let commands = self.clipboard_commands;
        let text = self.clipboard.clone();
        let done = done.map(str::to_owned);
        let mode = Osc52::from_setting(
            self.settings
                .get("clipboard.osc52")
                .and_then(serde_json::Value::as_str),
        );
        let sent = (mode == Osc52::Always || mode == Osc52::Auto && self.clipboard_host.remote)
            && self.send_osc52(&text);
        let retry = mode == Osc52::Auto && !sent;
        self.run_clipboard(move || {
            let result = crate::clipboard::write(commands, &text);
            AppEvent::ClipboardWritten {
                osc52: (retry && result.is_err()).then_some(text),
                sent,
                result,
                done,
                fallback,
            }
        });
    }

    /// Queues `text` for the host terminal's clipboard; false when it is too large.
    pub(super) fn send_osc52(&mut self, text: &str) -> bool {
        match crate::clipboard::osc52(text, self.clipboard_host) {
            Ok(bytes) => {
                self.host_output.extend_from_slice(&bytes);
                true
            }
            Err(_) => false,
        }
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
        let excludes = self.file_excludes();
        let mut loading = modal::search_results(Vec::new());
        loading.base_mut().hints = Some("Searching workspace…  Esc cancel".into());
        self.overlay = Some(loading);
        self.set_status(format!("Searching for {query}..."));
        self.jobs.spawn(async move {
            let items = tokio::task::spawn_blocking(move || {
                crate::workspace_search::search_excluding(&root, &search_query, &cancel, &excludes)
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

    /// Runs a line or selection edit on the active editor, with the tab size, and shows
    /// the status it returns. Read-only buffers, such as help pages, refuse with a status.
    pub(super) fn edit_lines(
        &mut self,
        edit: impl FnOnce(&mut EditorState, usize) -> Option<String>,
    ) {
        let tab_size = self.tab_size();
        let insert_spaces = self.setting_bool("editor.insertSpaces", true);
        let relative = self
            .settings
            .get("editor.lineNumbers")
            .and_then(serde_json::Value::as_str)
            == Some("relative");
        let Some(editor) = self.workbench.active_editor_mut() else {
            self.set_status("Open a file to edit its text");
            return;
        };
        if editor.is_read_only() {
            let title = editor
                .help
                .map_or_else(|| editor.title(), |page| page.title.to_owned());
            self.set_status(format!("{title} is read-only"));
            return;
        }
        editor.configure_editing(tab_size, insert_spaces, relative);
        let revision = editor.revision();
        let status = edit(editor, tab_size);
        if editor.revision() != revision {
            self.mark_session_dirty();
            self.lsp.invalidate();
        }
        if let Some(status) = status {
            self.set_status(status);
        }
    }

    pub(super) fn comment_lines(&mut self, mode: CommentMode) {
        self.edit_lines(|editor, _| {
            let marker = crate::editor::line_comment(editor.language_extension());
            editor.comment_lines(marker, mode);
            None
        });
    }

    pub(super) fn toggle_block_comment(&mut self) {
        self.edit_lines(|editor, _| {
            let language = editor.language_extension().to_owned();
            let Some((open, close)) = crate::editor::block_comment(&language) else {
                return Some(format!(
                    "{} has no block comments; use {}",
                    if language.is_empty() {
                        "This file".to_owned()
                    } else {
                        format!(".{language}")
                    },
                    "Toggle Line Comment"
                ));
            };
            editor.toggle_block_comment(open, close);
            None
        });
    }
}
