//! Drawing the workbench: regions, panes, tab bars, the dock and the status bar.
use super::*;
use num_traits::ToPrimitive;

type TabStripView = (Vec<(String, bool)>, usize, crate::tab_strip::TabStripState);

impl App {
    /// Lays out and draws one frame.
    pub fn draw(&mut self, frame: &mut Frame) {
        let mut layout = self.layout_frame(frame.area());
        let mut painted = self.render(frame, &layout);
        // The one place colours are fitted to the terminal: everything drawn this frame,
        // including terminal panes, goes through here.
        if self.color_depth() == crate::display::ColorDepth::Indexed {
            crate::display::reduce_colors(frame.buffer_mut().content.iter_mut());
            crate::display::reduce_colors(painted.curly_cells.iter_mut().map(|(.., cell)| cell));
        }
        layout.hits = painted.hits;
        layout.status_targets = painted.status_targets;
        layout.curly_cells = painted.curly_cells;
        self.status_action = self
            .status_action
            .min(layout.status_targets.len().saturating_sub(1));
        self.layout = layout;
    }

    /// Fits every visible region and widget to `area`: scroll positions, terminal sizes and
    /// per-frame settings change here, so [`Self::render`] only reads.
    fn layout_frame(&mut self, area: Rect) -> LayoutState {
        let focused = self.workbench.focused();
        for id in self.workbench.pane_ids() {
            if let Some(pane) = self.workbench.pane_mut(id) {
                for (index, tab) in pane.tabs.iter_mut().enumerate() {
                    if let Tab::Terminal(terminal) = tab {
                        terminal.set_focused(
                            self.window_focused
                                && self.focus == Focus::Main
                                && focused == id
                                && pane.active == index,
                        );
                    }
                }
            }
        }
        for (index, terminal) in self.dock.tabs.iter_mut().enumerate() {
            terminal.set_focused(
                self.window_focused
                    && self.focus == Focus::Dock
                    && self.dock.visible
                    && self.dock.active == index,
            );
        }

        let exclusions = self.file_excludes();
        self.file_tree.configure(exclusions);
        self.file_tree.set_ascii_icons(!self.nerd_icons());
        let regions = crate::regions::layout(
            area,
            crate::regions::Sidebar {
                visible: self.sidebar_visible,
                left: self.sidebar_left,
                width: self.sidebar_width,
                focused: self.focus == Focus::Sidebar,
            },
            crate::regions::Sidebar {
                visible: self.ai.agents_visible,
                left: self.agents_left,
                width: self.ai.agents_width,
                focused: self.focus == Focus::Agents,
            },
            self.dock.visible,
            self.dock.height,
            self.setting_cells("tui.mainRegion.minimumWidth", 56),
        );
        let mut main = regions.main;
        if regions.dock.is_some() {
            main.height = main.height.saturating_sub(1);
        }
        let mut layout = LayoutState {
            screen: area,
            sidebar: regions.sidebar,
            main: Some(main),
            dock: regions.dock,
            status: Some(regions.status),
            agents: regions.agents,
            ..LayoutState::default()
        };
        if let Some(sidebar) = regions.sidebar {
            self.layout_sidebar(sidebar, &mut layout);
        }
        let root = self.workbench.root().clone();
        self.layout_node(&root, main, &mut layout);
        self.workbench.resize_terminals();
        if let Some(agents) = regions.agents {
            self.layout_agents(agents, &mut layout);
        }
        if let Some(dock) = regions.dock {
            self.layout_dock(dock, &mut layout);
        }
        if let Some(overlay) = &mut self.overlay
            && *overlay.kind() == ModalKind::Find
        {
            let host = self.workbench.focused_pane().rect;
            overlay.base_mut().find_host = Some(Rect::new(
                host.x,
                host.y.saturating_add(1),
                host.width,
                host.height.saturating_sub(1),
            ));
        }
        layout
    }

    /// Draws the frame `layout` describes. Drawing never changes the app.
    pub(crate) fn render(&self, frame: &mut Frame, layout: &LayoutState) -> Painted {
        let mut out = Painted::default();
        let area = layout.screen;
        frame.render_widget(
            Block::default().style(Style::default().bg(self.palette.background)),
            area,
        );
        if let Some(sidebar) = layout.sidebar {
            self.render_sidebar(frame, sidebar, layout, &mut out);
        }
        for item in &layout.main_items {
            match item {
                MainItem::Pane(pane) => self.render_pane(frame, pane, &mut out),
                MainItem::Divider(divider) => self.render_divider(frame, divider, &mut out),
            }
        }
        if let (Some(agents), Some(parts)) = (layout.agents, &layout.agents_parts) {
            self.render_agents(frame, agents, parts, &mut out);
        }
        if let Some(dock) = &layout.dock_parts {
            self.render_dock(frame, dock, &mut out);
        }
        for (rect, label, command) in self.region_controls(layout) {
            frame.render_widget(
                Paragraph::new(format!(" {label} ")).style(
                    Style::default()
                        .fg(self.palette.foreground)
                        .bg(self.palette.tab_inactive),
                ),
                rect,
            );
            out.hits.push((rect, HitTarget::Command(command)));
        }
        if let Some(status) = layout.status {
            self.render_status(frame, status, &mut out);
        }
        if let Some(overlay) = &self.overlay {
            overlay.render(frame, area, self.palette);
        }
        crate::display::sanitize(frame.buffer_mut());
        if self.curly_underlines_enabled() {
            out.curly_cells = crate::display::diagnostic_underlines(
                frame.buffer_mut(),
                self.palette.error,
                self.palette.warning,
            );
        }
        out
    }

    pub(crate) fn curly_underlines_enabled(&self) -> bool {
        self.setting_bool("editor.diagnostics.curlyUnderline", true)
    }

    fn sidebar_block(&self) -> Block<'static> {
        Block::default()
            .borders(if self.sidebar_left {
                Borders::RIGHT
            } else {
                Borders::LEFT
            })
            .border_style(Style::default().fg(self.palette.border))
            .style(Style::default().bg(self.palette.sidebar))
    }

    fn layout_sidebar(&mut self, area: Rect, layout: &mut LayoutState) {
        let inner = self.sidebar_block().inner(area);
        self.accordion.set_status(
            SectionId::Explorer,
            Some(self.file_tree.entries.len().to_string()),
        );
        self.accordion.set_status(
            SectionId::SourceControl,
            (!self.git.state.changed().is_empty())
                .then(|| self.git.state.changed().len().to_string()),
        );
        let outline_count = self.outline_for_active().len();
        self.accordion.set_status(
            SectionId::Outline,
            (outline_count > 0).then(|| outline_count.to_string()),
        );
        self.accordion.set_status(
            SectionId::Timeline,
            (!self.git.timeline.entries.is_empty())
                .then(|| self.git.timeline.entries.len().to_string()),
        );

        let accordion = self.accordion.layout(inner);
        for section in &accordion.sections {
            if section.content.height == 0 {
                continue;
            }
            match section.id {
                SectionId::Explorer => self.file_tree.layout_content(section.content),
                SectionId::SourceControl => {
                    let header = self.git.source_control_header_rows();
                    let [_, list] =
                        Layout::vertical([Constraint::Length(header), Constraint::Min(1)])
                            .areas(section.content);
                    self.git.layout_source_control(list);
                }
                SectionId::Outline => self.layout_outline(section.content),
                SectionId::Timeline => self.git.layout_timeline(section.content),
            }
        }
        layout.sections = accordion.sections;
    }

    fn render_sidebar(
        &self,
        frame: &mut Frame,
        area: Rect,
        layout: &LayoutState,
        out: &mut Painted,
    ) {
        out.hits.push((area, HitTarget::Sidebar));
        let border = self.sidebar_block();
        let inner = border.inner(area);
        frame.render_widget(border, area);
        if area.width > 1 && area.height > 1 {
            out.hits.push((
                Rect::new(
                    if self.sidebar_left {
                        area.right() - 1
                    } else {
                        area.x
                    },
                    area.y,
                    1,
                    area.height,
                ),
                HitTarget::Resize(ResizeTarget::Sidebar),
            ));
        }
        let _ = inner;
        for section in &layout.sections {
            let header_style = if section.focused {
                Style::default()
                    .fg(self.palette.section_focused_fg)
                    .bg(self.palette.section_focused_bg)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default()
                    .fg(self.palette.section_fg)
                    .bg(self.palette.section_bg)
            };
            let status = self
                .accordion
                .sections
                .get(section.index)
                .and_then(|section| section.status.as_deref())
                .map_or_else(String::new, |status| format!(" [{status}]"));
            let glyph = if section.expanded { "▼" } else { "▶" };
            let text = format!(" {glyph} {} {status}", section.id.title());
            let padded = format!("{:<width$}", text, width = section.header.width as usize);
            frame.render_widget(
                Paragraph::new(Span::styled(padded, header_style)),
                section.header,
            );
            out.hits
                .push((section.header, HitTarget::AccordionHeader(section.index)));
            if section.index == 0 && section.header.width >= 22 {
                let rect = Rect::new(section.header.right() - 3, section.header.y, 3, 1);
                frame.render_widget(Paragraph::new(" ▾ ").style(header_style), rect);
                out.hits
                    .push((rect, HitTarget::Command(Command::WorkbenchSidebarMenu)));
            }
            if section.content.height == 0 {
                continue;
            }
            out.hits
                .push((section.content, HitTarget::AccordionContent(section.index)));
            let focused = self.focus == Focus::Sidebar
                && self.accordion.content_focus
                && self.accordion.focused == section.index;
            match section.id {
                SectionId::Explorer => {
                    self.file_tree
                        .render_content(frame, section.content, self.palette, focused)
                }
                SectionId::SourceControl => {
                    self.git
                        .render_source_control(frame, section.content, self.palette, focused);
                }
                SectionId::Outline => self.render_outline(frame, section.content, focused),
                SectionId::Timeline => {
                    let selected = self.focus == Focus::Sidebar
                        && self.accordion.content_focus
                        && self.accordion.focused_section() == SectionId::Timeline;
                    self.git.render_timeline(
                        frame,
                        section.content,
                        self.palette,
                        focused,
                        selected,
                    );
                }
            }
            let mut scroll_area = section.content;
            match section.id {
                SectionId::Explorer => {
                    scroll_area.height = scroll_area
                        .height
                        .saturating_sub(u16::from(scroll_area.height > 2))
                }
                SectionId::SourceControl | SectionId::Timeline => {
                    scroll_area.y += u16::from(scroll_area.height > 0);
                    scroll_area.height = scroll_area.height.saturating_sub(1);
                }
                SectionId::Outline => {}
            }
            self.register_scrollbar(frame, ScrollOwner::Section(section.id), scroll_area, out);
        }
    }

    pub(super) fn outline_for_active(&self) -> Vec<(usize, String)> {
        let mut hidden_depth = None;
        self.outline_raw()
            .into_iter()
            .filter(|(line, label)| {
                let depth = label.len() - label.trim_start().len();
                if hidden_depth.is_some_and(|d| depth > d) {
                    return false;
                }
                hidden_depth = self.outline_collapsed.contains(line).then_some(depth);
                true
            })
            .collect()
    }

    pub(super) fn outline_raw(&self) -> Vec<(usize, String)> {
        let Some(editor) = self.workbench.active_editor() else {
            return Vec::new();
        };
        if let Some(symbols) = editor
            .path()
            .and_then(path_to_uri)
            .and_then(|uri| self.lsp.outline.get(&uri))
        {
            return symbols
                .iter()
                .map(|symbol| {
                    (
                        symbol.line as usize,
                        format!("{}{}", "  ".repeat(symbol.depth.min(32)), symbol.name),
                    )
                })
                .collect();
        }
        let mut cache = self.outline_cache.borrow_mut();
        if let Some(cached) = cache.as_ref()
            && cached.revision == editor.revision()
            && cached.document.same_document(&editor.document)
        {
            return cached.items.clone();
        }
        let items = outline_items(editor);
        *cache = Some(CachedOutline {
            document: editor.document.clone(),
            revision: editor.revision(),
            items: items.clone(),
        });
        items
    }

    fn layout_outline(&mut self, area: Rect) {
        let document = self.workbench.active_editor().map(|e| e.document.clone());
        if document.as_ref().is_some_and(|d| {
            !self
                .outline_document
                .as_ref()
                .is_some_and(|old| old.same_document(d))
        }) {
            self.outline_document = document;
            self.outline_collapsed.clear();
            self.outline_scroll = 0;
            self.outline_selected = 0;
        }
        let items = self.outline_for_active();
        if self.setting_bool("tui.outline.autoFollow", true)
            && self.focus != Focus::Sidebar
            && let Some(editor) = self.workbench.active_editor()
        {
            self.outline_selected = items
                .iter()
                .rposition(|(line, _)| *line <= editor.document.cursor().position.line)
                .unwrap_or(0);
        }
        if self.outline_selected < self.outline_scroll {
            self.outline_scroll = self.outline_selected;
        }
        if self.outline_selected >= self.outline_scroll + area.height.max(1) as usize {
            self.outline_scroll = self.outline_selected + 1 - area.height.max(1) as usize;
        }
    }

    fn render_outline(&self, frame: &mut Frame, area: Rect, focused: bool) {
        let items = self.outline_for_active();
        let raw = self.outline_raw();
        let lines = if items.is_empty() {
            vec![Line::from(Span::styled(
                " No symbols",
                Style::default().fg(self.palette.description),
            ))]
        } else {
            let content_selected = self.focus == Focus::Sidebar
                && self.accordion.content_focus
                && self.accordion.focused_section() == SectionId::Outline;
            items
                .iter()
                .enumerate()
                .skip(self.outline_scroll)
                .take(area.height as usize)
                .map(|(index, (line, text))| {
                    let selected = content_selected && index == self.outline_selected;
                    Line::from(Span::styled(
                        format!(
                            " {:>4} {} {text}",
                            line + 1,
                            if raw
                                .iter()
                                .position(|(n, _)| n == line)
                                .and_then(|i| raw.get(i + 1))
                                .is_some_and(|(_, next)| next.len() - next.trim_start().len()
                                    > text.len() - text.trim_start().len())
                            {
                                if self.outline_collapsed.contains(line) {
                                    "▸"
                                } else {
                                    "▾"
                                }
                            } else {
                                " "
                            }
                        ),
                        Style::default()
                            .fg(if selected {
                                self.palette.selection_fg
                            } else if focused {
                                self.palette.foreground
                            } else {
                                self.palette.muted
                            })
                            .bg(if selected {
                                self.palette.selection_bg
                            } else {
                                self.palette.sidebar
                            }),
                    ))
                })
                .collect()
        };
        frame.render_widget(
            Paragraph::new(lines).style(Style::default().bg(self.palette.sidebar)),
            area,
        );
    }

    pub(super) fn first_leaf(node: &LayoutNode) -> Option<PaneId> {
        match node {
            LayoutNode::Leaf(id) => Some(*id),
            LayoutNode::Split { children, .. } => children.first().and_then(Self::first_leaf),
        }
    }

    /// Splits `area` along the layout tree, laying out each pane and recording the dividers.
    fn layout_node(&mut self, node: &LayoutNode, area: Rect, layout: &mut LayoutState) {
        match node {
            LayoutNode::Leaf(id) => self.layout_pane(*id, area, layout),
            LayoutNode::Split {
                direction,
                children,
                ratios,
            } => {
                let constraints = children
                    .iter()
                    .enumerate()
                    .map(|(index, _)| {
                        let ratio = ratios.get(index).copied().unwrap_or(0.5).max(0.05);
                        // Ratios are fractions of the split; the fill weight is per-mille.
                        let weight = (ratio * 1000.0).round().to_u16().unwrap_or(u16::MAX);
                        Constraint::Fill(weight)
                    })
                    .collect::<Vec<_>>();
                let direction = match direction {
                    SplitDirection::Vertical => Direction::Horizontal,
                    SplitDirection::Horizontal => Direction::Vertical,
                };
                let areas = Layout::new(direction, constraints).split(area);
                for (index, (child, mut child_area)) in
                    children.iter().zip(areas.iter().copied()).enumerate()
                {
                    if index + 1 < children.len() {
                        if direction == Direction::Horizontal {
                            child_area.width = child_area.width.saturating_sub(1);
                        } else {
                            child_area.height = child_area.height.saturating_sub(1);
                        }
                    }
                    self.layout_node(child, child_area, layout);
                }
                for index in 0..children.len().saturating_sub(1) {
                    let first = areas[index];
                    let axis = if direction == Direction::Horizontal {
                        SplitDirection::Vertical
                    } else {
                        SplitDirection::Horizontal
                    };
                    let rect = if axis == SplitDirection::Vertical {
                        Rect::new(first.right().saturating_sub(1), first.y, 1, first.height)
                    } else {
                        Rect::new(first.x, first.bottom().saturating_sub(1), first.width, 1)
                    };
                    let resize = Self::first_leaf(&children[index])
                        .zip(Self::first_leaf(&children[index + 1]))
                        .map(|(left, right)| {
                            ResizeTarget::PaneBoundary(
                                left,
                                right,
                                axis,
                                if axis == SplitDirection::Vertical {
                                    area.width
                                } else {
                                    area.height
                                },
                            )
                        });
                    layout
                        .main_items
                        .push(MainItem::Divider(Divider { rect, axis, resize }));
                }
            }
        }
    }

    fn render_divider(&self, frame: &mut Frame, divider: &Divider, out: &mut Painted) {
        let rect = divider.rect;
        frame.render_widget(
            Paragraph::new(if divider.axis == SplitDirection::Vertical {
                "│\n".repeat(rect.height as usize)
            } else {
                "─".repeat(rect.width as usize)
            })
            .style(Style::default().fg(self.palette.border)),
            rect,
        );
        if let Some(resize) = divider.resize {
            out.hits.push((rect, HitTarget::Resize(resize)));
        }
    }

    /// The labels, active index and scroll state of a pane's tab strip (or the dock's).
    fn tab_strip(&self, id: PaneId) -> Option<TabStripView> {
        if id == DOCK_PANE {
            return Some((
                self.dock
                    .tabs
                    .iter()
                    .map(|tab| (tab.title.clone(), false))
                    .collect(),
                self.dock.active,
                self.dock.strip,
            ));
        }
        self.workbench.pane(id).map(|pane| {
            (
                pane.tabs
                    .iter()
                    .map(|tab| match tab {
                        Tab::Editor(editor) if editor.is_file_read_only() => {
                            (format!("{} [RO]", tab.title()), tab.is_dirty())
                        }
                        _ => (tab.title(), tab.is_dirty()),
                    })
                    .collect(),
                pane.active,
                crate::tab_strip::TabStripState {
                    scroll: pane.tab_scroll,
                    manual: pane.tab_scroll_manual,
                    last_active: pane.tab_last_active,
                },
            )
        })
    }

    fn layout_tab_bar(&mut self, id: PaneId, area: Rect) {
        let Some((tabs, active, mut state)) = self.tab_strip(id) else {
            return;
        };
        crate::tab_strip::layout(area, &tabs, active, &mut state);
        if id == DOCK_PANE {
            self.dock.strip = state;
        } else if let Some(pane) = self.workbench.pane_mut(id) {
            pane.tab_scroll = state.scroll;
            pane.tab_scroll_manual = state.manual;
            pane.tab_last_active = state.last_active;
        }
    }

    fn render_tab_bar(
        &self,
        frame: &mut Frame,
        id: PaneId,
        area: Rect,
        palette: Palette,
        out: &mut Painted,
    ) {
        let Some((tabs, active, state)) = self.tab_strip(id) else {
            return;
        };
        let hits = crate::tab_strip::render(frame, area, palette, &tabs, active, &state);
        out.hits.extend(hits.into_iter().map(|(rect, hit)| {
            (
                rect,
                match hit {
                    crate::tab_strip::TabHit::Bar => HitTarget::TabBar(id),
                    crate::tab_strip::TabHit::Select(index) => HitTarget::Tab(id, index),
                    crate::tab_strip::TabHit::Close(index) => HitTarget::TabClose(id, index),
                    crate::tab_strip::TabHit::Scroll(forward) => HitTarget::TabScroll(id, forward),
                    crate::tab_strip::TabHit::Overflow => HitTarget::TabOverflow(id),
                },
            )
        }));
    }

    fn layout_pane(&mut self, id: PaneId, area: Rect, layout: &mut LayoutState) {
        let focused = self.focus == Focus::Main && self.workbench.focused() == id;
        let show_line_numbers = self.show_line_numbers();
        let mut tab_size = self.tab_size();
        let defaults = self.indent_defaults();
        if let Some(pane) = self.workbench.pane_mut(id) {
            pane.rect = area;
        }
        let [mut tab_area, content_area] =
            Layout::vertical([Constraint::Length(1), Constraint::Min(1)]).areas(area);
        for (rect, _, _) in self.region_controls(layout) {
            if rect.y == tab_area.y && rect.intersection(tab_area).width > 0 {
                if rect.x <= tab_area.x {
                    let used = rect.right().saturating_sub(tab_area.x).min(tab_area.width);
                    tab_area.x += used;
                    tab_area.width -= used;
                } else if rect.right() >= tab_area.right() {
                    tab_area.width = rect.x.saturating_sub(tab_area.x);
                }
            }
        }
        let mut close = None;
        if self.workbench.pane_ids().len() > 1 && tab_area.width >= 6 {
            close = Some(Rect::new(tab_area.right() - 3, tab_area.y, 3, 1));
            tab_area.width -= 3;
        }
        layout.main_items.push(MainItem::Pane(PaneLayout {
            id,
            area,
            tab_bar: tab_area,
            content: content_area,
            close,
        }));
        if self
            .workbench
            .pane(id)
            .is_none_or(|pane| pane.tabs.is_empty())
        {
            return;
        }
        self.layout_tab_bar(id, tab_area);
        let wrap_mode = self
            .settings
            .get("editor.wordWrap")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("off");
        let word_wrap = wrap_mode != "off";
        let wrap_column = matches!(wrap_mode, "bounded" | "wordWrapColumn").then(|| {
            self.settings
                .get("editor.wordWrapColumn")
                .and_then(serde_json::Value::as_u64)
                .unwrap_or(80)
                .clamp(1, 1000) as usize
        });
        let relative = self
            .settings
            .get("editor.lineNumbers")
            .and_then(serde_json::Value::as_str)
            == Some("relative");
        let minimap = self.setting_bool("editor.minimap.enabled", true);
        let Some(tab) = self
            .workbench
            .pane_mut(id)
            .and_then(|pane| pane.tabs.get_mut(pane.active))
        else {
            return;
        };
        if let Some(editor) = tab.editor_mut() {
            editor.diagnostics_enabled = self
                .settings
                .get("lsp.diagnostics.enabled")
                .and_then(serde_json::Value::as_bool)
                != Some(false);
            editor.show_diagnostic_gutter = self
                .settings
                .get("lsp.diagnostics.showInGutter")
                .and_then(serde_json::Value::as_bool)
                != Some(false);
            editor.underline_diagnostics = self
                .settings
                .get("lsp.diagnostics.underlineErrors")
                .and_then(serde_json::Value::as_bool)
                != Some(false);
            editor.minimap = minimap && editor.help.is_none();
            editor.auto_indent = self
                .settings
                .get("editor.autoIndent")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("full")
                .to_owned();
            let undo_limit = self
                .settings
                .get("editor.undoHistoryLimit")
                .and_then(serde_json::Value::as_u64)
                .map_or(1000, saturating_usize);
            editor.document.with_mut(|d| d.set_undo_limit(undo_limit));
            editor.inline_max_height = self
                .settings
                .get("git.inlineDiff.maxHeight")
                .and_then(serde_json::Value::as_u64)
                .unwrap_or(15)
                .clamp(3, 100) as u16;
            editor.configure_editing(defaults, relative);
            tab_size = editor.tab_width();
            // Help pages are laid out for the pane's width already.
            editor.set_word_wrap(word_wrap && editor.help.is_none());
            editor.set_wrap_column(wrap_column.filter(|_| editor.help.is_none()));
        }
        let view = EditorView {
            palette: self.palette,
            syntax: &self.syntax,
            show_line_numbers,
            tab_size,
            focused,
        };
        match tab {
            Tab::Editor(editor) => editor.layout(content_area, &view),
            Tab::Terminal(terminal) => terminal.layout(content_area, self.palette),
            Tab::Diff(diff) => {
                if let Some(browser) = &mut diff.browser {
                    browser.layout(content_area);
                }
            }
            Tab::Sql(sql) => sql.layout(content_area, &view),
            Tab::Welcome => {}
        }
    }

    fn render_pane(&self, frame: &mut Frame, pane: &PaneLayout, out: &mut Painted) {
        let PaneLayout {
            id,
            area,
            tab_bar: tab_area,
            content: content_area,
            close,
        } = *pane;
        let focused = self.focus == Focus::Main && self.workbench.focused() == id;
        let show_line_numbers = self.show_line_numbers();
        out.hits.push((area, HitTarget::Pane(id)));
        let [tab_row, _] =
            Layout::vertical([Constraint::Length(1), Constraint::Min(1)]).areas(area);
        let palette = self.palette.host(focused, false);
        frame.render_widget(
            Block::default().style(Style::default().bg(palette.tab_inactive)),
            tab_row,
        );
        if let Some(rect) = close {
            frame.render_widget(
                Paragraph::new(" × ").style(
                    Style::default()
                        .fg(palette.tab_inactive_fg)
                        .bg(palette.tab_inactive),
                ),
                rect,
            );
            out.hits
                .push((rect, HitTarget::PaneControl(id, PaneControl::Close)));
        }
        let Some(pane) = self.workbench.pane(id) else {
            return;
        };
        if pane.tabs.is_empty() {
            frame.render_widget(
                Paragraph::new(self.key_hint(Command::WorkbenchQuickOpen).map_or_else(
                    || "No open tabs".to_owned(),
                    |key| format!("No open tabs — {key} to quick open"),
                ))
                .style(Style::default().fg(self.palette.muted)),
                content_area,
            );
            return;
        }
        self.render_tab_bar(frame, id, tab_area, palette, out);
        out.hits
            .push((content_area, HitTarget::EditorContent(id, content_area)));
        let Some(tab) = pane.tabs.get(pane.active) else {
            return;
        };
        let tab_size = tab
            .editor()
            .map_or_else(|| self.tab_size(), EditorState::tab_width);
        let view = EditorView {
            palette: self.palette,
            syntax: &self.syntax,
            show_line_numbers,
            tab_size,
            focused,
        };
        match tab {
            Tab::Editor(editor) => {
                editor.render(frame, &view);
                if !editor.inline_area.is_empty() {
                    out.hits.push((
                        editor.inline_area,
                        HitTarget::DiffContent(id, editor.inline_area),
                    ));
                }
            }
            Tab::Terminal(terminal) => {
                terminal.render(frame, content_area, self.palette, focused);
            }
            Tab::Diff(diff) => {
                out.hits
                    .push((content_area, HitTarget::DiffContent(id, content_area)));
                if let Some(browser) = &diff.browser {
                    browser.render(frame, content_area, self.palette, focused);
                } else {
                    render_diff(frame, content_area, diff, self.palette, focused);
                }
            }
            Tab::Sql(sql) => {
                out.hits
                    .push((content_area, HitTarget::SqlContent(id, content_area)));
                sql.render(
                    frame,
                    &view,
                    self.key_hint(Command::DatabaseExecuteQuery).as_deref(),
                );
                let (total, visible, top) = sql.scroll_metrics();
                if let Some(bar) = crate::scrollbar::paint(
                    frame,
                    sql.scroll_area(),
                    total,
                    visible,
                    top,
                    self.palette,
                ) {
                    out.hits
                        .push((bar, HitTarget::ScrollBar(ScrollOwner::SqlResults(id), bar)));
                }
                if !sql.editor.inline_area.is_empty() {
                    out.hits.push((
                        sql.editor.inline_area,
                        HitTarget::DiffContent(id, sql.editor.inline_area),
                    ));
                }
            }
            Tab::Welcome => render_welcome(frame, content_area, self.palette, &self.start_hints()),
        }
    }

    fn layout_dock(&mut self, area: Rect, layout: &mut LayoutState) {
        let [mut tab_area, content_area] =
            Layout::vertical([Constraint::Length(1), Constraint::Min(1)]).areas(area);
        let mut add = None;
        if tab_area.width >= 9 {
            add = Some(Rect::new(tab_area.right() - 6, tab_area.y, 3, 1));
            tab_area.width -= 6;
        }
        layout.dock_parts = Some(DockLayout {
            area,
            tab_bar: tab_area,
            content: content_area,
            add,
        });
        if self.dock.tabs.is_empty() {
            return;
        }
        self.layout_tab_bar(DOCK_PANE, tab_area);
        for terminal in &mut self.dock.tabs {
            terminal.resize(content_area.width, content_area.height);
        }
        let palette = self.palette;
        if let Some(terminal) = self.dock.active_mut() {
            terminal.layout(content_area, palette);
        }
    }

    fn render_dock(&self, frame: &mut Frame, dock: &DockLayout, out: &mut Painted) {
        let DockLayout {
            area,
            tab_bar: tab_area,
            content: content_area,
            add,
        } = *dock;
        let focused = self.focus == Focus::Dock;
        out.hits.push((area, HitTarget::Dock));
        let [tab_row, _] =
            Layout::vertical([Constraint::Length(1), Constraint::Min(1)]).areas(area);
        let palette = self.palette.host(focused, true);
        frame.render_widget(
            Block::default().style(Style::default().bg(palette.tab_inactive)),
            tab_row,
        );
        if let Some(rect) = add {
            frame.render_widget(
                Paragraph::new(" + ").style(
                    Style::default()
                        .fg(palette.tab_inactive_fg)
                        .bg(palette.tab_inactive),
                ),
                rect,
            );
            out.hits
                .push((rect, HitTarget::Command(Command::TerminalNew)));
        }
        if area.y > 0 {
            let divider = Rect::new(area.x, area.y - 1, area.width, 1);
            frame.render_widget(
                Paragraph::new("─".repeat(area.width as usize))
                    .style(Style::default().fg(self.palette.border)),
                divider,
            );
            out.hits
                .push((divider, HitTarget::Resize(ResizeTarget::Dock)));
        }
        if self.dock.tabs.is_empty() {
            frame.render_widget(
                Paragraph::new(match self.key_hint(Command::WorkbenchToggleTerminal) {
                    Some(key) => format!("No terminals — {key} to open one"),
                    None => {
                        "No terminals — run Open Bottom Terminal Tab from the palette".to_owned()
                    }
                })
                .style(Style::default().fg(self.palette.muted)),
                content_area,
            );
            return;
        }
        self.render_tab_bar(frame, DOCK_PANE, tab_area, palette, out);
        out.hits
            .push((content_area, HitTarget::DockTerminal(content_area)));
        if let Some(terminal) = self.dock.active() {
            terminal.render(frame, content_area, self.palette, focused);
        }
    }

    fn render_status(&self, frame: &mut Frame, area: Rect, out: &mut Painted) {
        out.hits.push((area, HitTarget::Status));
        let base = Style::default()
            .fg(self.palette.status_foreground)
            .bg(self.palette.status);
        frame.render_widget(Block::default().style(base), area);
        if area.is_empty() {
            return;
        }
        let mut left: Vec<(String, Command)> =
            vec![(" >_ ".into(), Command::WorkbenchCommandPalette)];
        if let Some(git) = &self.git.state.snapshot {
            let head = git.branch.clone().or_else(|| {
                git.head
                    .as_deref()
                    .map(|head| format!("({})", crate::display::short_id(head)))
            });
            if let Some(head) = head {
                left.push((
                    format!(
                        " {} ",
                        truncate_to_width(&head, (area.width as usize / 8).clamp(6, 24))
                    ),
                    Command::GitSwitchBranch,
                ));
            }
            if let Some(operation) = &git.operation {
                left.push((
                    format!(" {} ", operation.short_label()),
                    Command::GitFocusPanel,
                ));
            }
            if let Some((ahead, behind)) = git.ahead_behind {
                let sync = [
                    if ahead > 0 {
                        format!("↑{ahead}")
                    } else {
                        String::new()
                    },
                    if behind > 0 {
                        format!("↓{behind}")
                    } else {
                        String::new()
                    },
                ]
                .into_iter()
                .filter(|s| !s.is_empty())
                .collect::<Vec<_>>()
                .join(" ");
                if !sync.is_empty() {
                    left.push((sync, Command::GitFocusPanel));
                }
            }
        }
        let mut right: Vec<(String, Command)> = Vec::new();
        if let Some(editor) = self.workbench.active_editor() {
            left.push((
                format!(
                    " {}{} ",
                    if editor.document.is_dirty() {
                        "● "
                    } else {
                        ""
                    },
                    truncate_to_width(&editor.title(), (area.width as usize / 4).clamp(8, 32))
                ),
                Command::WorkbenchDocumentInfo,
            ));
            if editor.document.with(Document::is_lossy) {
                left.push((
                    " Read-only: invalid UTF-8 ".into(),
                    Command::WorkbenchDocumentInfo,
                ));
            } else if editor.is_file_read_only() {
                left.push((" Read-only ".into(), Command::WorkbenchDocumentInfo));
            }
            let cursor = editor.document.cursor().position;
            let (eol, selected) = editor.document.status_metadata();
            right.push((
                format!("Ln {}, Col {}", cursor.line + 1, cursor.column + 1),
                Command::EditorGotoLine,
            ));
            if selected > 0 {
                right.push((
                    format!("{selected} selected"),
                    Command::WorkbenchDocumentInfo,
                ));
            }
            let language = match editor.language_extension() {
                "ts" | "tsx" => "TypeScript",
                "js" | "jsx" => "JavaScript",
                "rs" => "Rust",
                "py" => "Python",
                "md" => "Markdown",
                "sql" => "SQL",
                "json" => "JSON",
                "txt" | "" => "Plain Text",
                value => value,
            };
            right.push((language.into(), Command::WorkbenchDocumentInfo));
            if let Some(status) = editor
                .path()
                .and_then(language_id_for_path)
                .and_then(|id| self.lsp.server_status(&self.settings, id))
            {
                right.push((format!("LSP: {status}"), Command::LspShowDiagnostics));
            }
            right.push((editor.indent().label(), Command::WorkbenchOpenSettings));
            if let Some(encoding) = editor.document.with(Document::encoding_label) {
                right.push((encoding.into(), Command::WorkbenchDocumentInfo));
            }
            right.push((eol.into(), Command::WorkbenchDocumentInfo));
        }
        // Preserve cursor position first as width tightens, dropping whole low-priority fields.
        let left_width = left.iter().map(|(s, _)| display_width(s)).sum::<usize>()
            + left.len().saturating_sub(1);
        let feedback_width = display_width(&self.status)
            .min(40)
            .min(area.width as usize / 3);
        while right.len() > 1
            && right.iter().map(|(s, _)| display_width(s)).sum::<usize>()
                + right.len().saturating_sub(1) * 3
                + left_width
                + feedback_width
                + 2
                > area.width as usize
        {
            right.pop();
        }
        let right_width = right.iter().map(|(s, _)| display_width(s)).sum::<usize>()
            + right.len().saturating_sub(1) * 3
            + usize::from(!right.is_empty());
        let right_start = area
            .right()
            .saturating_sub(saturating_u16(right_width).min(area.width));
        let mut x = area.x;
        for (label, command) in left {
            let limit = right_start.saturating_sub(x + u16::from(x < right_start));
            if limit == 0 {
                break;
            }
            let width = saturating_u16(display_width(&label)).min(limit);
            self.render_status_item(frame, Rect::new(x, area.y, width, 1), &label, command, out);
            x += width;
            if x < right_start {
                frame.render_widget(Paragraph::new("│").style(base), Rect::new(x, area.y, 1, 1));
                x += 1;
            }
        }
        if x < right_start {
            let width = right_start - x;
            self.render_status_item(
                frame,
                Rect::new(x, area.y, width, 1),
                &self.status.clone(),
                Command::WorkbenchStatusHistory,
                out,
            );
        }
        x = right_start;
        for (index, (label, command)) in right.into_iter().enumerate() {
            if index > 0 && x + 3 <= area.right() {
                frame.render_widget(
                    Paragraph::new(" │ ").style(base),
                    Rect::new(x, area.y, 3, 1),
                );
                x += 3;
            }
            let width = saturating_u16(display_width(&label)).min(area.right().saturating_sub(x));
            self.render_status_item(frame, Rect::new(x, area.y, width, 1), &label, command, out);
            x += width;
        }
    }

    fn render_status_item(
        &self,
        frame: &mut Frame,
        rect: Rect,
        label: &str,
        command: Command,
        out: &mut Painted,
    ) {
        if rect.is_empty() {
            return;
        }
        let style = Style::default().fg(self.palette.status_foreground).bg(
            if self.focus == Focus::Status && self.status_action == out.status_targets.len() {
                self.palette.selection
            } else {
                self.palette.status
            },
        );
        frame.render_widget(
            Paragraph::new(truncate_to_width(label, rect.width as usize)).style(style),
            rect,
        );
        out.hits.push((rect, HitTarget::Command(command)));
        out.status_targets.push(command);
    }

    pub(super) fn region_controls(
        &self,
        layout: &LayoutState,
    ) -> Vec<(Rect, &'static str, Command)> {
        let Some(main) = layout.main else {
            return Vec::new();
        };
        if main.width < 12 || main.height == 0 {
            return Vec::new();
        }
        let ascii = !self.nerd_icons();
        let mut controls = Vec::new();
        let mut left = main.x;
        let mut right = main.right();
        for (on_left, visible, command) in [
            (
                self.sidebar_left,
                layout.sidebar.is_some(),
                Command::WorkbenchToggleSidebar,
            ),
            (
                self.agents_left,
                layout.agents.is_some(),
                Command::AgentsTogglePanel,
            ),
        ] {
            let x = if on_left {
                let x = left;
                left += 3;
                x
            } else {
                right -= 3;
                right
            };
            let arrow_left = on_left == visible;
            let label = match (ascii, arrow_left) {
                (true, true) => "<",
                (true, false) => ">",
                (false, true) => "←",
                (false, false) => "→",
            };
            controls.push((Rect::new(x, main.y, 3, 1), label, command));
        }
        let (rect, label) = if let Some(dock) = layout.dock {
            (
                Rect::new(dock.right().saturating_sub(3), dock.y, 3.min(dock.width), 1),
                if ascii { "v" } else { "↓" },
            )
        } else {
            (
                Rect::new(
                    main.right().saturating_sub(3),
                    main.bottom().saturating_sub(1),
                    3,
                    1,
                ),
                if ascii { "^" } else { "↑" },
            )
        };
        controls.push((rect, label, Command::WorkbenchToggleTerminal));
        controls
    }

    fn render_region_chrome(
        &self,
        frame: &mut Frame,
        area: Rect,
        title: &str,
        actions: [(&str, Command); 2],
        out: &mut Painted,
    ) {
        if area.is_empty() {
            return;
        }
        frame.render_widget(
            Paragraph::new(title).style(
                Style::default()
                    .fg(self.palette.foreground)
                    .bg(self.palette.sidebar),
            ),
            area,
        );
        if area.width >= 14 {
            for (index, (label, command)) in actions.into_iter().enumerate() {
                let rect = Rect::new(area.right() - 6 + saturating_u16(index) * 3, area.y, 3, 1);
                frame.render_widget(
                    Paragraph::new(format!(" {label} ")).style(
                        Style::default()
                            .fg(self.palette.foreground)
                            .bg(self.palette.sidebar),
                    ),
                    rect,
                );
                out.hits.push((rect, HitTarget::Command(command)));
            }
        }
    }

    pub(super) fn agent_session_marker(&self, session: &CliSession) -> &str {
        for id in self.workbench.pane_ids() {
            if let Some(pane) = self.workbench.pane(id) {
                for tab in &pane.tabs {
                    if let Tab::Terminal(terminal) = tab
                        && terminal.ai_provider.as_deref() == Some(session.provider_id.as_str())
                        && terminal.ai_session_id.as_deref() == Some(session.session_id.as_str())
                    {
                        return match terminal.exited {
                            None => "●",
                            Some(0) => "○",
                            Some(_) => "×",
                        };
                    }
                }
            }
        }
        " "
    }

    fn agents_block(&self) -> Block<'static> {
        Block::default()
            .borders(if self.agents_left {
                Borders::RIGHT
            } else {
                Borders::LEFT
            })
            .border_style(Style::default().fg(if self.focus == Focus::Agents {
                self.palette.border_focused
            } else {
                self.palette.border
            }))
            .style(Style::default().bg(self.palette.sidebar))
    }

    fn layout_agents(&mut self, area: Rect, layout: &mut LayoutState) {
        let inner = self.agents_block().inner(area);
        let inner = Rect::new(
            inner.x,
            inner.y.saturating_add(u16::from(inner.height > 0)),
            inner.width,
            inner.height.saturating_sub(1),
        );
        // The footer wraps rather than clipping its keys or error, up to three rows
        // and leaving the list at least one.
        let footer_height = self
            .agents_footer_lines(inner.width)
            .len()
            .min(3)
            .min(usize::from(inner.height.saturating_sub(2)))
            .max(1);
        let [search, body, footer] = Layout::vertical([
            Constraint::Length(1),
            Constraint::Min(1),
            Constraint::Length(saturating_u16(footer_height)),
        ])
        .areas(inner);
        let rows = self.ai.rows();
        self.ai.agent_row = self.ai.agent_row.min(rows.len().saturating_sub(1));
        let height = body.height.max(1) as usize;
        // A status row can wrap onto several lines, so scroll by the lines rows take.
        let heights = rows
            .iter()
            .map(|row| self.agent_row_height(row, body.width))
            .collect::<Vec<_>>();
        let row = self.ai.agent_row;
        let mut scroll = self.ai.agent_scroll.min(row);
        while scroll < row && heights[scroll..=row].iter().sum::<usize>() > height {
            scroll += 1;
        }
        let mut last = heights.len();
        let mut below = 0;
        while last > 0 && below + heights[last - 1] <= height {
            last -= 1;
            below += heights[last];
        }
        self.ai.agent_scroll = scroll.min(last);
        layout.agents_parts = Some(AgentsLayout {
            search,
            body,
            footer,
        });
    }

    fn render_agents(
        &self,
        frame: &mut Frame,
        area: Rect,
        parts: &AgentsLayout,
        out: &mut Painted,
    ) {
        out.hits.push((area, HitTarget::Agents));
        let block = self.agents_block();
        frame.render_widget(block, area);
        if area.width > 1 && area.height > 1 {
            out.hits.push((
                Rect::new(
                    if self.agents_left {
                        area.right() - 1
                    } else {
                        area.x
                    },
                    area.y + 1,
                    1,
                    area.height - 1,
                ),
                HitTarget::Resize(ResizeTarget::Agents),
            ));
        }
        if area.height > 0 {
            self.render_region_chrome(
                frame,
                Rect::new(area.x + 1, area.y, area.width.saturating_sub(2), 1),
                if self.ai.agents_loading {
                    "Agents (refreshing)"
                } else {
                    "Agents"
                },
                [
                    ("+", Command::AgentsNewSession),
                    ("▾", Command::WorkbenchAgentsMenu),
                ],
                out,
            );
        }

        let AgentsLayout {
            search,
            body,
            footer,
        } = *parts;
        frame.render_widget(
            Paragraph::new(format!(
                " / {}",
                if self.ai.agent_query.is_empty() {
                    "Search sessions"
                } else {
                    &self.ai.agent_query
                }
            ))
            .style(
                Style::default()
                    .fg(self.palette.description)
                    .bg(self.palette.input_bg),
            ),
            search,
        );
        out.hits.push((search, HitTarget::AgentSearch));
        if self.focus == Focus::Agents && self.ai.agent_search && search.width > 3 {
            frame.set_cursor_position((
                search.x
                    + 3
                    + saturating_u16(display_width(&self.ai.agent_query)).min(search.width - 4),
                search.y,
            ));
        }
        let rows = self.ai.rows();
        let mut y = body.y;
        for (offset, row) in rows.iter().enumerate().skip(self.ai.agent_scroll) {
            let bottom = body.y.saturating_add(body.height);
            if y >= bottom {
                break;
            }
            let (lines, target) = self.agent_row_lines(row, body.width);
            let shown = saturating_u16(lines.len()).min(bottom - y);
            let rect = Rect::new(body.x, y, body.width, shown);
            y += shown;
            let selected = self.focus == Focus::Agents && offset == self.ai.agent_row;
            let style = Style::default()
                .fg(if selected {
                    self.palette.selection_fg
                } else {
                    self.palette.foreground
                })
                .bg(if selected {
                    self.palette.selection_bg
                } else {
                    self.palette.sidebar
                });
            frame.render_widget(
                Paragraph::new(
                    lines
                        .into_iter()
                        .take(usize::from(shown))
                        .map(Line::from)
                        .collect::<Vec<_>>(),
                )
                .style(style),
                rect,
            );
            out.hits.push((rect, target));
        }
        self.register_scrollbar(frame, ScrollOwner::Agents, body, out);
        if rows.is_empty() {
            frame.render_widget(
                Paragraph::new(" No AI CLI sessions found")
                    .style(Style::default().fg(self.palette.description)),
                body,
            );
        }
        let mut lines = self.agents_footer_lines(footer.width);
        if lines.len() > usize::from(footer.height) {
            lines.truncate(usize::from(footer.height));
            if let Some(last) = lines.last_mut() {
                let width = usize::from(footer.width.saturating_sub(2));
                *last = format!("{}…", truncate_to_width(last, width));
            }
        }
        frame.render_widget(
            Paragraph::new(
                lines
                    .into_iter()
                    .map(|line| Line::from(format!(" {line}")))
                    .collect::<Vec<_>>(),
            )
            .style(Style::default().fg(if self.ai.agents_error.is_some() {
                self.palette.error
            } else {
                self.palette.description
            })),
            footer,
        );
    }

    /// The lines of the Agents sidebar a row takes: a status row wraps, at most
    /// [`AGENT_STATUS_LINES`] lines; every other row is one line.
    fn agent_row_height(&self, row: &AgentRow, width: u16) -> usize {
        match row {
            AgentRow::Status(_) => self.agent_row_lines(row, width).0.len(),
            _ => 1,
        }
    }

    fn agent_row_lines(&self, row: &AgentRow, width: u16) -> (Vec<String>, HitTarget) {
        let width = usize::from(width);
        let (text, target) = match *row {
            AgentRow::Provider(provider) => {
                let count = self
                    .ai
                    .agent_sessions
                    .iter()
                    .filter(|session| session.provider_id == provider.id())
                    .count();
                (
                    format!(
                        " {} {} ({count}){}",
                        if self.ai.agent_collapsed.contains(provider.id()) {
                            "▸"
                        } else {
                            "▾"
                        },
                        provider.display_name(),
                        if self
                            .ai
                            .agent_reports
                            .get(&provider)
                            .is_some_and(|report| report.error.is_some())
                        {
                            " !"
                        } else {
                            ""
                        }
                    ),
                    HitTarget::AgentProvider(provider),
                )
            }
            AgentRow::Status(provider) => {
                let error = self
                    .ai
                    .agent_reports
                    .get(&provider)
                    .and_then(|r| r.error.as_deref());
                let installed = self
                    .ai
                    .cli_status
                    .iter()
                    .any(|s| s.provider == provider && s.installed);
                let notice = self
                    .ai
                    .agent_reports
                    .get(&provider)
                    .and_then(|report| report.notice.as_deref())
                    .map(tide_ai::cli::SetupNotice::of);
                let message = if let Some(error) = error {
                    error.to_owned()
                } else if let Some(notice) = notice {
                    format!("{} · Enter for details", notice.label())
                } else if !self.ai.agent_query.is_empty() {
                    "No matching sessions".into()
                } else if !installed {
                    "Not installed".into()
                } else {
                    let other = self
                        .ai
                        .agent_candidates
                        .iter()
                        .filter(|s| s.provider_id == provider.id())
                        .count();
                    if other > 0 {
                        format!("No workspace sessions · {other} elsewhere · w associate")
                    } else {
                        "No workspace sessions".into()
                    }
                };
                let parts = message.split(" · ").map(str::to_owned).collect::<Vec<_>>();
                let text_width = width.saturating_sub(3).max(1);
                let mut lines = pack_hints(&parts, text_width);
                if lines.len() > AGENT_STATUS_LINES {
                    lines.truncate(AGENT_STATUS_LINES);
                    if let Some(last) = lines.last_mut() {
                        *last =
                            format!("{}…", truncate_to_width(last, text_width.saturating_sub(1)));
                    }
                }
                if lines.is_empty() {
                    lines.push(String::new());
                }
                return (
                    lines.into_iter().map(|line| format!("   {line}")).collect(),
                    HitTarget::AgentStatus(provider),
                );
            }
            AgentRow::Session(index) => {
                let session = &self.ai.agent_sessions[index];
                (
                    format!(
                        " {} {} · {}{}",
                        self.agent_session_marker(session),
                        session.title,
                        session.session_id.chars().take(8).collect::<String>(),
                        session
                            .model
                            .as_ref()
                            .map_or(String::new(), |model| format!(" · {model}"))
                    ),
                    HitTarget::AgentSession(index),
                )
            }
        };
        (vec![truncate_to_width(&text, width).to_owned()], target)
    }

    /// The Agents footer for a sidebar `width` cells wide: its keys, or the last
    /// discovery error and how to retry, wrapped between items rather than inside one.
    fn agents_footer_lines(&self, width: u16) -> Vec<String> {
        let items = self.ai.agents_error.as_ref().map_or_else(
            || {
                ["/ search", "n new", "r refresh", "w workspace"]
                    .map(str::to_owned)
                    .to_vec()
            },
            |error| vec![format!("! {error}"), "r retry".to_owned()],
        );
        pack_hints(&items, usize::from(width.saturating_sub(1)))
    }
}

/// The most lines a status row in the Agents sidebar wraps onto; Enter on it
/// shows the whole message.
const AGENT_STATUS_LINES: usize = 4;

/// Packs `items` into lines at most `width` cells wide, joined by " · ". An item
/// longer than a line wraps by its words.
fn pack_hints(items: &[String], width: usize) -> Vec<String> {
    let mut lines: Vec<String> = Vec::new();
    for item in items {
        for (index, part) in crate::modal::wrap_message(item, width)
            .into_iter()
            .enumerate()
        {
            match lines.last_mut() {
                Some(line)
                    if index == 0 && display_width(line) + 3 + display_width(&part) <= width =>
                {
                    line.push_str(" · ");
                    line.push_str(&part);
                }
                _ => lines.push(part),
            }
        }
    }
    lines
}

pub(super) fn outline_items(editor: &EditorState) -> Vec<(usize, String)> {
    let mut items = Vec::new();
    for line_number in 0..editor.document.len_lines() {
        let line = editor.document.line(line_number).unwrap_or_default();
        let trimmed = line.trim();
        let interesting = trimmed.starts_with("fn ")
            || trimmed.starts_with("pub fn ")
            || trimmed.starts_with("async fn ")
            || trimmed.starts_with("pub async fn ")
            || trimmed.starts_with("class ")
            || trimmed.starts_with("struct ")
            || trimmed.starts_with("enum ")
            || trimmed.starts_with("impl ")
            || trimmed.starts_with("trait ")
            || trimmed.starts_with("function ")
            || trimmed.starts_with("def ")
            || trimmed.starts_with("interface ")
            || trimmed.starts_with("type ")
            || trimmed.starts_with("const ")
            || trimmed.starts_with("# ");
        if interesting {
            items.push((
                line_number,
                format!(
                    "{}{}",
                    " ".repeat((line.len() - line.trim_start().len()).min(64)),
                    trimmed
                ),
            ));
            if items.len() >= 200 {
                break;
            }
        }
    }
    items
}

impl App {
    /// The most useful key for `command` from the live keymap.
    pub(super) fn key_hint(&self, command: Command) -> Option<String> {
        crate::help::primary_key(&self.keybindings, &self.user_keybindings, command.as_str())
    }

    /// The start screen's shortcut hints, from the live keymap.
    pub(super) fn start_hints(&self) -> Vec<String> {
        let pairs = [
            (Command::WorkbenchQuickOpen, "quick open"),
            (Command::WorkbenchCommandPalette, "commands"),
            (Command::WorkbenchToggleTerminal, "terminal"),
            (Command::ViewSplitVertical, "split pane"),
        ]
        .into_iter()
        .filter_map(|(command, label)| Some(format!("{}  {label}", self.key_hint(command)?)))
        .collect::<Vec<_>>();
        let width = pairs
            .iter()
            .map(|pair| display_width(pair))
            .max()
            .unwrap_or(0)
            + 4;
        let mut lines = pairs
            .chunks(2)
            .map(|row| match row {
                [left, right] => format!(
                    "{left}{}{right}",
                    " ".repeat(width.saturating_sub(display_width(left)))
                ),
                [only] => only.clone(),
                _ => String::new(),
            })
            .collect::<Vec<_>>();
        lines.push(String::new());
        lines.push(match self.key_hint(Command::HelpWelcome) {
            Some(key) => format!("New here? {key} opens the Welcome guide."),
            None => "New here? Run “Help: Welcome” from the command palette.".to_owned(),
        });
        lines
    }
}

pub(super) fn render_welcome(frame: &mut Frame, area: Rect, palette: Palette, hints: &[String]) {
    let mut lines = vec![
        Line::from(Span::styled(
            "Tide",
            Style::default()
                .fg(palette.accent)
                .add_modifier(Modifier::BOLD),
        )),
        Line::raw(""),
        Line::from(Span::styled(
            "A terminal-native code editor.",
            Style::default().fg(palette.foreground),
        )),
        Line::raw(""),
    ];
    lines.extend(hints.iter().map(|hint| {
        Line::from(Span::styled(
            hint.clone(),
            Style::default().fg(palette.muted),
        ))
    }));
    frame.render_widget(Paragraph::new(lines), area);
}

pub(super) fn render_diff(
    frame: &mut Frame,
    area: Rect,
    diff: &DiffTab,
    palette: Palette,
    focused: bool,
) {
    let lines = diff
        .text
        .lines()
        .skip(diff.scroll_top)
        .take(area.height as usize)
        .map(|line| {
            let style = if line.starts_with('+') {
                Style::default().fg(palette.diff_add)
            } else if line.starts_with('-') {
                Style::default().fg(palette.diff_delete)
            } else if line.starts_with("@@") {
                Style::default().fg(palette.accent)
            } else {
                Style::default().fg(palette.foreground)
            };
            Line::from(Span::styled(line.to_owned(), style))
        })
        .collect::<Vec<_>>();
    let mut block = Block::bordered();
    if focused {
        block = block.border_style(Style::default().fg(palette.border_focused));
    }
    frame.render_widget(
        Paragraph::new(lines)
            .style(Style::default().bg(palette.background))
            .block(block),
        area,
    );
}
