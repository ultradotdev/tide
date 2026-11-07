//! Keyboard and paste routing: bindings, `when` clauses, and per-region key handlers.
use super::*;

impl App {
    pub(super) fn rebuild_keymap(&mut self) {
        self.keymap.clear();
        for binding in &self.keybindings {
            let Ok(command) = binding.command.parse() else {
                continue;
            };
            self.keymap
                .entry(canonical_key(&binding.key))
                .or_insert(command);
        }
    }

    /// Terminal-safe alternates for Ctrl+Shift combinations. The original
    /// bindings remain active for terminals that transmit them correctly.
    pub(super) fn install_alternate_bindings(&mut self) {
        crate::catalog::install_bindings(&mut self.keybindings);
    }

    pub(super) fn handle_paste(&mut self, text: &str) {
        self.pointer.agent_click = None;
        if self.overlay.as_ref().is_some_and(|o| o.is_popup()) {
            self.overlay = None;
            self.lsp.invalidate();
        }
        if self.focus == Focus::Agents && self.overlay.is_none() {
            if self.ai.agent_search {
                self.ai.agent_query.extend(
                    text.chars()
                        .filter(|ch| !ch.is_control())
                        .take(512usize.saturating_sub(self.ai.agent_query.chars().count())),
                );
                self.ai.agent_row = 0;
                self.ai.sync_selection();
            }
            return;
        }
        if matches!(self.focus, Focus::Sidebar | Focus::Status) && self.overlay.is_none() {
            return;
        }

        if let Some(mut overlay) = self.overlay.take() {
            overlay.paste(text);
            if *overlay.kind() == ModalKind::Find {
                self.last_find_query = overlay.input().to_owned();
                self.last_replace_text = overlay.base().replacement.clone();
                self.search_feedback(overlay.as_mut());
            }
            self.overlay = Some(overlay);
            return;
        }
        if let Some(terminal) = self.active_terminal_mut() {
            terminal.send_paste(text);
            return;
        }
        if matches!(self.workbench.active_tab(),Some(Tab::Sql(sql)) if sql.results_focused) {
            return;
        }
        if let Some(editor) = self.workbench.active_editor_mut() {
            editor.insert_text(text);
            self.mark_session_dirty();
        }
    }

    pub(super) fn handle_input_key(&mut self, key: KeyEvent) {
        self.pointer.agent_click = None;
        if key.code == KeyCode::Esc {
            if let Some(editor) = self.workbench.active_editor_mut() {
                editor.inline_signature = None;
            }
            self.lsp.cancel();
        }
        let before = self.workbench.active_editor().map(|e| {
            (
                e.document.clone(),
                e.revision(),
                e.document.cursor().position,
            )
        });
        let edit_key = key.modifiers.difference(KeyModifiers::SHIFT).is_empty()
            && matches!(
                key.code,
                KeyCode::Char(_) | KeyCode::Backspace | KeyCode::Delete
            );
        if edit_key && self.overlay.as_ref().is_some_and(|o| o.is_popup()) {
            self.overlay = None;
        }
        let typing = edit_key
            || key.code == KeyCode::BackTab
            || (key.modifiers.is_empty() && matches!(key.code, KeyCode::Enter | KeyCode::Tab));
        let help = self
            .workbench
            .active_editor()
            .and_then(|editor| editor.help);
        let help = help.filter(|_| typing && self.overlay.is_none() && self.focus == Focus::Main);
        self.handle_key(key);
        if let Some(page) = help
            && self.overlay.is_none()
            && self
                .workbench
                .active_editor()
                .and_then(|editor| editor.help)
                .is_some()
        {
            self.set_status(format!("{} is read-only", page.title));
        }
        if let (Some((document, revision, position)), Some(editor)) =
            (before, self.workbench.active_editor())
            && document.same_view(&editor.document)
            && (revision != editor.revision() || position != editor.document.cursor().position)
        {
            self.lsp.invalidate();
            if self.overlay.as_ref().is_some_and(|o| o.is_popup()) {
                self.overlay = None;
            }
            if edit_key
                && revision != editor.revision()
                && self.setting_bool("lsp.enabled", true)
                && editor.path().is_some()
                && let Some(kind) = LspRequestKind::typed(key.code, &self.settings)
            {
                self.lsp.pending = Some((Instant::now(), kind));
            }
        }
    }

    pub(super) fn handle_key(&mut self, key: KeyEvent) {
        if key.code == KeyCode::Esc {
            self.pending_chord = None;
        }
        if self.overlay.is_some() {
            if let Some((prefix, started)) = self.pending_chord.take()
                && started.elapsed() < Duration::from_millis(750)
                && let Some(stroke) = key_string(key)
                && let Some(action) = self.resolve_binding(&format!("{prefix} {stroke}"))
            {
                self.overlay = None;
                self.run_binding(action);
                return;
            }
            self.handle_modal_key(key);
            return;
        }

        if self.focus_in_terminal() && self.is_terminal_control_key(key) {
            if let Some(terminal) = self.active_terminal_mut() {
                terminal.send_key(key);
            }
            return;
        }

        if (self.is_global_key(key)
            || self.pending_chord.is_some()
            || (self.focus == Focus::Sidebar && self.accordion.content_focus)
            || self.focus == Focus::Agents
            || key.code == KeyCode::Esc)
            && let Some(action) = self.resolve_global_key(key)
        {
            self.run_binding(action);
            return;
        }

        match self.focus {
            Focus::Sidebar => self.handle_sidebar_key(key),
            Focus::Agents => {
                self.pointer.agent_click = None;
                self.with_ai(|ai, ui| ai.handle_agents_key(key, ui));
            }
            Focus::Dock => {
                if let Some(terminal) = self.dock.active_mut() {
                    terminal.send_key(key);
                }
            }
            Focus::Main => self.handle_main_key(key),
            Focus::Status => self.handle_status_key(key),
        }
    }

    pub(super) fn is_global_key(&self, key: KeyEvent) -> bool {
        key.modifiers.intersects(
            KeyModifiers::CONTROL | KeyModifiers::ALT | KeyModifiers::SUPER | KeyModifiers::META,
        ) || matches!(key.code, KeyCode::F(_))
    }

    pub(super) fn is_terminal_control_key(&self, key: KeyEvent) -> bool {
        if !key.modifiers.contains(KeyModifiers::CONTROL) {
            return false;
        }
        // Shifted shortcuts belong to the workbench (Ctrl+Shift+P, Ctrl+Shift+G,
        // Ctrl+Shift+T, ...). Plain Ctrl+letter and Ctrl+symbols go to the child.
        if key.modifiers.intersects(
            KeyModifiers::SHIFT | KeyModifiers::ALT | KeyModifiers::SUPER | KeyModifiers::META,
        ) {
            return false;
        }
        if key.code == KeyCode::Char('`') {
            return false;
        }
        if let KeyCode::Char(ch) = key.code {
            let name = match ch.to_ascii_lowercase() {
                ' ' => "space".to_owned(),
                other => other.to_string(),
            };
            return TERMINAL_CONTROL_KEYS.contains(&name.as_str());
        }
        false
    }

    pub(super) fn run_binding(&mut self, action: BindingAction) {
        if let BindingAction::Run(command) = action {
            self.execute_command(command);
        }
    }

    pub(super) fn resolve_global_key(&mut self, event: KeyEvent) -> Option<BindingAction> {
        let key = key_string(event)?;
        if let Some((prefix, started)) = self.pending_chord.take()
            && started.elapsed() < Duration::from_millis(750)
        {
            return self
                .resolve_binding(&format!("{prefix} {key}"))
                .or(Some(BindingAction::Unbound));
        }
        if self.keybindings.iter().any(|binding| {
            command_supported(&binding.command)
                && binding
                    .when
                    .as_deref()
                    .is_none_or(|when| self.eval_when(when))
                && canonical_key(&binding.key).starts_with(&(key.clone() + " "))
        }) {
            let immediate = self
                .resolve_binding(&key)
                .filter(|action| *action == BindingAction::Run(Command::WorkbenchCommandPalette));
            self.pending_chord = Some((key, Instant::now()));
            return immediate.or(Some(BindingAction::Unbound));
        }
        self.resolve_binding(&key)
    }

    /// Resolve a canonical key through `when` contexts. Context-specific bindings
    /// win over unconditional bindings for the same key; otherwise catalog order
    /// decides. User bindings keep their merged position.
    pub(super) fn resolve_binding(&self, key: &str) -> Option<BindingAction> {
        let key = canonical_key(key);
        let candidates = self
            .keybindings
            .iter()
            .filter(|binding| canonical_key(&binding.key) == key)
            .filter(|binding| self.binding_applies(binding))
            .collect::<Vec<_>>();
        winning_binding(&candidates).map(|binding| {
            binding
                .command
                .parse()
                .map_or(BindingAction::Unbound, BindingAction::Run)
        })
    }

    /// Whether `binding` takes part in resolving its key right now: it runs a routed
    /// command or removes a key, and its `when` context holds.
    fn binding_applies(&self, binding: &Keybinding) -> bool {
        (command_supported(&binding.command)
            || binding.command.is_empty()
            || binding.command.starts_with('-'))
            && binding
                .when
                .as_deref()
                .is_none_or(|expr| self.eval_when(expr))
    }

    /// The binding each canonical key resolves to in the current context, as
    /// [`Self::resolve_binding`] would choose it, for every key at once.
    pub(super) fn resolved_keys(&self) -> HashMap<String, &Keybinding> {
        let mut candidates: HashMap<String, Vec<&Keybinding>> = HashMap::new();
        for binding in &self.keybindings {
            if self.binding_applies(binding) {
                candidates
                    .entry(canonical_key(&binding.key))
                    .or_default()
                    .push(binding);
            }
        }
        candidates
            .into_iter()
            .filter_map(|(key, bindings)| Some((key, winning_binding(&bindings)?)))
            .collect()
    }

    pub(super) fn eval_when(&self, expression: &str) -> bool {
        let context = |name: &str| self.context_active(name);
        let mut parser = WhenParser::new(expression, &context);
        parser.parse().unwrap_or(false)
    }

    pub(super) fn context_active(&self, name: &str) -> bool {
        let sidebar_content = self.focus == Focus::Sidebar && self.accordion.content_focus;
        match name {
            "editorFocus" => {
                self.focus == Focus::Main
                    && matches!(self.workbench.active_tab(), Some(Tab::Editor(_)))
            }
            "editorHasSelection" => self
                .workbench
                .active_editor()
                .and_then(|editor| editor.document.selection_positions())
                .is_some_and(|(start, end)| start != end),
            "editorHasMultipleCursors" => self
                .workbench
                .active_editor()
                .is_some_and(|editor| !editor.extra_cursors().is_empty()),
            "fileTreeFocus" => {
                sidebar_content && self.accordion.focused_section() == SectionId::Explorer
            }
            "gitPanelFocus" => {
                sidebar_content && self.accordion.focused_section() == SectionId::SourceControl
            }
            "outlinePanelFocus" => {
                sidebar_content && self.accordion.focused_section() == SectionId::Outline
            }
            "timelinePanelFocus" => {
                sidebar_content && self.accordion.focused_section() == SectionId::Timeline
            }
            "sqlEditorFocus" => {
                self.focus == Focus::Main
                    && matches!(self.workbench.active_tab(), Some(Tab::Sql(sql)) if !sql.results_focused)
            }
            "queryResultsFocus" => {
                self.focus == Focus::Main
                    && matches!(self.workbench.active_tab(),Some(Tab::Sql(sql)) if sql.results_focused && !sql.details_mode)
            }
            "agentsFocus" => self.focus == Focus::Agents,
            "terminalFocus" => self.focus_in_terminal(),
            "modalFocus" => self.overlay.is_some(),
            _ => false,
        }
    }

    pub(super) fn handle_main_key(&mut self, key: KeyEvent) {
        if let Some(editor) = self.workbench.active_editor_mut()
            && editor.inline_focus
        {
            if key.code == KeyCode::Esc {
                editor.inline_focus = false;
                return;
            }
            if let Some(browser) = &mut editor.inline {
                let action = browser.key(key);
                if let Some(action) = action {
                    with_ui!(self, |ui| self.git.diff_action(action, ui));
                }
                return;
            }
        }
        if let Some(Tab::Diff(diff)) = self.workbench.active_tab_mut()
            && let Some(browser) = &mut diff.browser
        {
            let action = browser.key(key);
            if let Some(action) = action {
                with_ui!(self, |ui| self.git.diff_action(action, ui));
            }
            return;
        }
        if matches!(self.workbench.active_tab(), Some(Tab::Sql(_))) {
            self.with_db(|db, ui| db.handle_key(key, ui));
            return;
        }
        // A Markdown preview scrolls; it has no cursor and takes no typing.
        if let Some(editor) = self.workbench.active_editor_mut()
            && editor.is_previewing()
        {
            match key.code {
                KeyCode::Up => editor.scroll_by(-1),
                KeyCode::Down => editor.scroll_by(1),
                KeyCode::PageUp => editor.scroll_by(-20),
                KeyCode::PageDown | KeyCode::Char(' ') => editor.scroll_by(20),
                KeyCode::Home => editor.scroll_by(isize::MIN),
                KeyCode::End => editor.scroll_by(isize::MAX),
                _ => {}
            }
            return;
        }
        let defaults = self.indent_defaults();
        let relative = self
            .settings
            .get("editor.lineNumbers")
            .and_then(serde_json::Value::as_str)
            == Some("relative");
        let auto_indent = self
            .settings
            .get("editor.autoIndent")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("full")
            .to_owned();
        if let Some(editor) = self.workbench.active_editor_mut() {
            editor.auto_indent = auto_indent;
            editor.configure_editing(defaults, relative);
            editor.follow_cursor();
        }
        let shift = key.modifiers.contains(KeyModifiers::SHIFT);
        // Ctrl (Option on macOS, which terminals send as Alt) moves by word; Cmd, which
        // only terminals with the kitty keyboard protocol report, moves to line and
        // document edges.
        let by_word = key
            .modifiers
            .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT);
        let command = key
            .modifiers
            .intersects(KeyModifiers::SUPER | KeyModifiers::META);
        match self.workbench.active_tab_mut() {
            Some(Tab::Editor(editor)) => match key.code {
                // Emacs and readline word keys, which macOS terminals send for Option+Left,
                // Option+Right and Option+Delete.
                KeyCode::Char('b') if key.modifiers == KeyModifiers::ALT => {
                    editor.move_word(false, false)
                }
                KeyCode::Char('f') if key.modifiers == KeyModifiers::ALT => {
                    editor.move_word(true, false)
                }
                KeyCode::Char('d') if key.modifiers == KeyModifiers::ALT => {
                    editor.delete_word(true);
                    self.mark_session_dirty();
                }
                KeyCode::Char(ch)
                    if !key.modifiers.intersects(
                        KeyModifiers::CONTROL | KeyModifiers::ALT | KeyModifiers::SUPER,
                    ) =>
                {
                    editor.insert_char(ch);
                    self.mark_session_dirty();
                }
                KeyCode::Enter => {
                    editor.newline();
                    self.mark_session_dirty();
                }
                KeyCode::Tab if editor.selection_spans_lines() => {
                    editor.indent_lines(false, editor.tab_width());
                    self.mark_session_dirty();
                }
                KeyCode::Tab => {
                    editor.insert_tab();
                    self.mark_session_dirty();
                }
                KeyCode::BackTab => {
                    editor.indent_lines(true, editor.tab_width());
                    self.mark_session_dirty();
                }
                KeyCode::Backspace if by_word => {
                    editor.delete_word(false);
                    self.mark_session_dirty();
                }
                KeyCode::Backspace => {
                    editor.backspace();
                    self.mark_session_dirty();
                }
                KeyCode::Delete if by_word => {
                    editor.delete_word(true);
                    self.mark_session_dirty();
                }
                KeyCode::Delete => {
                    editor.delete_forward();
                    self.mark_session_dirty();
                }
                KeyCode::Left if command => editor.home(shift),
                KeyCode::Right if command => editor.end(shift),
                KeyCode::Up if command => editor.move_to_document_edge(false, shift),
                KeyCode::Down if command => editor.move_to_document_edge(true, shift),
                KeyCode::Left if by_word => editor.move_word(false, shift),
                KeyCode::Right if by_word => editor.move_word(true, shift),
                KeyCode::Left => editor.move_left(shift),
                KeyCode::Right => editor.move_right(shift),
                KeyCode::Up => editor.move_up(shift),
                KeyCode::Down => editor.move_down(shift),
                KeyCode::Home if by_word => editor.move_to_document_edge(false, shift),
                KeyCode::End if by_word => editor.move_to_document_edge(true, shift),
                KeyCode::Home => editor.home(shift),
                KeyCode::End => editor.end(shift),
                KeyCode::PageUp => editor.page_up(20, key.modifiers.contains(KeyModifiers::SHIFT)),
                KeyCode::PageDown => {
                    editor.page_down(20, key.modifiers.contains(KeyModifiers::SHIFT))
                }
                KeyCode::Esc => editor.clear_selection(),
                _ => {}
            },
            Some(Tab::Terminal(terminal)) => {
                terminal.send_key(key);
            }
            Some(Tab::Diff(diff)) => match key.code {
                KeyCode::Up => diff.scroll_top = diff.scroll_top.saturating_sub(1),
                KeyCode::Down => diff.scroll_top += 1,
                KeyCode::PageUp => diff.scroll_top = diff.scroll_top.saturating_sub(20),
                KeyCode::PageDown => diff.scroll_top += 20,
                _ => {}
            },
            Some(Tab::Sql(_)) => {}
            Some(Tab::Welcome) | None => {}
        }
    }

    pub(super) fn handle_sidebar_key(&mut self, key: KeyEvent) {
        if key
            .modifiers
            .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT | KeyModifiers::SUPER)
        {
            return;
        }
        if self.accordion.content_focus {
            if key.code == KeyCode::Esc {
                self.accordion.leave_content();
                return;
            }
            match self.accordion.focused_section() {
                SectionId::Explorer => self.handle_file_tree_key(key),
                SectionId::SourceControl => with_ui!(self, |ui| self.git.handle_key(key, ui)),
                SectionId::Outline => self.handle_outline_key(key),
                SectionId::Timeline => with_ui!(self, |ui| self.git.handle_timeline_key(key, ui)),
            }
            return;
        }
        match self.accordion.handle_key(key) {
            AccordionAction::Blur => self.focus = Focus::Main,
            AccordionAction::Toggle(index) | AccordionAction::Expand(index) => {
                self.accordion.focus(index);
            }
            _ => {}
        }
    }

    pub(super) fn toggle_outline(&mut self, expand: Option<bool>) {
        if let Some((line, _)) = self.outline_for_active().get(self.outline_selected) {
            let line = *line;
            if expand.unwrap_or(self.outline_collapsed.contains(&line)) {
                self.outline_collapsed.remove(&line);
            } else {
                self.outline_collapsed.insert(line);
            }
        }
    }

    pub(super) fn handle_outline_key(&mut self, key: KeyEvent) {
        if matches!(key.code, KeyCode::Left | KeyCode::Right) {
            self.toggle_outline(Some(key.code == KeyCode::Right));
            return;
        }
        let count = self.outline_for_active().len();
        if count == 0 {
            return;
        }
        match key.code {
            KeyCode::Char('j') | KeyCode::Down => {
                self.outline_selected = (self.outline_selected + 1).min(count - 1);
            }
            KeyCode::Char('k') | KeyCode::Up => {
                self.outline_selected = self.outline_selected.saturating_sub(1);
            }
            KeyCode::Home => self.outline_selected = 0,
            KeyCode::End => self.outline_selected = count - 1,
            KeyCode::PageDown => {
                self.outline_selected = (self.outline_selected + 10).min(count - 1);
            }
            KeyCode::PageUp => self.outline_selected = self.outline_selected.saturating_sub(10),
            KeyCode::Enter | KeyCode::Char(' ') => {
                if let Some((line, _)) = self.outline_for_active().get(self.outline_selected)
                    && let Some(editor) = self.workbench.active_editor_mut()
                {
                    editor.goto_line(line + 1);
                }
            }
            _ => {}
        }
    }

    pub(super) fn handle_file_tree_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Char('j') | KeyCode::Down => self.file_tree.move_down(),
            KeyCode::Char('k') | KeyCode::Up => self.file_tree.move_up(),
            KeyCode::Char('h') | KeyCode::Left => self.file_tree.collapse_selected(),
            KeyCode::Char('l') | KeyCode::Right => self.file_tree.expand_selected(),
            KeyCode::Enter | KeyCode::Char(' ') => {
                if let Some(path) = self.file_tree.activate_selected() {
                    self.open_file(path);
                }
            }
            KeyCode::Char('r') => self.file_tree.refresh(),
            KeyCode::Char('.') => self.file_tree.toggle_hidden(),
            _ => {}
        }
    }

    pub(super) fn handle_status_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Left => self.status_action = self.status_action.saturating_sub(1),
            KeyCode::Right => {
                self.status_action =
                    (self.status_action + 1).min(self.layout.status_targets.len().saturating_sub(1))
            }
            KeyCode::Enter | KeyCode::Char(' ') => {
                if let Some(command) = self.layout.status_targets.get(self.status_action).copied() {
                    self.execute_command(command);
                }
            }
            KeyCode::Esc => self.focus = Focus::Main,
            _ => {}
        }
    }

    pub(super) fn search_feedback(&mut self, overlay: &mut dyn Modal) {
        let query = overlay.input().to_owned();
        let count = self
            .workbench
            .active_editor()
            .map(|editor| editor.search_ranges(&query, self.search_options))
            .transpose();
        overlay.base_mut().message = None;
        let count = match count {
            Ok(Some(ranges)) => ranges.len().to_string(),
            Ok(None) => "0".into(),
            Err(error) => {
                overlay.base_mut().message = Some(error.clone());
                "invalid".into()
            }
        };
        let base = overlay.base_mut();
        base.search_matches = count.parse().ok();
        base.title = format!(
            "{} · {count} matches",
            if base.replace_mode {
                "Find and Replace"
            } else {
                "Find"
            }
        );
        base.checked_actions = [
            (2, self.search_options.case_sensitive),
            (3, self.search_options.whole_word),
            (4, self.search_options.regex),
            (5, self.search_options.selection.is_some()),
        ]
        .into_iter()
        .filter_map(|(index, checked)| checked.then_some(index))
        .collect();
    }

    pub(super) fn handle_modal_key(&mut self, key: KeyEvent) {
        let find = self
            .overlay
            .as_ref()
            .is_some_and(|o| *o.kind() == ModalKind::Find);
        let modal_binding = if find
            && key
                .modifiers
                .intersects(KeyModifiers::CONTROL | KeyModifiers::META | KeyModifiers::SUPER)
        {
            key_string(key).and_then(|stroke| self.resolve_binding(&stroke))
        } else {
            None
        };
        let Some(mut overlay) = self.overlay.take() else {
            return;
        };
        if find {
            self.handle_find_key(overlay, key, modal_binding);
            return;
        }
        match overlay.handle_key(key) {
            ModalOutcome::Close => self.close_modal(overlay.as_ref()),
            ModalOutcome::SaveBeforeConfirm => {
                if let ModalKind::Confirm(action) = overlay.kind() {
                    let title = overlay.base().title.clone();
                    let message = overlay.base().message.clone().unwrap_or_default();
                    self.begin_save_before(action.clone(), title, message);
                }
            }
            ModalOutcome::None | ModalOutcome::FindNext | ModalOutcome::SearchAction(_) => {
                self.overlay = Some(overlay);
            }
            ModalOutcome::Submit(submitted) => self.submit_modal(overlay, submitted),
        }
    }

    pub(super) fn handle_find_key(
        &mut self,
        mut overlay: Box<dyn Modal>,
        key: KeyEvent,
        binding: Option<BindingAction>,
    ) {
        self.last_find_query = overlay.input().to_owned();
        self.last_replace_text = overlay.base().replacement.clone();
        if key
            .modifiers
            .intersects(KeyModifiers::CONTROL | KeyModifiers::META | KeyModifiers::SUPER)
            && let Some(action) = binding
        {
            let command = match action {
                BindingAction::Run(command) => Some(command),
                BindingAction::Unbound => None,
            };
            if matches!(
                command,
                Some(
                    Command::EditSelectAll
                        | Command::EditCopy
                        | Command::EditCut
                        | Command::EditPaste
                        | Command::EditUndo
                        | Command::EditRedo
                )
            ) && !overlay.base().actions_focused
            {
                let replacement = overlay.base().replacement_focused;
                if command == Some(Command::EditSelectAll) {
                    if replacement {
                        overlay.base_mut().replacement_selected = true;
                    } else {
                        overlay.base_mut().field_selected = true;
                    }
                } else if matches!(command, Some(Command::EditCopy | Command::EditCut)) {
                    let selected = if replacement {
                        overlay.base().replacement_selected
                    } else {
                        overlay.base().field_selected
                    };
                    if selected {
                        self.clipboard = if replacement {
                            overlay.base().replacement.clone()
                        } else {
                            overlay.input().to_owned()
                        };
                        self.export_clipboard(None, None);
                        if command == Some(Command::EditCut) {
                            if replacement {
                                overlay.base_mut().replacement.clear();
                                overlay.base_mut().replacement_cursor = 0;
                                overlay.base_mut().replacement_selected = false;
                            } else {
                                overlay.base_mut().input.clear();
                                overlay.base_mut().cursor = 0;
                                overlay.base_mut().field_selected = false;
                            }
                        }
                    }
                }
                self.last_find_query = overlay.input().to_owned();
                self.last_replace_text = overlay.base().replacement.clone();
                self.search_feedback(overlay.as_mut());
                self.overlay = Some(overlay);
                if command == Some(Command::EditPaste) {
                    self.paste_clipboard();
                }
                return;
            }
            if command == Some(Command::SearchReplace) {
                let base = overlay.base_mut();
                base.replace_mode = !base.replace_mode;
                base.replacement_focused = false;
                base.actions_focused = false;
                self.search_feedback(overlay.as_mut());
                self.overlay = Some(overlay);
                return;
            }
            if command == Some(Command::SearchFind) {
                if overlay.base().replace_mode {
                    let base = overlay.base_mut();
                    base.replace_mode = false;
                    base.replacement_focused = false;
                    base.actions_focused = false;
                    self.search_feedback(overlay.as_mut());
                    self.overlay = Some(overlay);
                }
                return;
            }
            self.run_binding(action);
            return;
        }
        if key.modifiers.contains(KeyModifiers::ALT) {
            match key.code {
                KeyCode::Char('c') => {
                    self.search_options.case_sensitive = !self.search_options.case_sensitive
                }
                KeyCode::Char('w') => {
                    self.search_options.whole_word = !self.search_options.whole_word
                }
                KeyCode::Char('r') => self.search_options.regex = !self.search_options.regex,
                KeyCode::Char('s') => {
                    self.search_options.selection = if self.search_options.selection.is_some() {
                        None
                    } else {
                        self.workbench
                            .active_editor()
                            .and_then(|e| e.document.cursor().range())
                    }
                }
                _ => {
                    self.overlay = Some(overlay);
                    return;
                }
            }
            self.search_feedback(overlay.as_mut());
            self.overlay = Some(overlay);
            return;
        }
        let outcome = overlay.handle_key(key);
        self.last_find_query = overlay.input().to_owned();
        self.last_replace_text = overlay.base().replacement.clone();
        match outcome {
            ModalOutcome::Close | ModalOutcome::SaveBeforeConfirm => {}
            ModalOutcome::None => {
                self.search_feedback(overlay.as_mut());
                self.overlay = Some(overlay);
            }
            ModalOutcome::SearchAction(index) => {
                self.last_find_query = overlay.input().to_owned();
                match index {
                    0 => {
                        self.find_next();
                        self.overlay = Some(overlay);
                    }
                    1 => {
                        self.find_previous();
                        self.overlay = Some(overlay);
                    }
                    2..=5 => {
                        self.overlay = Some(overlay);
                        self.handle_modal_key(KeyEvent::new(
                            KeyCode::Char(['c', 'w', 'r', 's'][index - 2]),
                            KeyModifiers::ALT,
                        ));
                    }
                    6 | 7 => {
                        if !overlay.base().replace_mode {
                            let base = overlay.base_mut();
                            base.replace_mode = true;
                            base.actions_focused = false;
                            base.replacement_focused = true;
                        } else if let Some(editor) = self.workbench.active_editor_mut() {
                            let scope = self.search_options.selection.map(|range| {
                                editor.document.with(|doc| {
                                    (
                                        doc.position_to_char(range.start),
                                        doc.position_to_char(range.end),
                                        doc.len_chars(),
                                    )
                                })
                            });
                            match editor.replace_matches(
                                &self.last_find_query,
                                &overlay.base().replacement,
                                self.search_options,
                                index == 7,
                            ) {
                                Ok(count) => {
                                    if let Some((start, end, before)) = scope {
                                        self.search_options.selection =
                                            Some(editor.document.with(|doc| {
                                                let end = if doc.len_chars() >= before {
                                                    end.saturating_add(doc.len_chars() - before)
                                                } else {
                                                    end.saturating_sub(before - doc.len_chars())
                                                };
                                                tide_core::Range::new(
                                                    doc.char_to_position(start),
                                                    doc.char_to_position(end.max(start)),
                                                )
                                            }));
                                    }
                                    if count > 0 {
                                        self.mark_session_dirty();
                                    }
                                    self.set_status(format!("Replaced {count} occurrence(s)"));
                                }
                                Err(error) => overlay.base_mut().message = Some(error),
                            }
                        }
                        self.search_feedback(overlay.as_mut());
                        self.overlay = Some(overlay);
                    }
                    _ => {}
                }
            }
            ModalOutcome::FindNext | ModalOutcome::Submit(_) => {
                self.find_next();
                self.overlay = Some(overlay);
            }
        }
    }

    pub(super) fn outline_key(&mut self, code: KeyCode) {
        if self.reveal_outline() {
            self.handle_outline_key(KeyEvent::new(code, KeyModifiers::NONE));
        }
    }

    pub(super) fn timeline_key(&mut self, code: KeyCode) {
        if self.reveal_timeline("move through it") {
            with_ui!(self, |ui| self
                .git
                .handle_timeline_key(KeyEvent::new(code, KeyModifiers::NONE), ui));
        }
    }

    pub(super) fn git_panel_key(&mut self, code: KeyCode) {
        if !self.reveal_panel(SectionId::SourceControl, "move through changes") {
            return;
        }
        if self.git.has_changes() {
            with_ui!(self, |ui| self
                .git
                .handle_key(KeyEvent::new(code, KeyModifiers::NONE), ui));
        } else {
            self.set_status("Source Control has no changes to select");
        }
    }
}

pub(super) fn unshift_char(ch: char) -> Option<char> {
    match ch {
        '|' => Some('\\'),
        '{' => Some('['),
        '}' => Some(']'),
        ':' => Some(';'),
        '"' => Some('\''),
        '<' => Some(','),
        '>' => Some('.'),
        '?' => Some('/'),
        '~' => Some('`'),
        '!' => Some('1'),
        '@' => Some('2'),
        '#' => Some('3'),
        '$' => Some('4'),
        '%' => Some('5'),
        '^' => Some('6'),
        '&' => Some('7'),
        '*' => Some('8'),
        '(' => Some('9'),
        ')' => Some('0'),
        '_' => Some('-'),
        '+' => Some('='),
        _ => None,
    }
}

pub(super) struct WhenParser<'a> {
    tokens: Vec<&'a str>,
    index: usize,
    context: &'a dyn Fn(&str) -> bool,
}

impl<'a> WhenParser<'a> {
    fn new(expression: &'a str, context: &'a dyn Fn(&str) -> bool) -> Self {
        let mut tokens = Vec::new();
        let mut index = 0;
        let bytes = expression.as_bytes();
        while index < bytes.len() {
            match bytes[index] {
                b' ' | b'\t' => index += 1,
                b'(' | b')' | b'!' => {
                    tokens.push(&expression[index..index + 1]);
                    index += 1;
                }
                b'&' if bytes.get(index + 1) == Some(&b'&') => {
                    tokens.push("&&");
                    index += 2;
                }
                b'|' if bytes.get(index + 1) == Some(&b'|') => {
                    tokens.push("||");
                    index += 2;
                }
                _ => {
                    let start = index;
                    while index < bytes.len()
                        && (bytes[index].is_ascii_alphanumeric() || bytes[index] == b'_')
                    {
                        index += 1;
                    }
                    if start == index {
                        index += 1;
                    } else {
                        tokens.push(&expression[start..index]);
                    }
                }
            }
        }
        Self {
            tokens,
            index: 0,
            context,
        }
    }

    fn parse(&mut self) -> Option<bool> {
        let value = self.parse_or()?;
        (self.index == self.tokens.len()).then_some(value)
    }

    fn parse_or(&mut self) -> Option<bool> {
        let mut value = self.parse_and()?;
        while self.tokens.get(self.index) == Some(&"||") {
            self.index += 1;
            let right = self.parse_and()?;
            value = value || right;
        }
        Some(value)
    }

    fn parse_and(&mut self) -> Option<bool> {
        let mut value = self.parse_unary()?;
        while self.tokens.get(self.index) == Some(&"&&") {
            self.index += 1;
            let right = self.parse_unary()?;
            value = value && right;
        }
        Some(value)
    }

    fn parse_unary(&mut self) -> Option<bool> {
        if self.tokens.get(self.index) == Some(&"!") {
            self.index += 1;
            return Some(!self.parse_unary()?);
        }
        if self.tokens.get(self.index) == Some(&"(") {
            self.index += 1;
            let value = self.parse_or()?;
            if self.tokens.get(self.index) != Some(&")") {
                return None;
            }
            self.index += 1;
            return Some(value);
        }
        let token = self.tokens.get(self.index)?;
        self.index += 1;
        Some((self.context)(token))
    }
}

/// Keys that, with Ctrl alone, go to a focused terminal's child process instead of
/// workbench bindings: every letter (shells, nano, emacs, tmux and AI CLIs all use
/// them), Space and the symbols that also map to control bytes.
pub(crate) const TERMINAL_CONTROL_KEYS: &[&str] = &[
    "a", "b", "c", "d", "e", "f", "g", "h", "i", "j", "k", "l", "m", "n", "o", "p", "q", "r", "s",
    "t", "u", "v", "w", "x", "y", "z", "space", "@", "[", "\\", "]", "^", "_",
];

pub(crate) fn canonical_key(key: &str) -> String {
    if key == " " {
        return "space".to_owned();
    }
    key.split_whitespace()
        .map(canonical_chord)
        .collect::<Vec<_>>()
        .join(" ")
}

pub(super) fn canonical_chord(key: &str) -> String {
    let mut modifiers = Vec::new();
    let mut key_name = String::new();
    for part in key.trim().to_lowercase().split('+') {
        match part {
            "ctrl" | "control" => modifiers.push("ctrl"),
            "alt" | "option" => modifiers.push("alt"),
            "shift" => modifiers.push("shift"),
            "meta" | "cmd" | "command" | "super" => modifiers.push("meta"),
            other => key_name = other.to_owned(),
        }
    }
    modifiers.sort_unstable();
    modifiers.dedup();
    let key_name = match key_name.as_str() {
        "esc" => "escape",
        "return" => "enter",
        "up" => "arrowup",
        "down" => "arrowdown",
        "left" => "arrowleft",
        "right" => "arrowright",
        " " => "space",
        other => other,
    };
    modifiers.push(key_name);
    modifiers.join("+")
}

pub(super) fn key_string(key: KeyEvent) -> Option<String> {
    let mut modifiers = Vec::new();
    if key.modifiers.contains(KeyModifiers::ALT) {
        modifiers.push("alt");
    }
    if key.modifiers.contains(KeyModifiers::CONTROL) {
        modifiers.push("ctrl");
    }
    if key
        .modifiers
        .intersects(KeyModifiers::SUPER | KeyModifiers::META)
    {
        modifiers.push("meta");
    }
    if key.modifiers.contains(KeyModifiers::SHIFT) || key.code == KeyCode::BackTab {
        modifiers.push("shift");
    }
    let name = match key.code {
        KeyCode::Char(' ') => "space".to_owned(),
        KeyCode::Char('4') if key.modifiers.contains(KeyModifiers::CONTROL) => "\\".to_owned(),
        KeyCode::Char('5') if key.modifiers.contains(KeyModifiers::CONTROL) => "]".to_owned(),
        KeyCode::Char('6') if key.modifiers.contains(KeyModifiers::CONTROL) => "^".to_owned(),
        KeyCode::Char('7') if key.modifiers.contains(KeyModifiers::CONTROL) => "_".to_owned(),
        KeyCode::Char(ch) if key.modifiers.contains(KeyModifiers::SHIFT) => {
            unshift_char(ch).map_or_else(|| ch.to_lowercase().to_string(), |ch| ch.to_string())
        }
        KeyCode::Char(ch) => ch.to_lowercase().to_string(),
        KeyCode::Enter => "enter".to_owned(),
        KeyCode::Tab => "tab".to_owned(),
        KeyCode::BackTab => "tab".to_owned(),
        KeyCode::Backspace => "backspace".to_owned(),
        KeyCode::Esc => "escape".to_owned(),
        KeyCode::Delete => "delete".to_owned(),
        KeyCode::Insert => "insert".to_owned(),
        KeyCode::Home => "home".to_owned(),
        KeyCode::End => "end".to_owned(),
        KeyCode::PageUp => "pageup".to_owned(),
        KeyCode::PageDown => "pagedown".to_owned(),
        KeyCode::Up => "arrowup".to_owned(),
        KeyCode::Down => "arrowdown".to_owned(),
        KeyCode::Left => "arrowleft".to_owned(),
        KeyCode::Right => "arrowright".to_owned(),
        KeyCode::F(number) => format!("f{number}"),
        _ => return None,
    };
    let mut parts = modifiers;
    parts.sort_unstable();
    parts.push(&name);
    Some(parts.join("+"))
}

/// Of the bindings for one key whose context holds, in catalog order, the one that
/// runs: the last context-specific binding, otherwise the last unconditional one.
fn winning_binding<'a>(candidates: &[&'a Keybinding]) -> Option<&'a Keybinding> {
    candidates
        .iter()
        .rev()
        .find(|binding| binding.when.is_some())
        .or_else(|| {
            candidates
                .iter()
                .rev()
                .find(|binding| binding.when.is_none())
        })
        .copied()
}
