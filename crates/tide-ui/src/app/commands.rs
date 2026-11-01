//! Workbench command dispatch.
use super::*;

impl App {
    pub(super) fn execute_command(&mut self, command: Command) {
        self.lsp.pending = None;
        if command.as_str().starts_with("edit.")
            && !matches!(
                command,
                Command::EditCopy | Command::EditCut | Command::EditPaste
            )
            && matches!(self.workbench.active_tab(),Some(Tab::Sql(sql)) if sql.results_focused)
        {
            self.set_status("Focus the SQL editor to edit the query");
            return;
        }
        match command {
            Command::FileSave => self.save_active(),
            Command::FileSaveAs => self.open_save_as_prompt(),
            Command::FileSaveAll => self.save_all(),
            Command::FileReload => {
                if self.workbench.active_editor().is_some() {
                    self.overlay = Some(modal::confirm(
                        ConfirmAction::ReloadFile,
                        "Reload File",
                        "Discard buffer changes and reload the file from disk?",
                    ));
                }
            }
            Command::FileOverwrite => {
                if self.workbench.active_editor().is_some() {
                    self.overlay = Some(modal::confirm(
                        ConfirmAction::OverwriteFile,
                        "Overwrite File",
                        "Overwrite the file on disk with this buffer?",
                    ));
                }
            }
            Command::FileOpen => self.open_quick_open(),
            Command::FileNew => self.new_file(),
            Command::FileClose => self.close_active(),
            Command::FileCloseAll => self.close_all(),
            Command::EditUndo => {
                if let Some(editor) = self.workbench.active_editor_mut() {
                    editor.undo();
                    self.mark_session_dirty();
                }
            }
            Command::EditRedo => {
                if let Some(editor) = self.workbench.active_editor_mut() {
                    editor.redo();
                    self.mark_session_dirty();
                }
            }
            Command::EditCopy => self.copy_selection(),
            Command::EditCut => self.cut_selection(),
            Command::EditPaste => self.paste_clipboard(),
            Command::EditSelectAll => self.with_active_editor(EditorState::select_all),
            Command::WorkbenchFocusWorkspaceSidebar => {
                self.focus = Focus::Sidebar;
                self.accordion.leave_content();
            }
            Command::WorkbenchFocusMainRegion => self.focus = Focus::Main,
            Command::WorkbenchFocusTerminal => self.focus_terminal(),
            Command::WorkbenchFocusAgentsSidebar => {
                self.ai.agents_visible = true;
                self.focus = Focus::Agents;
            }
            Command::TerminalInterrupt => {
                if let Some(terminal) = self.active_terminal_mut() {
                    terminal.send_key(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL));
                }
            }
            Command::WorkbenchRestart | Command::WorkbenchRestartAndRebuild => {
                self.restart_tide(command == Command::WorkbenchRestartAndRebuild)
            }
            Command::ThemeNext => self.cycle_theme(1, false),
            Command::ThemePrevious => self.cycle_theme(-1, false),
            Command::ThemeToggleDarkMode => self.cycle_theme(1, true),
            Command::WorkbenchFocusBottomDock => {
                self.dock.visible = true;
                self.focus = Focus::Dock;
            }
            Command::WorkbenchToggleWordWrap => self.toggle_word_wrap(),
            Command::WorkbenchResetRegionSizes => self.reset_region_sizes(),
            Command::WorkbenchCommandPalette => self.open_command_palette(),
            Command::WorkbenchQuickOpen => self.open_quick_open(),
            Command::WorkbenchToggleSidebar | Command::ViewToggleSidebar => self.toggle_sidebar(),
            Command::WorkbenchToggleTerminal | Command::ViewToggleTerminal => {
                self.toggle_terminal()
            }
            Command::WorkbenchFocusNextRegion => self.focus_next_region(false),
            Command::WorkbenchFocusPreviousRegion => self.focus_next_region(true),
            Command::WorkbenchFocusNextPane => {
                self.focus = Focus::Main;
                self.workbench.focus_next();
            }
            Command::WorkbenchFocusPreviousPane => {
                self.focus = Focus::Main;
                self.workbench.focus_previous();
            }
            Command::WorkbenchFocusPaneLeft => self.focus_spatial(FocusDirection::Left),
            Command::WorkbenchFocusPaneRight => self.focus_spatial(FocusDirection::Right),
            Command::WorkbenchFocusPaneUp => self.focus_spatial(FocusDirection::Up),
            Command::WorkbenchFocusPaneDown => self.focus_spatial(FocusDirection::Down),
            Command::WorkbenchFocusEditor1 => self.focus_pane(0),
            Command::WorkbenchFocusEditor2 => self.focus_pane(1),
            Command::WorkbenchFocusEditor3 => self.focus_pane(2),
            Command::ViewSplitVertical => self.split(SplitDirection::Vertical),
            Command::ViewSplitHorizontal => self.split(SplitDirection::Horizontal),
            Command::PaneAddPanel => self.open_panel_picker(),
            Command::ViewClosePane => self.close_pane(self.workbench.focused()),
            Command::EditorSwitchTab => self.open_tabs(if self.focus == Focus::Dock {
                DOCK_PANE
            } else {
                self.workbench.focused()
            }),
            Command::EditorSwitchTabAllPanes => self.open_all_tabs(),
            Command::EditorMoveTabToPane => self.open_move_tab_picker(),
            Command::EditorMoveTabToNextPane => self.move_relative_tab(true),
            Command::EditorMoveTabToPreviousPane => self.move_relative_tab(false),
            Command::WorkbenchMoveSidebarLeft => {
                self.sidebar_left = true;
                self.mark_session_dirty();
            }
            Command::WorkbenchMoveSidebarRight => {
                self.sidebar_left = false;
                self.mark_session_dirty();
            }
            Command::WorkbenchMoveAgentsSidebarLeft => {
                self.agents_left = true;
                self.mark_session_dirty();
            }
            Command::WorkbenchMoveAgentsSidebarRight => {
                self.agents_left = false;
                self.mark_session_dirty();
            }
            Command::WorkbenchResizeLeft => self.resize_region(FocusDirection::Left),
            Command::WorkbenchResizeRight => self.resize_region(FocusDirection::Right),
            Command::WorkbenchResizeUp => self.resize_region(FocusDirection::Up),
            Command::WorkbenchResizeDown => self.resize_region(FocusDirection::Down),
            Command::WorkbenchStatusHistory => self.open_status_history(),
            Command::WorkbenchDocumentInfo => self.show_document_info(),
            Command::WorkbenchSidebarMenu | Command::WorkbenchAgentsMenu => {
                self.open_sidebar_menu(command == Command::WorkbenchAgentsMenu)
            }
            Command::SidebarAddFileTree => self.add_sidebar_section(SectionId::Explorer),
            Command::SidebarAddGitPanel => self.add_sidebar_section(SectionId::SourceControl),
            Command::SidebarAddOutline => self.add_sidebar_section(SectionId::Outline),
            Command::SidebarAddTimeline => self.add_sidebar_section(SectionId::Timeline),
            Command::EditorNextTab => self.workbench.next_tab(),
            Command::EditorPreviousTab => self.workbench.previous_tab(),
            Command::EditorGotoLine => self.open_goto_prompt(),
            Command::LspTriggerCompletion => self.lsp_request(LspRequestKind::Completion),
            Command::LspShowHover => self.lsp_request(LspRequestKind::Hover),
            Command::LspGoToDefinition => self.lsp_request(LspRequestKind::Definition),
            Command::LspTriggerSignatureHelp => self.lsp_request(LspRequestKind::SignatureHelp),
            Command::LspFindReferences => self.lsp_request(LspRequestKind::References),
            Command::LspRename => self.open_prompt(PromptAction::Rename, "Rename Symbol", ""),
            Command::LspCodeActions => self.lsp_request(LspRequestKind::CodeActions),
            Command::EditorFormatDocument => self.lsp_request(LspRequestKind::Format),
            Command::LspNextDiagnostic => {
                with_ui!(self, |ui| self.lsp.goto_adjacent_diagnostic(true, ui))
            }
            Command::LspPreviousDiagnostic => {
                with_ui!(self, |ui| self.lsp.goto_adjacent_diagnostic(false, ui))
            }
            Command::EditorGotoSymbol => self.lsp_request(LspRequestKind::DocumentSymbols),
            Command::EditorGotoWorkspaceSymbol => {
                self.open_prompt(PromptAction::WorkspaceSymbol, "Workspace Symbol", "")
            }
            Command::LspShowDiagnostics => with_ui!(self, |ui| self.lsp.show_diagnostics(ui)),
            Command::SearchFind => self.open_find(),
            Command::SearchFindNext => self.find_next(),
            Command::SearchReplace => self.open_replace(),
            Command::SearchFindInFiles => self.open_find_in_files(),
            Command::WorkbenchSelectTheme => self.open_theme_picker(),
            Command::WorkbenchOpenSettings => self.open_settings(),
            Command::WorkbenchOpenKeybindings => self.open_keybindings(),
            Command::EditorCommentLine => self.toggle_comment(),
            Command::EditorUncommentLine => self.toggle_comment(),
            Command::EditorDuplicateLine | Command::EditorDuplicateSelection => {
                self.duplicate_line()
            }
            Command::GitFocusPanel => {
                self.focus_sidebar_section(1, true);
                with_ui!(self, |ui| self.git.refresh(ui));
            }
            Command::WorkbenchFocusExplorer => self.focus_sidebar_section(0, false),
            Command::WorkbenchFocusOutline => self.focus_sidebar_section(2, false),
            Command::WorkbenchFocusTimeline => self.focus_sidebar_section(3, false),
            Command::GitRefresh => with_ui!(self, |ui| self
                .git
                .request_refresh(Some("Git status refreshed".into()), ui)),
            Command::GitStage => with_ui!(self, |ui| self.git.stage_selected(ui)),
            Command::GitStageAll => with_ui!(self, |ui| self.git.stage_all(ui)),
            Command::GitUnstage => with_ui!(self, |ui| self.git.unstage_selected(ui)),
            Command::GitCommit => with_ui!(self, |ui| self.git.open_commit_prompt(ui)),
            Command::GitViewChanges => with_ui!(self, |ui| self.git.open_selected_diff(ui)),
            Command::GitDiffStaged | Command::GitViewStagedChanges => {
                with_ui!(self, |ui| self.git.open_staged_diff(ui))
            }
            Command::GitLog => with_ui!(self, |ui| self.git.open_log(None, ui)),
            Command::GitLogFile => with_ui!(self, |ui| self.git.open_selected_log(ui)),
            Command::GitBlame => with_ui!(self, |ui| self.git.open_blame(ui)),
            Command::GitBranches | Command::GitCheckout | Command::GitSwitchBranch => {
                with_ui!(self, |ui| self.git.open_branch_picker(ui))
            }
            Command::GitCreateBranch => {
                self.open_prompt(PromptAction::CreateBranch, "Create Branch", "")
            }
            Command::GitStash => self.open_prompt(PromptAction::StashMessage, "Stash Changes", ""),
            Command::GitStashApply => with_ui!(self, |ui| self.git.open_choices("stashApply", ui)),
            Command::GitStashPop => with_ui!(self, |ui| self.git.open_choices("stashPop", ui)),
            Command::GitStashDrop => with_ui!(self, |ui| self.git.open_choices("stashDrop", ui)),
            Command::GitDeleteBranch => {
                with_ui!(self, |ui| self.git.open_choices("deleteBranch", ui))
            }
            Command::GitRenameBranch => {
                with_ui!(self, |ui| self.git.open_choices("renameBranch", ui))
            }
            Command::GitMerge => with_ui!(self, |ui| self.git.open_choices("merge", ui)),
            Command::GitAbortMerge => with_ui!(self, |ui| self.git.confirm_abort_merge(ui)),
            Command::GitOpenFileDiff | Command::GitDiff => {
                with_ui!(self, |ui| self.git.open_active_diff(ui))
            }
            Command::GitStashList => with_ui!(self, |ui| self.git.open_output_tab(
                "Stashes",
                &["stash", "list"],
                ui
            )),
            Command::GitTags => {
                with_ui!(self, |ui| self.git.open_output_tab(
                    "Tags",
                    &["tag", "-n"],
                    ui
                ))
            }
            Command::GitRemotes => with_ui!(self, |ui| self.git.open_output_tab(
                "Remotes",
                &["remote", "-v"],
                ui
            )),
            Command::GitUnstageAll => with_ui!(self, |ui| self.git.unstage_all(ui)),
            Command::GitFetch => with_ui!(self, |ui| self.git.run(&["fetch"], ui)),
            Command::GitPull => with_ui!(self, |ui| self.git.run(&["pull"], ui)),
            Command::GitPush => with_ui!(self, |ui| self.git.run(&["push"], ui)),
            Command::GitStatus => with_ui!(self, |ui| self.git.refresh(ui)),
            Command::TerminalNew => self.open_terminal(false),
            Command::TerminalNewInPane => self.open_terminal(true),
            Command::TerminalClose => self.close_terminal(),
            Command::TerminalNext => self.dock.next(),
            Command::TerminalPrevious => self.dock.previous(),
            Command::EditorToggleComment => self.toggle_comment(),
            Command::EditorFold => self.with_active_editor(EditorState::fold),
            Command::EditorFoldAll => self.with_active_editor(EditorState::fold_all),
            Command::EditorUnfold => self.with_active_editor(EditorState::unfold),
            Command::EditorUnfoldAll => self.with_active_editor(EditorState::unfold_all),
            Command::EditorAddCursorAbove => self.with_active_editor(EditorState::add_cursor_above),
            Command::EditorAddCursorBelow => self.with_active_editor(EditorState::add_cursor_below),
            Command::EditorClearCursors => {
                self.with_active_editor(EditorState::clear_extra_cursors)
            }
            Command::EditSelectAllOccurrences => {
                if let Some(editor) = self.workbench.active_editor_mut() {
                    if editor.select_all_occurrences() {
                        self.set_status("Selected all occurrences");
                    } else {
                        self.set_status("No word under cursor");
                    }
                }
            }
            Command::EditSelectNextMatch => {
                if let Some(editor) = self.workbench.active_editor_mut() {
                    if editor.select_next_match() {
                        self.set_status("Added next occurrence");
                    } else {
                        self.set_status("No further occurrence");
                    }
                }
            }
            Command::EditorSelectLine => self.with_active_editor(EditorState::select_line),
            Command::SearchFindPrevious => self.find_previous(),
            Command::SessionSaveAs => {
                let initial = self.session_name.clone().unwrap_or_default();
                self.overlay = Some(modal::prompt(
                    PromptAction::SaveSessionName,
                    "Save Session As",
                    &initial,
                ));
            }
            Command::SessionOpen => self.open_session_picker(),
            Command::OutlinePanelMoveDown => self.outline_key(KeyCode::Down),
            Command::OutlinePanelMoveUp => self.outline_key(KeyCode::Up),
            Command::OutlinePanelPageDown => self.outline_key(KeyCode::PageDown),
            Command::OutlinePanelPageUp => self.outline_key(KeyCode::PageUp),
            Command::OutlinePanelGoToFirst => self.outline_key(KeyCode::Home),
            Command::OutlinePanelGoToLast => self.outline_key(KeyCode::End),
            Command::OutlinePanelSelect => self.outline_key(KeyCode::Enter),
            Command::OutlinePanelExpand => self.toggle_outline(Some(true)),
            Command::OutlinePanelCollapse => self.toggle_outline(Some(false)),
            Command::OutlinePanelToggleExpand => self.toggle_outline(None),
            Command::TimelinePanelMoveDown => self.timeline_key(KeyCode::Down),
            Command::TimelinePanelMoveUp => self.timeline_key(KeyCode::Up),
            Command::TimelinePanelPageDown => self.timeline_key(KeyCode::PageDown),
            Command::TimelinePanelPageUp => self.timeline_key(KeyCode::PageUp),
            Command::TimelinePanelGoToFirst => self.timeline_key(KeyCode::Home),
            Command::TimelinePanelGoToLast => self.timeline_key(KeyCode::End),
            Command::TimelinePanelViewDiff => with_ui!(self, |ui| self.git.view_timeline_diff(ui)),
            Command::TimelinePanelOpenFileAtCommit => {
                with_ui!(self, |ui| self.git.open_historical_file(ui))
            }
            Command::TimelinePanelCopyHash => {
                with_ui!(self, |ui| self.git.copy_selected_commit_hash(ui))
            }
            Command::TimelinePanelToggleMode => {
                with_ui!(self, |ui| self.git.toggle_timeline_mode(ui))
            }
            Command::GitPanelDiscard => with_ui!(self, |ui| self.git.discard_selected(ui)),
            Command::AiCliToggle => self.with_ai(|ai, ui| ai.open_cli_picker(ui)),
            Command::FileTreeMoveDown => self.with_file_tree(FileTreeState::move_down),
            Command::FileTreeMoveUp => self.with_file_tree(FileTreeState::move_up),
            Command::FileTreeGoToFirst => self.with_file_tree(FileTreeState::select_first),
            Command::FileTreeGoToLast => self.with_file_tree(FileTreeState::select_last),
            Command::FileTreePageDown => self.with_file_tree(|tree| tree.page_down(10)),
            Command::FileTreePageUp => self.with_file_tree(|tree| tree.page_up(10)),
            Command::FileTreeExpand => self.with_file_tree(FileTreeState::expand_selected),
            Command::FileTreeCollapse => self.with_file_tree(FileTreeState::collapse_selected),
            Command::FileTreeOpen => {
                self.focus_sidebar_section(0, true);
                if let Some(path) = self.file_tree.activate_selected() {
                    self.open_file(path);
                }
            }
            Command::FileTreeRefresh => self.file_tree.refresh(),
            Command::FileTreeNewFile => {
                self.open_prompt(PromptAction::NewFile, "New File (workspace-relative)", "")
            }
            Command::FileTreeNewFolder => self.open_prompt(
                PromptAction::NewFolder,
                "New Folder (workspace-relative)",
                "",
            ),
            Command::FileTreeRename => self.prompt_rename_selected_path(),
            Command::FileTreeDelete => self.confirm_delete_selected_path(),
            Command::GitPanelMoveDown => self.git_panel_key(KeyCode::Down),
            Command::GitPanelMoveUp => self.git_panel_key(KeyCode::Up),
            Command::GitPanelStage => {
                self.focus_sidebar_section(1, true);
                with_ui!(self, |ui| self.git.stage_selected(ui));
            }
            Command::GitPanelStageAll => with_ui!(self, |ui| self.git.stage_all(ui)),
            Command::GitPanelUnstage => with_ui!(self, |ui| self.git.unstage_selected(ui)),
            Command::GitPanelCommit => with_ui!(self, |ui| self.git.open_commit_prompt(ui)),
            Command::GitPanelRefresh => with_ui!(self, |ui| self.git.refresh(ui)),
            Command::GitPanelOpenDiff => with_ui!(self, |ui| self.git.open_selected_diff(ui)),
            Command::GitPanelOpenFile => with_ui!(self, |ui| self.git.open_selected_file(ui)),
            Command::GitPanelToggleSection => self.toggle_git_section(),
            Command::QueryResultsMoveDown => self.with_sql_tab(|tab| tab.move_selection(true)),
            Command::QueryResultsMoveUp => self.with_sql_tab(|tab| tab.move_selection(false)),
            Command::QueryResultsPageDown => self.with_sql_tab(|tab| tab.scroll_results(10)),
            Command::QueryResultsPageUp => self.with_sql_tab(|tab| tab.scroll_results(-10)),
            Command::QueryResultsGoToFirst => self.with_sql_tab(|tab| {
                tab.selected_row = 0;
                tab.result_scroll = 0;
            }),
            Command::QueryResultsGoToLast => {
                if let Some(Tab::Sql(tab)) = self.workbench.active_tab_mut()
                    && let Some(result) = &tab.result
                {
                    tab.selected_row = result.rows.len().saturating_sub(1);
                }
            }
            Command::QueryResultsExport => self.open_prompt(
                PromptAction::ExportResults,
                "Export Results (CSV)",
                "results.csv",
            ),
            Command::QueryResultsRefresh => self.with_db(|db, ui| db.refresh_results(ui)),
            Command::QueryResultsMoveLeft => self.with_sql_tab(|tab| tab.move_column(false)),
            Command::QueryResultsMoveRight => self.with_sql_tab(|tab| tab.move_column(true)),
            Command::QueryResultsSort => self.with_sql_tab(|tab| tab.sort_by_selected_column()),
            Command::QueryResultsToggleViewMode => self.with_sql_tab(|tab| tab.cycle_view()),
            Command::QueryResultsShowDetails => self.with_sql_tab(|tab| tab.show_details()),
            Command::DatabaseNewQuery => self.with_db(|db, ui| db.open_tab(None, ui)),
            Command::DatabaseExecuteQuery => self.with_db(|db, ui| db.execute(false, ui)),
            Command::DatabaseExecuteSelection => {
                self.with_db(|db, ui| db.execute_selection(false, ui))
            }
            Command::DatabaseConnect => self.with_db(|db, ui| db.open_connection_picker(ui)),
            Command::DatabaseNewConnection => {
                self.with_db(|db, ui| db.open_connection_form(None, ui))
            }
            Command::DatabaseDisconnect => self.with_db(|db, ui| db.disconnect(ui)),
            Command::DatabaseTestConnection => self.with_db(|db, ui| db.test_connection(ui)),
            Command::DatabaseViewHistory => self.with_db(|db, ui| db.open_history(ui)),
            Command::DatabaseAddRow => self.with_db(|db, ui| db.open_add_row(ui)),
            Command::DatabaseBrowseSchema => self.with_db(|db, ui| {
                db.describe_mode = false;
                db.browse_schema(ui);
            }),
            Command::DatabaseDescribeTable => self.with_db(|db, ui| {
                db.describe_mode = true;
                db.browse_schema(ui);
            }),
            Command::DatabaseEditConnection => self.with_db(|db, ui| db.edit_connection_prompt(ui)),
            Command::DatabaseDeleteConnection => {
                self.with_db(|db, ui| db.delete_connection_prompt(ui))
            }
            Command::DatabaseComplete => self.with_db(|db, ui| db.open_completion(ui)),
            Command::DatabaseExportResults => self.open_prompt(
                PromptAction::ExportResults,
                "Export Results (CSV)",
                "results.csv",
            ),
            Command::AgentsOpenSession => self.with_ai(|ai, ui| ai.open_session_picker(ui)),
            Command::AgentsAssociateWorkspace => self.with_ai(|ai, ui| ai.open_workspaces(ui)),
            Command::AgentsNewSession => self.with_ai(|ai, ui| ai.open_cli_picker(ui)),
            Command::AgentsRenameSession => self.with_ai(|ai, ui| ai.rename_selected_session(ui)),
            Command::AgentsDeleteSession => {
                self.with_ai(|ai, ui| ai.delete_selected_session(false, ui))
            }
            Command::AgentsArchiveSession => {
                self.with_ai(|ai, ui| ai.delete_selected_session(true, ui))
            }
            Command::AgentsRefreshSessions => {
                self.with_ai(|ai, ui| ai.refresh_sessions(ui));
                self.set_status("Agents: sessions refreshed");
            }
            Command::AgentsTogglePanel => self.toggle_agents(),
            Command::SecretsManage => self.with_db(|db, ui| db.manage_secrets(ui)),
            Command::WorkbenchQuit | Command::Quit => self.quit(),
            Command::WorkbenchSaveSession | Command::SessionSave => match self.save_session() {
                Ok(()) => self.set_status("Session saved"),
                Err(error) => self.set_status(format!("Session save failed: {error:#}")),
            },
        }
    }

    pub(super) fn focus_terminal(&mut self) {
        if self.dock.tabs.is_empty() {
            self.open_terminal(false);
        }
        self.dock.visible = true;
        self.focus = Focus::Dock;
    }

    pub(super) fn restart_tide(&mut self, rebuild: bool) {
        if rebuild && crate::run::rebuild_source().is_none() {
            self.set_status("Rebuild requires a debug build from its Tide source checkout; Restart reloads an installed binary");
        } else {
            self.restart = Some(rebuild);
            self.quit();
        }
    }

    pub(super) fn toggle_word_wrap(&mut self) {
        let current = self
            .settings
            .get("editor.wordWrap")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("off")
            .to_owned();
        let next = if current == "off" { "on" } else { "off" };
        self.settings.set(
            "editor.wordWrap",
            serde_json::Value::String(next.to_owned()),
        );
        self.set_status(format!("Word wrap {next}"));
    }

    pub(super) fn reset_region_sizes(&mut self) {
        self.sidebar_width = 36;
        self.dock.height = 10;
        self.ai.agents_width = 36;
        self.workbench.reset_ratios();
    }

    pub(super) fn toggle_sidebar(&mut self) {
        if self.layout.screen.width > 0 && self.sidebar_visible && self.layout.sidebar.is_none() {
            self.focus = Focus::Sidebar;
        } else {
            self.sidebar_visible = !self.sidebar_visible;
            if self.sidebar_visible {
                self.focus = Focus::Sidebar;
            }
        }
        if !self.sidebar_visible && self.focus == Focus::Sidebar {
            self.focus = Focus::Main;
        }
        self.mark_session_dirty();
    }

    pub(super) fn toggle_terminal(&mut self) {
        self.dock.visible = !self.dock.visible;
        if self.dock.visible {
            self.focus = Focus::Dock;
        } else if self.focus == Focus::Dock {
            self.focus = Focus::Main;
        }
        self.mark_session_dirty();
        if self.dock.visible && self.dock.tabs.is_empty() {
            self.open_terminal(false);
        }
    }

    pub(super) fn show_document_info(&mut self) {
        if let Some(editor) = self.workbench.active_editor() {
            let (eol, selected) = editor.document.status_metadata();
            self.overlay = Some(modal::info(
                "Document Information",
                &format!(
                    "{}\nLanguage: {}\nEncoding: UTF-8\nLine endings: {eol}\nSelection: {selected} characters",
                    editor
                        .path()
                        .map_or_else(|| editor.title(), |p| p.display().to_string()),
                    editor.language_extension()
                ),
            ));
        }
    }

    pub(super) fn open_sidebar_menu(&mut self, agents: bool) {
        self.overlay = Some(modal::picker(
            ModalKind::AddPanel,
            if agents {
                "Agents Sidebar"
            } else {
                "Workspace Sidebar"
            },
            vec![
                ModalItem::new(
                    "Move Left",
                    "",
                    if agents {
                        "workbench.moveAgentsSidebarLeft"
                    } else {
                        "workbench.moveSidebarLeft"
                    },
                ),
                ModalItem::new(
                    "Move Right",
                    "",
                    if agents {
                        "workbench.moveAgentsSidebarRight"
                    } else {
                        "workbench.moveSidebarRight"
                    },
                ),
                ModalItem::new(
                    "Hide Sidebar",
                    "",
                    if agents {
                        "agents.togglePanel"
                    } else {
                        "workbench.toggleSidebar"
                    },
                ),
            ],
        ));
    }

    pub(super) fn open_replace(&mut self) {
        self.open_find();
        if let Some(overlay) = &mut self.overlay {
            overlay.base_mut().replace_mode = true;
        }
        if let Some(mut overlay) = self.overlay.take() {
            self.search_feedback(overlay.as_mut());
            self.overlay = Some(overlay);
        }
    }

    pub(super) fn prompt_rename_selected_path(&mut self) {
        if let Some(entry) = self.file_tree.selected() {
            let path = entry.path.clone();
            let initial = entry.name.clone();
            self.overlay = Some(modal::prompt(
                PromptAction::RenameFile(path),
                "Rename",
                &initial,
            ));
        }
    }

    pub(super) fn confirm_delete_selected_path(&mut self) {
        if let Some(entry) = self.file_tree.selected() {
            let path = entry.path.clone();
            self.overlay = Some(modal::confirm(
                ConfirmAction::DeletePath(path.clone()),
                "Delete",
                &format!("Delete {}? This cannot be undone.", path.display()),
            ));
        }
    }

    pub(super) fn toggle_git_section(&mut self) {
        self.focus_sidebar_section(1, false);
        let index = self.accordion.focused;
        if let Some(section) = self.accordion.sections.get_mut(index) {
            section.expanded = !section.expanded;
        }
    }

    pub(super) fn with_active_editor(&mut self, action: impl FnOnce(&mut EditorState)) {
        if let Some(editor) = self.workbench.active_editor_mut() {
            action(editor);
        }
    }

    pub(super) fn with_sql_tab(&mut self, action: impl FnOnce(&mut SqlTab)) {
        if let Some(Tab::Sql(tab)) = self.workbench.active_tab_mut() {
            action(tab);
        }
    }

    pub(super) fn with_file_tree(&mut self, action: impl FnOnce(&mut FileTreeState)) {
        self.focus_sidebar_section(0, true);
        action(&mut self.file_tree);
    }

    pub(super) fn open_prompt(&mut self, action: PromptAction, title: &str, initial: &str) {
        self.overlay = Some(modal::prompt(action, title, initial));
    }

    pub(super) fn focus_sidebar_section(&mut self, index: usize, content: bool) {
        self.focus = Focus::Sidebar;
        if let Some(section) = self.accordion.sections.get_mut(index) {
            section.expanded = true;
        }
        self.accordion.focus(index);
        if content {
            self.accordion.enter_content();
        } else {
            self.accordion.leave_content();
        }
    }
}
