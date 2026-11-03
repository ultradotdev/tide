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
            Command::FileCloseOthers => self.close_others(),
            Command::FileRename => self.prompt_rename_file(),
            Command::FileDelete => {
                if let Some(path) = self.file_target("delete it") {
                    self.confirm_delete(&path);
                }
            }
            Command::FileNewFolder => self.prompt_new_folder(),
            Command::FileCopyPath => self.copy_file_path(false),
            Command::FileCopyRelativePath => self.copy_file_path(true),
            Command::FileRevealInFinder => self.reveal_file(),
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
            Command::WorkbenchToggleSidebar => self.toggle_sidebar(),
            Command::WorkbenchToggleTerminal => self.toggle_terminal(),
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
            Command::LspGoToDeclaration => self.lsp_request(LspRequestKind::Declaration),
            Command::LspGoToTypeDefinition => self.lsp_request(LspRequestKind::TypeDefinition),
            Command::LspGoToImplementation => self.lsp_request(LspRequestKind::Implementation),
            Command::LspRestartServer => with_ui!(self, |ui| self.lsp.restart_server(ui)),
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
            Command::SearchReplaceInFiles => self.open_replace_in_files(),
            Command::WorkbenchSelectTheme => self.open_theme_picker(),
            Command::WorkbenchOpenSettings => self.open_settings(),
            Command::WorkbenchOpenKeybindings => self.open_keybindings(),
            Command::EditorCommentLine => self.comment_lines(CommentMode::Add),
            Command::EditorUncommentLine => self.comment_lines(CommentMode::Remove),
            Command::EditorDuplicateLine => self.edit_lines(|editor, _| {
                editor.duplicate_lines();
                None
            }),
            Command::EditorDuplicateSelection => self.edit_lines(|editor, _| {
                editor.duplicate_selections();
                None
            }),
            Command::GitFocusPanel => {
                self.add_sidebar_section(SectionId::SourceControl);
                self.accordion.enter_content();
                with_ui!(self, |ui| self.git.refresh(ui));
            }
            Command::WorkbenchFocusExplorer => self.add_sidebar_section(SectionId::Explorer),
            Command::WorkbenchFocusOutline => self.add_sidebar_section(SectionId::Outline),
            Command::WorkbenchFocusTimeline => self.add_sidebar_section(SectionId::Timeline),
            Command::GitRefresh => with_ui!(self, |ui| self
                .git
                .request_refresh(Some("Git status refreshed".into()), ui)),
            Command::GitStage => with_ui!(self, |ui| self.git.stage_active(ui, true)),
            Command::GitStageAll => with_ui!(self, |ui| self.git.stage_all(ui)),
            Command::GitUnstage => with_ui!(self, |ui| self.git.stage_active(ui, false)),
            Command::GitCommit => with_ui!(self, |ui| self.git.open_commit_prompt(ui)),
            Command::GitDiffStaged => with_ui!(self, |ui| self.git.open_staged_diff(ui)),
            Command::GitLog => with_ui!(self, |ui| self.git.open_log(None, ui)),
            Command::GitLogFile => with_ui!(self, |ui| self.git.open_selected_log(ui)),
            Command::GitBlame => with_ui!(self, |ui| self.git.open_blame(ui)),
            Command::GitSwitchBranch => {
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
            Command::GitDiff => with_ui!(self, |ui| self.git.open_active_diff(ui)),
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
            Command::GitDiscard => with_ui!(self, |ui| self.git.discard_active(ui)),
            Command::GitDiscardAll => with_ui!(self, |ui| self.git.confirm_discard_all(ui)),
            Command::GitCommitAmend => with_ui!(self, |ui| self.git.prepare_amend(ui)),
            Command::GitPullRebase => with_ui!(self, |ui| self.git.run(&["pull", "--rebase"], ui)),
            Command::GitFetchAll => with_ui!(self, |ui| self.git.run(&["fetch", "--all"], ui)),
            Command::GitRebase => with_ui!(self, |ui| self.git.open_choices("rebase", ui)),
            Command::GitContinueRebase => {
                with_ui!(self, |ui| self.git.continue_operation(true, ui))
            }
            Command::GitAbortRebase => with_ui!(self, |ui| self.git.prepare_abort(true, ui)),
            Command::GitContinue => with_ui!(self, |ui| self.git.continue_operation(false, ui)),
            Command::GitAbort => with_ui!(self, |ui| self.git.prepare_abort(false, ui)),
            Command::GitConflicts => with_ui!(self, |ui| self.git.open_conflicts(ui)),
            Command::GitCreateTag => self.open_prompt(PromptAction::CreateTag, "Create Tag", ""),
            Command::GitCherryPick => with_ui!(self, |ui| self.git.open_choices("cherryPick", ui)),
            Command::GitRevert => with_ui!(self, |ui| self.git.open_choices("revert", ui)),
            Command::GitReset => with_ui!(self, |ui| self.git.open_choices("reset", ui)),
            Command::GitClean => with_ui!(self, |ui| self.git.confirm_clean(ui)),
            Command::GitPushForce => with_ui!(self, |ui| self.git.confirm_push_force(ui)),
            Command::TerminalNew => self.open_terminal(false),
            Command::TerminalNewInPane => self.open_terminal(true),
            Command::TerminalClose => self.close_terminal(),
            Command::TerminalNext | Command::TerminalPrevious => {
                if self.dock.tabs.is_empty() {
                    self.set_status("No bottom terminal tab is open");
                } else {
                    self.dock.visible = true;
                    if command == Command::TerminalNext {
                        self.dock.next();
                    } else {
                        self.dock.previous();
                    }
                }
            }
            Command::TerminalClear => self.clear_terminal(),
            Command::TerminalCopySelection => self.copy_terminal_selection(),
            Command::TerminalKill => self.confirm_kill_terminal(),
            Command::TerminalScrollToTop => self.scroll_terminal(true),
            Command::TerminalScrollToBottom => self.scroll_terminal(false),
            Command::TerminalCloseAll => self.confirm_close_all_terminals(),
            Command::EditorToggleComment => self.comment_lines(CommentMode::Toggle),
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
            Command::EditMoveLinesUp | Command::EditMoveLinesDown => {
                self.edit_lines(|editor, _| {
                    editor.move_lines(command == Command::EditMoveLinesUp);
                    None
                })
            }
            Command::EditDeleteLine => self.edit_lines(|editor, _| {
                editor.delete_lines();
                None
            }),
            Command::EditIndentLine | Command::EditOutdentLine => {
                self.edit_lines(|editor, tab_size| {
                    editor.indent_lines(command == Command::EditOutdentLine, tab_size);
                    None
                })
            }
            Command::EditJoinLines => self.edit_lines(|editor, _| {
                editor.join_lines();
                None
            }),
            Command::EditTransformToUppercase
            | Command::EditTransformToLowercase
            | Command::EditTransformToTitleCase => self.edit_lines(|editor, _| {
                let transform = match command {
                    Command::EditTransformToUppercase => CaseTransform::Upper,
                    Command::EditTransformToLowercase => CaseTransform::Lower,
                    _ => CaseTransform::Title,
                };
                (!editor.transform_case(transform))
                    .then(|| "Select text, or put the cursor on a word, to change its case".into())
            }),
            Command::EditorSortLinesAscending | Command::EditorSortLinesDescending => self
                .edit_lines(|editor, _| {
                    editor
                        .sort_lines(command == Command::EditorSortLinesDescending)
                        .map(|count| format!("Sorted {count} line(s)"))
                }),
            Command::EditorTrimTrailingWhitespace => self.edit_lines(|editor, _| {
                editor.trim_trailing_whitespace().map(|count| match count {
                    0 => "No trailing whitespace to trim".to_owned(),
                    count => format!("Trimmed trailing whitespace on {count} line(s)"),
                })
            }),
            Command::EditorToggleBlockComment => self.toggle_block_comment(),
            Command::EditorFormatSelection => self.lsp_request(LspRequestKind::FormatSelection),
            Command::EditorFoldLevel1 | Command::EditorFoldLevel2 | Command::EditorFoldLevel3 => {
                let level = match command {
                    Command::EditorFoldLevel1 => 1,
                    Command::EditorFoldLevel2 => 2,
                    _ => 3,
                };
                if let Some(editor) = self.workbench.active_editor_mut() {
                    let folded = editor.fold_level(level);
                    self.set_status(if folded == 0 {
                        format!("No regions to fold at level {level}")
                    } else {
                        format!("Folded {folded} region(s) at level {level}")
                    });
                } else {
                    self.set_status("Open a file to fold its regions");
                }
            }
            Command::WorkbenchToggleMinimap => self.toggle_minimap(),
            Command::WorkbenchToggleZenMode => self.toggle_zen_mode(),
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
            Command::SessionList => self.list_workspace_sessions(false),
            Command::SessionDelete => self.list_workspace_sessions(true),
            Command::OutlinePanelMoveDown => self.outline_key(KeyCode::Down),
            Command::OutlinePanelMoveUp => self.outline_key(KeyCode::Up),
            Command::OutlinePanelPageDown => self.outline_key(KeyCode::PageDown),
            Command::OutlinePanelPageUp => self.outline_key(KeyCode::PageUp),
            Command::OutlinePanelGoToFirst => self.outline_key(KeyCode::Home),
            Command::OutlinePanelGoToLast => self.outline_key(KeyCode::End),
            Command::OutlinePanelSelect => self.outline_key(KeyCode::Enter),
            Command::OutlinePanelExpand => self.outline_key(KeyCode::Right),
            Command::OutlinePanelCollapse => self.outline_key(KeyCode::Left),
            Command::OutlinePanelToggleExpand => {
                if self.reveal_outline() {
                    self.toggle_outline(None);
                }
            }
            Command::TimelinePanelMoveDown => self.timeline_key(KeyCode::Down),
            Command::TimelinePanelMoveUp => self.timeline_key(KeyCode::Up),
            Command::TimelinePanelPageDown => self.timeline_key(KeyCode::PageDown),
            Command::TimelinePanelPageUp => self.timeline_key(KeyCode::PageUp),
            Command::TimelinePanelGoToFirst => self.timeline_key(KeyCode::Home),
            Command::TimelinePanelGoToLast => self.timeline_key(KeyCode::End),
            Command::TimelinePanelViewDiff => {
                if self.reveal_timeline("view a commit's diff") {
                    with_ui!(self, |ui| self.git.view_timeline_diff(ui));
                }
            }
            Command::TimelinePanelOpenFileAtCommit => {
                if self.reveal_timeline("open a file at a commit") {
                    with_ui!(self, |ui| self.git.open_historical_file(ui));
                }
            }
            Command::TimelinePanelCopyHash => {
                if self.reveal_timeline("copy a commit hash") {
                    with_ui!(self, |ui| self.git.copy_selected_commit_hash(ui));
                }
            }
            Command::TimelinePanelToggleMode => {
                if self.reveal_panel(SectionId::Timeline, "switch its history") {
                    with_ui!(self, |ui| self.git.toggle_timeline_mode(ui));
                }
            }
            Command::GitPanelDiscard => {
                if self.reveal_git_file("discard its changes") {
                    with_ui!(self, |ui| self.git.discard_selected(ui));
                }
            }
            Command::FileTreeMoveDown => self.with_file_tree(FileTreeState::move_down),
            Command::FileTreeMoveUp => self.with_file_tree(FileTreeState::move_up),
            Command::FileTreeGoToFirst => self.with_file_tree(FileTreeState::select_first),
            Command::FileTreeGoToLast => self.with_file_tree(FileTreeState::select_last),
            Command::FileTreePageDown => self.with_file_tree(|tree| tree.page_down(10)),
            Command::FileTreePageUp => self.with_file_tree(|tree| tree.page_up(10)),
            Command::FileTreeExpand => self.with_file_tree(FileTreeState::expand_selected),
            Command::FileTreeCollapse => self.with_file_tree(FileTreeState::collapse_selected),
            Command::FileTreeOpen => {
                if self.reveal_panel(SectionId::Explorer, "open a file") {
                    if self.file_tree.selected().is_none() {
                        self.set_status("Select a file or folder in the Explorer to open");
                    } else if let Some(path) = self.file_tree.activate_selected() {
                        self.open_file(path);
                    }
                }
            }
            Command::FileTreeRefresh => {
                if self.reveal_panel(SectionId::Explorer, "refresh it") {
                    self.file_tree.refresh();
                    self.set_status("Explorer refreshed");
                }
            }
            Command::FileTreeNewFile | Command::FileTreeNewFolder => {
                if self.reveal_panel(SectionId::Explorer, "create a file there") {
                    let folder = self.explorer_folder();
                    let (action, title) = if command == Command::FileTreeNewFile {
                        (PromptAction::NewFile, "New File (workspace-relative)")
                    } else {
                        (PromptAction::NewFolder, "New Folder (workspace-relative)")
                    };
                    self.open_prompt(action, title, &folder);
                }
            }
            Command::FileTreeRename => {
                if self.reveal_panel(SectionId::Explorer, "rename a file") {
                    self.prompt_rename_selected_path();
                }
            }
            Command::FileTreeDelete => {
                if self.reveal_panel(SectionId::Explorer, "delete a file") {
                    self.confirm_delete_selected_path();
                }
            }
            Command::GitPanelMoveDown => self.git_panel_key(KeyCode::Down),
            Command::GitPanelMoveUp => self.git_panel_key(KeyCode::Up),
            Command::GitPanelStage => {
                if self.reveal_git_file("stage it") {
                    with_ui!(self, |ui| self.git.stage_selected(ui));
                }
            }
            Command::GitPanelUnstage => {
                if self.reveal_git_file("unstage it") {
                    with_ui!(self, |ui| self.git.unstage_selected(ui));
                }
            }
            Command::GitPanelOpenDiff => {
                if self.reveal_git_file("open its diff") {
                    with_ui!(self, |ui| self.git.open_selected_diff(ui));
                }
            }
            Command::GitPanelOpenFile => {
                if self.reveal_git_file("open it") {
                    with_ui!(self, |ui| self.git.open_selected_file(ui));
                }
            }
            Command::GitPanelToggleSection => self.toggle_git_section(),
            Command::QueryResultsMoveDown => self.with_results(|tab| tab.move_selection(true)),
            Command::QueryResultsMoveUp => self.with_results(|tab| tab.move_selection(false)),
            Command::QueryResultsPageDown => self.with_results(|tab| tab.scroll_results(10)),
            Command::QueryResultsPageUp => self.with_results(|tab| tab.scroll_results(-10)),
            Command::QueryResultsGoToFirst => self.with_results(|tab| {
                tab.selected_row = 0;
                tab.result_scroll = 0;
            }),
            Command::QueryResultsGoToLast => self.with_results(|tab| {
                tab.selected_row = tab
                    .result
                    .as_ref()
                    .map_or(0, |result| result.rows.len().saturating_sub(1));
            }),
            Command::QueryResultsRefresh => {
                if self.focus_results() {
                    self.with_db(|db, ui| db.refresh_results(ui));
                }
            }
            Command::QueryResultsMoveLeft => self.with_results(|tab| tab.move_column(false)),
            Command::QueryResultsMoveRight => self.with_results(|tab| tab.move_column(true)),
            Command::QueryResultsSort => self.with_results(SqlTab::sort_by_selected_column),
            Command::QueryResultsToggleViewMode => self.with_results(SqlTab::cycle_view),
            Command::QueryResultsShowDetails => self.with_results(SqlTab::show_details),
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
            Command::WorkbenchQuit => self.quit(),
            Command::HelpWelcome
            | Command::HelpGettingStarted
            | Command::HelpWorkbench
            | Command::HelpEditor
            | Command::HelpGit
            | Command::HelpTerminal
            | Command::HelpDatabase
            | Command::HelpAgents
            | Command::HelpThemes
            | Command::HelpKeyboardShortcuts
            | Command::HelpCommands
            | Command::HelpSettings
            | Command::HelpTroubleshooting
            | Command::HelpAbout
            | Command::HelpDocumentation
            | Command::HelpReportIssue
            | Command::HelpCheckForUpdates => self.run_help(command),
            Command::SessionSave => match self.adopt_session() {
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

    pub(super) fn toggle_minimap(&mut self) {
        let enabled = !self.setting_bool("editor.minimap.enabled", true);
        self.settings
            .set("editor.minimap.enabled", serde_json::Value::Bool(enabled));
        self.mark_session_dirty();
        self.set_status(format!("Minimap {}", if enabled { "on" } else { "off" }));
    }

    /// Hides the sidebars and the bottom terminal around the main area, or restores the
    /// layout zen mode hid.
    pub(super) fn toggle_zen_mode(&mut self) {
        if let Some(zen) = self.zen.take() {
            self.sidebar_visible = zen.sidebar;
            self.dock.visible = zen.dock && !self.dock.tabs.is_empty();
            self.ai.agents_visible = zen.agents;
            self.focus = match zen.focus {
                Focus::Sidebar if !self.sidebar_visible => Focus::Main,
                Focus::Dock if !self.dock.visible => Focus::Main,
                Focus::Agents if !self.ai.agents_visible => Focus::Main,
                focus => focus,
            };
            self.set_status("Zen mode off");
        } else {
            self.zen = Some(ZenLayout {
                sidebar: self.sidebar_visible,
                dock: self.dock.visible,
                agents: self.ai.agents_visible,
                focus: self.focus,
            });
            self.sidebar_visible = false;
            self.dock.visible = false;
            self.ai.agents_visible = false;
            self.focus = Focus::Main;
            self.set_status("Zen mode on; run Toggle Zen Mode again to restore the layout");
        }
        self.mark_session_dirty();
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
        let Some(entry) = self.file_tree.selected() else {
            self.set_status("Select a file or folder in the Explorer to rename");
            return;
        };
        let path = entry.path.clone();
        let initial = entry.name.clone();
        self.overlay = Some(modal::prompt(
            PromptAction::RenameFile(path),
            "Rename",
            &initial,
        ));
    }

    pub(super) fn confirm_delete_selected_path(&mut self) {
        let Some(entry) = self.file_tree.selected() else {
            self.set_status("Select a file or folder in the Explorer to delete");
            return;
        };
        let path = entry.path.clone();
        self.confirm_delete(&path);
    }

    /// The workspace-relative folder of the Explorer selection, ending in a separator,
    /// for prompts that create a path there.
    pub(super) fn explorer_folder(&self) -> String {
        self.file_tree
            .selected()
            .map_or_else(String::new, |entry| self.relative_folder(&entry.path))
    }

    /// Collapses or expands the Source Control group (Staged, Changes, ...)
    /// under the selection, not the whole sidebar section.
    pub(super) fn toggle_git_section(&mut self) {
        if self.reveal_section(SectionId::SourceControl, true, "collapse or expand a group") {
            self.git.sync_panel();
            self.git.toggle_panel_group();
        }
    }

    pub(super) fn with_active_editor(&mut self, action: impl FnOnce(&mut EditorState)) {
        if let Some(editor) = self.workbench.active_editor_mut() {
            action(editor);
        }
    }

    /// Runs `action` on the active SQL editor's results once they are focused.
    pub(super) fn with_results(&mut self, action: impl FnOnce(&mut SqlTab)) {
        if self.focus_results()
            && let Some(Tab::Sql(tab)) = self.workbench.active_tab_mut()
        {
            action(tab);
        }
    }

    /// Focuses the active SQL editor's results. False, with a status, when there are
    /// no results to act on.
    pub(super) fn focus_results(&mut self) -> bool {
        match self.workbench.active_tab_mut() {
            Some(Tab::Sql(tab)) if tab.result.is_some() => {
                tab.results_focused = true;
                self.focus = Focus::Main;
                true
            }
            Some(Tab::Sql(_)) => {
                self.set_status("Run a query first; this SQL editor has no results yet");
                false
            }
            _ => {
                self.set_status("Open a SQL editor and run a query to use its results");
                false
            }
        }
    }

    pub(super) fn with_file_tree(&mut self, action: impl FnOnce(&mut FileTreeState)) {
        if !self.reveal_panel(SectionId::Explorer, "browse files") {
            return;
        }
        if self.file_tree.entries.is_empty() {
            self.set_status("The Explorer has no files to select");
        } else {
            action(&mut self.file_tree);
        }
    }

    /// Reveals the Timeline when it has commits to act on.
    pub(super) fn reveal_timeline(&mut self, purpose: &str) -> bool {
        if !self.reveal_panel(SectionId::Timeline, purpose) {
            return false;
        }
        if self.git.timeline.entries.is_empty() {
            self.set_status(format!(
                "The Timeline has no commits; select one to {purpose}"
            ));
            return false;
        }
        true
    }

    /// Reveals the Outline when the active editor has symbols in it.
    pub(super) fn reveal_outline(&mut self) -> bool {
        if !self.reveal_panel(SectionId::Outline, "browse symbols") {
            return false;
        }
        if self.outline_for_active().is_empty() {
            self.set_status("The Outline has no symbols for the active editor");
            return false;
        }
        true
    }

    /// Reveals Source Control when a changed file is selected in it.
    pub(super) fn reveal_git_file(&mut self, purpose: &str) -> bool {
        if !self.reveal_panel(SectionId::SourceControl, purpose) {
            return false;
        }
        if !self.git.has_selected_file() {
            self.set_status(format!(
                "Select a changed file in Source Control to {purpose}"
            ));
            return false;
        }
        true
    }

    pub(super) fn open_prompt(&mut self, action: PromptAction, title: &str, initial: &str) {
        self.overlay = Some(modal::prompt(action, title, initial));
    }

    /// Shows and focuses the content of sidebar panel `id`, so a panel command run from
    /// the palette acts on that panel's selection. False, with a status saying how to
    /// add the panel, when the sidebar does not have it.
    pub(super) fn reveal_panel(&mut self, id: SectionId, purpose: &str) -> bool {
        self.reveal_section(id, true, purpose)
    }

    pub(super) fn reveal_section(&mut self, id: SectionId, content: bool, purpose: &str) -> bool {
        let Some(index) = self
            .accordion
            .sections
            .iter()
            .position(|section| section.id == id)
        else {
            let title = id.title();
            self.set_status(format!(
                "Open the {title} to {purpose}: run “Sidebar: Add {title}”"
            ));
            return false;
        };
        if !self.sidebar_visible {
            self.sidebar_visible = true;
            self.mark_session_dirty();
        }
        self.focus_sidebar_section(index, content);
        true
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
