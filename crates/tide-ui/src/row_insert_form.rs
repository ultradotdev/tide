//! A table-aware insert form. Defaults and SQL NULL are explicit field modes.
use crate::modal::{ModalOutcome, Submitted};
use crate::palette::{ActionRole, Palette};
use crate::{Modal, ModalBase, ModalKind};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::{
    Frame,
    layout::Rect,
    style::Style,
    widgets::{Block, Clear, Paragraph, Wrap},
};
use serde_json::Value;
use tide_core::num::saturating_u16;
use tide_db::{InsertTarget, RowInsert};

#[derive(Clone, Copy, PartialEq, Eq)]
enum Mode {
    Default,
    Null,
    Value,
}
const ACTIONS: [&str; 5] = ["Default", "NULL", "Value", "Add Row", "Cancel"];
type Layout = (Rect, Vec<(usize, Rect)>, Vec<(usize, Rect)>);
pub struct RowInsertForm {
    base: ModalBase,
    request: RowInsert,
    fields: Vec<ModalBase>,
    modes: Vec<Mode>,
    focus: usize,
    selected: usize,
    error: String,
    submitted: bool,
}
impl RowInsertForm {
    pub fn new(connection_id: String, query_id: String, target: InsertTarget) -> Self {
        let request = RowInsert {
            connection_id,
            query_id,
            target,
            values: Default::default(),
        };
        Self::restore(request, String::new())
    }
    pub fn restore(request: RowInsert, error: String) -> Self {
        let fields = request
            .target
            .columns
            .iter()
            .map(|c| {
                ModalBase::prompt(
                    ModalKind::RowInsert,
                    &c.name,
                    &request
                        .values
                        .get(&c.name)
                        .map_or(String::new(), Value::to_string),
                )
            })
            .collect();
        let modes = request
            .target
            .columns
            .iter()
            .map(|c| match request.values.get(&c.name) {
                Some(Value::Null) => Mode::Null,
                Some(_) => Mode::Value,
                None if c.generated || c.has_default => Mode::Default,
                None if c.nullable => Mode::Null,
                None => Mode::Value,
            })
            .collect();
        let selected = request
            .target
            .columns
            .iter()
            .position(|c| !c.generated && !c.has_default && !c.nullable)
            .unwrap_or(0);
        Self {
            base: ModalBase::new(
                ModalKind::RowInsert,
                format!(
                    "Add Row · {}.{}",
                    request.target.schema, request.target.table
                ),
                vec![],
            ),
            request,
            fields,
            modes,
            focus: selected,
            selected,
            error,
            submitted: false,
        }
    }
    fn layout(&self, bounds: Rect) -> Layout {
        let width = bounds.width.min(96);
        let height = bounds.height.min(
            saturating_u16(self.fields.len())
                .saturating_add(7)
                .clamp(8, 28),
        );
        let area = Rect::new(
            bounds.x + (bounds.width - width) / 2,
            bounds.y + (bounds.height - height) / 2,
            width,
            height,
        );
        if width < 6 || height < 4 {
            return (area, vec![], vec![]);
        }
        let visible = height.saturating_sub(6) as usize;
        let top = self.selected.saturating_sub(visible.saturating_sub(1));
        let fields = (top..(top + visible).min(self.fields.len()))
            .enumerate()
            .map(|(row, i)| {
                (
                    i,
                    Rect::new(area.x + 1, area.y + 1 + saturating_u16(row), width - 2, 1),
                )
            })
            .collect();
        let button = ((width - 2) / 5).min(12);
        let x = area.x + 1 + (width - 2 - button * 5) / 2;
        let actions = (0..5_u16)
            .map(|i| {
                (
                    usize::from(i),
                    Rect::new(x + i * button, area.bottom() - 2, button, 1),
                )
            })
            .collect();
        (area, fields, actions)
    }
    fn mode(&mut self, mode: Mode) {
        let c = &self.request.target.columns[self.selected];
        if c.generated {
            self.error = "Generated columns are supplied by PostgreSQL".into();
            return;
        }
        if mode == Mode::Null && !c.nullable {
            self.error = format!("{} cannot be NULL", c.name);
            return;
        }
        if mode == Mode::Default && !c.has_default && !c.nullable {
            self.error = format!("{} has no default and needs a value", c.name);
            return;
        }
        self.modes[self.selected] = mode;
        self.error.clear();
    }
    fn submit(&mut self) -> ModalOutcome {
        let mut values = std::collections::BTreeMap::new();
        for (i, c) in self.request.target.columns.iter().enumerate() {
            if c.generated {
                continue;
            }
            match self.modes[i] {
                Mode::Default => {}
                Mode::Null => {
                    values.insert(c.name.clone(), Value::Null);
                }
                Mode::Value => {
                    let text = &self.fields[i].input;
                    // SQL NULL is exclusively the explicit NULL mode.
                    let value = serde_json::from_str(text)
                        .ok()
                        .filter(|v| !matches!(v, Value::Null))
                        .unwrap_or_else(|| Value::String(text.clone()));
                    values.insert(c.name.clone(), value);
                }
            }
        }
        self.request.values = values;
        self.submitted = true;
        ModalOutcome::Submit(Submitted::RowInsert(Box::new(self.request.clone())))
    }
}
impl Modal for RowInsertForm {
    fn base(&self) -> &ModalBase {
        &self.base
    }
    fn base_mut(&mut self) -> &mut ModalBase {
        &mut self.base
    }
    fn submitted_insert(&self) -> Option<&RowInsert> {
        self.submitted.then_some(&self.request)
    }
    fn paste(&mut self, text: &str) {
        if self.base.loading
            || self.focus >= self.fields.len()
            || self.request.target.columns[self.focus].generated
        {
            return;
        }
        self.error.clear();
        self.selected = self.focus;
        self.modes[self.focus] = Mode::Value;
        let field = &mut self.fields[self.focus];
        for c in text
            .chars()
            .filter(|c| !c.is_control())
            .take(4096usize.saturating_sub(field.input.len()))
        {
            field.push_char(c);
        }
    }
    fn handle_key(&mut self, key: KeyEvent) -> ModalOutcome {
        if self.base.loading {
            return ModalOutcome::None;
        }
        if key.code == KeyCode::Esc {
            return ModalOutcome::Close;
        }
        if key.code == KeyCode::Enter && key.modifiers.contains(KeyModifiers::CONTROL) {
            return self.submit();
        }
        if key.modifiers.contains(KeyModifiers::ALT) {
            match key.code {
                KeyCode::Char('d') => self.mode(Mode::Default),
                KeyCode::Char('n') => self.mode(Mode::Null),
                KeyCode::Char('v') => self.mode(Mode::Value),
                _ => {}
            }
            return ModalOutcome::None;
        }
        let n = self.fields.len();
        let total = n + 5;
        match key.code {
            KeyCode::Tab => self.focus = (self.focus + 1) % total,
            KeyCode::BackTab => self.focus = (self.focus + total - 1) % total,
            KeyCode::Up => self.focus = self.focus.saturating_sub(1),
            KeyCode::Down => self.focus = (self.focus + 1).min(total - 1),
            KeyCode::Enter | KeyCode::Char(' ') if self.focus >= n => match self.focus - n {
                0 => self.mode(Mode::Default),
                1 => self.mode(Mode::Null),
                2 => {
                    self.mode(Mode::Value);
                    self.focus = self.selected;
                }
                3 => return self.submit(),
                _ => return ModalOutcome::Close,
            },
            KeyCode::Enter => self.focus = (self.focus + 1).min(total - 1),
            _ if self.focus < n && !self.request.target.columns[self.focus].generated => {
                if matches!(
                    key.code,
                    KeyCode::Char(_) | KeyCode::Backspace | KeyCode::Delete
                ) {
                    self.error.clear();
                }
                let field = &mut self.fields[self.focus];
                match key.code {
                    KeyCode::Left => field.move_left(),
                    KeyCode::Right => field.move_right(),
                    KeyCode::Home => field.cursor = 0,
                    KeyCode::End => field.cursor = field.input.chars().count(),
                    KeyCode::Backspace => {
                        self.modes[self.focus] = Mode::Value;
                        field.backspace();
                    }
                    KeyCode::Delete => {
                        self.modes[self.focus] = Mode::Value;
                        field.delete();
                    }
                    KeyCode::Char('u') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                        field.input.clear();
                        field.cursor = 0;
                        self.modes[self.focus] = Mode::Value;
                    }
                    KeyCode::Char(c)
                        if !key.modifiers.intersects(
                            KeyModifiers::CONTROL
                                | KeyModifiers::ALT
                                | KeyModifiers::META
                                | KeyModifiers::SUPER,
                        ) && field.input.len() < 4096 =>
                    {
                        self.modes[self.focus] = Mode::Value;
                        field.push_char(c);
                    }
                    _ => {}
                }
            }
            _ => {}
        }
        if self.focus < n {
            self.selected = self.focus;
        }
        ModalOutcome::None
    }
    fn click_key(&mut self, bounds: Rect, x: u16, y: u16) -> Option<KeyEvent> {
        if self.base.loading {
            return None;
        }
        let (_, fields, actions) = self.layout(bounds);
        for (i, r) in actions {
            if r.contains((x, y).into()) {
                self.focus = self.fields.len() + i;
                return Some(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
            }
        }
        for (i, r) in fields {
            if r.contains((x, y).into()) {
                self.focus = i;
                self.selected = i;
                self.fields[i].cursor = self.fields[i].input.chars().count();
                break;
            }
        }
        None
    }
    fn render(&self, frame: &mut Frame, bounds: Rect, palette: Palette) {
        let (area, fields, actions) = self.layout(bounds);
        frame.render_widget(Clear, area);
        frame.render_widget(
            Block::bordered()
                .title(self.base.title.as_str())
                .style(Style::default().fg(palette.modal_fg).bg(palette.modal_bg)),
            area,
        );
        for (i, r) in fields {
            let c = &self.request.target.columns[i];
            let label_width = r.width.min(32).min(r.width / 2);
            let label = format!("{}: {}", c.name, c.data_type);
            frame.render_widget(
                Paragraph::new(tide_core::truncate_to_width(&label, label_width as usize)).style(
                    Style::default().fg(if i == self.selected {
                        palette.accent
                    } else {
                        palette.modal_fg
                    }),
                ),
                Rect::new(r.x, r.y, label_width, 1),
            );
            let value = if c.generated {
                "<generated>"
            } else {
                match self.modes[i] {
                    Mode::Default => "<default>",
                    Mode::Null => "<NULL>",
                    Mode::Value => &self.fields[i].input,
                }
            };
            let input = Rect::new(r.x + label_width, r.y, r.width - label_width, 1);
            let field = &self.fields[i];
            let prefix = field.input.chars().take(field.cursor).collect::<String>();
            let offset = tide_core::display_width(&prefix)
                .saturating_sub(input.width.saturating_sub(1) as usize);
            let visible = value
                .chars()
                .scan(0, |cells, ch| {
                    let start = *cells;
                    *cells += tide_core::display_width(&ch.to_string());
                    Some((start, ch))
                })
                .filter(|(start, _)| *start >= offset)
                .map(|(_, ch)| ch)
                .collect::<String>();
            frame.render_widget(
                Paragraph::new(tide_core::truncate_to_width(&visible, input.width as usize)).style(
                    Style::default()
                        .fg(if c.generated {
                            palette.description
                        } else {
                            palette.input_fg
                        })
                        .bg(palette.input_bg),
                ),
                input,
            );
            if self.focus == i
                && !c.generated
                && self.modes[i] == Mode::Value
                && !input.is_empty()
                && !self.base.loading
            {
                frame.set_cursor_position((
                    input.x
                        + saturating_u16(tide_core::display_width(&prefix).saturating_sub(offset))
                            .min(input.width - 1),
                    input.y,
                ));
            }
        }
        if area.height >= 4 && area.width > 2 {
            let hint = if self.base.loading {
                "Adding row…"
            } else if !self.error.is_empty() {
                &self.error
            } else {
                "Default omits field · NULL is SQL NULL · Tab fields · Ctrl+Enter add"
            };
            frame.render_widget(
                Paragraph::new(hint)
                    .wrap(Wrap { trim: false })
                    .style(Style::default().fg(if self.error.is_empty() {
                        palette.description
                    } else {
                        palette.error
                    })),
                Rect::new(area.x + 1, area.bottom() - 4, area.width - 2, 2),
            );
        }
        for (i, r) in actions {
            frame.render_widget(
                Paragraph::new(tide_core::truncate_to_width(ACTIONS[i], r.width as usize))
                    .alignment(ratatui::layout::Alignment::Center)
                    .style(palette.action(
                        if i == 3 {
                            ActionRole::Confirmation
                        } else {
                            ActionRole::Neutral
                        },
                        self.focus == self.fields.len() + i,
                        !self.base.loading,
                    )),
                r,
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modal::KeyOutcome;
    fn form() -> RowInsertForm {
        RowInsertForm::new(
            "connection".into(),
            "query".into(),
            InsertTarget {
                oid: 1,
                schema: "public".into(),
                table: "items".into(),
                columns: vec![
                    tide_db::InsertColumn {
                        name: "id".into(),
                        data_type: "bigint".into(),
                        nullable: false,
                        has_default: true,
                        generated: true,
                    },
                    tide_db::InsertColumn {
                        name: "name".into(),
                        data_type: "text".into(),
                        nullable: false,
                        has_default: false,
                        generated: false,
                    },
                    tide_db::InsertColumn {
                        name: "note".into(),
                        data_type: "text".into(),
                        nullable: true,
                        has_default: true,
                        generated: false,
                    },
                ],
            },
        )
    }
    #[test]
    fn defaults_nulls_generated_fields_and_failed_drafts_keep_their_meaning() {
        let mut form = form();
        form.paste("O'Reilly 界");
        form.selected = 2;
        form.mode(Mode::Null);
        let ModalOutcome::Submit(Submitted::RowInsert(request)) = form.submit() else {
            panic!("the form did not submit its request");
        };
        let request = *request;
        assert_eq!(form.submitted_insert().unwrap().values, request.values);
        assert!(!request.values.contains_key("id"));
        assert_eq!(request.values["name"], "O'Reilly 界");
        assert_eq!(request.values["note"], Value::Null);
        let mut restored = RowInsertForm::restore(request, "Unique constraint; change name".into());
        assert!(restored.fields[1].input.contains("O'Reilly"));
        restored.selected = 0;
        restored.mode(Mode::Value);
        assert!(restored.modes[0] == Mode::Default);
        restored.base.loading = true;
        assert_eq!(
            restored
                .handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE))
                .key(),
            KeyOutcome::None
        );
    }
    #[test]
    fn rendered_form_actions_have_real_mouse_routes_and_fit_tiny_surfaces() {
        let mut form = form();
        for (w, h) in [(160, 50), (56, 16), (22, 8), (8, 3), (1, 1)] {
            let bounds = Rect::new(0, 0, w, h);
            let mut terminal =
                ratatui::Terminal::new(ratatui::backend::TestBackend::new(w, h)).unwrap();
            terminal
                .draw(|f| form.render(f, bounds, Palette::ultra_dusk()))
                .unwrap();
            let (_, fields, actions) = form.layout(bounds);
            for (_, rect) in fields.into_iter().chain(actions.iter().copied()) {
                assert_eq!(rect.intersection(bounds), rect);
            }
            if let Some((_, rect)) = actions.iter().find(|(i, _)| *i == 4) {
                let key = form.click_key(bounds, rect.x, rect.y).unwrap();
                assert_eq!(form.handle_key(key).key(), KeyOutcome::Close);
            }
        }
    }
}
