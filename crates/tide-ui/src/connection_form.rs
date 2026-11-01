//! Connection presentation only. Persistence, secrets and network policy stay in App.
use crate::modal::{ModalOutcome, Submitted};
use crate::palette::Palette;
use crate::{Modal, ModalBase, ModalKind};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::{
    Frame,
    layout::Rect,
    style::Style,
    widgets::{Block, Borders, Clear, Paragraph},
};
use tide_core::num::saturating_u16;
use tide_db::{ConnectionConfig, ConnectionScope};

/// One editable row of the form, in display order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Field {
    Name,
    Host,
    Port,
    Database,
    Username,
    Password,
    TlsMode,
    CaCertificate,
    ReadOnly,
    Scope,
    ConnectTimeout,
    QueryTimeout,
}

impl Field {
    const ALL: [Self; 12] = [
        Self::Name,
        Self::Host,
        Self::Port,
        Self::Database,
        Self::Username,
        Self::Password,
        Self::TlsMode,
        Self::CaCertificate,
        Self::ReadOnly,
        Self::Scope,
        Self::ConnectTimeout,
        Self::QueryTimeout,
    ];

    fn at(index: usize) -> Option<Self> {
        Self::ALL.get(index).copied()
    }

    fn label(self) -> &'static str {
        match self {
            Self::Name => "Name",
            Self::Host => "Host",
            Self::Port => "Port",
            Self::Database => "Database",
            Self::Username => "Username",
            Self::Password => "Password",
            Self::TlsMode => "TLS mode",
            Self::CaCertificate => "CA certificate",
            Self::ReadOnly => "Read only",
            Self::Scope => "Scope",
            Self::ConnectTimeout => "Connect ms",
            Self::QueryTimeout => "Query ms",
        }
    }

    /// The values a choice field cycles through; empty for free text.
    fn choices(self) -> &'static [&'static str] {
        match self {
            Self::TlsMode => &["disable", "prefer", "require"],
            Self::ReadOnly => &["true", "false"],
            Self::Scope => &["global", "project"],
            _ => &[],
        }
    }
}

/// The form's buttons, in display order after the fields.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Button {
    Save,
    Test,
    Delete,
    Cancel,
}

impl Button {
    const ALL: [Self; 4] = [Self::Save, Self::Test, Self::Delete, Self::Cancel];

    fn label(self) -> &'static str {
        match self {
            Self::Save => "Save",
            Self::Test => "Test",
            Self::Delete => "Delete",
            Self::Cancel => "Cancel",
        }
    }

    /// The focus position of this button.
    fn focus(self) -> usize {
        Field::ALL.len() + self as usize
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConnectionAction {
    Save,
    Test,
    Delete,
}
pub struct Submission {
    pub config: ConnectionConfig,
    pub password: Option<String>,
    pub action: ConnectionAction,
}
impl std::fmt::Debug for Submission {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Submission")
            .field("config", &self.config)
            .field("password", &self.password.as_ref().map(|_| "<redacted>"))
            .field("action", &self.action)
            .finish()
    }
}
type FormLayout = (Rect, Vec<(Field, Rect)>, Vec<(Button, Rect)>);
pub struct ConnectionForm {
    base: ModalBase,
    original: ConnectionConfig,
    fields: Vec<ModalBase>,
    focus: usize,
    error: String,
}
impl ConnectionForm {
    pub fn new(config: ConnectionConfig) -> Self {
        let values = [
            config.name.clone(),
            config.host.clone(),
            config.port.to_string(),
            config.database.clone(),
            config.username.clone(),
            String::new(),
            match config.ssl_mode() {
                tide_db::SslPreference::Disable => "disable",
                tide_db::SslPreference::Prefer => "prefer",
                tide_db::SslPreference::Require => "require",
            }
            .into(),
            config
                .ssl
                .as_ref()
                .and_then(|v| v.get("ca"))
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .into(),
            config.read_only.to_string(),
            match config.scope {
                ConnectionScope::Global => "global",
                ConnectionScope::Project => "project",
            }
            .into(),
            config.connection_timeout.unwrap_or(10000).to_string(),
            config.query_timeout.unwrap_or(30000).to_string(),
        ];
        Self {
            base: ModalBase::new(
                ModalKind::ConnectionForm,
                if config.id.is_empty() {
                    "New PostgreSQL Connection"
                } else {
                    "Edit PostgreSQL Connection"
                },
                vec![],
            ),
            original: config,
            fields: values
                .into_iter()
                .map(|v| ModalBase::prompt(ModalKind::ConnectionForm, "", &v))
                .collect(),
            focus: 0,
            error: String::new(),
        }
    }
    pub fn set_password(&mut self, password: &str) {
        let field = self.field_mut(Field::Password);
        field.input = password.into();
        field.cursor = password.chars().count();
    }
    fn field(&self, field: Field) -> &ModalBase {
        &self.fields[field as usize]
    }
    fn field_mut(&mut self, field: Field) -> &mut ModalBase {
        &mut self.fields[field as usize]
    }
    fn focused_field(&self) -> Option<Field> {
        Field::at(self.focus)
    }
    fn focused_choices(&self) -> &'static [&'static str] {
        self.focused_field().map_or(&[], Field::choices)
    }
    fn cycle_choice(&mut self, forward: bool) {
        let choices = self.focused_choices();
        if choices.is_empty() {
            return;
        }
        let field = &mut self.fields[self.focus];
        let old = choices.iter().position(|v| *v == field.input).unwrap_or(0);
        let index = (old + if forward { 1 } else { choices.len() - 1 }) % choices.len();
        field.input = choices[index].into();
        field.cursor = field.input.chars().count();
    }
    fn layout(&self, bounds: Rect) -> FormLayout {
        let w = bounds.width.min(78);
        let h = bounds.height.min(19);
        let area = Rect::new(
            bounds.x + (bounds.width - w) / 2,
            bounds.y + (bounds.height - h) / 2,
            w,
            h,
        );
        if w < 4 || h < 4 {
            return (area, vec![], vec![]);
        }
        let visible = (h as usize).saturating_sub(5).min(Field::ALL.len());
        let top = self
            .focus
            .min(Field::ALL.len() - 1)
            .saturating_sub(visible.saturating_sub(1));
        let fields = Field::ALL[top..(top + visible).min(Field::ALL.len())]
            .iter()
            .enumerate()
            .map(|(row, field)| {
                (
                    *field,
                    Rect::new(area.x + 1, area.y + 1 + saturating_u16(row), w - 2, 1),
                )
            })
            .collect();
        let buttons = Button::ALL
            .into_iter()
            .filter(|button| *button != Button::Delete || !self.original.id.is_empty())
            .collect::<Vec<_>>();
        let button_count = saturating_u16(buttons.len());
        let button_w = ((w - 2) / button_count).min(12);
        let left = area.x + 1 + (w - 2 - button_w * button_count) / 2;
        let actions = buttons
            .iter()
            .enumerate()
            .map(|(offset, button)| {
                (
                    *button,
                    Rect::new(
                        left + saturating_u16(offset) * button_w,
                        area.bottom() - 2,
                        button_w,
                        1,
                    ),
                )
            })
            .collect();
        (area, fields, actions)
    }
    fn build(&self, action: ConnectionAction) -> Result<Submission, String> {
        let get = |field: Field| self.field(field).input.trim();
        let mut config = self.original.clone();
        config.name = get(Field::Name).into();
        config.host = get(Field::Host).into();
        config.port = get(Field::Port)
            .parse()
            .map_err(|_| "Port must be 1–65535")?;
        config.database = get(Field::Database).into();
        config.username = get(Field::Username).into();
        config.ssl = match get(Field::TlsMode) {
            // Unset means `require` for remote hosts, so a chosen `disable` must be explicit.
            "disable" => Some(false.into()),
            "require" | "prefer" => {
                let mut ssl = self
                    .original
                    .ssl
                    .as_ref()
                    .filter(|v| v.is_object())
                    .cloned()
                    .unwrap_or_else(|| serde_json::json!({}));
                if get(Field::TlsMode) == "prefer" && get(Field::CaCertificate).is_empty() {
                    Some(serde_json::json!("prefer"))
                } else {
                    ssl["rejectUnauthorized"] = true.into();
                    if !get(Field::CaCertificate).is_empty() {
                        ssl["ca"] = self.field(Field::CaCertificate).input.clone().into();
                    } else if let Some(o) = ssl.as_object_mut() {
                        o.remove("ca");
                    }
                    Some(ssl)
                }
            }
            _ => return Err("TLS mode must be disable, prefer, or require".into()),
        };
        let old_mode = match self.original.ssl_mode() {
            tide_db::SslPreference::Disable => "disable",
            tide_db::SslPreference::Prefer => "prefer",
            tide_db::SslPreference::Require => "require",
        };
        let old_ca = self
            .original
            .ssl
            .as_ref()
            .and_then(|v| v.get("ca"))
            .and_then(|v| v.as_str())
            .unwrap_or("");
        if get(Field::TlsMode) == old_mode && self.field(Field::CaCertificate).input == old_ca {
            config.ssl = self.original.ssl.clone();
        } else if get(Field::TlsMode) == "prefer"
            && config.ssl.as_ref().is_some_and(|v| v.is_object())
        {
            config.ssl.as_mut().unwrap()["mode"] = "prefer".into();
        }
        config.read_only = match get(Field::ReadOnly) {
            "true" => true,
            "false" => false,
            _ => return Err("Read only must be true or false".into()),
        };
        config.scope = match get(Field::Scope) {
            "global" => ConnectionScope::Global,
            "project" => ConnectionScope::Project,
            _ => return Err("Scope must be global or project".into()),
        };
        config.connection_timeout = Some(
            get(Field::ConnectTimeout)
                .parse::<u64>()
                .ok()
                .filter(|n| (1000..=120000).contains(n))
                .ok_or("Connect timeout must be 1000–120000 ms")?,
        );
        config.query_timeout = Some(
            get(Field::QueryTimeout)
                .parse::<u64>()
                .ok()
                .filter(|n| (100..=3600000).contains(n))
                .ok_or("Query timeout must be 100–3600000 ms")?,
        );
        config.validate().map_err(|e| e.to_string())?;
        Ok(Submission {
            config,
            password: Some(self.field(Field::Password).input.clone())
                .filter(|password| !password.is_empty()),
            action,
        })
    }
    fn submit(&mut self, button: Button) -> ModalOutcome {
        let submit = |submission| ModalOutcome::Submit(Submitted::Connection(Box::new(submission)));
        let action = match button {
            Button::Cancel => return ModalOutcome::Close,
            Button::Delete if self.original.id.is_empty() => return ModalOutcome::None,
            Button::Delete => {
                return submit(Submission {
                    config: self.original.clone(),
                    password: None,
                    action: ConnectionAction::Delete,
                });
            }
            Button::Save => ConnectionAction::Save,
            Button::Test => ConnectionAction::Test,
        };
        match self.build(action) {
            Ok(submission) => submit(submission),
            Err(error) => {
                self.error = error;
                ModalOutcome::None
            }
        }
    }
}
impl Modal for ConnectionForm {
    fn base(&self) -> &ModalBase {
        &self.base
    }
    fn base_mut(&mut self) -> &mut ModalBase {
        &mut self.base
    }
    fn paste(&mut self, text: &str) {
        if let Some(field) = self.fields.get_mut(self.focus) {
            for ch in text
                .chars()
                .filter(|c| !c.is_control())
                .take(4096usize.saturating_sub(field.input.len()))
            {
                field.push_char(ch);
            }
        }
    }
    fn handle_key(&mut self, key: KeyEvent) -> ModalOutcome {
        if key.code == KeyCode::Esc {
            return ModalOutcome::Close;
        }
        if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Enter {
            return self.submit(Button::Save);
        }
        if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('t') {
            return self.submit(Button::Test);
        }
        if !self.focused_choices().is_empty()
            && matches!(
                key.code,
                KeyCode::Left | KeyCode::Right | KeyCode::Char(' ')
            )
        {
            self.cycle_choice(key.code != KeyCode::Left);
            return ModalOutcome::None;
        }
        let fields = Field::ALL.len();
        let total = fields + Button::ALL.len();
        match key.code {
            KeyCode::Tab => {
                self.focus = (self.focus + 1) % total;
                if self.focus == Button::Delete.focus() && self.original.id.is_empty() {
                    self.focus += 1;
                }
            }
            KeyCode::BackTab => {
                self.focus = (self.focus + total - 1) % total;
                if self.focus == Button::Delete.focus() && self.original.id.is_empty() {
                    self.focus -= 1;
                }
            }
            KeyCode::Up => self.focus = self.focus.saturating_sub(1),
            KeyCode::Down => self.focus = (self.focus + 1).min(total - 1),
            KeyCode::Enter if self.focus >= fields => {
                return self.submit(Button::ALL[self.focus - fields]);
            }
            KeyCode::Enter => self.focus = (self.focus + 1).min(total - 1),
            KeyCode::Left | KeyCode::Right if self.focus >= fields => {
                let step = if key.code == KeyCode::Right { 1 } else { 3 };
                self.focus = fields + (self.focus - fields + step) % Button::ALL.len();
            }
            _ => {
                if let Some(field) = self.fields.get_mut(self.focus) {
                    match key.code {
                        KeyCode::Left => field.move_left(),
                        KeyCode::Right => field.move_right(),
                        KeyCode::Home => field.cursor = 0,
                        KeyCode::End => field.cursor = field.input.chars().count(),
                        KeyCode::Backspace => field.backspace(),
                        KeyCode::Delete => field.delete(),
                        KeyCode::Char('u') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                            field.input.clear();
                            field.cursor = 0;
                        }
                        KeyCode::Char(c)
                            if !key.modifiers.intersects(
                                KeyModifiers::CONTROL | KeyModifiers::ALT | KeyModifiers::SUPER,
                            ) && field.input.len() < 4096 =>
                        {
                            field.push_char(c)
                        }
                        _ => {}
                    }
                }
            }
        }
        if self.original.id.is_empty() && self.focus == Button::Delete.focus() {
            self.focus = Button::Cancel.focus();
        }
        ModalOutcome::None
    }
    fn click_key(&mut self, bounds: Rect, x: u16, y: u16) -> Option<KeyEvent> {
        let (_, fields, actions) = self.layout(bounds);
        for (button, rect) in actions {
            if rect.contains((x, y).into()) {
                self.focus = button.focus();
                return Some(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
            }
        }
        for (kind, rect) in fields {
            if rect.contains((x, y).into()) {
                self.focus = kind as usize;
                if !kind.choices().is_empty() {
                    return Some(KeyEvent::new(KeyCode::Right, KeyModifiers::NONE));
                }
                let label_width = 14u16.min(rect.width.saturating_sub(1) / 2);
                let offset = usize::from(x.saturating_sub(rect.x + label_width));
                let field = self.field_mut(kind);
                field.cursor = if kind == Field::Password {
                    offset.min(field.input.chars().count())
                } else {
                    let mut cells = 0;
                    field
                        .input
                        .chars()
                        .take_while(|c| {
                            cells += tide_core::display_width(&c.to_string());
                            cells <= offset
                        })
                        .count()
                };
                break;
            }
        }
        None
    }
    fn render(&self, frame: &mut Frame, bounds: Rect, palette: Palette) {
        let (area, fields, actions) = self.layout(bounds);
        frame.render_widget(Clear, area);
        frame.render_widget(
            Block::default()
                .borders(Borders::ALL)
                .title(self.base.title.clone())
                .style(
                    Style::default()
                        .bg(palette.background)
                        .fg(palette.border_focused),
                ),
            area,
        );
        for (kind, rect) in fields {
            let label_width = 14usize.min(rect.width.saturating_sub(1) as usize / 2);
            let label = tide_core::truncate_to_width(kind.label(), label_width);
            let field = self.field(kind);
            let focused = self.focus == kind as usize;
            let value = if kind == Field::Password {
                "•".repeat(field.input.chars().count())
            } else if kind == Field::ReadOnly {
                if field.input == "true" {
                    "[x]".into()
                } else {
                    "[ ]".into()
                }
            } else if !kind.choices().is_empty() {
                format!("‹ {} ›", field.input)
            } else {
                field.input.clone()
            };
            let width = rect.width as usize - label_width;
            let prefix = if kind == Field::Password {
                "•".repeat(field.cursor)
            } else {
                field.input.chars().take(field.cursor).collect()
            };
            let cursor = tide_core::display_width(&prefix);
            let offset = cursor.saturating_sub(width.saturating_sub(1));
            let visible = value
                .chars()
                .scan(0usize, |cells, c| {
                    let start = *cells;
                    *cells += tide_core::display_width(&c.to_string());
                    Some((start, c))
                })
                .filter(|(start, _)| *start >= offset)
                .map(|(_, c)| c)
                .collect::<String>();
            let text = format!(
                "{label}{}{}",
                " ".repeat(label_width - tide_core::display_width(label)),
                tide_core::truncate_to_width(&visible, width)
            );
            frame.render_widget(
                Paragraph::new(text).style(Style::default().fg(palette.foreground).bg(
                    if focused {
                        palette.input_bg
                    } else {
                        palette.background
                    },
                )),
                rect,
            );
            if focused && width > 0 && kind.choices().is_empty() {
                frame.set_cursor_position((
                    rect.x
                        + saturating_u16(label_width)
                        + saturating_u16(cursor.saturating_sub(offset).min(width - 1)),
                    rect.y,
                ));
            }
        }
        if area.height >= 4 && area.width >= 4 {
            let hint = if self.error.is_empty() {
                if self.focused_field() == Some(Field::Password) {
                    "Blank keeps a saved password only for the unchanged destination"
                } else {
                    "Tab fields/actions · Arrows change options · Ctrl+Enter save · Ctrl+T test"
                }
            } else {
                &self.error
            };
            frame.render_widget(
                Paragraph::new(
                    tide_core::truncate_to_width(hint, area.width.saturating_sub(2) as usize)
                        .to_owned(),
                )
                .style(Style::default().fg(if self.error.is_empty() {
                    palette.description
                } else {
                    palette.error
                })),
                Rect::new(area.x + 1, area.bottom() - 3, area.width - 2, 1),
            );
        }
        for (button, rect) in actions {
            frame.render_widget(
                Paragraph::new(format!("[{}]", button.label()))
                    .alignment(ratatui::layout::Alignment::Center)
                    .style(palette.action(
                        match button {
                            Button::Save => crate::palette::ActionRole::Confirmation,
                            Button::Delete => crate::palette::ActionRole::Destructive,
                            _ => crate::palette::ActionRole::Neutral,
                        },
                        self.focus == button.focus(),
                        true,
                    )),
                rect,
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modal::KeyOutcome;
    fn submitted(outcome: ModalOutcome) -> Submission {
        let ModalOutcome::Submit(Submitted::Connection(submission)) = outcome else {
            panic!("the form did not submit a connection");
        };
        *submission
    }
    fn config() -> ConnectionConfig {
        ConnectionConfig {
            name: "Fixture".into(),
            database: "app".into(),
            username: "fixture".into(),
            ssl: Some(true.into()),
            password_secret: Some("approved-reference".into()),
            ..Default::default()
        }
    }
    #[test]
    fn masked_password_transport_preservation_buttons_and_tiny_geometry() {
        let mut form = ConnectionForm::new(config());
        form.set_password("fixture-only-secret");
        let mut terminal =
            ratatui::Terminal::new(ratatui::backend::TestBackend::new(82, 26)).unwrap();
        terminal
            .draw(|f| form.render(f, f.area(), Palette::ultra_dusk()))
            .unwrap();
        let text = terminal
            .backend()
            .buffer()
            .content
            .iter()
            .map(|c| c.symbol())
            .collect::<String>();
        assert!(!text.contains("fixture-only-secret"));
        assert!(text.contains('•'));
        let (_, _, actions) = form.layout(Rect::new(0, 0, 82, 26));
        let test = actions
            .iter()
            .find(|(id, _)| *id == Button::Test)
            .unwrap()
            .1;
        let key = form
            .click_key(Rect::new(0, 0, 82, 26), test.x, test.y)
            .unwrap();
        let submitted = submitted(form.handle_key(key));
        assert_eq!(submitted.action, ConnectionAction::Test);
        assert_eq!(submitted.password.as_deref(), Some("fixture-only-secret"));
        assert_eq!(submitted.config.ssl, Some(true.into()));
        for width in [0, 1, 3, 12] {
            let mut tiny =
                ratatui::Terminal::new(ratatui::backend::TestBackend::new(width, 3)).unwrap();
            tiny.draw(|f| form.render(f, f.area(), Palette::ultra_dusk()))
                .unwrap();
        }
    }
    #[test]
    fn choosing_disable_for_a_remote_host_stays_plaintext() {
        let mut form = ConnectionForm::new(ConnectionConfig {
            host: "db.example.com".into(),
            ..config()
        });
        form.field_mut(Field::TlsMode).input = "disable".into();
        let submitted = submitted(form.submit(Button::Save));
        assert_eq!(submitted.config.ssl_mode(), tide_db::SslPreference::Disable);
    }
    #[test]
    fn invalid_fields_never_submit_and_password_paste_goes_to_the_focused_field() {
        let mut form = ConnectionForm::new(config());
        form.field_mut(Field::Port).input = "0".into();
        assert_eq!(form.submit(Button::Save).key(), KeyOutcome::None);
        assert!(!form.error.is_empty());
        form.focus = Field::Password as usize;
        form.paste("secret\nfixture");
        assert_eq!(form.field(Field::Password).input, "secretfixture");
        assert!(form.base.input.is_empty());
        form.field_mut(Field::Port).input = "5432".into();
        assert_eq!(form.submit(Button::Save).key(), KeyOutcome::Submit);
    }
}
