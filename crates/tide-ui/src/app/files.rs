//! Workspace file operations and the save flows, including save-before-close.
use super::*;

impl App {
    pub(super) fn create_workspace_path(&mut self, value: &str, directory: bool) {
        let Ok(workspace) = ToolWorkspace::new(&self.workspace) else {
            self.set_status("File: workspace is unavailable");
            return;
        };
        match workspace.resolve(value) {
            Ok(path) => {
                let result = if directory {
                    std::fs::create_dir_all(&path)
                } else {
                    path.parent()
                        .map_or(Ok(()), std::fs::create_dir_all)
                        .and_then(|()| {
                            std::fs::OpenOptions::new()
                                .write(true)
                                .create_new(true)
                                .open(&path)
                                .map(|_| ())
                        })
                };
                match result {
                    Ok(()) => {
                        self.file_tree.refresh();
                        if !directory {
                            self.open_file(path);
                        }
                        self.set_status(format!(
                            "{} {}",
                            if directory {
                                "Created folder"
                            } else {
                                "Created file"
                            },
                            value
                        ));
                        self.mark_session_dirty();
                    }
                    Err(error) => self.set_status(format!("File: {error}")),
                }
            }
            Err(error) => self.set_status(format!("File: {error}")),
        }
    }

    pub(super) fn rename_selected_path(&mut self, from: &Path, value: &str) {
        let Ok(workspace) = ToolWorkspace::new(&self.workspace) else {
            return;
        };
        let Ok(to) = workspace.resolve(value) else {
            self.set_status("File: target path escapes the workspace");
            return;
        };
        if to.exists() {
            self.overlay = Some(modal::confirm(
                ConfirmAction::RenamePath(from.to_path_buf(), to.clone()),
                "Replace File",
                "Replace the existing destination with the renamed file?",
            ));
            return;
        }
        self.rename_path_confirmed(from, &to);
    }

    pub(super) fn rename_path_confirmed(&mut self, from: &Path, to: &Path) {
        if self.documents.get(to).is_some() {
            self.set_status("File: close the destination tab before replacing it");
            return;
        }
        if let Some(parent) = to.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        match std::fs::rename(from, to) {
            Ok(()) => {
                let mut renamed_paths = Vec::new();
                for tab in self.workbench.tabs_mut() {
                    if let Some(editor) = tab.editor_mut()
                        && let Some(old) = editor.path().map(Path::to_path_buf)
                        && let Ok(suffix) = old.strip_prefix(from)
                    {
                        let new = if suffix.as_os_str().is_empty() {
                            to.to_path_buf()
                        } else {
                            to.join(suffix)
                        };
                        self.documents.forget(&old);
                        renamed_paths.push(old);
                        editor.retarget(new.clone());
                        self.documents.register(new, editor.document.clone());
                    }
                }
                for old in renamed_paths {
                    self.lsp
                        .close_document(&old, &self.settings, &self.workspace);
                }
                self.lsp.origin.document = None;
                self.file_tree.refresh();
                self.set_status(format!("Renamed to {}", to.display()));
                self.mark_session_dirty();
            }
            Err(error) => self.set_status(format!("File: rename failed: {error}")),
        }
    }

    pub(super) fn delete_path(&mut self, path: &Path) {
        let result = if path.is_dir() {
            std::fs::remove_dir_all(path)
        } else {
            std::fs::remove_file(path)
        };
        match result {
            Ok(()) => {
                self.file_tree.refresh();
                self.set_status(format!("Deleted {}", path.display()));
                self.mark_session_dirty();
            }
            Err(error) => self.set_status(format!("File: delete failed: {error}")),
        }
    }

    pub fn open_file(&mut self, path: impl AsRef<Path>) {
        let path = path.as_ref();
        let path = if path.is_absolute() {
            path.to_path_buf()
        } else {
            self.workspace.join(path)
        };
        let path = std::fs::canonicalize(&path).unwrap_or(path);
        if self.workbench.focus_editor_in_focused(&path) {
            self.focus = Focus::Main;
            with_ui!(self, |ui| self.git.load_inline_diff(ui));
            return;
        }
        match self.documents.open(&path) {
            Ok(editor) => {
                let text = editor.document.text();
                let version = tide_core::num::saturating_i64(editor.revision());
                let lossy = editor.document.with(Document::is_lossy);
                self.workbench.insert_tab_focused(Tab::Editor(editor));
                self.focus = Focus::Main;
                self.mark_session_dirty();
                self.set_status(if lossy {
                    format!(
                        "Opened {} read-only: it is not valid UTF-8, so saving could corrupt it",
                        path.display()
                    )
                } else {
                    format!("Opened {}", path.display())
                });
                self.lsp
                    .open_document(&path, text, version, &self.settings, &self.workspace);
                with_ui!(self, |ui| self.git.load_inline_diff(ui));
            }
            Err(error) => self.set_status(format!("Open failed: {error}")),
        }
    }

    pub(super) fn new_file(&mut self) {
        self.workbench
            .insert_tab_focused(Tab::Editor(EditorState::untitled()));
        self.focus = Focus::Main;
        self.mark_session_dirty();
    }

    pub(super) fn action_tabs(&self, action: &ConfirmAction) -> Vec<(PaneId, usize)> {
        let target = match action {
            ConfirmAction::CloseTab { pane, index } => {
                let found = self.pending_close.as_ref().and_then(|(owner, identity)| {
                    self.workbench.pane(*owner).and_then(|p| {
                        p.tabs
                            .iter()
                            .position(|t| identity.matches(t))
                            .map(|i| (*owner, i))
                    })
                });
                if self.pending_close.is_some() {
                    return found.into_iter().collect();
                }
                return found
                    .or_else(|| {
                        self.workbench
                            .pane(*pane)
                            .and_then(|p| p.tabs.get(*index))
                            .map(|_| (*pane, *index))
                    })
                    .into_iter()
                    .collect();
            }
            ConfirmAction::CloseDirty => {
                return vec![(
                    self.workbench.focused(),
                    self.workbench.focused_pane().active,
                )];
            }
            ConfirmAction::ClosePane(pane) => Some(*pane),
            _ => None,
        };
        self.workbench
            .pane_ids()
            .into_iter()
            .filter(|id| target.is_none_or(|target| target == *id))
            .flat_map(|id| {
                (0..self.workbench.pane(id).unwrap().tabs.len()).map(move |index| (id, index))
            })
            .collect()
    }

    pub(super) fn begin_save_before(
        &mut self,
        action: ConfirmAction,
        title: String,
        message: String,
    ) {
        let mut documents: Vec<crate::editor::DocumentHandle> = Vec::new();
        for (pane, index) in self.action_tabs(&action) {
            if let Some(editor) = self
                .workbench
                .pane(pane)
                .and_then(|p| p.tabs.get(index))
                .and_then(Tab::editor)
                && editor.is_dirty()
                && !documents.iter().any(|d| d.same_document(&editor.document))
            {
                documents.push(editor.document.clone());
            }
        }
        let origin = self
            .workbench
            .active_tab()
            .map(|tab| (self.workbench.focused(), TabIdentity::of(tab)));
        self.save_before = Some(SaveBeforeAction {
            action,
            title,
            message,
            documents,
            waiting_hunk: None,
            origin,
            focus: self.focus,
        });
        self.continue_save_before();
    }

    pub(super) fn restore_save_origin(&mut self, request: &SaveBeforeAction) {
        if let Some((pane, identity)) = &request.origin
            && let Some(index) = self
                .workbench
                .pane(*pane)
                .and_then(|p| p.tabs.iter().position(|t| identity.matches(t)))
        {
            self.activate_tab(*pane, index);
        }
        self.focus = request.focus;
    }

    pub(super) fn cancel_save_before(&mut self) {
        if let Some(request) = self.save_before.take() {
            self.restore_save_origin(&request);
            if request.action == ConfirmAction::QuitDirty {
                self.restart = None;
            }
            self.pending_close = None;
            self.set_status("Close canceled; buffers remain open");
        }
    }

    pub(super) fn fail_save_before(&mut self, request: SaveBeforeAction, error: String) {
        self.restore_save_origin(&request);
        self.overlay = Some(modal::confirm(
            request.action,
            &request.title,
            &format!("{error}\n\nBuffers remain open. {}", request.message),
        ));
        self.set_status(error);
    }

    pub(super) fn finish_hunk_save_before(
        &mut self,
        document: &crate::editor::DocumentHandle,
        version: u64,
        success: bool,
    ) {
        if !self.save_before.as_ref().is_some_and(|r| {
            r.waiting_hunk
                .as_ref()
                .is_some_and(|(d, v)| d.same_document(document) && *v == version)
        }) {
            return;
        }
        let mut request = self.save_before.take().unwrap();
        request.waiting_hunk = None;
        self.overlay = None;
        if success && document.version() == version && !document.is_dirty() {
            self.save_before = Some(request);
            self.continue_save_before();
        } else {
            let error = if success {
                "Buffer changed while saving; the newer edit remains open".into()
            } else {
                self.status.clone()
            };
            self.fail_save_before(request, error);
        }
    }

    pub(super) fn continue_save_before(&mut self) {
        let Some(mut request) = self.save_before.take() else {
            return;
        };
        while let Some(document) = request.documents.first() {
            if !document.is_dirty() {
                request.documents.remove(0);
                continue;
            }
            let target = self.workbench.pane_ids().into_iter().find_map(|id| {
                self.workbench.pane(id).and_then(|p| {
                    p.tabs
                        .iter()
                        .position(|t| {
                            t.editor()
                                .is_some_and(|e| e.document.same_document(document))
                        })
                        .map(|index| (id, index))
                })
            });
            let Some((pane, index)) = target else {
                self.fail_save_before(
                    request,
                    "Save canceled: target buffer is no longer open".into(),
                );
                return;
            };
            let editor = self.workbench.pane_mut(pane).unwrap().tabs[index]
                .editor_mut()
                .unwrap();
            if editor.hunk_edit.is_some() {
                let document = editor.document.clone();
                request.waiting_hunk = Some((document.clone(), document.version()));
                self.activate_tab(pane, index);
                self.save_before = Some(request);
                self.overlay = Some(modal::info(
                    "Saving Edited Hunk",
                    "Saving this hunk before closing. Escape keeps the buffer open.",
                ));
                with_ui!(self, |ui| self.git.save_hunk_edit(ui));
                return;
            }
            if editor.path().is_none() {
                self.activate_tab(pane, index);
                self.save_before = Some(request);
                self.open_save_as_prompt();
                return;
            }
            if let Err(error) = editor.save() {
                self.fail_save_before(request, format!("Save failed: {error}"));
                return;
            }
            self.mark_session_dirty();
            request.documents.remove(0);
        }
        self.restore_save_origin(&request);
        if self
            .action_tabs(&request.action)
            .iter()
            .any(|(pane, index)| {
                self.workbench
                    .pane(*pane)
                    .and_then(|p| p.tabs.get(*index))
                    .is_some_and(Tab::is_dirty)
            })
        {
            self.fail_save_before(request,"Database row edits remain. Save them explicitly from Row Details, or choose Discard".into());
            return;
        }
        self.overlay = Some(modal::confirm(
            request.action,
            &request.title,
            &request.message,
        ));
        self.handle_modal_key(KeyEvent::new(KeyCode::Char('n'), KeyModifiers::NONE));
    }

    pub(super) fn save_as_to(&mut self, path: PathBuf, overwrite: bool) {
        let path = std::fs::canonicalize(&path).unwrap_or(path);
        if self.documents.get(&path).is_some()
            && self.workbench.active_editor().and_then(EditorState::path) != Some(path.as_path())
        {
            self.set_status("Save As: destination is already open; choose another path");
            if let Some(form) = self.save_as_form.take() {
                self.overlay = Some(form);
            }
            return;
        }
        if path.exists() && !overwrite {
            self.overlay = Some(modal::confirm(
                ConfirmAction::SaveAs(path.clone()),
                "Replace File",
                &format!(
                    "Replace {} with the contents of this buffer?",
                    path.display()
                ),
            ));
            return;
        }
        let Some(editor) = self.workbench.active_editor_mut() else {
            return;
        };
        let old = editor.path().map(Path::to_path_buf);
        let result = if overwrite {
            editor.save_as_overwrite(path.clone())
        } else {
            editor.save_as(path.clone())
        };
        match result {
            Ok(()) => {
                let handle = editor.document.clone();
                if let Some(old) = old {
                    self.lsp
                        .close_document(&old, &self.settings, &self.workspace);
                    self.documents.forget(&old);
                }
                self.lsp.origin.document = None;
                for tab in self.workbench.tabs_mut() {
                    if let Some(view) = tab.editor_mut()
                        && view.document.same_document(&handle)
                    {
                        view.retarget(path.clone());
                    }
                }
                self.documents.register(path.clone(), handle);
                self.mark_session_dirty();
                self.set_status(format!("Saved {}", path.display()));
                self.save_as_form = None;
                self.continue_save_before();
            }
            Err(error) => {
                self.set_status(format!("Save failed: {error}"));
                if let Some(form) = self.save_as_form.take() {
                    self.overlay = Some(form);
                }
            }
        }
    }

    pub(super) fn save_active(&mut self) {
        if matches!(self.workbench.active_tab(),Some(Tab::Sql(sql)) if sql.results_focused&&sql.details_mode)
        {
            self.with_db(|db, ui| db.save_row(false, ui));
            return;
        }
        if self
            .workbench
            .active_editor()
            .is_some_and(|e| e.hunk_edit.is_some())
        {
            with_ui!(self, |ui| self.git.save_hunk_edit(ui));
            return;
        }
        let Some(editor) = self.workbench.active_editor_mut() else {
            self.set_status("No editor is active");
            return;
        };
        if editor.path().is_none() {
            self.open_save_as_prompt();
            return;
        }
        match editor.save() {
            Ok(()) => {
                let title = editor.title();
                let saved = editor.path().map(Path::to_path_buf);
                let (text, version) = (
                    editor.document.text(),
                    tide_core::num::saturating_i64(editor.revision()),
                );
                self.mark_session_dirty();
                if let Some(path) = saved {
                    self.lsp
                        .document_saved(&path, text, version, &self.settings, &self.workspace);
                }
                self.set_status(format!("Saved {title}"));
                with_ui!(self, |ui| self.lsp.refresh_outline(ui));
                with_ui!(self, |ui| self.git.load_inline_diff(ui));
            }
            Err(error) => self.set_status(format!("Save failed: {error}")),
        }
    }

    pub(super) fn save_all(&mut self) {
        let mut saved = 0;
        let mut errors = Vec::new();
        let mut notifications = Vec::new();
        for tab in self.workbench.tabs_mut() {
            if let Some(editor) = tab.editor_mut()
                && let Some(path) = editor.path().map(Path::to_path_buf)
            {
                match editor.save() {
                    Ok(()) => {
                        saved += 1;
                        notifications.push((
                            path,
                            editor.document.text(),
                            tide_core::num::saturating_i64(editor.revision()),
                        ));
                    }
                    Err(error) => errors.push(format!("{}: {error}", editor.title())),
                }
            }
        }
        for (path, text, version) in notifications {
            self.lsp
                .document_saved(&path, text, version, &self.settings, &self.workspace);
        }
        self.mark_session_dirty();
        if errors.is_empty() {
            self.set_status(format!("Saved {saved} file(s)"));
        } else {
            self.set_status(format!(
                "Saved {saved}; {} failed: {}",
                errors.len(),
                errors.join("; ")
            ));
        }
    }
}

pub(super) fn write_file_atomic(path: &Path, content: &str) -> std::io::Result<()> {
    crate::editor::write_atomic(path, content.as_bytes())
}

pub(super) fn collect_files(root: &Path, limit: usize) -> Vec<PathBuf> {
    let mut files = ignore::WalkBuilder::new(root)
        .hidden(false)
        .git_ignore(true)
        .git_global(false)
        .git_exclude(true)
        .build()
        .flatten()
        .filter(|entry| entry.file_type().is_some_and(|kind| kind.is_file()))
        .map(|entry| entry.into_path())
        .take(limit.clamp(100, 100000))
        .collect::<Vec<_>>();
    files.sort();
    files
}

pub(crate) fn resolve_path(workspace: &Path, value: &str) -> PathBuf {
    let path = PathBuf::from(value);
    if path.is_absolute() {
        path
    } else {
        workspace.join(path)
    }
}
