use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::Line;
use ratatui::widgets::{Block, Borders, Paragraph};

use tide_core::num::saturating_u16;
use tide_db::QueryResult;

use crate::editor::{EditorState, EditorView};

#[derive(Debug, Clone, Copy)]
pub enum SqlAction {
    Editor,
    View,
    Details,
    Schema,
    Add,
    Back,
    Edit,
    Save,
    Delete,
    Export,
    Refresh,
    Cancel,
}
pub struct SqlTab {
    pub read_only: bool,
    pub results_focused: bool,
    pub view_mode: u8,
    pub row_edits: std::collections::BTreeMap<String, serde_json::Value>,
    order: Vec<usize>,
    sort_column: Option<usize>,
    sort_state: u8,
    column_widths: Vec<usize>,
    text_lines: Vec<String>,
    text_scroll: usize,
    detail_scroll: usize,
    visible_rows: usize,
    editor_area: Rect,
    results_area: Rect,
    data_area: Rect,
    headers: Vec<(usize, Rect)>,
    actions: Vec<(Rect, SqlAction)>,
    /// Toolbar labels, where they sit and whether they are disabled.
    toolbar: Vec<(&'static str, Rect, bool)>,
    /// The results body below the toolbar.
    body_area: Rect,
    pub editor: EditorState,
    pub connection_id: Option<String>,
    pub connection_name: Option<String>,
    pub result: Option<QueryResult>,
    pub error: Option<String>,
    pub running: bool,
    pub selected_row: usize,
    pub result_scroll: usize,
    pub title: String,
    pub generation: u64,
    pub selected_column: usize,
    pub column_offset: usize,
    pub details_mode: bool,
    pub sort_ascending: bool,
}

impl std::fmt::Debug for SqlTab {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("SqlTab")
            .field("connection_id", &self.connection_id)
            .field("running", &self.running)
            .field(
                "rows",
                &self.result.as_ref().map_or(0, |result| result.row_count),
            )
            .finish_non_exhaustive()
    }
}

impl Default for SqlTab {
    fn default() -> Self {
        Self::new()
    }
}

impl SqlTab {
    pub fn new() -> Self {
        Self {
            read_only: true,
            results_focused: false,
            view_mode: 0,
            row_edits: Default::default(),
            order: Vec::new(),
            sort_column: None,
            sort_state: 0,
            column_widths: Vec::new(),
            text_lines: Vec::new(),
            text_scroll: 0,
            detail_scroll: 0,
            visible_rows: 10,
            editor_area: Rect::default(),
            results_area: Rect::default(),
            data_area: Rect::default(),
            headers: Vec::new(),
            actions: Vec::new(),
            toolbar: Vec::new(),
            body_area: Rect::default(),
            editor: EditorState::scratch("", "sql"),
            connection_id: None,
            connection_name: None,
            result: None,
            error: None,
            running: false,
            selected_row: 0,
            result_scroll: 0,
            title: "SQL".to_owned(),
            generation: 0,
            selected_column: 0,
            column_offset: 0,
            details_mode: false,
            sort_ascending: true,
        }
    }

    pub fn from_content(content: &str) -> Self {
        let mut tab = Self::new();
        tab.editor = EditorState::scratch(content, "sql");
        tab
    }

    pub fn sql(&self) -> String {
        self.editor.document.text()
    }

    pub fn set_sql(&mut self, sql: &str) {
        self.editor.document.set_content(sql);
        self.editor
            .document
            .set_cursor(tide_core::Position::new(0, 0));
    }

    pub fn set_connection(&mut self, id: Option<String>, name: Option<String>) {
        self.connection_id = id;
        self.connection_name = name.clone();
        self.title = name.map_or_else(|| "SQL".to_owned(), |name| format!("SQL · {name}"));
    }

    pub fn clear_result(&mut self) {
        self.result = None;
        self.order.clear();
        self.text_lines.clear();
        self.row_edits.clear();
        self.error = None;
        self.selected_row = 0;
        self.result_scroll = 0;
    }

    pub fn set_result(&mut self, result: QueryResult) {
        self.selected_row = 0;
        self.result_scroll = 0;
        self.error = None;
        self.order = (0..result.rows.len()).collect();
        self.column_widths = result
            .fields
            .iter()
            .map(|f| {
                result.rows.iter().take(100).fold(
                    tide_core::display_width(&f.name).clamp(6, 28),
                    |w, row| {
                        w.max(
                            row.get(&f.name)
                                .map_or(0, |v| tide_core::display_width(&display_value(v)).min(28)),
                        )
                    },
                )
            })
            .collect();
        self.sort_column = None;
        self.sort_state = 0;
        self.row_edits.clear();
        self.details_mode = false;
        self.result = Some(result);
        self.results_focused = true;
        self.rebuild_text();
        self.selected_column = 0;
        self.column_offset = 0;
        self.running = false;
    }

    pub fn move_column(&mut self, forward: bool) {
        let Some(result) = &self.result else {
            return;
        };
        if result.fields.is_empty() {
            return;
        }
        self.selected_column = if forward {
            (self.selected_column + 1).min(result.fields.len() - 1)
        } else {
            self.selected_column.saturating_sub(1)
        };
        if self.selected_column < self.column_offset {
            self.column_offset = self.selected_column;
        }
    }

    pub fn sort_by_selected_column(&mut self) {
        let Some(result) = &self.result else {
            return;
        };
        let Some(field) = result.fields.get(self.selected_column) else {
            return;
        };
        self.sort_state = if self.sort_column == Some(self.selected_column) {
            (self.sort_state + 1) % 3
        } else {
            1
        };
        self.sort_column = (self.sort_state != 0).then_some(self.selected_column);
        self.order = (0..result.rows.len()).collect();
        if self.sort_state != 0 {
            self.order.sort_by(|a, b| {
                let order = compare_values(
                    result.rows[*a].get(&field.name),
                    result.rows[*b].get(&field.name),
                    &field.data_type,
                );
                if self.sort_state == 2 {
                    order.reverse()
                } else {
                    order
                }
            });
        }
        self.selected_row = 0;
        self.result_scroll = 0;
        self.rebuild_text();
    }

    pub fn set_error(&mut self, error: String) {
        self.error = Some(error);
        self.running = false;
    }

    pub fn move_selection(&mut self, forward: bool) {
        if self.details_mode {
            let limit = self
                .result
                .as_ref()
                .map_or(0, |r| r.fields.len().saturating_sub(1));
            self.selected_column = self
                .selected_column
                .saturating_add_signed(if forward { 1 } else { -1 })
                .min(limit);
            return;
        }
        if self.view_mode != 0 {
            self.scroll_results(if forward { 1 } else { -1 });
            return;
        }
        let Some(result) = &self.result else {
            return;
        };
        if result.rows.is_empty() {
            return;
        }
        self.selected_row = if forward {
            (self.selected_row + 1).min(result.rows.len() - 1)
        } else {
            self.selected_row.saturating_sub(1)
        };
        self.ensure_row_visible(self.visible_rows.max(1));
    }

    pub fn scroll_results(&mut self, lines: isize) {
        if self.view_mode != 0 && !self.details_mode {
            self.text_scroll = self
                .text_scroll
                .saturating_add_signed(lines)
                .min(self.text_lines.len().saturating_sub(self.visible_rows));
            return;
        }
        let Some(result) = &self.result else {
            return;
        };
        let max = result.rows.len().saturating_sub(1);
        self.selected_row = if lines.is_negative() {
            self.selected_row.saturating_sub(lines.unsigned_abs())
        } else {
            self.selected_row
                .saturating_add(lines.unsigned_abs())
                .min(max)
        };
        self.ensure_row_visible(self.visible_rows.max(1));
    }

    fn ensure_row_visible(&mut self, visible: usize) {
        if self.selected_row < self.result_scroll {
            self.result_scroll = self.selected_row;
        } else if self.selected_row >= self.result_scroll + visible {
            self.result_scroll = self.selected_row + 1 - visible;
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub fn apply_row_change(
        &mut self,
        original: &std::collections::BTreeMap<String, serde_json::Value>,
        updated: Option<std::collections::BTreeMap<String, serde_json::Value>>,
    ) {
        let Some(mut result) = self.result.take() else {
            return;
        };
        let Some(index) = result.rows.iter().position(|row| row == original) else {
            self.result = Some(result);
            return;
        };
        if let Some(row) = updated {
            result.rows[index] = row;
        } else {
            result.rows.remove(index);
        }
        result.row_count = result.rows.len();
        self.set_result(result);
    }
    pub fn cycle_view(&mut self) {
        self.details_mode = false;
        self.view_mode = (self.view_mode + 1) % 3;
        self.text_scroll = 0;
        self.rebuild_text();
    }
    fn rebuild_text(&mut self) {
        self.text_lines.clear();
        let Some(result) = &self.result else {
            return;
        };
        if self.view_mode == 1 {
            let rows = self
                .order
                .iter()
                .filter_map(|i| result.rows.get(*i))
                .collect::<Vec<_>>();
            self.text_lines = serde_json::to_string_pretty(&rows)
                .unwrap_or_default()
                .lines()
                .map(str::to_owned)
                .collect();
        } else if self.view_mode == 2 {
            self.text_lines.push(
                result
                    .fields
                    .iter()
                    .map(|f| f.name.clone())
                    .collect::<Vec<_>>()
                    .join("\t"),
            );
            self.text_lines.extend(self.order.iter().map(|i| {
                result
                    .fields
                    .iter()
                    .map(|f| {
                        result.rows[*i]
                            .get(&f.name)
                            .map_or_else(String::new, display_value)
                    })
                    .collect::<Vec<_>>()
                    .join("\t")
            }));
        }
    }
    pub fn selected_record(
        &self,
    ) -> Option<&std::collections::BTreeMap<String, serde_json::Value>> {
        self.result
            .as_ref()?
            .rows
            .get(*self.order.get(self.selected_row)?)
    }
    pub fn show_details(&mut self) {
        self.details_mode = true;
        self.results_focused = true;
        self.row_edits.clear();
    }
    pub fn editor_area(&self) -> Rect {
        self.editor_area
    }
    pub fn results_area(&self) -> Rect {
        self.results_area
    }
    pub fn scroll_area(&self) -> Rect {
        self.data_area
    }
    pub fn scroll_metrics(&self) -> (usize, usize, usize) {
        if self.result.is_none() {
            return (0, 0, 0);
        }
        if self.details_mode {
            (
                self.result.as_ref().unwrap().fields.len(),
                self.data_area.height as usize,
                self.detail_scroll,
            )
        } else if self.view_mode != 0 {
            (
                self.text_lines.len(),
                self.data_area.height as usize,
                self.text_scroll,
            )
        } else {
            (
                self.order.len(),
                self.data_area.height as usize,
                self.result_scroll,
            )
        }
    }
    pub fn jump_results(&mut self, top: usize) {
        let (total, visible, _) = self.scroll_metrics();
        let top = top.min(total.saturating_sub(visible));
        if self.details_mode {
            self.detail_scroll = top;
            self.selected_column = self
                .selected_column
                .max(top)
                .min(top + visible.saturating_sub(1));
        } else if self.view_mode != 0 {
            self.text_scroll = top;
        } else {
            self.result_scroll = top;
            self.selected_row = self
                .selected_row
                .max(top)
                .min(top + visible.saturating_sub(1));
        }
    }
    pub fn mouse(&mut self, x: u16, y: u16, clicks: u8) -> Option<SqlAction> {
        if self.editor_area.contains((x, y).into()) {
            self.results_focused = false;
            return None;
        }
        if !self.results_area.contains((x, y).into()) {
            return None;
        }
        self.results_focused = true;
        for (rect, action) in &self.actions {
            if rect.contains((x, y).into()) {
                return Some(*action);
            }
        }
        for (column, rect) in &self.headers {
            if rect.contains((x, y).into()) {
                self.selected_column = *column;
                self.sort_by_selected_column();
                return None;
            }
        }
        if self.details_mode {
            if y >= self.data_area.y && y < self.data_area.bottom() {
                self.selected_column =
                    (self.detail_scroll + y.saturating_sub(self.data_area.y) as usize).min(
                        self.result
                            .as_ref()
                            .map_or(0, |r| r.fields.len().saturating_sub(1)),
                    );
                if clicks >= 2 {
                    return Some(SqlAction::Edit);
                }
            }
        } else if self.view_mode == 0 && y >= self.data_area.y && y < self.data_area.bottom() {
            self.selected_row = (self.result_scroll + y.saturating_sub(self.data_area.y) as usize)
                .min(self.order.len().saturating_sub(1));
            if clicks >= 2 {
                return Some(SqlAction::Details);
            }
        }
        None
    }
    /// Places the editor, toolbar and result grid inside `area`, keeping the selection in view,
    /// for the next [`Self::render`].
    pub fn layout(&mut self, area: Rect, view: &EditorView<'_>) {
        let [editor, results] =
            Layout::vertical([Constraint::Percentage(55), Constraint::Min(5)]).areas(area);
        self.editor_area = editor;
        self.results_area = results;
        self.headers.clear();
        self.actions.clear();
        self.toolbar.clear();
        self.data_area = Rect::default();
        self.body_area = Rect::default();
        self.editor.layout(
            editor,
            &EditorView {
                focused: view.focused && !self.results_focused,
                ..*view
            },
        );
        let inner = Block::default().borders(Borders::ALL).inner(results);
        if inner.is_empty() {
            return;
        }
        let menu = if self.details_mode {
            vec![
                ("← Results", SqlAction::Back),
                ("Edit", SqlAction::Edit),
                ("Save", SqlAction::Save),
                ("Delete", SqlAction::Delete),
            ]
        } else {
            vec![
                ("← SQL", SqlAction::Editor),
                ("Schema", SqlAction::Schema),
                ("Add Row", SqlAction::Add),
                (
                    match self.view_mode {
                        1 => "JSON",
                        2 => "Text",
                        _ => "Table",
                    },
                    SqlAction::View,
                ),
                ("Details", SqlAction::Details),
                ("Export", SqlAction::Export),
                (
                    if self.running { "Cancel" } else { "Refresh" },
                    if self.running {
                        SqlAction::Cancel
                    } else {
                        SqlAction::Refresh
                    },
                ),
            ]
        };
        let mut x = inner.x;
        let mut toolbar_row = 0;
        for (label, action) in menu {
            if x > inner.x
                && x.saturating_add(
                    saturating_u16(tide_core::display_width(label)).saturating_add(2),
                ) > inner.right()
                && toolbar_row < 2
                && inner.height > toolbar_row + 4
            {
                toolbar_row += 1;
                x = inner.x;
            }
            let width = saturating_u16(tide_core::display_width(label) + 2)
                .min(inner.right().saturating_sub(x));
            if width == 0 {
                break;
            }
            let rect = Rect::new(x, inner.y + toolbar_row, width, 1);
            let write = matches!(
                action,
                SqlAction::Edit | SqlAction::Save | SqlAction::Delete | SqlAction::Add
            );
            let missing_source = self.result.as_ref().is_none_or(|r| {
                if matches!(action, SqlAction::Add) {
                    r.row_source.is_none()
                } else {
                    r.row_target.is_none()
                }
            });
            let disabled = write && (self.read_only || self.running || missing_source);
            if !disabled {
                self.actions.push((rect, action));
            }
            self.toolbar.push((label, rect, disabled));
            x += width + 1;
        }
        let mut body = Rect::new(
            inner.x,
            inner.y + toolbar_row + 1,
            inner.width,
            inner.height.saturating_sub(toolbar_row + 2),
        );
        self.body_area = body;
        self.visible_rows = body.height as usize;
        if self.error.is_some() {
            return;
        }
        let Some(result) = &self.result else {
            return;
        };
        if self.details_mode {
            if self.selected_column < self.detail_scroll {
                self.detail_scroll = self.selected_column;
            }
            if self.selected_column >= self.detail_scroll + body.height as usize {
                self.detail_scroll = self.selected_column + 1 - body.height as usize;
            }
            self.data_area = body;
            return;
        }
        if self.view_mode != 0 {
            self.text_scroll = self
                .text_scroll
                .min(self.text_lines.len().saturating_sub(body.height as usize));
            self.data_area = body;
            return;
        }
        if body.height == 0 {
            return;
        }
        let mut columns = Vec::new();
        let mut offset = self.column_offset;
        // Keep the selected column in the visible horizontal viewport.
        if self.selected_column < offset {
            offset = self.selected_column;
        }
        loop {
            columns.clear();
            let mut used = 0;
            for (i, _) in result.fields.iter().enumerate().skip(offset) {
                let width = (self.column_widths.get(i).copied().unwrap_or(8))
                    .min(body.width.saturating_sub(used) as usize);
                if width == 0 {
                    break;
                }
                columns.push((i, width));
                used = used.saturating_add(saturating_u16(width + 1));
                if used >= body.width {
                    break;
                }
            }
            if columns.iter().any(|(i, _)| *i == self.selected_column)
                || offset >= self.selected_column
            {
                break;
            }
            offset += 1;
        }
        self.column_offset = offset;
        let mut x = body.x;
        for (i, width) in &columns {
            self.headers
                .push((*i, Rect::new(x, body.y, saturating_u16(*width), 1)));
            x += saturating_u16(*width) + 1;
        }
        body.y += 1;
        body.height -= 1;
        self.visible_rows = body.height as usize;
        self.data_area = body;
        self.ensure_row_visible(self.visible_rows.max(1));
    }

    /// Lays out and draws the tab into `area`.
    pub fn draw(&mut self, frame: &mut Frame, area: Rect, view: &EditorView<'_>) {
        self.layout(area, view);
        self.render(frame, view);
    }

    /// Draws the tab where the last [`Self::layout`] placed it.
    pub fn render(&self, frame: &mut Frame, view: &EditorView<'_>) {
        let (palette, focused) = (view.palette, view.focused);
        self.editor.render(
            frame,
            &EditorView {
                focused: focused && !self.results_focused,
                ..*view
            },
        );
        let results = self.results_area;
        let block = Block::default()
            .title(format!(
                " Results{} · {}{} ",
                if self.read_only { " [read-only]" } else { "" },
                self.connection_name.as_deref().unwrap_or("none"),
                if self.running { " · running…" } else { "" }
            ))
            .borders(Borders::ALL)
            .border_style(Style::default().fg(if focused && self.results_focused {
                palette.border_focused
            } else {
                palette.border
            }))
            .style(Style::default().bg(palette.background));
        let inner = block.inner(results);
        frame.render_widget(block, results);
        if inner.is_empty() {
            return;
        }
        for (label, rect, disabled) in &self.toolbar {
            frame.render_widget(
                Paragraph::new(
                    tide_core::truncate_to_width(&format!("[{label}]"), rect.width as usize)
                        .to_owned(),
                )
                .style(Style::default().fg(if *disabled {
                    palette.description
                } else {
                    palette.accent
                })),
                *rect,
            );
        }
        let body = self.body_area;
        if let Some(error) = &self.error {
            frame.render_widget(
                Paragraph::new(error.clone())
                    .wrap(ratatui::widgets::Wrap { trim: false })
                    .style(Style::default().fg(palette.error)),
                body,
            );
            return;
        }
        let Some(result) = &self.result else {
            frame.render_widget(
                Paragraph::new("Ctrl+R / F5 run query · choose a connection first")
                    .wrap(ratatui::widgets::Wrap { trim: false })
                    .style(Style::default().fg(palette.description)),
                body,
            );
            return;
        };
        let footer = format!(
            "{}{} rows · {} ms{}{} · click header to sort · Enter details{}",
            result
                .notices
                .first()
                .map_or_else(String::new, |notice| format!("⚠ {notice} · ")),
            result.row_count,
            result.duration_ms,
            result
                .affected_rows
                .map_or(String::new(), |n| format!(" · {n} affected")),
            if result.truncated {
                " · truncated"
            } else {
                ""
            },
            if self.read_only { " · read-only" } else { "" }
        );
        if inner.height > 1 {
            let color = if result.notices.is_empty() {
                palette.description
            } else {
                palette.warning
            };
            frame.render_widget(
                Paragraph::new(footer).style(Style::default().fg(color)),
                Rect::new(inner.x, inner.bottom() - 1, inner.width, 1),
            );
        }
        if self.details_mode {
            let fields = &result.fields;
            let original = self.selected_record().cloned().unwrap_or_default();
            let lines = fields
                .iter()
                .enumerate()
                .skip(self.detail_scroll)
                .take(body.height as usize)
                .map(|(i, f)| {
                    let value = self
                        .row_edits
                        .get(&f.name)
                        .or_else(|| original.get(&f.name));
                    let value = value.map_or(String::new(), |v| {
                        serde_json::to_string(v).unwrap_or_default()
                    });
                    Line::styled(
                        format!(
                            "{} {}: {}",
                            if self.row_edits.contains_key(&f.name) {
                                "*"
                            } else {
                                " "
                            },
                            f.name,
                            value
                        ),
                        Style::default()
                            .fg(palette.foreground)
                            .bg(if i == self.selected_column {
                                palette.selection_bg
                            } else {
                                palette.background
                            }),
                    )
                })
                .collect::<Vec<_>>();
            frame.render_widget(Paragraph::new(lines), body);
            return;
        }
        if self.view_mode != 0 {
            frame.render_widget(
                Paragraph::new(
                    self.text_lines
                        .iter()
                        .skip(self.text_scroll)
                        .take(body.height as usize)
                        .map(|l| Line::raw(l.clone()))
                        .collect::<Vec<_>>(),
                )
                .style(Style::default().fg(palette.foreground)),
                body,
            );
            return;
        }
        let columns = self
            .headers
            .iter()
            .map(|(i, rect)| (*i, rect.width as usize))
            .collect::<Vec<_>>();
        for (i, rect) in &self.headers {
            let marker = if self.sort_column == Some(*i) {
                match self.sort_state {
                    1 => " ▲",
                    2 => " ▼",
                    _ => "",
                }
            } else {
                ""
            };
            frame.render_widget(
                Paragraph::new(pad(
                    &(result.fields[*i].name.clone() + marker),
                    rect.width as usize,
                ))
                .style(
                    Style::default()
                        .fg(palette.accent)
                        .bg(if *i == self.selected_column {
                            palette.selection_bg
                        } else {
                            palette.background
                        })
                        .add_modifier(Modifier::BOLD),
                ),
                *rect,
            );
        }
        let body = self.data_area;
        for row in 0..body.height as usize {
            let visible = self.result_scroll + row;
            let Some(record) = self.order.get(visible).and_then(|i| result.rows.get(*i)) else {
                break;
            };
            let mut x = body.x;
            for (i, width) in &columns {
                let text = record
                    .get(&result.fields[*i].name)
                    .map_or_else(String::new, display_value);
                frame.render_widget(
                    Paragraph::new(pad(&text, *width)).style(
                        Style::default().fg(palette.foreground).bg(
                            if visible == self.selected_row {
                                palette.selection_bg
                            } else {
                                palette.background
                            },
                        ),
                    ),
                    Rect::new(x, body.y + saturating_u16(row), saturating_u16(*width), 1),
                );
                x += saturating_u16(*width) + 1;
            }
        }
    }
}

fn display_value(value: &serde_json::Value) -> String {
    match value {
        serde_json::Value::Null => "NULL".to_owned(),
        serde_json::Value::String(text) => text.clone(),
        other => other.to_string(),
    }
}

fn pad(text: &str, width: usize) -> String {
    let mut text = tide_core::truncate_to_width(text, width).to_owned();
    let length = tide_core::display_width(&text);
    if length < width {
        text.push_str(&" ".repeat(width - length));
    }
    text
}

fn compare_values(
    a: Option<&serde_json::Value>,
    b: Option<&serde_json::Value>,
    kind: &str,
) -> std::cmp::Ordering {
    use std::cmp::Ordering;
    let a = a.unwrap_or(&serde_json::Value::Null);
    let b = b.unwrap_or(&serde_json::Value::Null);
    if a.is_null() || b.is_null() {
        return a.is_null().cmp(&b.is_null());
    }
    if matches!(kind, "float4" | "float8")
        && let (Ok(a), Ok(b)) = (
            display_value(a).parse::<f64>(),
            display_value(b).parse::<f64>(),
        )
    {
        return a.total_cmp(&b);
    }
    if matches!(
        kind,
        "int2" | "int4" | "int8" | "oid" | "numeric" | "decimal" | "float4" | "float8"
    ) {
        return decimal_compare(&display_value(a), &display_value(b))
            .unwrap_or_else(|| display_value(a).cmp(&display_value(b)));
    }
    if let (Some(a), Some(b)) = (a.as_bool(), b.as_bool()) {
        return a.cmp(&b);
    }
    let order = display_value(a).cmp(&display_value(b));
    if order == Ordering::Equal {
        Ordering::Equal
    } else {
        order
    }
}
// Compare ordinary decimal text without losing bigint/numeric precision.
fn decimal_compare(a: &str, b: &str) -> Option<std::cmp::Ordering> {
    fn parts(s: &str) -> Option<(bool, String, String)> {
        let s = s.trim();
        let neg = s.starts_with('-');
        let s = s.trim_start_matches(['-', '+']);
        let (whole, fraction) = s.split_once('.').unwrap_or((s, ""));
        if whole.is_empty()
            || !whole.chars().all(|c| c.is_ascii_digit())
            || !fraction.chars().all(|c| c.is_ascii_digit())
        {
            return None;
        }
        let whole = whole.trim_start_matches('0').to_owned();
        let fraction = fraction.trim_end_matches('0').to_owned();
        Some((
            neg && (!whole.is_empty() || !fraction.is_empty()),
            whole,
            fraction,
        ))
    }
    let (an, aw, af) = parts(a)?;
    let (bn, bw, bf) = parts(b)?;
    let ordering = bn.cmp(&an).then_with(|| {
        let v = aw
            .len()
            .cmp(&bw.len())
            .then_with(|| aw.cmp(&bw))
            .then_with(|| {
                let width = af.len().max(bf.len());
                af.bytes()
                    .chain(std::iter::repeat(b'0'))
                    .take(width)
                    .cmp(bf.bytes().chain(std::iter::repeat(b'0')).take(width))
            });
        if an { v.reverse() } else { v }
    });
    Some(ordering)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::palette::Palette;
    fn result() -> QueryResult {
        serde_json::from_value(serde_json::json!({"queryId":"fixture","connectionId":"local","sql":"select n from fixture","rows":[{"n":"10"},{"n":"2"},{"n":null},{"n":"-12"},{"n":"9007199254740993"},{"n":"9007199254740992"},{"n":"1.10"}],"fields":[{"name":"n","dataType":"numeric","dataTypeId":1700}],"rowCount":7,"startedAt":"","completedAt":"","durationMs":1})).unwrap()
    }
    #[test]
    fn numeric_sort_preserves_precision_nulls_and_original_order() {
        let mut tab = SqlTab::new();
        tab.set_result(result());
        tab.sort_by_selected_column();
        assert_eq!(tab.order, vec![3, 6, 1, 0, 5, 4, 2]);
        tab.sort_by_selected_column();
        assert_eq!(tab.order, vec![2, 4, 5, 0, 1, 6, 3]);
        tab.sort_by_selected_column();
        assert_eq!(tab.order, (0..7).collect::<Vec<_>>());
        assert_eq!(
            decimal_compare("-0.10", "-0.100"),
            Some(std::cmp::Ordering::Equal)
        );
        assert_eq!(tide_core::display_width(&pad("👩‍💻 x", 3)), 3);
    }
    #[test]
    fn rendered_headers_rows_views_and_owner_have_actual_mouse_routes() {
        let mut tab = SqlTab::new();
        tab.set_result(result());
        let mut terminal =
            ratatui::Terminal::new(ratatui::backend::TestBackend::new(82, 26)).unwrap();
        let syntax = tide_services::SyntaxHighlighter::new(
            tide_config::builtin::themes().get("ultra-dusk").unwrap(),
        );
        terminal
            .draw(|f| {
                tab.draw(
                    f,
                    f.area(),
                    &EditorView {
                        palette: Palette::ultra_dusk(),
                        syntax: &syntax,
                        show_line_numbers: true,
                        tab_size: 2,
                        focused: true,
                    },
                )
            })
            .unwrap();
        let header = tab.headers[0].1;
        assert!(tab.mouse(header.x, header.y, 1).is_none());
        assert_eq!(tab.order[0], 3);
        let row = tab.data_area;
        tab.mouse(row.x, row.y + 1, 1);
        assert_eq!(tab.selected_row, 1);
        assert!(matches!(
            tab.mouse(row.x, row.y + 1, 2),
            Some(SqlAction::Details)
        ));
        tab.show_details();
        assert!(tab.details_mode);
        tab.cycle_view();
        assert_eq!(tab.view_mode, 1);
        assert!(tab.text_lines.join("\n").contains("9007199254740993"));
        tab.cycle_view();
        assert_eq!(tab.view_mode, 2);
        assert_eq!(tab.text_lines[0], "n");
        tab.cycle_view();
        assert_eq!(tab.view_mode, 0);
        let editor = tab.editor_area;
        tab.mouse(editor.x, editor.y, 1);
        assert!(!tab.results_focused);
    }
}
