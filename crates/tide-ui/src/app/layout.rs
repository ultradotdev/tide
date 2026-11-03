//! Frame geometry from the last render, and pointer gestures in progress.
use super::*;

/// What the last frame drew where. Every render rebuilds it.
#[derive(Default)]
pub(crate) struct LayoutState {
    pub screen: Rect,
    pub hits: Vec<(Rect, HitTarget)>,
    pub sidebar: Option<Rect>,
    pub main: Option<Rect>,
    pub dock: Option<Rect>,
    pub status: Option<Rect>,
    pub agents: Option<Rect>,
    /// The status bar's actionable items, in keyboard order.
    pub status_targets: Vec<Command>,
    /// Cells that get a curly diagnostic underline after the frame is flushed.
    pub curly_cells: Vec<(u16, u16, ratatui::buffer::Cell)>,
    /// The sidebar accordion's sections.
    pub sections: Vec<crate::accordion::SectionLayout>,
    /// Panes and split dividers in drawing order.
    pub main_items: Vec<MainItem>,
    pub dock_parts: Option<DockLayout>,
    pub agents_parts: Option<AgentsLayout>,
}

/// What drawing a frame produced besides cells.
#[derive(Default)]
pub(crate) struct Painted {
    pub hits: Vec<(Rect, HitTarget)>,
    pub status_targets: Vec<Command>,
    pub curly_cells: Vec<(u16, u16, ratatui::buffer::Cell)>,
}

pub(crate) enum MainItem {
    Pane(PaneLayout),
    Divider(Divider),
}

#[derive(Clone, Copy)]
pub(crate) struct PaneLayout {
    pub id: PaneId,
    pub area: Rect,
    /// The tab strip, after region controls and the close button take their cells.
    pub tab_bar: Rect,
    pub content: Rect,
    /// The pane's close button, shown while there are several panes.
    pub close: Option<Rect>,
}

/// The boundary between two split children.
pub(crate) struct Divider {
    pub rect: Rect,
    pub axis: SplitDirection,
    pub resize: Option<ResizeTarget>,
}

#[derive(Clone, Copy)]
pub(crate) struct DockLayout {
    pub area: Rect,
    pub tab_bar: Rect,
    pub content: Rect,
    /// The new-terminal button.
    pub add: Option<Rect>,
}

#[derive(Clone, Copy)]
pub(crate) struct AgentsLayout {
    pub search: Rect,
    pub body: Rect,
    pub footer: Rect,
}

/// Mouse gestures that span several events.
#[derive(Default)]
pub(crate) struct PointerState {
    pub resize: Option<(ResizeTarget, u16, u16)>,
    pub editor_drag: Option<(PaneId, Rect)>,
    pub scroll_drag: Option<(ScrollOwner, Rect)>,
    pub terminal_capture: Option<(PaneId, Rect)>,
    pub editor_click: Option<(PaneId, u16, u16, Instant, u8)>,
    /// The last left press in a terminal, for double- and triple-click selection.
    pub terminal_click: Option<(PaneId, u16, u16, Instant, u8)>,
    pub agent_click: Option<(String, String, u16, u16, Instant)>,
}
