//! Pane, tab and region management: focus, splits, moves, resizing and terminals.
use super::*;

impl App {
    pub(super) fn close_pane(&mut self, pane: PaneId) {
        if self.workbench.pane_ids().len() <= 1 {
            self.set_status("The last main pane stays open; close its tabs individually");
            return;
        }
        if self
            .workbench
            .pane(pane)
            .is_some_and(|pane| pane.tabs.iter().any(Tab::is_dirty))
        {
            self.overlay = Some(modal::confirm(
                ConfirmAction::ClosePane(pane),
                "Close Pane",
                "Save changes before closing this pane?",
            ));
        } else {
            self.force_close_pane(pane);
        }
    }

    pub(super) fn force_close_pane(&mut self, pane: PaneId) {
        if self.workbench.pane_ids().len() <= 1 || self.workbench.pane(pane).is_none() {
            return;
        }
        while self
            .workbench
            .pane(pane)
            .is_some_and(|pane| !pane.tabs.is_empty())
        {
            let index = self.workbench.pane(pane).unwrap().tabs.len() - 1;
            self.force_close_tab(pane, index);
        }
        self.workbench.set_focused(pane);
        self.workbench.close_focused_pane();
        self.focus = Focus::Main;
        self.mark_session_dirty();
    }

    pub(super) fn move_pending_tab(&mut self, destination: PaneId) {
        let Some((source, identity)) = self.pending_move.take() else {
            return;
        };
        if source == DOCK_PANE {
            let found=self.dock.tabs.iter().position(|terminal|matches!(&identity,TabIdentity::Terminal(id,cwd,provider,session) if *id==terminal.id && *cwd==terminal.cwd.display().to_string() && *provider==terminal.ai_provider && *session==terminal.ai_session_id));
            if let Some(index) = found {
                self.dock.active = index;
                self.focus = Focus::Dock;
                self.move_active_tab(destination);
            } else {
                self.set_status("That terminal has closed");
            }
        } else {
            let found = self
                .workbench
                .pane(source)
                .and_then(|pane| pane.tabs.iter().position(|tab| identity.matches(tab)));
            if let Some(index) = found {
                self.activate_tab(source, index);
                self.move_active_tab(destination);
            } else {
                self.set_status("That tab has closed");
            }
        }
    }

    pub(super) fn move_relative_tab(&mut self, forward: bool) {
        let ids = self.workbench.pane_ids();
        if ids.len() < 2 {
            self.set_status("Split a second pane to move this tab");
            return;
        }
        let index = ids
            .iter()
            .position(|id| *id == self.workbench.focused())
            .unwrap_or(0);
        self.move_active_tab(
            ids[if forward {
                (index + 1) % ids.len()
            } else {
                (index + ids.len() - 1) % ids.len()
            }],
        );
    }

    pub(super) fn move_active_tab(&mut self, destination: PaneId) {
        if self.focus == Focus::Dock {
            if destination == DOCK_PANE
                || self.workbench.pane(destination).is_none()
                || self.dock.active >= self.dock.tabs.len()
            {
                return;
            }
            let terminal = self.dock.tabs.remove(self.dock.active);
            self.dock.active = self.dock.active.min(self.dock.tabs.len().saturating_sub(1));
            self.workbench
                .insert_tab(destination, Tab::Terminal(terminal));
            self.workbench.set_focused(destination);
            self.focus = Focus::Main;
        } else if destination == DOCK_PANE {
            if !matches!(self.workbench.active_tab(), Some(Tab::Terminal(_))) {
                self.set_status("Only terminals can move to the bottom dock");
                return;
            }
            if let Some(Tab::Terminal(terminal)) = self.workbench.close_active_tab() {
                self.dock.tabs.push(terminal);
                self.dock.active = self.dock.tabs.len() - 1;
                self.dock.visible = true;
                self.focus = Focus::Dock;
            }
        } else {
            let source = self.workbench.focused();
            let index = self.workbench.focused_pane().active;
            if self.workbench.move_tab(source, index, destination) {
                self.focus = Focus::Main;
            }
        }
        self.mark_session_dirty();
    }

    pub(super) fn resize_region(&mut self, direction: FocusDirection) {
        match self.focus {
            Focus::Dock if matches!(direction, FocusDirection::Up | FocusDirection::Down) => {
                self.dock.height = self
                    .dock
                    .height
                    .saturating_add_signed(if direction == FocusDirection::Up {
                        1
                    } else {
                        -1
                    })
                    .clamp(3, 100)
            }
            Focus::Sidebar
                if direction
                    == if self.sidebar_left {
                        FocusDirection::Right
                    } else {
                        FocusDirection::Left
                    } =>
            {
                self.sidebar_width = self.sidebar_width.saturating_add(1).min(120)
            }
            Focus::Agents
                if direction
                    == if self.agents_left {
                        FocusDirection::Right
                    } else {
                        FocusDirection::Left
                    } =>
            {
                self.ai.agents_width = self.ai.agents_width.saturating_add(1).min(120)
            }
            Focus::Main => {
                if !self
                    .workbench
                    .resize_toward(self.workbench.focused(), direction, 0.04)
                {
                    if direction == FocusDirection::Down && self.dock.visible {
                        self.dock.height = self.dock.height.saturating_sub(1).max(3);
                    } else if matches!(direction, FocusDirection::Left | FocusDirection::Right) {
                        let left = direction == FocusDirection::Left;
                        if self.ai.agents_visible && self.agents_left == left {
                            self.ai.agents_width = self.ai.agents_width.saturating_sub(1).max(12);
                        } else if self.sidebar_visible && self.sidebar_left == left {
                            self.sidebar_width = self.sidebar_width.saturating_sub(1).max(12);
                        } else {
                            self.set_status("No boundary in that direction");
                        }
                    } else {
                        self.set_status("No boundary in that direction");
                    }
                }
            }
            _ => self.set_status("No boundary in that direction"),
        }
        self.mark_session_dirty();
    }

    pub(super) fn focus_spatial(&mut self, direction: FocusDirection) {
        if self.focus == Focus::Main && self.workbench.focus_direction(direction) {
            self.accordion.leave_content();
            return;
        }
        let current = self.focus;
        let current_rect = match current {
            Focus::Sidebar => self.layout.sidebar,
            Focus::Main => self.layout.main,
            Focus::Agents => self.layout.agents,
            Focus::Dock => self.layout.dock,
            Focus::Status => self.layout.status,
        };
        let Some(current_rect) = current_rect else {
            return;
        };
        let current_x = i64::from(current_rect.x) * 2 + i64::from(current_rect.width);
        let current_y = i64::from(current_rect.y) * 2 + i64::from(current_rect.height);
        let candidates = [
            (Focus::Sidebar, self.layout.sidebar),
            (Focus::Main, self.layout.main),
            (Focus::Agents, self.layout.agents),
            (Focus::Dock, self.layout.dock),
            (Focus::Status, self.layout.status),
        ];
        let mut best: Option<(i64, Focus)> = None;
        for (candidate, rect) in candidates {
            if candidate == current {
                continue;
            }
            let Some(rect) = rect else {
                continue;
            };
            if rect.width == 0 || rect.height == 0 {
                continue;
            }
            if !overlaps_region_axis(current_rect, rect, direction) {
                continue;
            }
            let candidate_x = i64::from(rect.x) * 2 + i64::from(rect.width);
            let candidate_y = i64::from(rect.y) * 2 + i64::from(rect.height);
            let (primary, secondary) = match direction {
                FocusDirection::Left if candidate_x < current_x => {
                    (current_x - candidate_x, (candidate_y - current_y).abs())
                }
                FocusDirection::Right if candidate_x > current_x => {
                    (candidate_x - current_x, (candidate_y - current_y).abs())
                }
                FocusDirection::Up if candidate_y < current_y => {
                    (current_y - candidate_y, (candidate_x - current_x).abs())
                }
                FocusDirection::Down if candidate_y > current_y => {
                    (candidate_y - current_y, (candidate_x - current_x).abs())
                }
                _ => continue,
            };
            let score = primary.saturating_mul(10).saturating_add(secondary);
            if best.is_none_or(|(best_score, _)| score < best_score) {
                best = Some((score, candidate));
            }
        }
        if let Some((_, target)) = best {
            self.focus = target;
            if target == Focus::Sidebar {
                self.accordion.leave_content();
            }
        }
    }

    pub(super) fn focus_pane(&mut self, index: usize) {
        self.focus = Focus::Main;
        self.workbench.focus_index(index);
    }

    pub(super) fn focus_next_region(&mut self, reverse: bool) {
        let mut order = Vec::new();
        if self.sidebar_visible {
            order.push(Focus::Sidebar);
        }
        order.push(Focus::Main);
        if self.dock.visible {
            order.push(Focus::Dock);
        }
        order.push(Focus::Status);
        if self.ai.agents_visible {
            let index = order
                .iter()
                .position(|focus| *focus == Focus::Main)
                .unwrap_or(0)
                + 1;
            order.insert(index, Focus::Agents);
        }
        let current = order
            .iter()
            .position(|focus| *focus == self.focus)
            .unwrap_or(1);
        let next = if reverse {
            (current + order.len() - 1) % order.len()
        } else {
            (current + 1) % order.len()
        };
        self.focus = order[next];
        if self.focus == Focus::Sidebar {
            self.accordion.leave_content();
        }
    }

    pub(super) fn toggle_agents(&mut self) {
        if self.ai.agents_visible && self.layout.agents.is_none() && self.layout.screen.width > 0 {
            self.focus = Focus::Agents;
            return;
        }
        self.ai.agents_visible = !self.ai.agents_visible;
        if self.ai.agents_visible {
            self.focus = Focus::Agents;
            self.with_ai(|ai, ui| ai.refresh_sessions(ui));
        } else if self.focus == Focus::Agents {
            self.focus = Focus::Main;
        }
        self.mark_session_dirty();
    }

    pub(super) fn close_active(&mut self) {
        let pane = self.workbench.focused();
        let index = self.workbench.focused_pane().active;
        self.close_tab(pane, index);
    }

    pub(super) fn activate_tab(&mut self, pane: PaneId, index: usize) {
        if pane == DOCK_PANE {
            if index < self.dock.tabs.len() {
                self.dock.active = index;
                self.dock.strip.manual = false;
                self.focus = Focus::Dock;
            }
            return;
        }
        if let Some(target) = self.workbench.pane_mut(pane)
            && index < target.tabs.len()
        {
            target.active = index;
            target.tab_scroll_manual = false;
            self.workbench.set_focused(pane);
            self.focus = Focus::Main;
        }
    }

    pub(super) fn close_tab(&mut self, pane: PaneId, index: usize) {
        if pane == DOCK_PANE {
            self.dock.close(index);
            self.mark_session_dirty();
            return;
        }
        let Some(tab) = self
            .workbench
            .pane(pane)
            .and_then(|pane| pane.tabs.get(index))
        else {
            return;
        };
        if tab.is_dirty() {
            self.pending_close = Some((pane, TabIdentity::of(tab)));
            self.overlay = Some(modal::confirm(
                ConfirmAction::CloseTab { pane, index },
                "Unsaved Changes",
                &format!("Save changes in {} before closing?", tab.title()),
            ));
        } else {
            self.force_close_tab(pane, index);
        }
    }

    pub(super) fn force_close_tab(&mut self, pane: PaneId, index: usize) {
        if let Some(tab) = self.workbench.close_tab(pane, index)
            && let Some(path) = tab.editor().and_then(EditorState::path)
            && self.workbench.find_editor(path).is_none()
        {
            self.lsp
                .close_document(path, &self.settings, &self.workspace);
            self.documents.forget(path);
        }
        self.mark_session_dirty();
    }

    pub(super) fn force_close_active(&mut self) {
        let pane = self.workbench.focused();
        let index = self.workbench.focused_pane().active;
        self.force_close_tab(pane, index);
    }

    pub(super) fn scroll_tabs(&mut self, pane: PaneId, forward: bool) {
        let amount = saturating_usize(self.setting_u64("tui.tabBar.scrollAmount", 1));
        if pane == DOCK_PANE {
            let scroll = self.dock.strip.scroll;
            self.dock.strip.scroll = if forward {
                scroll.saturating_add(amount)
            } else {
                scroll.saturating_sub(amount)
            }
            .min(self.dock.tabs.len().saturating_sub(1));
            self.dock.strip.manual = true;
            return;
        }
        if let Some(pane) = self.workbench.pane_mut(pane) {
            pane.tab_scroll = if forward {
                (pane.tab_scroll + amount).min(pane.tabs.len().saturating_sub(1))
            } else {
                pane.tab_scroll.saturating_sub(amount)
            };
            pane.tab_scroll_manual = true;
        }
    }

    pub(super) fn open_tabs(&mut self, pane: PaneId) {
        if pane == DOCK_PANE {
            self.overlay = Some(modal::tabs(
                pane,
                self.dock
                    .tabs
                    .iter()
                    .enumerate()
                    .map(|(index, tab)| {
                        ModalItem::new(&tab.title, "Terminal", index.to_string())
                            .current(index == self.dock.active)
                    })
                    .collect(),
            ));
            return;
        }
        if let Some(target) = self.workbench.pane(pane) {
            let items = target
                .tabs
                .iter()
                .enumerate()
                .map(|(index, tab)| {
                    ModalItem::new(
                        tab.title(),
                        if tab.is_dirty() { "unsaved" } else { "" },
                        index.to_string(),
                    )
                    .current(index == target.active)
                })
                .collect();
            self.overlay = Some(modal::tabs(pane, items));
        }
    }

    pub(super) fn has_dirty_tabs(&self) -> bool {
        self.workbench
            .pane_ids()
            .into_iter()
            .filter_map(|id| self.workbench.pane(id))
            .flat_map(|pane| &pane.tabs)
            .any(Tab::is_dirty)
    }

    pub(super) fn close_all(&mut self) {
        if self.has_dirty_tabs() {
            self.overlay = Some(modal::confirm(
                ConfirmAction::CloseAllDirty,
                "Unsaved Changes",
                "Save changes before closing all tabs?",
            ));
            return;
        }
        self.force_close_all();
    }

    /// Closes every tab but the active one in the focused pane, or in the dock while it
    /// has focus. Unsaved tabs ask first.
    pub(super) fn close_others(&mut self) {
        if self.focus == Focus::Dock {
            if self.dock.tabs.len() < 2 {
                self.set_status("No other terminal tabs to close");
                return;
            }
            let active = self.dock.active;
            for index in (0..self.dock.tabs.len()).rev() {
                if index != active {
                    self.dock.close(index);
                }
            }
            self.mark_session_dirty();
            return;
        }
        let pane = self.workbench.focused();
        let Some(kept) = self.workbench.active_tab().map(TabIdentity::of) else {
            self.set_status("No other tabs to close");
            return;
        };
        let target = self.workbench.focused_pane();
        if target.tabs.len() < 2 {
            self.set_status("No other tabs to close");
            return;
        }
        let dirty = target
            .tabs
            .iter()
            .filter(|tab| !kept.matches(tab) && tab.is_dirty())
            .count();
        self.pending_keep = Some((pane, kept));
        if dirty > 0 {
            self.overlay = Some(modal::confirm(
                ConfirmAction::CloseOtherTabs(pane),
                "Unsaved Changes",
                &format!("Save changes in {dirty} other tab(s) before closing them?"),
            ));
        } else {
            self.force_close_others(pane);
        }
    }

    pub(super) fn force_close_others(&mut self, pane: PaneId) {
        let Some((owner, kept)) = self.pending_keep.take().filter(|(owner, _)| *owner == pane)
        else {
            return;
        };
        let count = self
            .workbench
            .pane(owner)
            .map_or(0, |target| target.tabs.len());
        for index in (0..count).rev() {
            if self
                .workbench
                .pane(owner)
                .and_then(|target| target.tabs.get(index))
                .is_some_and(|tab| !kept.matches(tab))
            {
                self.force_close_tab(owner, index);
            }
        }
        if let Some(index) = self
            .workbench
            .pane(owner)
            .and_then(|target| target.tabs.iter().position(|tab| kept.matches(tab)))
        {
            self.activate_tab(owner, index);
        }
    }

    pub(super) fn force_close_all(&mut self) {
        self.lsp
            .close_all_documents(&self.settings, &self.workspace);
        self.documents = DocumentRegistry::default();
        for pane in self.workbench.pane_ids() {
            if let Some(pane) = self.workbench.pane_mut(pane) {
                pane.tabs.clear();
                pane.active = 0;
            }
        }
        self.mark_session_dirty();
    }

    pub(super) fn split(&mut self, direction: SplitDirection) {
        self.focus = Focus::Main;
        let editor = self
            .workbench
            .active_editor()
            .cloned()
            .unwrap_or_else(EditorState::untitled);
        let id = self.workbench.split_focused(direction);
        self.workbench.insert_tab(id, Tab::Editor(editor));
        self.mark_session_dirty();
    }

    pub(super) fn open_terminal(&mut self, in_pane: bool) {
        let scrollback = self
            .settings
            .get("tui.terminal.scrollback")
            .and_then(serde_json::Value::as_u64)
            .map_or(5_000, saturating_usize);
        let shell = self
            .settings
            .get("terminal.shell")
            .and_then(serde_json::Value::as_str)
            .filter(|s| !s.trim().is_empty());
        if in_pane {
            let id = self.dock.next_id;
            self.dock.next_id += 1;
            match TerminalTab::new(id, &self.workspace, 80, 20, scrollback, shell) {
                Ok(terminal) => {
                    self.workbench.insert_tab_focused(Tab::Terminal(terminal));
                    self.focus = Focus::Main;
                    self.mark_session_dirty();
                }
                Err(error) => self.set_status(format!("Terminal failed: {error}")),
            }
        } else if let Err(error) = self.dock.open(&self.workspace, shell, scrollback) {
            self.set_status(format!("Terminal failed: {error}"));
        } else {
            self.focus = Focus::Dock;
            self.mark_session_dirty();
        }
    }

    pub(super) fn close_terminal(&mut self) {
        match self.focus {
            Focus::Dock => self.dock.close_active(),
            Focus::Main => {
                if matches!(self.workbench.active_tab(), Some(Tab::Terminal(_))) {
                    self.force_close_active();
                }
            }
            _ => {}
        }
        self.mark_session_dirty();
    }

    pub(super) fn active_terminal_mut(&mut self) -> Option<&mut TerminalTab> {
        if self.focus == Focus::Dock {
            return self.dock.active_mut();
        }
        match self.workbench.active_tab_mut() {
            Some(Tab::Terminal(terminal)) => Some(terminal),
            _ => None,
        }
    }

    pub(super) fn focus_in_terminal(&self) -> bool {
        match self.focus {
            Focus::Dock => !self.dock.tabs.is_empty(),
            Focus::Main => matches!(self.workbench.active_tab(), Some(Tab::Terminal(_))),
            Focus::Sidebar | Focus::Agents | Focus::Status => false,
        }
    }
}

pub(super) fn overlaps_region_axis(
    current: Rect,
    candidate: Rect,
    direction: FocusDirection,
) -> bool {
    match direction {
        FocusDirection::Left | FocusDirection::Right => {
            let top = current.y.max(candidate.y);
            let bottom = (current.y + current.height).min(candidate.y + candidate.height);
            bottom > top
        }
        FocusDirection::Up | FocusDirection::Down => {
            let left = current.x.max(candidate.x);
            let right = (current.x + current.width).min(candidate.x + candidate.width);
            right > left
        }
    }
}
