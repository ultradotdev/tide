//! Opening, submitting and closing modal overlays.
use super::*;

impl App {
    pub(super) fn open_status_history(&mut self) {
        let items = self
            .status_history
            .iter()
            .rev()
            .enumerate()
            .map(|(index, message)| {
                ModalItem::new(truncate_to_width(message, 50), message, index.to_string())
            })
            .collect();
        self.overlay = Some(modal::picker(
            ModalKind::StatusHistory,
            "Status History",
            items,
        ));
    }

    pub(super) fn open_panel_picker(&mut self) {
        let mut items = Vec::new();
        if self.focus != Focus::Sidebar {
            items.extend(
                [
                    ("New File", "file.new"),
                    ("Open File...", "workbench.quickOpen"),
                    ("Terminal", "terminal.newInPane"),
                    ("SQL Editor", "database.newQuery"),
                    ("AI CLI", "agents.newSession"),
                ]
                .into_iter()
                .map(|(title, command)| ModalItem::new(title, "Main pane", command)),
            );
        }
        items.extend(
            [
                ("Explorer", "sidebar.addFileTree"),
                ("Source Control", "sidebar.addGitPanel"),
                ("Outline", "sidebar.addOutline"),
                ("Timeline", "sidebar.addTimeline"),
            ]
            .into_iter()
            .map(|(title, command)| ModalItem::new(title, "Sidebar", command)),
        );
        self.overlay = Some(modal::picker(ModalKind::AddPanel, "Add Panel", items));
    }

    pub(super) fn add_sidebar_section(&mut self, id: SectionId) {
        self.sidebar_visible = true;
        self.accordion.add(id);
        self.focus = Focus::Sidebar;
        self.mark_session_dirty();
    }

    pub(super) fn open_all_tabs(&mut self) {
        let mut items = Vec::new();
        for id in self.workbench.pane_ids() {
            if let Some(pane) = self.workbench.pane(id) {
                for (index, tab) in pane.tabs.iter().enumerate() {
                    items.push(
                        ModalItem::new(
                            tab.title(),
                            format!(
                                "Pane {}{}",
                                id + 1,
                                if tab.is_dirty() { " · unsaved" } else { "" }
                            ),
                            format!("{id}:{index}"),
                        )
                        .current(
                            self.focus == Focus::Main
                                && self.workbench.focused() == id
                                && pane.active == index,
                        ),
                    );
                }
            }
        }
        items.extend(self.dock.tabs.iter().enumerate().map(|(index, tab)| {
            ModalItem::new(
                &tab.title,
                "Bottom terminal",
                format!("{DOCK_PANE}:{index}"),
            )
            .current(self.focus == Focus::Dock && self.dock.active == index)
        }));
        self.overlay = Some(modal::picker(
            ModalKind::AllTabs,
            "Tabs Across Panes",
            items,
        ));
    }

    pub(super) fn open_move_tab_picker(&mut self) {
        let source = if self.focus == Focus::Dock {
            DOCK_PANE
        } else {
            self.workbench.focused()
        };
        self.pending_move = if source == DOCK_PANE {
            self.dock.active().map(|terminal| {
                (
                    source,
                    TabIdentity::Terminal(
                        terminal.id,
                        terminal.cwd.display().to_string(),
                        terminal.ai_provider.clone(),
                        terminal.ai_session_id.clone(),
                    ),
                )
            })
        } else {
            self.workbench
                .active_tab()
                .map(|tab| (source, TabIdentity::of(tab)))
        };
        let terminal =
            source == DOCK_PANE || matches!(self.workbench.active_tab(), Some(Tab::Terminal(_)));
        let mut items = self
            .workbench
            .pane_ids()
            .into_iter()
            .filter(|id| *id != source)
            .map(|id| {
                ModalItem::new(
                    format!("Pane {}", id + 1),
                    "Move active tab",
                    id.to_string(),
                )
            })
            .collect::<Vec<_>>();
        if terminal && source != DOCK_PANE {
            items.push(ModalItem::new(
                "Bottom terminal",
                "Move the running terminal",
                DOCK_PANE.to_string(),
            ));
        }
        self.overlay = Some(modal::picker(ModalKind::MoveTab, "Move Tab to Pane", items));
    }

    /// Clean-up for a modal dismissed without submitting.
    pub(super) fn close_modal(&mut self, overlay: &dyn Modal) {
        match overlay.kind() {
            ModalKind::Info => {
                if self
                    .save_before
                    .as_ref()
                    .is_some_and(|r| r.waiting_hunk.is_some())
                {
                    self.cancel_save_before();
                }
            }
            ModalKind::Confirm(ConfirmAction::SaveAs(_)) if self.save_as_form.is_some() => {
                self.overlay = self.save_as_form.take();
            }
            ModalKind::SaveAs
            | ModalKind::Prompt(PromptAction::SaveAs)
            | ModalKind::Confirm(ConfirmAction::SaveAs(_)) => {
                self.save_as_form = None;
                self.cancel_save_before();
            }
            ModalKind::SearchResults => {
                self.search_cancel
                    .store(true, std::sync::atomic::Ordering::Release);
            }
            ModalKind::Prompt(PromptAction::UnlockConnectionSecrets) => {
                self.with_db(|db, ui| db.restore_pending_form(ui))
            }
            ModalKind::Confirm(ConfirmAction::QuitDirty | ConfirmAction::QuitBusy { .. }) => {
                self.restart = None
            }
            ModalKind::Prompt(PromptAction::Commit) => self.git.keep_draft(overlay.input()),
            ModalKind::MoveTab => self.pending_move = None,
            ModalKind::Confirm(ConfirmAction::CloseTab { .. }) => self.pending_close = None,
            ModalKind::Confirm(ConfirmAction::ReplaceInFiles(_)) => self.pending_replace = None,
            ModalKind::Confirm(ConfirmAction::CloseOtherTabs(_)) => self.pending_keep = None,
            _ => {}
        }
    }

    pub(super) fn submit_modal(&mut self, overlay: Box<dyn Modal>, submitted: Submitted) {
        let Some(submitted) = with_ui!(self, |ui| self.git.submit(submitted, ui)) else {
            return;
        };
        let Some(submitted) = with_ui!(self, |ui| self.lsp.submit(submitted, ui)) else {
            return;
        };
        let Some((submitted, overlay)) = self.with_db(|db, ui| db.submit(submitted, overlay, ui))
        else {
            return;
        };
        let Some(submitted) = self.with_ai(|ai, ui| ai.submit(submitted, ui)) else {
            return;
        };
        match submitted {
            Submitted::Command(command) => self.execute_command(command),
            Submitted::OpenFile(path) => self.open_file(path),
            Submitted::SearchResult { path, line } => {
                // Language-server pickers name files by URI.
                let path = if path.starts_with("file:") {
                    tide_lsp::uri_to_path(&path).unwrap_or_else(|| PathBuf::from(&path))
                } else {
                    PathBuf::from(path)
                };
                self.open_file(path);
                if let Some(line) = line
                    && let Some(editor) = self.workbench.active_editor_mut()
                {
                    editor.goto_line(line);
                }
            }
            Submitted::Theme(theme) => self.select_theme(&theme),
            // The domain controllers have already taken these.
            Submitted::AgentSession { .. }
            | Submitted::AiCli(_)
            | Submitted::Prompt(PromptAction::RenameAgent(..), _)
            | Submitted::Confirm(ConfirmAction::DeleteAiSession { .. })
            | Submitted::RowInsert(_)
            | Submitted::Connection(_)
            | Submitted::DatabaseConnection(_)
            | Submitted::CatalogItem(_)
            | Submitted::HistoryEntry(_)
            | Submitted::HistoryControl { .. }
            | Submitted::RowCell { .. }
            | Submitted::Prompt(
                PromptAction::NewConnection
                | PromptAction::EditConnection(_)
                | PromptAction::ExportResults
                | PromptAction::UnlockSecrets
                | PromptAction::UnlockConnectionSecrets,
                _,
            )
            | Submitted::Confirm(
                ConfirmAction::RunSql(_)
                | ConfirmAction::DeleteRow(_)
                | ConfirmAction::ClearQueryHistory(_)
                | ConfirmAction::DeleteConnection(_)
                | ConfirmAction::ExportResults(_),
            )
            | Submitted::CodeAction(_)
            | Submitted::LspCompletion(_)
            | Submitted::Prompt(PromptAction::Rename | PromptAction::WorkspaceSymbol, _)
            | Submitted::GitChoice { .. }
            | Submitted::Branch(_)
            | Submitted::Prompt(
                PromptAction::Commit
                | PromptAction::CreateBranch
                | PromptAction::RenameBranch(_)
                | PromptAction::StashMessage
                | PromptAction::CreateTag
                | PromptAction::TagMessage(_)
                | PromptAction::CommitAmend { .. },
                _,
            )
            | Submitted::Confirm(
                ConfirmAction::GitCommand(_)
                | ConfirmAction::DiscardGitChange(_)
                | ConfirmAction::DiscardAllGitChanges(_),
            ) => {}
            Submitted::AgentWorkspace(path) => self.associate_agent_workspace(&path),
            Submitted::Session(name) => self.open_named_session(&name),
            Submitted::DeleteSession(name) => self.confirm_delete_session(&name),
            Submitted::Confirm(ConfirmAction::DeleteSession(name)) => self.delete_session(&name),
            Submitted::Setting(key) => {
                let current = self
                    .settings
                    .get(&key)
                    .map_or_else(String::new, ToString::to_string);
                let title = format!("Edit Setting: {key}");
                self.open_prompt(PromptAction::EditSetting(key), &title, &current);
            }
            Submitted::Keybinding(selected) => {
                if let Ok(binding) = serde_json::from_str::<Keybinding>(&selected) {
                    let title = format!(
                        "Edit Keybinding: {}",
                        self.commands
                            .get(&binding.command)
                            .map_or(binding.command.as_str(), |info| info.label.as_str())
                    );
                    self.open_prompt(PromptAction::EditKeybinding(selected), &title, &binding.key);
                }
            }
            // Row cell editors submit as `RowCell`.
            Submitted::Nothing | Submitted::Prompt(PromptAction::EditRowCell(..), _) => {}
            Submitted::CompletionText(text) => self.insert_completion(&text),
            Submitted::SaveAs(path) => {
                self.save_as_form = Some(overlay);
                self.save_as_to(path, false);
            }
            Submitted::Prompt(PromptAction::SaveAs, input) => {
                let path = resolve_path(&self.workspace, &input);
                self.save_as_form = Some(overlay);
                self.save_as_to(path, false);
            }
            Submitted::Prompt(PromptAction::GotoLine, input) => {
                if let Ok(line) = input.parse::<usize>()
                    && let Some(editor) = self.workbench.active_editor_mut()
                {
                    editor.goto_line(line);
                }
            }
            Submitted::Prompt(PromptAction::Find, query) | Submitted::Find(query) => {
                self.last_find_query = query;
                self.find_next();
                self.overlay = Some(overlay);
            }
            Submitted::Prompt(PromptAction::ReplaceFind, input) => {
                let query = input;
                if !query.is_empty() {
                    self.overlay = Some(modal::prompt(
                        PromptAction::ReplaceWith(query),
                        "Replace All: Replacement",
                        "",
                    ));
                }
            }
            Submitted::Prompt(PromptAction::ReplaceCurrentWith(query), input) => {
                if let Some(editor) = self.workbench.active_editor_mut() {
                    match editor.replace_matches(&query, &input, self.search_options, false) {
                        Ok(count) => {
                            if count > 0 {
                                self.mark_session_dirty();
                            }
                            self.set_status(format!("Replaced {count} occurrence(s)"));
                        }
                        Err(error) => self.set_status(format!("Search: {error}")),
                    }
                }
            }
            Submitted::Prompt(PromptAction::ReplaceWith(query), input) => {
                if let Some(editor) = self.workbench.active_editor_mut() {
                    match editor.replace_matches(&query, &input, self.search_options, true) {
                        Ok(count) => {
                            if count > 0 {
                                self.mark_session_dirty();
                            }
                            self.set_status(format!("Replaced {count} occurrence(s)"));
                        }
                        Err(error) => self.set_status(format!("Search: {error}")),
                    }
                }
            }
            Submitted::Prompt(PromptAction::ReplaceInFilesFind, query) => {
                self.prompt_replacement_in_files(query);
            }
            Submitted::Prompt(PromptAction::ReplaceInFilesWith(query), replacement) => {
                self.preview_replace_in_files(query, replacement);
            }
            Submitted::Confirm(ConfirmAction::ReplaceInFiles(generation)) => {
                self.apply_replace_in_files(generation);
            }
            Submitted::Prompt(PromptAction::FindInFiles, input) => {
                let query = input;
                self.spawn_search_in_files(&query);
            }
            Submitted::Prompt(PromptAction::NewFile, input) => {
                let path = input;
                self.create_workspace_path(&path, false);
            }
            Submitted::Prompt(PromptAction::NewFolder, input) => {
                let path = input;
                self.create_workspace_path(&path, true);
            }
            Submitted::Prompt(PromptAction::RenameFile(path), input) => {
                let name = input.trim();
                if name.is_empty() || name == path.file_name().unwrap_or_default() {
                    return;
                }
                let target = path.parent().unwrap_or(&self.workspace).join(name);
                self.rename_selected_path(&path, &target.display().to_string());
            }
            Submitted::Prompt(PromptAction::EditSetting(key), input) => {
                let value = input;
                self.update_setting(&key, &value);
            }
            Submitted::Prompt(PromptAction::EditKeybinding(selected), input) => {
                if let Ok(binding) = serde_json::from_str::<Keybinding>(&selected) {
                    self.update_keybinding(&binding, &input);
                }
            }
            Submitted::Prompt(PromptAction::SaveSessionName, input) => {
                let name = input;
                self.save_session_as(&name);
            }
            Submitted::ActivateTab { pane, index } => self.activate_tab(pane, index),
            Submitted::MoveTab(destination) => self.move_pending_tab(destination),
            Submitted::StatusEntry(detail) => {
                self.overlay = Some(modal::info("Status History", &detail));
            }
            Submitted::Confirm(ConfirmAction::ClosePane(pane)) => self.force_close_pane(pane),
            Submitted::Confirm(ConfirmAction::CloseTab { pane, index }) => {
                if let Some((target, identity)) = self.pending_close.take() {
                    if let Some(index) = self
                        .workbench
                        .pane(target)
                        .and_then(|pane| pane.tabs.iter().position(|tab| identity.matches(tab)))
                    {
                        self.force_close_tab(target, index);
                    } else {
                        self.set_status("That tab has closed");
                    }
                } else {
                    self.force_close_tab(pane, index);
                }
            }
            Submitted::Confirm(ConfirmAction::CloseDirty) => {
                self.force_close_active();
            }
            Submitted::Confirm(ConfirmAction::DeletePath(path)) => {
                self.delete_path(&path);
            }
            Submitted::Confirm(ConfirmAction::ReloadFile) => {
                if let Some(editor) = self.workbench.active_editor_mut() {
                    match editor.reload() {
                        Ok(()) => {
                            self.mark_session_dirty();
                            self.set_status("Reloaded file from disk");
                        }
                        Err(error) => self.set_status(format!("Reload failed: {error}")),
                    }
                }
            }
            Submitted::Confirm(ConfirmAction::OverwriteFile) => {
                if let Some(editor) = self.workbench.active_editor_mut() {
                    match editor.save_force() {
                        Ok(()) => {
                            self.mark_session_dirty();
                            self.set_status("Overwrote file on disk");
                        }
                        Err(error) => self.set_status(format!("Overwrite failed: {error}")),
                    }
                }
            }
            Submitted::Confirm(ConfirmAction::SaveAs(path)) => self.save_as_to(path, true),
            Submitted::Confirm(ConfirmAction::RenamePath(from, to)) => {
                self.rename_path_confirmed(&from, &to)
            }
            Submitted::Confirm(ConfirmAction::CloseAllDirty) => self.force_close_all(),
            Submitted::Confirm(ConfirmAction::CloseOtherTabs(pane)) => {
                self.force_close_others(pane)
            }
            Submitted::Confirm(ConfirmAction::SwitchSession(name)) => {
                self.switch_named_session(&name)
            }
            Submitted::Confirm(ConfirmAction::QuitDirty) => self.quit_unless_busy(),
            Submitted::Confirm(ConfirmAction::QuitBusy { .. }) => self.finish_quit(),
            Submitted::Confirm(ConfirmAction::KillTerminal(serial)) => self.kill_terminal(serial),
            Submitted::Confirm(ConfirmAction::CloseAllTerminals) => self.close_all_terminals(),
        }
    }

    pub(super) fn open_command_palette(&mut self) {
        let commands = crate::catalog::palette_commands(&self.commands)
            .map(|(id, info)| (id.clone(), info.clone()))
            .collect::<Vec<_>>();
        self.overlay = Some(modal::command_palette(command_items(&commands)));
    }

    pub(super) fn open_quick_open(&mut self) {
        self.quick_open_generation = self.quick_open_generation.wrapping_add(1);
        let generation = self.quick_open_generation;
        let sender = self.events_tx.clone();
        let root = self.workspace.clone();
        let limit = saturating_usize(self.setting_u64("tui.filePicker.maxFiles", 10000));
        let excludes = self.file_excludes();
        let mut loading = modal::quick_open(Vec::new());
        loading.base_mut().loading = true;
        loading.base_mut().hints = Some("Scanning workspace files…  Esc cancel".into());
        self.overlay = Some(loading);
        self.set_status("Scanning workspace files...");
        self.jobs.spawn(async move {
            let files = tokio::task::spawn_blocking(move || collect_files(&root, limit, &excludes))
                .await
                .unwrap_or_default();
            let _ = sender.send(AppEvent::QuickOpen { generation, files }).await;
        });
    }

    pub(super) fn open_save_as_prompt(&mut self) {
        if self.refuse_help_save() {
            return;
        }
        let initial = self
            .workbench
            .active_editor()
            .and_then(EditorState::path)
            .map_or_else(
                || {
                    self.workspace
                        .join(
                            if matches!(self.workbench.active_tab(), Some(Tab::Sql(_))) {
                                "query.sql"
                            } else {
                                "untitled.txt"
                            },
                        )
                        .display()
                        .to_string()
                },
                |path| path.display().to_string(),
            );
        self.overlay = Some(modal::save_as(&initial));
    }

    pub(super) fn open_goto_prompt(&mut self) {
        let initial = self
            .workbench
            .active_editor()
            .map_or(1, |editor| editor.document.cursor().position.line + 1)
            .to_string();
        self.overlay = Some(modal::prompt(
            PromptAction::GotoLine,
            "Go to Line",
            &initial,
        ));
    }

    pub(super) fn open_find(&mut self) {
        let mut overlay = modal::find(&self.last_find_query);
        overlay.base_mut().replacement = self.last_replace_text.clone();
        overlay.base_mut().replacement_cursor = self.last_replace_text.chars().count();
        self.search_feedback(overlay.as_mut());
        self.overlay = Some(overlay);
    }

    pub(super) fn open_find_in_files(&mut self) {
        self.overlay = Some(modal::prompt(
            PromptAction::FindInFiles,
            "Find in Files",
            "",
        ));
    }

    pub(super) fn open_settings(&mut self) {
        let items = self
            .settings
            .iter()
            .filter(|(key, _)| crate::preferences::supported(key))
            .map(|(key, value)| ModalItem::new(key.clone(), value.to_string(), key.clone()))
            .collect();
        self.overlay = Some(modal::settings(items));
    }

    pub(super) fn open_keybindings(&mut self) {
        let items = self
            .keybindings
            .iter()
            .filter(|binding| command_supported(&binding.command))
            .map(|binding| {
                // The value names the whole binding: several share a key under different
                // `when` contexts, and editing one must not rebind another.
                ModalItem::new(
                    binding.key.clone(),
                    binding.command.clone(),
                    serde_json::to_string(binding).unwrap_or_default(),
                )
            })
            .collect();
        self.overlay = Some(modal::keybindings(items));
    }

    pub(super) fn open_theme_picker(&mut self) {
        let items = self
            .themes
            .iter()
            .map(|theme| ModalItem::new(theme.name.clone(), theme.id.clone(), theme.id.clone()))
            .collect();
        self.overlay = Some(modal::theme_picker(items));
    }
}
