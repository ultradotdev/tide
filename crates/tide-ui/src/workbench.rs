use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use ratatui::layout::Rect;

use crate::editor::EditorState;
use crate::sql_tab::SqlTab;
use crate::terminal_tab::TerminalTab;

pub type PaneId = usize;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SplitDirection {
    /// Children are placed side by side.
    Vertical,
    /// Children are stacked top to bottom.
    Horizontal,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FocusDirection {
    Left,
    Right,
    Up,
    Down,
}

#[derive(Debug, Clone)]
pub enum LayoutNode {
    Leaf(PaneId),
    Split {
        direction: SplitDirection,
        children: Vec<LayoutNode>,
        ratios: Vec<f32>,
    },
}

#[derive(Debug)]
pub struct Pane {
    pub id: PaneId,
    pub tabs: Vec<Tab>,
    pub active: usize,
    pub rect: Rect,
    pub tab_scroll: usize,
    pub tab_scroll_manual: bool,
    pub tab_last_active: Option<usize>,
}

impl Pane {
    #[must_use]
    pub fn new(id: PaneId) -> Self {
        Self {
            id,
            tabs: Vec::new(),
            active: 0,
            rect: Rect::default(),
            tab_scroll: 0,
            tab_scroll_manual: false,
            tab_last_active: None,
        }
    }
}

#[derive(Debug)]
#[allow(clippy::large_enum_variant)]
pub enum Tab {
    Editor(EditorState),
    Terminal(TerminalTab),
    Diff(DiffTab),
    Sql(SqlTab),
    Welcome,
}

impl Tab {
    #[must_use]
    pub fn title(&self) -> String {
        match self {
            Self::Editor(editor) => editor.title(),
            Self::Terminal(terminal) => terminal.title.clone(),
            Self::Diff(diff) => diff.title.clone(),
            Self::Sql(sql) => sql.title.clone(),
            Self::Welcome => "Welcome".to_owned(),
        }
    }

    #[must_use]
    pub fn editor(&self) -> Option<&EditorState> {
        match self {
            Self::Editor(editor) => Some(editor),
            Self::Sql(sql) => Some(&sql.editor),
            _ => None,
        }
    }
    pub fn editor_mut(&mut self) -> Option<&mut EditorState> {
        match self {
            Self::Editor(editor) => Some(editor),
            Self::Sql(sql) => Some(&mut sql.editor),
            _ => None,
        }
    }
    pub fn is_dirty(&self) -> bool {
        match self {
            Self::Editor(editor) => editor.is_dirty(),
            Self::Sql(sql) => sql.editor.is_dirty() || !sql.row_edits.is_empty(),
            _ => false,
        }
    }
}

#[derive(Debug)]
pub struct DiffTab {
    pub id: u64,
    pub browser: Option<crate::diff_browser::DiffBrowser>,
    pub title: String,
    pub text: String,
    pub scroll_top: usize,
}

impl DiffTab {
    #[must_use]
    pub fn new(title: impl Into<String>, text: impl Into<String>) -> Self {
        Self {
            id: {
                static ID: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
                ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
            },
            browser: None,
            title: title.into(),
            text: text.into(),
            scroll_top: 0,
        }
    }
}

#[derive(Debug)]
pub struct Workbench {
    root: LayoutNode,
    panes: BTreeMap<PaneId, Pane>,
    focused: PaneId,
    next_pane_id: PaneId,
}

impl Workbench {
    #[must_use]
    pub fn new() -> Self {
        let mut panes = BTreeMap::new();
        panes.insert(0, Pane::new(0));
        Self {
            root: LayoutNode::Leaf(0),
            panes,
            focused: 0,
            next_pane_id: 1,
        }
    }

    #[must_use]
    pub fn with_tab(tab: Tab) -> Self {
        let mut workbench = Self::new();
        workbench.insert_tab_focused(tab);
        workbench
    }

    #[must_use]
    pub fn restore(root: LayoutNode, panes: BTreeMap<PaneId, Pane>, focused: PaneId) -> Self {
        let next_pane_id = panes.keys().max().copied().unwrap_or(0) + 1;
        let focused = if panes.contains_key(&focused) {
            focused
        } else {
            panes.keys().next().copied().unwrap_or(0)
        };
        Self {
            root,
            panes,
            focused,
            next_pane_id,
        }
    }

    pub fn add_pane(&mut self, pane: Pane) {
        self.next_pane_id = self.next_pane_id.max(pane.id + 1);
        self.panes.insert(pane.id, pane);
    }

    #[must_use]
    pub fn root(&self) -> &LayoutNode {
        &self.root
    }

    #[must_use]
    pub fn pane(&self, id: PaneId) -> Option<&Pane> {
        self.panes.get(&id)
    }

    pub fn pane_mut(&mut self, id: PaneId) -> Option<&mut Pane> {
        self.panes.get_mut(&id)
    }

    #[must_use]
    pub fn focused(&self) -> PaneId {
        self.focused
    }

    pub fn set_focused(&mut self, id: PaneId) {
        if self.panes.contains_key(&id) {
            self.focused = id;
        }
    }

    #[must_use]
    pub fn focused_pane(&self) -> &Pane {
        self.panes.get(&self.focused).expect("focused pane exists")
    }

    pub fn focused_pane_mut(&mut self) -> &mut Pane {
        self.panes
            .get_mut(&self.focused)
            .expect("focused pane exists")
    }

    #[must_use]
    pub fn active_tab(&self) -> Option<&Tab> {
        let pane = self.panes.get(&self.focused)?;
        pane.tabs.get(pane.active)
    }

    pub fn active_tab_mut(&mut self) -> Option<&mut Tab> {
        let focused = self.focused;
        let pane = self.panes.get_mut(&focused)?;
        pane.tabs.get_mut(pane.active)
    }

    #[must_use]
    pub fn active_editor(&self) -> Option<&EditorState> {
        self.active_tab()?.editor()
    }
    pub fn active_editor_mut(&mut self) -> Option<&mut EditorState> {
        self.active_tab_mut()?.editor_mut()
    }

    pub fn insert_tab_focused(&mut self, tab: Tab) {
        let focused = self.focused;
        let pane = self.panes.get_mut(&focused).expect("focused pane");
        pane.tabs.push(tab);
        pane.active = pane.tabs.len().saturating_sub(1);
    }

    pub fn insert_tab(&mut self, pane_id: PaneId, tab: Tab) {
        if let Some(pane) = self.panes.get_mut(&pane_id) {
            pane.tabs.push(tab);
            pane.active = pane.tabs.len().saturating_sub(1);
        }
    }

    /// Every pane in layout order.
    pub fn panes_mut(&mut self) -> impl Iterator<Item = &mut Pane> {
        let order = self.pane_ids();
        let mut panes = self
            .panes
            .iter_mut()
            .filter_map(|(id, pane)| Some((order.iter().position(|o| o == id)?, pane)))
            .collect::<Vec<_>>();
        panes.sort_by_key(|(position, _)| *position);
        panes.into_iter().map(|(_, pane)| pane)
    }

    /// Every tab in every pane, in layout order.
    pub fn tabs(&self) -> impl Iterator<Item = &Tab> {
        self.pane_ids()
            .into_iter()
            .filter_map(|id| self.panes.get(&id))
            .flat_map(|pane| pane.tabs.iter())
    }

    /// Every tab in every pane, in layout order.
    pub fn tabs_mut(&mut self) -> impl Iterator<Item = &mut Tab> {
        self.panes_mut().flat_map(|pane| pane.tabs.iter_mut())
    }

    #[must_use]
    pub fn pane_ids(&self) -> Vec<PaneId> {
        let mut ids = Vec::new();
        collect_ids(&self.root, &mut ids);
        ids
    }

    pub fn focus_next(&mut self) {
        let ids = self.pane_ids();
        if ids.is_empty() {
            return;
        }
        let position = ids.iter().position(|id| *id == self.focused).unwrap_or(0);
        self.focused = ids[(position + 1) % ids.len()];
    }

    pub fn focus_previous(&mut self) {
        let ids = self.pane_ids();
        if ids.is_empty() {
            return;
        }
        let position = ids.iter().position(|id| *id == self.focused).unwrap_or(0);
        self.focused = ids[(position + ids.len() - 1) % ids.len()];
    }

    pub fn focus_index(&mut self, index: usize) {
        if let Some(id) = self.pane_ids().get(index) {
            self.focused = *id;
        }
    }

    /// Focus the nearest pane in a spatial direction using rendered pane rects.
    pub fn focus_direction(&mut self, direction: FocusDirection) -> bool {
        let Some(current) = self.panes.get(&self.focused).map(|pane| pane.rect) else {
            return false;
        };
        if current.width == 0 && current.height == 0 {
            match direction {
                FocusDirection::Left | FocusDirection::Up => self.focus_previous(),
                FocusDirection::Right | FocusDirection::Down => self.focus_next(),
            }
            return true;
        }
        let current_x = i64::from(current.x) * 2 + i64::from(current.width);
        let current_y = i64::from(current.y) * 2 + i64::from(current.height);
        let mut best: Option<(i64, PaneId)> = None;
        for (id, pane) in &self.panes {
            if *id == self.focused || (pane.rect.width == 0 && pane.rect.height == 0) {
                continue;
            }
            if !overlaps_axis(current, pane.rect, direction) {
                continue;
            }
            let candidate_x = i64::from(pane.rect.x) * 2 + i64::from(pane.rect.width);
            let candidate_y = i64::from(pane.rect.y) * 2 + i64::from(pane.rect.height);
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
                best = Some((score, *id));
            }
        }
        if let Some((_, id)) = best {
            self.focused = id;
            true
        } else {
            false
        }
    }

    pub fn split_focused(&mut self, direction: SplitDirection) -> PaneId {
        let new_id = self.next_pane_id;
        self.next_pane_id += 1;
        self.panes.insert(new_id, Pane::new(new_id));
        let target = self.focused;
        if split_node(&mut self.root, target, direction, new_id) {
            self.focused = new_id;
        } else {
            self.panes.remove(&new_id);
        }
        new_id
    }

    pub fn move_tab(&mut self, source: PaneId, index: usize, destination: PaneId) -> bool {
        if source == destination || !self.panes.contains_key(&destination) {
            return false;
        }
        let Some(source_pane) = self.panes.get_mut(&source) else {
            return false;
        };
        if index >= source_pane.tabs.len() {
            return false;
        }
        let tab = source_pane.tabs.remove(index);
        if index < source_pane.active {
            source_pane.active = source_pane.active.saturating_sub(1);
        }
        source_pane.active = source_pane
            .active
            .min(source_pane.tabs.len().saturating_sub(1));
        let target = self.panes.get_mut(&destination).unwrap();
        target.tabs.push(tab);
        target.active = target.tabs.len() - 1;
        target.tab_scroll_manual = false;
        self.focused = destination;
        true
    }

    pub fn resize_toward(
        &mut self,
        target: PaneId,
        direction: FocusDirection,
        amount: f32,
    ) -> bool {
        resize_node(&mut self.root, target, direction, amount)
    }

    pub fn resize_between(
        &mut self,
        left: PaneId,
        right: PaneId,
        axis: SplitDirection,
        amount: f32,
    ) -> bool {
        resize_between(&mut self.root, left, right, axis, amount)
    }

    pub fn reset_ratios(&mut self) {
        reset_ratios(&mut self.root);
    }

    pub fn close_focused_pane(&mut self) -> bool {
        if self.pane_ids().len() <= 1 {
            return false;
        }
        let target = self.focused;
        if let Some(root) = close_node(self.root.clone(), target) {
            self.root = root;
            self.panes.remove(&target);
            self.focused = self.pane_ids().first().copied().unwrap_or(0);
            true
        } else {
            false
        }
    }

    pub fn next_tab(&mut self) {
        let focused = self.focused;
        if let Some(pane) = self.panes.get_mut(&focused)
            && !pane.tabs.is_empty()
        {
            pane.active = (pane.active + 1) % pane.tabs.len();
        }
    }

    pub fn previous_tab(&mut self) {
        let focused = self.focused;
        if let Some(pane) = self.panes.get_mut(&focused)
            && !pane.tabs.is_empty()
        {
            pane.active = (pane.active + pane.tabs.len() - 1) % pane.tabs.len();
        }
    }

    pub fn close_active_tab(&mut self) -> Option<Tab> {
        let active = self.panes.get(&self.focused)?.active;
        self.close_tab(self.focused, active)
    }

    pub fn close_tab(&mut self, pane_id: PaneId, index: usize) -> Option<Tab> {
        let pane = self.panes.get_mut(&pane_id)?;
        if index >= pane.tabs.len() {
            return None;
        }
        let tab = pane.tabs.remove(index);
        if index < pane.active {
            pane.active -= 1;
        }
        pane.active = pane.active.min(pane.tabs.len().saturating_sub(1));
        pane.tab_scroll = pane.tab_scroll.min(pane.tabs.len().saturating_sub(1));
        Some(tab)
    }

    #[must_use]
    pub fn find_editor(&self, path: &Path) -> Option<PaneId> {
        self.panes.iter().find_map(|(id, pane)| {
            pane.tabs
                .iter()
                .any(|tab| {
                    tab.editor()
                        .is_some_and(|editor| editor.path() == Some(path))
                })
                .then_some(*id)
        })
    }

    pub fn focus_editor_in_focused(&mut self, path: &Path) -> bool {
        let pane = self.focused_pane_mut();
        if let Some(index) = pane.tabs.iter().position(|tab| {
            tab.editor()
                .is_some_and(|editor| editor.path() == Some(path))
        }) {
            pane.active = index;
            true
        } else {
            false
        }
    }

    pub fn focus_editor(&mut self, path: &Path) -> bool {
        if let Some(pane_id) = self.find_editor(path) {
            self.focused = pane_id;
            if let Some(pane) = self.panes.get_mut(&pane_id)
                && let Some(index) = pane.tabs.iter().position(|tab| {
                    tab.editor()
                        .is_some_and(|editor| editor.path() == Some(path))
                })
            {
                pane.active = index;
            }
            true
        } else {
            false
        }
    }

    pub fn focus_ai_cli(&mut self, provider: &str, session: &str) -> bool {
        let found = self.panes.iter().find_map(|(id, pane)| {
            pane.tabs
                .iter()
                .position(|tab| {
                    matches!(tab, Tab::Terminal(terminal)
                        if terminal.ai_provider.as_deref() == Some(provider)
                            && terminal.ai_session_id.as_deref() == Some(session))
                })
                .map(|index| (*id, index))
        });
        if let Some((id, index)) = found {
            self.focused = id;
            if let Some(pane) = self.panes.get_mut(&id) {
                pane.active = index;
            }
            true
        } else {
            false
        }
    }

    pub fn close_ai_cli(&mut self, provider: &str, session: &str) -> bool {
        for pane in self.panes.values_mut() {
            if let Some(index) = pane.tabs.iter().position(|tab| {
                matches!(tab, Tab::Terminal(terminal)
                    if terminal.ai_provider.as_deref() == Some(provider)
                        && terminal.ai_session_id.as_deref() == Some(session))
            }) {
                pane.tabs.remove(index);
                if pane.active >= pane.tabs.len() {
                    pane.active = pane.tabs.len().saturating_sub(1);
                }
                return true;
            }
        }
        false
    }

    #[must_use]
    pub fn editor_paths(&self) -> Vec<PathBuf> {
        self.panes
            .values()
            .flat_map(|pane| pane.tabs.iter())
            .filter_map(|tab| {
                tab.editor()
                    .and_then(|editor| editor.path().map(Path::to_path_buf))
            })
            .collect()
    }

    pub fn poll_terminals(&mut self) -> bool {
        let mut changed = false;
        for pane in self.panes.values_mut() {
            for tab in &mut pane.tabs {
                if let Tab::Terminal(terminal) = tab {
                    changed |= terminal.poll();
                }
            }
        }
        changed
    }

    pub fn take_terminal_error(&mut self) -> Option<String> {
        self.panes
            .values_mut()
            .flat_map(|pane| pane.tabs.iter_mut())
            .find_map(|tab| {
                if let Tab::Terminal(terminal) = tab {
                    terminal.take_input_error()
                } else {
                    None
                }
            })
    }

    pub fn resize_terminals(&mut self) {
        for pane in self.panes.values_mut() {
            let width = pane.rect.width.max(2);
            let height = pane.rect.height.saturating_sub(1).max(1);
            for tab in &mut pane.tabs {
                if let Tab::Terminal(terminal) = tab {
                    terminal.resize(width, height);
                }
            }
        }
    }

    /// Closes every terminal concurrently: each hang-up can wait out a
    /// grace period, which should not add up across terminals. A terminal
    /// whose thread cannot start is still hung up when it is dropped.
    pub fn kill_terminals(&mut self) {
        std::thread::scope(|scope| {
            for pane in self.panes.values_mut() {
                for tab in &mut pane.tabs {
                    if let Tab::Terminal(terminal) = tab {
                        let _ = std::thread::Builder::new()
                            .name("tide-terminal-close".into())
                            .spawn_scoped(scope, || terminal.kill());
                    }
                }
            }
        });
    }
}

impl Default for Workbench {
    fn default() -> Self {
        Self::new()
    }
}

fn overlaps_axis(current: Rect, candidate: Rect, direction: FocusDirection) -> bool {
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

fn collect_ids(node: &LayoutNode, ids: &mut Vec<PaneId>) {
    match node {
        LayoutNode::Leaf(id) => ids.push(*id),
        LayoutNode::Split { children, .. } => {
            for child in children {
                collect_ids(child, ids);
            }
        }
    }
}

fn contains_pane(node: &LayoutNode, target: PaneId) -> bool {
    match node {
        LayoutNode::Leaf(id) => *id == target,
        LayoutNode::Split { children, .. } => {
            children.iter().any(|node| contains_pane(node, target))
        }
    }
}

fn resize_node(node: &mut LayoutNode, target: PaneId, toward: FocusDirection, amount: f32) -> bool {
    let LayoutNode::Split {
        direction,
        children,
        ratios,
    } = node
    else {
        return false;
    };
    let Some(index) = children.iter().position(|node| contains_pane(node, target)) else {
        return false;
    };
    if resize_node(&mut children[index], target, toward, amount) {
        return true;
    }
    let axis = match toward {
        FocusDirection::Left | FocusDirection::Right => SplitDirection::Vertical,
        _ => SplitDirection::Horizontal,
    };
    if *direction != axis {
        return false;
    }
    let neighbor = match toward {
        FocusDirection::Left | FocusDirection::Up => index.checked_sub(1),
        _ => (index + 1 < children.len()).then_some(index + 1),
    };
    let Some(neighbor) = neighbor else {
        return false;
    };
    shift_ratios(ratios, children.len(), index, neighbor, amount)
}

fn shift_ratios(
    ratios: &mut Vec<f32>,
    count: usize,
    index: usize,
    neighbor: usize,
    amount: f32,
) -> bool {
    ratios.resize(count, 1.0);
    let total = ratios.iter().map(|ratio| ratio.max(0.01)).sum::<f32>();
    for ratio in ratios.iter_mut() {
        *ratio = ratio.max(0.01) / total;
    }
    let minimum = 0.05f32.min((ratios[index] + ratios[neighbor]) / 3.0);
    let delta = amount
        .clamp(-0.5, 0.5)
        .clamp(minimum - ratios[index], ratios[neighbor] - minimum);
    ratios[index] += delta;
    ratios[neighbor] -= delta;
    delta.abs() > f32::EPSILON
}

fn resize_between(
    node: &mut LayoutNode,
    left: PaneId,
    right: PaneId,
    axis: SplitDirection,
    amount: f32,
) -> bool {
    let LayoutNode::Split {
        direction,
        children,
        ratios,
    } = node
    else {
        return false;
    };
    if *direction == axis {
        for index in 0..children.len().saturating_sub(1) {
            if contains_pane(&children[index], left) && contains_pane(&children[index + 1], right) {
                return shift_ratios(ratios, children.len(), index, index + 1, amount);
            }
        }
    }
    children
        .iter_mut()
        .any(|child| resize_between(child, left, right, axis, amount))
}

fn reset_ratios(node: &mut LayoutNode) {
    if let LayoutNode::Split {
        children, ratios, ..
    } = node
    {
        *ratios = vec![1.0 / children.len().max(1) as f32; children.len()];
        for child in children {
            reset_ratios(child);
        }
    }
}

fn split_node(
    node: &mut LayoutNode,
    target: PaneId,
    direction: SplitDirection,
    new_id: PaneId,
) -> bool {
    match node {
        LayoutNode::Leaf(id) if *id == target => {
            *node = LayoutNode::Split {
                direction,
                children: vec![LayoutNode::Leaf(target), LayoutNode::Leaf(new_id)],
                ratios: vec![0.5, 0.5],
            };
            true
        }
        LayoutNode::Leaf(_) => false,
        LayoutNode::Split { children, .. } => children
            .iter_mut()
            .any(|child| split_node(child, target, direction, new_id)),
    }
}

fn close_node(node: LayoutNode, target: PaneId) -> Option<LayoutNode> {
    match node {
        LayoutNode::Leaf(id) if id == target => None,
        LayoutNode::Leaf(_) => Some(node),
        LayoutNode::Split {
            direction,
            children,
            ratios,
        } => {
            let mut kept_children = Vec::new();
            let mut kept_ratios = Vec::new();
            for (child, ratio) in children.into_iter().zip(ratios) {
                if let Some(child) = close_node(child, target) {
                    kept_children.push(child);
                    kept_ratios.push(ratio);
                }
            }
            match kept_children.len() {
                0 => None,
                1 => kept_children.pop(),
                _ => Some(LayoutNode::Split {
                    direction,
                    children: kept_children,
                    ratios: kept_ratios,
                }),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quitting_hangs_up_every_terminal_at_once() {
        let root = tempfile::tempdir().unwrap();
        let mut workbench = Workbench::new();
        for id in 0..3 {
            let marker = root.path().join(format!("ready-{id}"));
            let spec = crate::terminal_tab::SpawnSpec::new(
                "/bin/sh",
                vec![
                    "-c".into(),
                    "trap '' HUP; : > \"$1\"; while :; do sleep 1; done".into(),
                    "fixture".into(),
                    marker.display().to_string(),
                ],
            );
            let terminal = crate::terminal_tab::TerminalTab::spawn(id, root.path(), spec).unwrap();
            workbench.insert_tab_focused(Tab::Terminal(terminal));
            let started = std::time::Instant::now();
            while !marker.exists() {
                assert!(started.elapsed() < std::time::Duration::from_secs(5));
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
        }
        let started = std::time::Instant::now();
        workbench.kill_terminals();
        // Each terminal ignores SIGHUP, so it waits out the full grace period.
        assert!(
            started.elapsed() < std::time::Duration::from_millis(1200),
            "{:?}",
            started.elapsed()
        );
    }

    #[test]
    fn moving_views_preserves_identity_and_nested_resize_changes_only_the_target_boundary() {
        let mut bench = Workbench::with_tab(Tab::Editor(EditorState::from_content(None, "shared")));
        let document = bench.active_editor().unwrap().document.clone();
        let right = bench.split_focused(SplitDirection::Vertical);
        let bottom = bench.split_focused(SplitDirection::Horizontal);
        assert!(bench.move_tab(0, 0, bottom));
        assert!(bench.active_editor().unwrap().document.same_view(&document));
        assert_eq!(bench.pane(0).unwrap().tabs.len(), 0);
        let before = bench.root().clone();
        assert!(bench.resize_between(0, right, SplitDirection::Vertical, 0.1));
        let LayoutNode::Split {
            ratios, children, ..
        } = bench.root()
        else {
            panic!()
        };
        assert!(ratios[0] > 0.5);
        let LayoutNode::Split { ratios: inner, .. } = &children[1] else {
            panic!()
        };
        assert_eq!(inner, &vec![0.5, 0.5]);
        assert!(bench.resize_toward(bottom, FocusDirection::Up, 0.1));
        assert!(!bench.resize_between(0, right, SplitDirection::Horizontal, 0.1));
        bench.reset_ratios();
        assert_eq!(format!("{:?}", bench.root()), format!("{before:?}"));
        assert!(!bench.move_tab(bottom, 99, right));
        assert!(!bench.move_tab(bottom, 0, 999));
    }

    #[test]
    fn splits_and_focuses_panes() {
        let mut workbench = Workbench::new();
        let second = workbench.split_focused(SplitDirection::Vertical);
        assert_eq!(workbench.pane_ids(), vec![0, second]);
        assert_eq!(workbench.focused(), second);
        workbench.focus_next();
        assert_eq!(workbench.focused(), 0);
        workbench.focus_next();
        assert_eq!(workbench.focused(), second);
    }

    #[test]
    fn closes_a_pane_and_collapses_the_split() {
        let mut workbench = Workbench::new();
        let second = workbench.split_focused(SplitDirection::Horizontal);
        workbench.set_focused(second);
        assert!(workbench.close_focused_pane());
        assert_eq!(workbench.pane_ids(), vec![0]);
        assert!(matches!(workbench.root(), LayoutNode::Leaf(0)));
    }

    #[test]
    fn focuses_spatially_in_stacked_and_side_by_side_splits() {
        let mut workbench = Workbench::new();
        let second = workbench.split_focused(SplitDirection::Horizontal);
        workbench.pane_mut(0).unwrap().rect = Rect::new(0, 0, 80, 10);
        workbench.pane_mut(second).unwrap().rect = Rect::new(0, 10, 80, 10);
        assert!(workbench.focus_direction(FocusDirection::Up));
        assert_eq!(workbench.focused(), 0);
        assert!(workbench.focus_direction(FocusDirection::Down));
        assert_eq!(workbench.focused(), second);

        let mut workbench = Workbench::new();
        let second = workbench.split_focused(SplitDirection::Vertical);
        workbench.pane_mut(0).unwrap().rect = Rect::new(0, 0, 40, 20);
        workbench.pane_mut(second).unwrap().rect = Rect::new(40, 0, 40, 20);
        assert!(workbench.focus_direction(FocusDirection::Left));
        assert_eq!(workbench.focused(), 0);
        assert!(workbench.focus_direction(FocusDirection::Right));
        assert_eq!(workbench.focused(), second);
    }

    #[test]
    fn tabs_cycle_and_close() {
        let mut workbench = Workbench::new();
        workbench.insert_tab_focused(Tab::Editor(EditorState::untitled()));
        workbench.insert_tab_focused(Tab::Welcome);
        assert_eq!(workbench.focused_pane().tabs.len(), 2);
        assert_eq!(workbench.focused_pane().active, 1);
        workbench.next_tab();
        assert_eq!(workbench.focused_pane().active, 0);
        assert!(workbench.close_active_tab().is_some());
        assert_eq!(workbench.focused_pane().tabs.len(), 1);
    }
}
