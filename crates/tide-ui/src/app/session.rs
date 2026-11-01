//! Session snapshots: autosave, named sessions, quit, and restore.
use super::*;
use num_traits::ToPrimitive;

impl App {
    /// Builds the recovery snapshot here and writes it on a blocking thread.
    pub(super) fn autosave_session(&mut self) -> Result<()> {
        if self.session_write_blocked {
            return self.save_session();
        }
        let session = self.build_session();
        self.session_ticket += 1;
        let ticket = self.session_ticket;
        let saves = self.session_saves.clone();
        let store = self.session_store.clone();
        let name = self.session_name.clone();
        let sender = self.events_tx.clone();
        self.session_dirty = false;
        self.autosave_running = true;
        self.jobs.spawn_blocking(move || {
            let result = write_session(&saves, ticket, &store, &session, name.as_deref())
                .map_err(|error| format!("{error:#}"));
            let _ = sender.blocking_send(AppEvent::SessionAutosaved(result));
        });
        Ok(())
    }

    pub(super) fn open_session_picker(&mut self) {
        let items = self
            .session_store
            .named_sessions()
            .into_iter()
            .map(|name| modal::ModalItem::new(name.clone(), "saved session", name))
            .collect::<Vec<_>>();
        if items.is_empty() {
            self.set_status("Sessions: no named sessions saved");
            return;
        }
        self.overlay = Some(modal::session_picker(items));
    }

    pub(super) fn open_named_session(&mut self, name: &str) {
        if self.has_dirty_tabs() {
            self.overlay = Some(modal::confirm(
                ConfirmAction::SwitchSession(name.into()),
                "Switch Session",
                "Save a recovery snapshot of the current session and switch?",
            ));
            return;
        }
        self.switch_named_session(name);
    }

    pub(super) fn switch_named_session(&mut self, name: &str) {
        match self.session_store.load_named(name) {
            Ok(Some(loaded)) => {
                if let Err(error) = self.save_session() {
                    self.set_status(format!("Session switch blocked: {error:#}"));
                    return;
                }
                self.lsp
                    .close_all_documents(&self.settings, &self.workspace);
                self.documents = DocumentRegistry::default();
                self.dock = DockState::new();
                self.session_name = Some(name.to_owned());
                self.session_baseline = Some(loaded.state.clone());
                match self.restore_session(loaded.state) {
                    Ok(()) => {
                        self.session_dirty = false;
                        self.set_status(format!("Session: opened {name}"));
                    }
                    Err(error) => self.set_status(format!("Session restore failed: {error:#}")),
                }
            }
            Ok(None) => self.set_status(format!("Session: {name} not found")),
            Err(error) => self.set_status(format!("Session: {error:#}")),
        }
    }

    pub(super) fn save_session_as(&mut self, name: &str) {
        let name = name.trim();
        if name.is_empty() {
            return;
        }
        self.session_name = Some(name.to_owned());
        match self.save_session() {
            Ok(()) => self.set_status(format!("Session: saved {name}")),
            Err(error) => self.set_status(format!("Session save failed: {error:#}")),
        }
    }

    pub(super) fn quit(&mut self) {
        let dirty = self
            .workbench
            .pane_ids()
            .into_iter()
            .filter_map(|id| self.workbench.pane(id))
            .flat_map(|pane| pane.tabs.iter())
            .any(Tab::is_dirty);
        if dirty {
            self.overlay = Some(modal::confirm(
                ConfirmAction::QuitDirty,
                "Unsaved Changes",
                if self.workbench.pane_ids().iter().any(|id| {
                    self.workbench
                        .pane(*id)
                        .unwrap()
                        .tabs
                        .iter()
                        .any(|t| matches!(t,Tab::Sql(sql) if !sql.row_edits.is_empty()))
                }) {
                    "Quit and discard unsaved database row edits? Query buffers remain in session recovery."
                } else {
                    "Save changes before quitting? Discard keeps a recovery copy."
                },
            ));
        } else {
            self.finish_quit();
        }
    }

    pub(super) fn finish_quit(&mut self) {
        match self.save_session() {
            Ok(()) => {
                self.workbench.kill_terminals();
                self.should_quit = true;
            }
            Err(error) => {
                self.restart = None;
                self.set_status(format!("Quit blocked: recovery snapshot failed: {error:#}"))
            }
        }
    }

    pub(crate) fn save_session(&mut self) -> Result<()> {
        if self.session_write_blocked {
            anyhow::bail!(
                "session writes are disabled after a load/restore failure; the existing state was not modified"
            );
        }
        let session = self.build_session();
        self.session_ticket += 1;
        write_session(
            &self.session_saves,
            self.session_ticket,
            &self.session_store,
            &session,
            self.session_name.as_deref(),
        )?;
        self.session_dirty = false;
        tracing::debug!("saved session");
        Ok(())
    }

    pub(super) fn build_session(&self) -> SessionState {
        let mut documents = Vec::new();
        let mut terminals = Vec::new();
        let mut ai_cli_panes = Vec::new();
        let mut sql_editors = Vec::new();
        for pane_id in self.workbench.pane_ids() {
            let Some(pane) = self.workbench.pane(pane_id) else {
                continue;
            };
            for (order, tab) in pane.tabs.iter().enumerate() {
                match tab {
                    Tab::Editor(editor) => {
                        let cursor = editor.document.cursor().position;
                        documents.push(SessionDocumentState {
                            file_path: editor
                                .path()
                                .map(|path| path.display().to_string())
                                .unwrap_or_default(),
                            scroll_top: saturating_u32(editor.scroll_top),
                            scroll_left: saturating_u32(editor.scroll_left),
                            cursor_line: saturating_u32(cursor.line),
                            cursor_column: saturating_u32(cursor.column),
                            selection_anchor_line: editor
                                .document
                                .cursor()
                                .selection
                                .map(|selection| saturating_u32(selection.anchor.line)),
                            selection_anchor_column: editor
                                .document
                                .cursor()
                                .selection
                                .map(|selection| saturating_u32(selection.anchor.column)),
                            folded_regions: editor
                                .folded_ranges()
                                .iter()
                                .filter_map(|(start, _)| u32::try_from(*start).ok())
                                .collect(),
                            pane_id: format!("pane-{pane_id}"),
                            tab_order: saturating_u32(order),
                            is_active_in_pane: pane.active == order,
                            unsaved_content: (editor.is_dirty() || editor.path().is_none()).then(|| editor.document.text()),
                            undo_history: None,
                            extra: {
                                let mut extra=BTreeMap::new();
                                extra.insert("rustView".into(),editor.view_state());
                                if let Some(hash)=editor.saved_disk_hash() {extra.insert("rustDiskHash".into(),serde_json::Value::String(hash));}
                                if let Some(edit)=&editor.hunk_edit {extra.insert("rustHunkEdit".into(),serde_json::json!({"root":edit.root,"diff":*edit.diff,"hunk":edit.hunk}));}
                                if let Some(history)=&editor.historical {extra.insert("rustHistory".into(),serde_json::json!(history));}
                                extra
                            },
                        });
                    }
                    Tab::Terminal(terminal)
                        if terminal.ai_provider.is_some() && terminal.ai_session_id.is_none() => {}
                    Tab::Terminal(terminal) if terminal.ai_provider.is_some() => {
                        ai_cli_panes.push(SessionAICLIPaneState {
                            element_id: format!("ai-cli-{pane_id}-{order}"),
                            pane_id: format!("pane-{pane_id}"),
                            tab_order: saturating_u32(order),
                            is_active_in_pane: pane.active == order,
                            provider: terminal
                                .ai_provider
                                .clone()
                                .unwrap_or_else(|| "claude-code".to_owned()),
                            session_id: terminal.ai_session_id.clone(),
                            session_key: terminal
                                .ai_session_id
                                .as_ref()
                                .map(|id| format!("{id}@{pane_id}-{order}")),
                            model: None,
                            cwd: terminal.cwd.display().to_string(),
                            title: terminal.title.clone(),
                            input_history: None,
                            extra: BTreeMap::new(),
                        });
                    }
                    Tab::Terminal(terminal) => {
                        terminals.push(SessionTerminalState {
                            element_id: format!("term-{pane_id}-{order}"),
                            pane_id: format!("pane-{pane_id}"),
                            tab_order: saturating_u32(order),
                            is_active_in_pane: pane.active == order,
                            cwd: terminal.cwd.display().to_string(),
                            title: terminal.title.clone(),
                            semantic_kind: Some("shell".to_owned()),
                            durability: Some("persistent".to_owned()),
                            extra: BTreeMap::new(),
                        });
                    }
                    Tab::Sql(sql) => {
                        let cursor = sql.editor.document.cursor().position;
                        sql_editors.push(SessionSQLEditorState {
                            element_id: format!("sql-{pane_id}-{order}"),
                            pane_id: format!("pane-{pane_id}"),
                            tab_order: saturating_u32(order),
                            is_active_in_pane: pane.active == order,
                            file_path: sql.editor.path().map(|p| p.display().to_string()),
                            content: sql.sql(),
                            connection_id: sql.connection_id.clone(),
                            cursor_line: saturating_u32(cursor.line),
                            cursor_column: saturating_u32(cursor.column),
                            scroll_top: saturating_u32(sql.editor.scroll_top),
                            title: sql.title.clone(),
                            extra: BTreeMap::from([
                                (
                                    "rustDiskHash".into(),
                                    serde_json::json!(sql.editor.saved_disk_hash()),
                                ),
                                ("rustDirty".into(), serde_json::json!(sql.editor.is_dirty())),
                                ("rustView".into(), sql.editor.view_state()),
                                ("rustResultView".into(), serde_json::json!(sql.view_mode)),
                            ]),
                        });
                    }
                    _ => {}
                }
            }
        }
        let bottom_terminals = self
            .dock
            .tabs
            .iter()
            .enumerate()
            .map(|(order, terminal)| SessionBottomTerminalState {
                tab_id: format!("dock-{}", terminal.id),
                tab_order: saturating_u32(order),
                is_active: order == self.dock.active,
                cwd: terminal.cwd.display().to_string(),
                title: terminal.title.clone(),
                semantic_kind: "shell".to_owned(),
                extra: BTreeMap::new(),
            })
            .collect();
        let layout = session_layout(self.workbench.root());
        let active_document_path = self
            .workbench
            .active_editor()
            .and_then(EditorState::path)
            .map(|path| path.display().to_string());
        let focused_region = match self.focus {
            Focus::Sidebar => "sidebar",
            Focus::Main => "main",
            Focus::Agents => "agents",
            Focus::Dock => "bottom-terminal",
            Focus::Status => "status",
        };
        let mut state = SessionState {
            version: 7,
            timestamp: timestamp_millis().to_string(),
            instance_id: self.instance_id.clone(),
            workspace_root: self.workspace.display().to_string(),
            session_name: self.session_name.clone(),
            documents,
            terminals,
            bottom_terminals,
            ai_cli_panes,
            sql_editors,
            active_document_path,
            active_pane_id: format!("pane-{}", self.workbench.focused()),
            layout,
            ui: SessionUIState {
                sidebar_visible: self.sidebar_visible,
                sidebar_width: self.sidebar_width,
                sidebar_location: Some(if self.sidebar_left { "left" } else { "right" }.to_owned()),
                agents_sidebar_visible: self.ai.agents_visible,
                agents_sidebar_width: self.ai.agents_width,
                agents_sidebar_location: Some(
                    if self.agents_left { "left" } else { "right" }.to_owned(),
                ),
                agents_sidebar_expanded_providers: ai_cli::CLI_PROVIDERS
                    .iter()
                    .filter(|provider| !self.ai.agent_collapsed.contains(provider.id()))
                    .map(|provider| provider.id().to_owned())
                    .collect(),
                agents_sidebar_expanded_sections: Vec::new(),
                terminal_visible: self.dock.visible,
                terminal_height: self.dock.height,
                git_panel_visible: self.accordion.focused_section() == SectionId::SourceControl,
                git_panel_width: self.sidebar_width,
                active_sidebar_panel: if self.accordion.focused_section()
                    == SectionId::SourceControl
                {
                    "git".to_owned()
                } else {
                    "files".to_owned()
                },
                minimap_enabled: self.setting_bool("editor.minimap.enabled", false),
                sidebar_accordion_expanded: self.accordion.expanded_ids(),
                focused_region: Some(focused_region.to_owned()),
                extra: BTreeMap::from([
                    (
                        "rustSidebarSections".into(),
                        serde_json::json!(
                            self.accordion
                                .sections
                                .iter()
                                .map(|section| section.id.id())
                                .collect::<Vec<_>>()
                        ),
                    ),
                    (
                        "rustSidebarFocused".into(),
                        serde_json::json!(self.accordion.focused_section().id()),
                    ),
                    (
                        "rustSidebarContentFocus".into(),
                        serde_json::json!(self.accordion.content_focus),
                    ),
                ]),
            },
            extra: BTreeMap::from([("rustFileTree".into(), self.file_tree.snapshot())]),
        };
        if let Some(previous) = &self.session_baseline {
            let mut unsupported = previous.clone();
            // These surfaces are managed by Rust, including an intentionally
            // empty list after the last tab closes. Never resurrect closed tabs.
            unsupported.ai_cli_panes.clear();
            unsupported.sql_editors.clear();
            state.preserve_unsupported_from(&unsupported);
        }
        state
    }

    pub(super) fn restore_session(&mut self, session: SessionState) -> Result<()> {
        self.settings.set(
            "editor.minimap.enabled",
            serde_json::json!(session.ui.minimap_enabled),
        );
        if let Some(tree) = session.extra.get("rustFileTree") {
            self.file_tree.restore(tree);
        }
        let mut pane_map: HashMap<String, PaneId> = HashMap::new();
        let mut next_pane = 0;
        let root = convert_session_layout(&session.layout, &mut pane_map, &mut next_pane);
        let mut panes = BTreeMap::new();
        for id in pane_map.values().copied() {
            panes.insert(id, Pane::new(id));
        }
        if panes.is_empty() {
            panes.insert(0, Pane::new(0));
        }

        for document in &session.documents {
            let pane_id = pane_map
                .get(&document.pane_id)
                .copied()
                .unwrap_or_else(|| panes.keys().next().copied().unwrap_or(0));
            let path = (!document.file_path.is_empty()).then(|| PathBuf::from(&document.file_path));
            let mut editor = if let Some(content) = &document.unsaved_content {
                let mut editor = self.documents.from_content(path, content);
                editor.restore_recovery(
                    document
                        .extra
                        .get("rustDiskHash")
                        .and_then(serde_json::Value::as_str),
                );
                editor.scroll_top = document.scroll_top as usize;
                editor.scroll_left = document.scroll_left as usize;
                editor.document.set_cursor(Position::new(
                    document.cursor_line as usize,
                    document.cursor_column as usize,
                ));
                editor
            } else if let Some(path) = path {
                match self.documents.open(&path) {
                    Ok(mut editor) => {
                        editor.scroll_top = document.scroll_top as usize;
                        editor.scroll_left = document.scroll_left as usize;
                        editor.document.set_cursor(Position::new(
                            document.cursor_line as usize,
                            document.cursor_column as usize,
                        ));
                        editor
                    }
                    Err(_) => continue,
                }
            } else {
                continue;
            };
            if let Some(value) = document.extra.get("rustHunkEdit")
                && let (Some(root), Some(diff), Some(hunk)) = (
                    value
                        .get("root")
                        .and_then(|v| serde_json::from_value::<PathBuf>(v.clone()).ok()),
                    value.get("diff").and_then(|v| {
                        serde_json::from_value::<tide_services::git::diff::FileDiff>(v.clone()).ok()
                    }),
                    value
                        .get("hunk")
                        .and_then(serde_json::Value::as_u64)
                        .and_then(|hunk| usize::try_from(hunk).ok()),
                )
                && std::fs::canonicalize(&root).ok()
                    == std::fs::canonicalize(self.git.root(&self.workspace)).ok()
                && tide_services::git::diff::relative_path(&diff.path).is_ok()
                && hunk < diff.hunks.len()
            {
                editor.hunk_edit = Some(crate::editor::HunkEdit {
                    root,
                    diff: Arc::new(diff),
                    hunk,
                });
            }
            if let Some(value) = document.extra.get("rustHistory")
                && let Ok(history) = serde_json::from_value::<(PathBuf, String)>(value.clone())
            {
                editor.historical = Some(history);
                editor.document.mark_clean();
                editor.document.with_mut(|d| d.set_read_only(true));
            }
            if let (Some(line), Some(column)) = (
                document.selection_anchor_line,
                document.selection_anchor_column,
            ) {
                editor.document.set_selection(tide_core::Selection {
                    anchor: Position::new(line as usize, column as usize),
                    active: editor.document.cursor().position,
                });
            }
            editor.restore_folds(&document.folded_regions);
            if let Some(view) = document.extra.get("rustView") {
                editor.restore_view_state(view);
            }
            if let Some(pane) = panes.get_mut(&pane_id) {
                if document.is_active_in_pane {
                    pane.active = pane.tabs.len();
                }
                pane.tabs.push(Tab::Editor(editor));
            }
        }

        let scrollback = self
            .settings
            .get("tui.terminal.scrollback")
            .and_then(serde_json::Value::as_u64)
            .map_or(5_000, saturating_usize);
        for terminal in &session.terminals {
            let pane_id = pane_map
                .get(&terminal.pane_id)
                .copied()
                .unwrap_or_else(|| panes.keys().next().copied().unwrap_or(0));
            let cwd = PathBuf::from(&terminal.cwd);
            if let Ok(mut tab) = TerminalTab::new(0, &cwd, 80, 20, scrollback, None) {
                tab.title = terminal.title.clone();
                if let Some(pane) = panes.get_mut(&pane_id) {
                    if terminal.is_active_in_pane {
                        pane.active = pane.tabs.len();
                    }
                    pane.tabs.push(Tab::Terminal(tab));
                }
            }
        }

        let mut restored_ai_sessions = std::collections::BTreeSet::new();
        for ai_cli_pane in &session.ai_cli_panes {
            let pane_id = pane_map
                .get(&ai_cli_pane.pane_id)
                .copied()
                .unwrap_or_else(|| panes.keys().next().copied().unwrap_or(0));
            let cwd = PathBuf::from(&ai_cli_pane.cwd);
            let Some(provider) = CliProvider::parse(&ai_cli_pane.provider) else {
                continue;
            };
            if let Some(session_id) = &ai_cli_pane.session_id
                && !restored_ai_sessions.insert((provider.id().to_owned(), session_id.clone()))
            {
                continue;
            }
            let Some(status) = ai_cli::detect(provider).executable else {
                self.set_status(format!(
                    "AI CLI: {} is not installed; skipping restored tab",
                    provider.display_name()
                ));
                continue;
            };
            let args = provider.launch_args(&cwd, ai_cli_pane.session_id.as_deref());
            let mut spec = SpawnSpec::new(status.display().to_string(), args);
            spec.scrollback = scrollback;
            if let Ok(mut tab) = TerminalTab::spawn_configured(
                0,
                &cwd,
                spec,
                self.setting_bool("ai.autoStartCli", true),
            ) {
                tab.title = ai_cli_pane.title.clone();
                tab.ai_provider = Some(provider.id().to_owned());
                tab.ai_session_id = ai_cli_pane.session_id.clone();
                if let Some(pane) = panes.get_mut(&pane_id) {
                    if ai_cli_pane.is_active_in_pane {
                        pane.active = pane.tabs.len();
                    }
                    pane.tabs.push(Tab::Terminal(tab));
                }
            }
        }

        for sql_state in &session.sql_editors {
            let pane_id = pane_map
                .get(&sql_state.pane_id)
                .copied()
                .unwrap_or_else(|| panes.keys().next().copied().unwrap_or(0));
            let mut tab = SqlTab::from_content(&sql_state.content);
            if let Some(path) = sql_state.file_path.as_ref().map(PathBuf::from) {
                let conflict = self
                    .documents
                    .get(&path)
                    .is_some_and(|h| h.text() != sql_state.content);
                if !conflict {
                    tab.editor = self.documents.from_content(Some(path), &sql_state.content);
                    tab.editor.restore_recovery(
                        sql_state.extra.get("rustDiskHash").and_then(|v| v.as_str()),
                    );
                } else {
                    self.set_status("SQL recovery: conflicting file content kept as an independent query buffer");
                }
            }
            if sql_state.extra.get("rustDirty").and_then(|v| v.as_bool()) == Some(true) {
                tab.editor.document.mark_dirty();
            } else if tab.editor.path().is_none()
                || tab
                    .editor
                    .path()
                    .and_then(|p| std::fs::read_to_string(p).ok())
                    .as_deref()
                    == Some(&sql_state.content)
            {
                tab.editor.document.mark_clean();
            }
            tab.title = sql_state.title.clone();
            tab.editor.scroll_top = sql_state.scroll_top as usize;
            tab.editor.document.set_cursor(Position::new(
                sql_state.cursor_line as usize,
                sql_state.cursor_column as usize,
            ));
            if let Some(view) = sql_state.extra.get("rustView") {
                tab.editor.restore_view_state(view);
            }
            tab.view_mode = sql_state
                .extra
                .get("rustResultView")
                .and_then(|v| v.as_u64())
                .unwrap_or(0)
                .min(2) as u8;
            if let Some(connection) = sql_state.connection_id.clone() {
                let name = self
                    .db
                    .store
                    .get(&connection)
                    .map(|config| config.name.clone());
                tab.read_only = self.db.store.get(&connection).is_none_or(|c| c.read_only);
                tab.set_connection(Some(connection), name);
            }
            if let Some(pane) = panes.get_mut(&pane_id) {
                if sql_state.is_active_in_pane {
                    pane.active = pane.tabs.len();
                }
                pane.tabs.push(Tab::Sql(tab));
            }
        }

        for pane in panes.values_mut() {
            if pane.tabs.is_empty() {
                pane.tabs.push(Tab::Welcome);
            }
            pane.active = pane.active.min(pane.tabs.len().saturating_sub(1));
        }

        let focused = pane_map
            .get(&session.active_pane_id)
            .copied()
            .unwrap_or_else(|| panes.keys().next().copied().unwrap_or(0));
        self.workbench = Workbench::restore(root, panes, focused);
        self.sidebar_visible = session.ui.sidebar_visible;
        self.sidebar_width = session.ui.sidebar_width.max(12);
        self.sidebar_left = session.ui.sidebar_location.as_deref() != Some("right");
        self.agents_left = session.ui.agents_sidebar_location.as_deref() == Some("left");
        if let Some(sections) = session
            .ui
            .extra
            .get("rustSidebarSections")
            .and_then(serde_json::Value::as_array)
        {
            self.accordion.sections.clear();
            for id in sections
                .iter()
                .filter_map(serde_json::Value::as_str)
                .filter_map(SectionId::from_id)
            {
                self.accordion.add(id);
            }
        }
        self.accordion
            .set_expanded_ids(&session.ui.sidebar_accordion_expanded);
        let focus = session
            .ui
            .extra
            .get("rustSidebarFocused")
            .and_then(serde_json::Value::as_str)
            .and_then(SectionId::from_id)
            .or_else(|| {
                (session.ui.active_sidebar_panel == "git").then_some(SectionId::SourceControl)
            });
        if let Some(id) = focus
            && let Some(index) = self
                .accordion
                .sections
                .iter()
                .position(|section| section.id == id)
        {
            self.accordion.focus(index);
        }
        self.accordion.content_focus = session
            .ui
            .extra
            .get("rustSidebarContentFocus")
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(false);
        self.ai.agents_visible = session.ui.agents_sidebar_visible;
        self.ai.agents_width = session.ui.agents_sidebar_width.max(24);
        self.ai.agent_collapsed = ai_cli::CLI_PROVIDERS
            .iter()
            .filter(|provider| {
                !session
                    .ui
                    .agents_sidebar_expanded_providers
                    .iter()
                    .any(|id| id == provider.id())
            })
            .map(|provider| provider.id().to_owned())
            .collect();
        self.ai.agent_row = 0;
        self.ai.agent_scroll = 0;
        self.dock.visible = session.ui.terminal_visible;
        self.dock.height = session.ui.terminal_height.max(5);
        for (order, bottom) in session.bottom_terminals.iter().enumerate() {
            let cwd = PathBuf::from(&bottom.cwd);
            if let Ok(mut terminal) = TerminalTab::new(order + 1, &cwd, 80, 10, scrollback, None) {
                terminal.title = bottom.title.clone();
                if bottom.is_active {
                    self.dock.active = self.dock.tabs.len();
                }
                self.dock.tabs.push(terminal);
            }
        }
        if let Some(document_path) = session.active_document_path
            && let Some(path) = (!document_path.is_empty()).then(|| PathBuf::from(document_path))
            && self.workbench.focus_editor(&path)
        {
            self.focus = Focus::Main;
        } else {
            self.focus = match session.ui.focused_region.as_deref() {
                Some("sidebar") => Focus::Sidebar,
                Some("agents") => Focus::Agents,
                Some("bottom-terminal") => Focus::Dock,
                Some("status") => Focus::Status,
                _ => Focus::Main,
            };
        }
        let lsp_starts = self
            .workbench
            .pane_ids()
            .into_iter()
            .filter_map(|id| self.workbench.pane(id))
            .flat_map(|pane| pane.tabs.iter())
            .filter_map(|tab| {
                tab.editor().and_then(|editor| {
                    editor.path().map(|path| {
                        (
                            path.to_path_buf(),
                            editor.document.text(),
                            tide_core::num::saturating_i64(editor.revision()),
                        )
                    })
                })
            })
            .collect::<Vec<_>>();
        for (path, text, version) in lsp_starts {
            self.lsp
                .open_document(&path, text, version, &self.settings, &self.workspace);
        }
        self.session_dirty = false;
        Ok(())
    }
}

/// Writes `session` unless a newer snapshot is already on disk. Saves are
/// serialized, so a slow background autosave never overwrites a later save.
pub(super) fn write_session(
    saves: &std::sync::Mutex<u64>,
    ticket: u64,
    store: &SessionStore,
    session: &SessionState,
    name: Option<&str>,
) -> Result<()> {
    let mut written = crate::lock(saves);
    if *written > ticket {
        return Ok(());
    }
    store.save(session, name)?;
    *written = ticket;
    Ok(())
}

pub(super) fn session_layout(node: &LayoutNode) -> SessionLayoutNode {
    match node {
        LayoutNode::Leaf(id) => SessionLayoutNode {
            node_type: "leaf".to_owned(),
            pane_id: Some(format!("pane-{id}")),
            children: None,
            ratios: None,
            extra: BTreeMap::new(),
        },
        LayoutNode::Split {
            direction,
            children,
            ratios,
        } => SessionLayoutNode {
            node_type: match direction {
                SplitDirection::Vertical => "horizontal".to_owned(),
                SplitDirection::Horizontal => "vertical".to_owned(),
            },
            pane_id: None,
            children: Some(children.iter().map(session_layout).collect()),
            ratios: Some(ratios.iter().map(|ratio| f64::from(*ratio)).collect()),
            extra: BTreeMap::new(),
        },
    }
}

pub(super) fn convert_session_layout(
    node: &SessionLayoutNode,
    pane_map: &mut HashMap<String, PaneId>,
    next_pane: &mut PaneId,
) -> LayoutNode {
    if node.node_type == "leaf" {
        let id = if let Some(pane_id) = &node.pane_id {
            *pane_map.entry(pane_id.clone()).or_insert_with(|| {
                let id = *next_pane;
                *next_pane += 1;
                id
            })
        } else {
            let id = *next_pane;
            *next_pane += 1;
            id
        };
        return LayoutNode::Leaf(id);
    }
    let direction = if node.node_type == "horizontal" {
        SplitDirection::Vertical
    } else {
        SplitDirection::Horizontal
    };
    let children = node
        .children
        .as_ref()
        .map(|children| {
            children
                .iter()
                .map(|child| convert_session_layout(child, pane_map, next_pane))
                .collect()
        })
        .unwrap_or_else(|| vec![LayoutNode::Leaf(0)]);
    let ratios = node
        .ratios
        .as_ref()
        // A ratio outside the f32 range is malformed; use the even split.
        .and_then(|ratios| ratios.iter().map(ToPrimitive::to_f32).collect())
        .unwrap_or_else(|| vec![1.0 / children.len() as f32; children.len()]);
    LayoutNode::Split {
        direction,
        children,
        ratios,
    }
}
