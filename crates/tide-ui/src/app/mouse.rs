//! Mouse routing against the hit map built by the last render.
use super::*;

impl App {
    pub(super) fn forward_terminal_mouse(&mut self, mouse: MouseEvent) -> bool {
        if self.overlay.is_some() {
            return false;
        }
        let captured = if matches!(mouse.kind, MouseEventKind::Drag(_) | MouseEventKind::Up(_)) {
            self.pointer.terminal_capture
        } else {
            None
        };
        // A captured pointer may leave the terminal; the terminal keeps it on its edge.
        let target = if let Some((id, area)) = captured {
            if area.is_empty() {
                self.pointer.terminal_capture = None;
                return false;
            }
            Some(if id == DOCK_PANE {
                HitTarget::DockTerminal(area)
            } else {
                HitTarget::EditorContent(id, area)
            })
        } else {
            self.layout
                .hits
                .iter()
                .rev()
                .find(|(rect, _)| rect.contains((mouse.column, mouse.row).into()))
                .map(|(_, target)| *target)
        };
        if matches!(mouse.kind, MouseEventKind::Up(_)) {
            self.pointer.terminal_capture = None;
        }
        let (id, area) = match target {
            Some(HitTarget::EditorContent(id, area))
                if self.workbench.pane(id).is_some_and(|pane| {
                    matches!(pane.tabs.get(pane.active), Some(Tab::Terminal(_)))
                }) =>
            {
                (id, area)
            }
            Some(HitTarget::DockTerminal(area)) => (DOCK_PANE, area),
            _ => return false,
        };
        let press = matches!(mouse.kind, MouseEventKind::Down(_));
        let clicks = if matches!(mouse.kind, MouseEventKind::Down(MouseButton::Left)) {
            self.count_terminal_clicks(id, mouse)
        } else {
            1
        };
        let terminal = if id == DOCK_PANE {
            self.dock.active_mut()
        } else {
            self.workbench
                .pane_mut(id)
                .and_then(|pane| match pane.tabs.get_mut(pane.active) {
                    Some(Tab::Terminal(terminal)) => Some(terminal),
                    _ => None,
                })
        };
        let outcome = terminal.map_or(TerminalMouse::Ignored, |terminal| {
            terminal.send_mouse(area, mouse, clicks)
        });
        if press {
            if id == DOCK_PANE {
                self.focus = Focus::Dock;
            } else {
                self.workbench.set_focused(id);
                self.focus = Focus::Main;
            }
        }
        if outcome != TerminalMouse::Ignored && press {
            self.pointer.terminal_capture = Some((id, area));
        }
        if outcome == TerminalMouse::Selected && self.setting_bool("terminal.copyOnSelect", true) {
            self.copy_terminal_selection();
        }
        outcome != TerminalMouse::Ignored || captured.is_some()
    }

    /// Counts quick left presses on one terminal cell: 1, 2 (word) or 3 (line), then 1 again.
    fn count_terminal_clicks(&mut self, id: PaneId, mouse: MouseEvent) -> u8 {
        let now = Instant::now();
        let clicks = self
            .pointer
            .terminal_click
            .filter(|(pane, x, y, at, _)| {
                *pane == id
                    && *x == mouse.column
                    && *y == mouse.row
                    && now.duration_since(*at) < Duration::from_millis(400)
            })
            .map_or(1, |(_, _, _, _, count)| count % 3 + 1);
        self.pointer.terminal_click = Some((id, mouse.column, mouse.row, now, clicks));
        clicks
    }

    pub(super) fn scroll_metrics(&self, owner: ScrollOwner, area: Rect) -> (usize, usize, usize) {
        match owner {
            ScrollOwner::Section(SectionId::Explorer) => self.file_tree.scroll_metrics(),
            ScrollOwner::Section(SectionId::SourceControl) => (
                self.git.panel.rows.len(),
                area.height as usize,
                self.git.panel.scroll,
            ),
            ScrollOwner::Section(SectionId::Outline) => (
                self.outline_for_active().len(),
                area.height as usize,
                self.outline_scroll,
            ),
            ScrollOwner::Section(SectionId::Timeline) => (
                self.git.timeline.entries.len(),
                area.height as usize,
                self.git.timeline.scroll,
            ),
            ScrollOwner::SqlResults(pane) => self
                .workbench
                .pane(pane)
                .and_then(|p| p.tabs.get(p.active))
                .and_then(|tab| {
                    if let Tab::Sql(sql) = tab {
                        Some(sql.scroll_metrics())
                    } else {
                        None
                    }
                })
                .unwrap_or((0, 0, 0)),
            ScrollOwner::Agents => (
                self.ai.rows().len(),
                area.height as usize,
                self.ai.agent_scroll,
            ),
        }
    }

    pub(super) fn register_scrollbar(
        &self,
        frame: &mut Frame,
        owner: ScrollOwner,
        area: Rect,
        out: &mut Painted,
    ) {
        let (total, visible, top) = self.scroll_metrics(owner, area);
        let mut palette = self.palette;
        palette.background = palette.sidebar;
        if let Some(bar) = crate::scrollbar::paint(frame, area, total, visible, top, palette) {
            out.hits.push((bar, HitTarget::ScrollBar(owner, bar)));
        }
    }

    pub(super) fn jump_scrollbar(&mut self, owner: ScrollOwner, area: Rect, y: u16) {
        let (total, visible, _) = self.scroll_metrics(owner, area);
        let top = crate::scrollbar::offset(
            area.height as usize,
            total,
            visible,
            y.saturating_sub(area.y) as usize,
        );
        let selected = |old: usize| {
            old.max(top)
                .min(top + visible.saturating_sub(1))
                .min(total.saturating_sub(1))
        };
        match owner {
            ScrollOwner::Section(section) => {
                self.focus = Focus::Sidebar;
                if let Some(index) = self.accordion.sections.iter().position(|s| s.id == section) {
                    self.accordion.focused = index;
                    self.accordion.content_focus = true;
                }
                match section {
                    SectionId::Explorer => self.file_tree.jump_scroll(top),
                    SectionId::SourceControl => {
                        self.git.panel.scroll = top;
                        self.git.panel.selected = selected(self.git.panel.selected);
                        self.git.sync_panel();
                    }
                    SectionId::Outline => {
                        self.outline_scroll = top;
                        self.outline_selected = selected(self.outline_selected);
                    }
                    SectionId::Timeline => {
                        self.git.timeline.scroll = top;
                        self.git.timeline.selected = selected(self.git.timeline.selected);
                    }
                }
            }
            ScrollOwner::SqlResults(pane) => {
                self.focus = Focus::Main;
                self.workbench.set_focused(pane);
                if let Some(Tab::Sql(sql)) = self.workbench.active_tab_mut() {
                    sql.results_focused = true;
                    sql.jump_results(top);
                }
            }
            ScrollOwner::Agents => {
                self.focus = Focus::Agents;
                self.ai.agent_scroll = top;
                self.ai.agent_row = selected(self.ai.agent_row);
            }
        }
    }

    pub(super) fn handle_mouse(&mut self, mouse: MouseEvent) {
        if !matches!(
            mouse.kind,
            MouseEventKind::Down(MouseButton::Left) | MouseEventKind::Up(MouseButton::Left)
        ) || self.overlay.is_some()
        {
            self.pointer.agent_click = None;
        }
        if matches!(mouse.kind, MouseEventKind::Down(MouseButton::Left))
            && self.overlay.as_ref().is_some_and(|o| {
                o.base().dismiss_on_outside_click()
                    && !o
                        .base()
                        .geometry(self.layout.screen)
                        .contains(ratatui::layout::Position::new(mouse.column, mouse.row))
            })
        {
            // Use Escape's cleanup (search cancellation, pending tab moves, LSP
            // requests, etc.) and consume the click before routing to the host.
            if self.save_before.is_none() {
                self.handle_input_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
            }
            return;
        }
        if self.forward_terminal_mouse(mouse) {
            return;
        }
        if self.overlay.is_some() {
            self.mouse_overlay(mouse);
            return;
        }
        if self.continue_drag(mouse) {
            return;
        }
        match mouse.kind {
            MouseEventKind::Down(MouseButton::Left) => self.click(mouse),

            MouseEventKind::ScrollUp
            | MouseEventKind::ScrollDown
            | MouseEventKind::ScrollLeft
            | MouseEventKind::ScrollRight => self.scroll(mouse),
            _ => {}
        }
    }

    /// Routes a mouse event to the open overlay.
    fn mouse_overlay(&mut self, mouse: MouseEvent) {
        if let Some(mut modal) = self.overlay.take() {
            let key = if matches!(mouse.kind, MouseEventKind::Down(MouseButton::Left)) {
                modal.click_key(self.layout.screen, mouse.column, mouse.row)
            } else {
                None
            };
            if matches!(
                mouse.kind,
                MouseEventKind::ScrollUp | MouseEventKind::ScrollDown
            ) {
                let forward = mouse.kind == MouseEventKind::ScrollDown;
                if modal.base().is_searchable() {
                    let max = modal.base().filtered.len().saturating_sub(1);
                    modal.base_mut().selected = modal
                        .base()
                        .selected
                        .saturating_add_signed(if forward { 3 } else { -3 })
                        .min(max);
                } else {
                    modal
                        .base_mut()
                        .scroll_message(self.layout.screen, if forward { 3 } else { -3 });
                }
            }
            self.overlay = Some(modal);
            if let Some(key) = key {
                self.handle_modal_key(key);
            }
        }
    }

    /// Continues a scrollbar, resize or editor drag. Returns whether the event was consumed.
    fn continue_drag(&mut self, mouse: MouseEvent) -> bool {
        if let Some((owner, area)) = self.pointer.scroll_drag {
            if matches!(mouse.kind, MouseEventKind::Drag(MouseButton::Left)) {
                self.jump_scrollbar(owner, area, mouse.row);
                return true;
            }
            if matches!(mouse.kind, MouseEventKind::Up(_)) {
                self.pointer.scroll_drag = None;
                return true;
            }
        }
        if let Some((target, x, y)) = self.pointer.resize {
            match mouse.kind {
                MouseEventKind::Drag(MouseButton::Left) => {
                    self.pointer.resize = Some((target, mouse.column, mouse.row));
                    let dx = mouse.column as i32 - x as i32;
                    let dy = mouse.row as i32 - y as i32;
                    match target {
                        ResizeTarget::PaneBoundary(left, right, axis, extent) => {
                            let amount = if axis == SplitDirection::Vertical {
                                dx
                            } else {
                                dy
                            };
                            self.workbench.resize_between(
                                left,
                                right,
                                axis,
                                amount as f32 / extent.max(1) as f32,
                            );
                        }
                        ResizeTarget::Sidebar => {
                            self.sidebar_width = resized(
                                self.sidebar_width,
                                if self.sidebar_left { dx } else { -dx },
                                12..=120,
                            );
                        }
                        ResizeTarget::Agents => {
                            self.ai.agents_width = resized(
                                self.ai.agents_width,
                                if self.agents_left { dx } else { -dx },
                                12..=120,
                            );
                        }
                        ResizeTarget::Dock => {
                            self.dock.height = resized(self.dock.height, -dy, 3..=100);
                        }
                    };
                    self.mark_session_dirty();
                    return true;
                }
                MouseEventKind::Up(_) => {
                    self.pointer.resize = None;
                    return true;
                }
                _ => {}
            }
        }
        if matches!(
            mouse.kind,
            MouseEventKind::Up(MouseButton::Left) | MouseEventKind::Drag(MouseButton::Left)
        ) {
            if let Some((pane_id, area)) = self.pointer.editor_drag {
                let numbers = self.show_line_numbers();
                if let Some(pane) = self.workbench.pane_mut(pane_id)
                    && let Some(editor) = pane.tabs.get_mut(pane.active).and_then(Tab::editor_mut)
                {
                    let tab_size = editor.tab_width();
                    if matches!(mouse.kind, MouseEventKind::Up(_)) {
                        editor.mouse_release();
                    } else {
                        editor.mouse_drag(area, mouse.column, mouse.row, numbers, tab_size);
                    }
                }
            }
            if matches!(mouse.kind, MouseEventKind::Up(_)) {
                self.pointer.editor_drag = None;
            }
            return true;
        }
        false
    }

    /// Routes a left click to whatever the last frame drew under it.
    fn click(&mut self, mouse: MouseEvent) {
        let point = Rect::new(mouse.column, mouse.row, 1, 1);
        let Some((rect, target)) = self
            .layout
            .hits
            .iter()
            .rev()
            .find(|(rect, _)| {
                rect.intersection(point).width > 0 && rect.intersection(point).height > 0
            })
            .copied()
        else {
            return;
        };
        if !matches!(target, HitTarget::AgentSession(_)) {
            self.pointer.agent_click = None;
        }
        match target {
            HitTarget::ScrollBar(owner, area) => {
                self.jump_scrollbar(owner, area, mouse.row);
                self.pointer.scroll_drag = Some((owner, area));
            }
            HitTarget::Command(command) => {
                if command == Command::PaneAddPanel {
                    self.focus = Focus::Sidebar;
                }
                self.execute_command(command);
            }
            HitTarget::PaneControl(pane, action) => {
                self.workbench.set_focused(pane);
                self.focus = Focus::Main;
                match action {
                    PaneControl::Add => self.open_panel_picker(),
                    PaneControl::Close => self.close_pane(pane),
                    PaneControl::SplitVertical => self.split(SplitDirection::Vertical),
                    PaneControl::SplitHorizontal => self.split(SplitDirection::Horizontal),
                }
            }
            HitTarget::Resize(target) => {
                self.pointer.resize = Some((target, mouse.column, mouse.row))
            }
            HitTarget::Sidebar => {
                self.focus = Focus::Sidebar;
                self.accordion.leave_content();
            }
            HitTarget::AccordionHeader(index) => {
                self.focus = Focus::Sidebar;
                self.accordion.focus(index);
                self.accordion.toggle(index);
            }
            HitTarget::AccordionContent(index) => {
                self.click_section(index, rect, mouse);
            }
            HitTarget::Pane(id) => {
                self.focus = Focus::Main;
                self.workbench.set_focused(id);
            }
            HitTarget::Tab(pane, index) => self.activate_tab(pane, index),
            HitTarget::TabClose(pane, index) => self.close_tab(pane, index),
            HitTarget::TabScroll(pane, forward) => self.scroll_tabs(pane, forward),
            HitTarget::TabOverflow(pane) => self.open_tabs(pane),
            HitTarget::TabBar(_) => {}
            HitTarget::AgentProvider(provider) => {
                self.with_ai(|ai, ui| ai.toggle_provider(provider, ui))
            }
            HitTarget::AgentSession(index) => {
                self.click_agent_session(index, mouse);
            }
            HitTarget::AgentSearch => {
                self.focus = Focus::Agents;
                self.ai.agent_search = true;
            }
            HitTarget::AgentStatus(provider) => self.with_ai(|ai, ui| ai.show_status(provider, ui)),
            HitTarget::DiffContent(pane, rect) => {
                self.click_diff(pane, rect, mouse);
            }
            HitTarget::SqlContent(pane, _) => {
                self.click_sql(pane, mouse);
            }
            HitTarget::EditorContent(pane, rect) => {
                self.click_editor(pane, rect, mouse);
            }
            HitTarget::Dock | HitTarget::DockTerminal(_) => {
                self.focus = Focus::Dock;
            }
            HitTarget::DockTab(index) => {
                self.focus = Focus::Dock;
                self.dock.active = index;
            }
            HitTarget::Agents => self.focus = Focus::Agents,
            HitTarget::Status => self.focus = Focus::Status,
        }
    }

    /// Clicks a row in a sidebar section.
    fn click_section(&mut self, index: usize, rect: Rect, mouse: MouseEvent) {
        self.focus = Focus::Sidebar;
        self.accordion.focus(index);
        self.accordion.enter_content();
        let row = mouse.row.saturating_sub(rect.y) as usize;
        match self.accordion.focused_section() {
            SectionId::Explorer => {
                if self.file_tree.select_at_row(row)
                    && let Some(path) = self.file_tree.activate_selected()
                {
                    self.open_file(path);
                }
            }
            SectionId::SourceControl if row > 0 => {
                self.git.sync_panel();
                if let Some((selected, double)) = self.git.panel.click_at(row - 1) {
                    self.git.sync_panel();
                    match selected {
                        crate::git_panel::Row::Header(..) => self.git.toggle_panel_group(),
                        crate::git_panel::Row::File(..) if double => {
                            with_ui!(self, |ui| self.git.open_selected_diff(ui))
                        }
                        crate::git_panel::Row::File(..) => {}
                    }
                }
            }
            SectionId::Outline if row < self.outline_for_active().len() => {
                self.outline_selected = (self.outline_scroll + row)
                    .min(self.outline_for_active().len().saturating_sub(1));
                if mouse.column.saturating_sub(rect.x) < 8 {
                    self.toggle_outline(None);
                    return;
                }
                self.handle_outline_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
            }
            SectionId::Timeline if row == 0 => {
                with_ui!(self, |ui| self.git.toggle_timeline_mode(ui));
            }
            SectionId::Timeline if row - 1 < self.git.timeline.entries.len() => {
                self.git.timeline.selected = (self.git.timeline.scroll + row - 1)
                    .min(self.git.timeline.entries.len().saturating_sub(1));
                with_ui!(self, |ui| self.git.open_selected_commit(ui));
            }
            _ => {}
        }
    }

    /// Clicks an Agents session row; a double click opens it.
    fn click_agent_session(&mut self, index: usize, mouse: MouseEvent) {
        self.focus = Focus::Agents;
        self.ai.agent_row = self
            .ai
            .rows()
            .iter()
            .position(|row| *row == AgentRow::Session(index))
            .unwrap_or(0);
        if let Some(session) = self.ai.agent_sessions.get(index) {
            let now = Instant::now();
            let double =
                self.pointer
                    .agent_click
                    .as_ref()
                    .is_some_and(|(provider, id, x, y, at)| {
                        *provider == session.provider_id
                            && *id == session.session_id
                            && *x == mouse.column
                            && *y == mouse.row
                            && now.duration_since(*at) < Duration::from_millis(400)
                    });
            self.pointer.agent_click = if double {
                None
            } else {
                Some((
                    session.provider_id.clone(),
                    session.session_id.clone(),
                    mouse.column,
                    mouse.row,
                    now,
                ))
            };
            if double {
                self.with_ai(|ai, ui| ai.open_session(index, ui));
            }
        }
    }

    /// Clicks inside a diff view.
    fn click_diff(&mut self, pane: PaneId, rect: Rect, mouse: MouseEvent) {
        self.focus = Focus::Main;
        self.workbench.set_focused(pane);
        let browser = match self.workbench.active_tab_mut() {
            Some(Tab::Diff(tab)) => tab.browser.as_mut(),
            Some(Tab::Editor(editor)) => editor.inline.as_mut(),
            Some(Tab::Sql(sql)) => sql.editor.inline.as_mut(),
            _ => None,
        };
        if let Some(browser) = browser {
            let action = browser.click(
                mouse.row.saturating_sub(rect.y) as usize,
                mouse.column.saturating_sub(rect.x) as usize,
            );
            if let Some(action) = action {
                with_ui!(self, |ui| self.git.diff_action(action, ui));
            }
        }
    }

    /// Clicks inside a SQL tab.
    fn click_sql(&mut self, pane: PaneId, mouse: MouseEvent) {
        self.focus = Focus::Main;
        self.workbench.set_focused(pane);
        let now = Instant::now();
        let clicks = self
            .pointer
            .editor_click
            .filter(|(id, x, y, at, _)| {
                *id == pane
                    && *x == mouse.column
                    && *y == mouse.row
                    && now.duration_since(*at) < Duration::from_millis(400)
            })
            .map_or(1, |(_, _, _, _, n)| n % 3 + 1);
        self.pointer.editor_click = Some((pane, mouse.column, mouse.row, now, clicks));
        let numbers = self.show_line_numbers();
        if let Some(Tab::Sql(sql)) = self.workbench.active_tab_mut() {
            let tabs = sql.editor.tab_width();
            let action = sql.mouse(mouse.column, mouse.row, clicks);
            if !sql.results_focused {
                let area = sql.editor_area();
                sql.editor.mouse_press(
                    area,
                    mouse.column,
                    mouse.row,
                    numbers,
                    tabs,
                    mouse.modifiers,
                    clicks,
                );
                self.pointer.editor_drag = Some((pane, area));
            }
            if let Some(action) = action {
                self.with_db(|db, ui| db.action(action, ui));
            }
        }
    }

    /// Clicks inside an editor, tracking double and triple clicks.
    fn click_editor(&mut self, pane: PaneId, rect: Rect, mouse: MouseEvent) {
        self.focus = Focus::Main;
        self.workbench.set_focused(pane);
        let show_line_numbers = self.show_line_numbers();
        if let Some(editor) = self.workbench.active_editor_mut() {
            let tab_size = editor.tab_width();
            let now = Instant::now();
            let clicks = self
                .pointer
                .editor_click
                .filter(|(id, x, y, at, _)| {
                    *id == pane
                        && *x == mouse.column
                        && *y == mouse.row
                        && now.duration_since(*at) < Duration::from_millis(400)
                })
                .map_or(1, |(_, _, _, _, count)| count % 3 + 1);
            self.pointer.editor_click = Some((pane, mouse.column, mouse.row, now, clicks));
            editor.mouse_press(
                rect,
                mouse.column,
                mouse.row,
                show_line_numbers,
                tab_size,
                mouse.modifiers,
                clicks,
            );
            self.pointer.editor_drag = Some((pane, rect));
        }
    }

    /// Routes a wheel event to the region under the pointer.
    fn scroll(&mut self, mouse: MouseEvent) {
        let horizontal = matches!(
            mouse.kind,
            MouseEventKind::ScrollLeft | MouseEventKind::ScrollRight
        );
        if horizontal {
            let target = self
                .layout
                .hits
                .iter()
                .rev()
                .find(|(r, _)| r.contains((mouse.column, mouse.row).into()))
                .map(|(_, t)| *t);
            if let Some(HitTarget::SqlContent(pane, _)) = target
                && let Some(pane) = self.workbench.pane_mut(pane)
                && let Some(Tab::Sql(sql)) = pane.tabs.get_mut(pane.active)
                && sql
                    .results_area()
                    .contains((mouse.column, mouse.row).into())
            {
                for _ in 0..3 {
                    sql.move_column(mouse.kind == MouseEventKind::ScrollRight);
                }
                return;
            }
        }
        let amount = if matches!(
            mouse.kind,
            MouseEventKind::ScrollUp | MouseEventKind::ScrollLeft
        ) {
            -3
        } else {
            3
        };
        let point = Rect::new(mouse.column, mouse.row, 1, 1);
        let target = self
            .layout
            .hits
            .iter()
            .rev()
            .find(|(rect, _)| {
                rect.intersection(point).width > 0 && rect.intersection(point).height > 0
            })
            .map(|(_, target)| *target);
        match target {
            Some(
                HitTarget::Tab(pane, _)
                | HitTarget::TabClose(pane, _)
                | HitTarget::TabBar(pane)
                | HitTarget::TabOverflow(pane)
                | HitTarget::TabScroll(pane, _),
            ) => self.scroll_tabs(pane, amount > 0),
            _ if horizontal => {}
            Some(
                HitTarget::Agents
                | HitTarget::AgentProvider(_)
                | HitTarget::AgentSession(_)
                | HitTarget::AgentStatus(_)
                | HitTarget::AgentSearch,
            ) => {
                let count = self.ai.rows().len();
                self.ai.agent_row = self
                    .ai
                    .agent_row
                    .saturating_add_signed(amount)
                    .min(count.saturating_sub(1));
                self.ai.sync_selection();
            }
            Some(HitTarget::AccordionContent(index)) => match self.accordion.sections[index].id {
                SectionId::Explorer => self.file_tree.scroll_by(amount),
                SectionId::SourceControl => {
                    self.git.sync_panel();
                    self.git.panel.move_by(amount);
                    self.git.sync_panel();
                }
                SectionId::Outline => {
                    self.outline_selected = self
                        .outline_selected
                        .saturating_add_signed(amount)
                        .min(self.outline_for_active().len().saturating_sub(1))
                }
                SectionId::Timeline => {
                    self.git.timeline.selected = self
                        .git
                        .timeline
                        .selected
                        .saturating_add_signed(amount)
                        .min(self.git.timeline.entries.len().saturating_sub(1))
                }
            },
            Some(HitTarget::SqlContent(pane_id, _)) => {
                if let Some(pane) = self.workbench.pane_mut(pane_id)
                    && let Some(Tab::Sql(sql)) = pane.tabs.get_mut(pane.active)
                {
                    if sql
                        .results_area()
                        .contains((mouse.column, mouse.row).into())
                    {
                        sql.scroll_results(amount);
                    } else {
                        sql.editor.scroll_by(amount);
                    }
                }
            }
            Some(HitTarget::EditorContent(pane_id, _)) => {
                if let Some(pane) = self.workbench.pane_mut(pane_id)
                    && let Some(editor) = pane.tabs.get_mut(pane.active).and_then(Tab::editor_mut)
                {
                    editor.scroll_by(amount);
                }
            }
            _ => {}
        }
    }
}

/// Grow or shrink a sidebar or dock size by a pointer delta, within `range`.
fn resized(size: u16, delta: i32, range: std::ops::RangeInclusive<u16>) -> u16 {
    let size = i32::from(size)
        .saturating_add(delta)
        .clamp(i32::from(*range.start()), i32::from(*range.end()));
    u16::try_from(size).unwrap_or(*range.end())
}
