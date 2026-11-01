use std::path::{Path, PathBuf};

use anyhow::Result;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

use tide_terminal::{
    CellSnapshot, PtyConfig, PtySession, TerminalColor, TerminalEmulator, TerminalSnapshot,
};

use crate::palette::Palette;
use tide_core::num::saturating_u16;

/// Woken by every terminal's reader thread when output arrives, so the event
/// loop can idle instead of polling PTYs on a fast timer.
pub(crate) static OUTPUT: std::sync::LazyLock<tokio::sync::Notify> =
    std::sync::LazyLock::new(tokio::sync::Notify::new);

fn signal_output(config: &mut PtyConfig) {
    config.on_output = Some(tide_terminal::OutputSignal(std::sync::Arc::new(|| {
        OUTPUT.notify_one();
    })));
}

pub struct SpawnSpec {
    pub program: String,
    pub args: Vec<String>,
    pub env: Vec<(String, String)>,
    pub cols: u16,
    pub rows: u16,
    pub scrollback: usize,
}

impl SpawnSpec {
    pub fn new(program: impl Into<String>, args: Vec<String>) -> Self {
        Self {
            program: program.into(),
            args,
            env: Vec::new(),
            cols: 100,
            rows: 24,
            scrollback: 5_000,
        }
    }
}

pub struct TerminalTab {
    pub id: usize,
    pub title: String,
    pub cwd: PathBuf,
    session: Option<PtySession>,
    pending: Option<PtyConfig>,
    emulator: TerminalEmulator,
    cols: u16,
    rows: u16,
    scrollback: usize,
    last_generation: u64,
    pub ai_provider: Option<String>,
    pub ai_session_id: Option<String>,
    pub exited: Option<u32>,
    reported_focus: Option<bool>,
    input_error: Option<String>,
}

impl std::fmt::Debug for TerminalTab {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("TerminalTab")
            .field("id", &self.id)
            .field("title", &self.title)
            .field("cwd", &self.cwd)
            .field("size", &(self.cols, self.rows))
            .finish_non_exhaustive()
    }
}

impl TerminalTab {
    pub fn new(
        id: usize,
        cwd: impl AsRef<Path>,
        cols: u16,
        rows: u16,
        scrollback: usize,
        shell: Option<&str>,
    ) -> Result<Self> {
        let mut config = if let Some(shell) = shell {
            let mut config = PtyConfig::shell(Some(cwd.as_ref().to_path_buf()));
            config.program = shell.to_owned();
            config.args = default_shell_args(shell);
            config
        } else {
            PtyConfig::shell(Some(cwd.as_ref().to_path_buf()))
        };
        config.cwd = Some(cwd.as_ref().to_path_buf());
        config.cols = cols.max(2);
        config.rows = rows.max(1);
        signal_output(&mut config);
        let session = PtySession::spawn(config)?;
        let emulator = TerminalEmulator::new(cols as usize, rows as usize, scrollback);
        Ok(Self {
            id,
            title: "Terminal".to_owned(),
            cwd: cwd.as_ref().to_path_buf(),
            session: Some(session),
            pending: None,
            emulator,
            cols: cols.max(2),
            rows: rows.max(1),
            scrollback,
            last_generation: 0,
            ai_provider: None,
            ai_session_id: None,
            exited: None,
            reported_focus: None,
            input_error: None,
        })
    }

    /// Spawn an arbitrary program (used by AI CLI adapters).
    pub fn spawn(id: usize, cwd: impl AsRef<Path>, spec: SpawnSpec) -> Result<Self> {
        Self::spawn_configured(id, cwd, spec, true)
    }
    pub fn spawn_configured(
        id: usize,
        cwd: impl AsRef<Path>,
        spec: SpawnSpec,
        start: bool,
    ) -> Result<Self> {
        let mut config = PtyConfig::shell(Some(cwd.as_ref().to_path_buf()));
        config.program = spec.program.clone();
        config.args = spec.args;
        config.env.extend(spec.env);
        config.cwd = Some(cwd.as_ref().to_path_buf());
        config.cols = spec.cols.max(2);
        config.rows = spec.rows.max(1);
        signal_output(&mut config);
        let session = if start {
            Some(PtySession::spawn(config.clone())?)
        } else {
            None
        };
        let pending = if start { None } else { Some(config) };
        let emulator =
            TerminalEmulator::new(spec.cols as usize, spec.rows as usize, spec.scrollback);
        Ok(Self {
            id,
            title: spec.program,
            cwd: cwd.as_ref().to_path_buf(),
            session,
            pending,
            emulator,
            cols: spec.cols.max(2),
            rows: spec.rows.max(1),
            scrollback: spec.scrollback,
            last_generation: 0,
            ai_provider: None,
            ai_session_id: None,
            exited: None,
            reported_focus: None,
            input_error: None,
        })
    }

    fn queue_input(&mut self, bytes: &[u8]) -> bool {
        let Some(session) = self.session.as_mut() else {
            return false;
        };
        match session.write(bytes) {
            Ok(()) => true,
            Err(error) => {
                self.input_error = Some(error.to_string());
                false
            }
        }
    }
    pub fn take_input_error(&mut self) -> Option<String> {
        self.input_error.take()
    }

    pub fn poll(&mut self) -> bool {
        let Some(session) = self.session.as_mut() else {
            return false;
        };
        let mut changed = false;
        if let Some(error) = session.take_write_error() {
            self.input_error = Some(error);
            changed = true;
        }
        let output = session.drain();
        if !output.is_empty() {
            self.emulator.feed(&output);
            self.last_generation = self.emulator.generation();
            let replies = self.emulator.take_replies();
            if !replies.is_empty() {
                self.queue_input(&replies);
            }
            changed = true;
        }
        if self.exited.is_none()
            && let Some(session) = self.session.as_mut()
            && let Ok(Some(code)) = session.poll_exit()
        {
            self.exited = Some(code);
            self.title = format!("{} [exit {code}]", self.title);
            changed = true;
        }
        changed
    }

    #[must_use]
    pub fn exit_status(&self) -> Option<u32> {
        self.exited
    }

    pub fn resize(&mut self, cols: u16, rows: u16) {
        if cols == self.cols && rows == self.rows {
            return;
        }
        self.cols = cols.max(2);
        self.rows = rows.max(1);
        if let Some(session) = &mut self.session {
            let _ = session.resize(self.cols, self.rows);
        }
        if let Some(config) = &mut self.pending {
            config.cols = self.cols;
            config.rows = self.rows;
        }
        self.emulator.resize(self.cols as usize, self.rows as usize);
    }

    #[must_use]
    pub fn snapshot(&self) -> TerminalSnapshot {
        self.emulator.snapshot()
    }

    #[must_use]
    pub fn generation(&self) -> u64 {
        self.last_generation
    }

    pub fn send_key(&mut self, key: KeyEvent) -> bool {
        if self.pending.is_some() {
            if key.code == KeyCode::Enter {
                let config = self.pending.take().unwrap();
                match PtySession::spawn(config.clone()) {
                    Ok(session) => self.session = Some(session),
                    Err(error) => {
                        self.input_error = Some(error.to_string());
                        self.pending = Some(config);
                    }
                }
            }
            return true;
        }
        if key.modifiers.contains(KeyModifiers::SHIFT)
            && matches!(key.code, KeyCode::PageUp | KeyCode::PageDown)
        {
            let lines =
                (self.rows as i32 - 1).max(1) * if key.code == KeyCode::PageUp { 1 } else { -1 };
            self.emulator.scroll_display(lines);
            return true;
        }
        self.emulator.scroll_to_bottom();
        if let Some(bytes) = negotiated_key_to_bytes(key, self.emulator.modes()) {
            self.queue_input(&bytes);
            true
        } else {
            false
        }
    }

    pub fn set_focused(&mut self, focused: bool) {
        if !self.emulator.modes().focus_reporting {
            self.reported_focus = None;
            return;
        }
        if self.reported_focus != Some(focused) {
            self.queue_input(if focused { b"\x1b[I" } else { b"\x1b[O" });
            self.reported_focus = Some(focused);
        }
    }

    pub fn send_paste(&mut self, text: &str) {
        self.emulator.scroll_to_bottom();
        let data = if self.emulator.modes().bracketed_paste {
            format!("\x1b[200~{text}\x1b[201~")
        } else {
            text.to_owned()
        };
        self.queue_input(data.as_bytes());
    }

    pub fn send_text(&mut self, text: &str) {
        self.emulator.scroll_to_bottom();
        self.queue_input(text.as_bytes());
    }

    /// Shift bypasses a child's mouse protocol so scrollback remains reachable.
    pub fn send_mouse(&mut self, area: Rect, event: MouseEvent) -> bool {
        let modes = self.emulator.modes();
        let reporting = modes.mouse_report_click || modes.mouse_drag || modes.mouse_motion;
        if matches!(
            event.kind,
            MouseEventKind::ScrollUp | MouseEventKind::ScrollDown
        ) && (!reporting || event.modifiers.contains(KeyModifiers::SHIFT))
        {
            self.emulator
                .scroll_display(if event.kind == MouseEventKind::ScrollUp {
                    3
                } else {
                    -3
                });
            return true;
        }
        if !reporting || event.modifiers.contains(KeyModifiers::SHIFT) {
            return false;
        }
        let Some(bytes) = mouse_to_bytes(event, area, modes) else {
            return false;
        };
        self.emulator.scroll_to_bottom();
        self.queue_input(&bytes);
        true
    }

    /// Applies the theme, takes pending output and fits the terminal to `area`, where the
    /// next render draws.
    pub fn layout(&mut self, area: Rect, palette: Palette) {
        self.emulator.set_ansi_palette(palette.terminal_ansi);
        let rgb = |color| match color {
            ratatui::style::Color::Rgb(r, g, b) => Some((r, g, b)),
            _ => None,
        };
        self.emulator
            .set_default_colors(rgb(palette.terminal_fg), rgb(palette.terminal_bg));
        self.poll();
        self.resize(area.width, area.height);
    }

    pub fn render(&self, frame: &mut Frame, area: Rect, palette: Palette, focused: bool) {
        if self.pending.is_some() {
            frame.render_widget(
                Paragraph::new("Press Enter to start this agent").style(
                    Style::default()
                        .fg(palette.description)
                        .bg(palette.terminal_bg),
                ),
                area,
            );
            return;
        }
        let snapshot = self.snapshot();
        let lines = snapshot_lines(&snapshot, palette);
        frame.render_widget(
            Paragraph::new(lines)
                .style(
                    Style::default()
                        .fg(palette.terminal_fg)
                        .bg(palette.terminal_bg),
                )
                .block(ratatui::widgets::Block::default()),
            area,
        );
        if self.emulator.display_offset() > 0 && area.width > 12 && area.height > 0 {
            frame.render_widget(
                Paragraph::new(format!(
                    "Scrollback +{} · type to return",
                    self.emulator.display_offset()
                ))
                .style(
                    Style::default()
                        .fg(palette.description)
                        .bg(palette.terminal_bg),
                ),
                Rect::new(area.x, area.bottom() - 1, area.width, 1),
            );
        }
        if focused && snapshot.cursor.visible {
            let x = area.x.saturating_add(saturating_u16(snapshot.cursor.x));
            let y = area.y.saturating_add(saturating_u16(snapshot.cursor.y));
            if x < area.x + area.width && y < area.y + area.height {
                frame.set_cursor_position((x, y));
            }
        }
    }

    pub fn kill(&mut self) {
        if let Some(session) = &mut self.session {
            let _ = session.kill();
        }
    }

    pub fn scrollback(&self) -> usize {
        self.scrollback
    }
}

fn mouse_to_bytes(
    event: MouseEvent,
    area: Rect,
    modes: tide_terminal::TerminalModes,
) -> Option<Vec<u8>> {
    if !area.contains((event.column, event.row).into()) {
        return None;
    }
    let button = |button| match button {
        MouseButton::Left => 0,
        MouseButton::Middle => 1,
        MouseButton::Right => 2,
    };
    let (mut code, release): (u8, bool) = match event.kind {
        MouseEventKind::Down(b) => (button(b), false),
        MouseEventKind::Up(b) => (if modes.sgr_mouse { button(b) } else { 3 }, true),
        MouseEventKind::Drag(b) if modes.mouse_drag || modes.mouse_motion => {
            (32 + button(b), false)
        }
        MouseEventKind::Moved if modes.mouse_motion => (35, false),
        MouseEventKind::ScrollUp => (64, false),
        MouseEventKind::ScrollDown => (65, false),
        MouseEventKind::ScrollLeft => (66, false),
        MouseEventKind::ScrollRight => (67, false),
        _ => return None,
    };
    code += if event.modifiers.contains(KeyModifiers::SHIFT) {
        4
    } else {
        0
    };
    code += if event.modifiers.contains(KeyModifiers::ALT) {
        8
    } else {
        0
    };
    code += if event.modifiers.contains(KeyModifiers::CONTROL) {
        16
    } else {
        0
    };
    let x = event.column - area.x + 1;
    let y = event.row - area.y + 1;
    if modes.sgr_mouse {
        Some(format!("\x1b[<{code};{x};{y}{}", if release { 'm' } else { 'M' }).into_bytes())
    } else if x <= 223 && y <= 223 {
        // X10 encoding: each value is offset by 32 and must fit one byte.
        Some(vec![
            27,
            b'[',
            b'M',
            32 + code,
            32 + u8::try_from(x).ok()?,
            32 + u8::try_from(y).ok()?,
        ])
    } else {
        None
    }
}

fn default_shell_args(shell: &str) -> Vec<String> {
    let name = Path::new(shell)
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or_default();
    if matches!(name, "bash" | "zsh" | "sh") {
        vec!["-il".to_owned()]
    } else {
        Vec::new()
    }
}

fn snapshot_lines(snapshot: &TerminalSnapshot, palette: Palette) -> Vec<Line<'static>> {
    let mut lines = Vec::with_capacity(snapshot.rows);
    for y in 0..snapshot.rows {
        let mut spans: Vec<Span<'static>> = Vec::new();
        let mut current = String::new();
        let mut current_style = cell_style(snapshot.cell(0, y), palette);
        for x in 0..snapshot.cols {
            let cell = snapshot.cell(x, y);
            if cell.is_some_and(|cell| cell.spacer) {
                continue;
            }
            let style = cell_style(cell, palette);
            let symbol = cell.map_or(" ", |cell| cell.symbol.as_str());
            if style != current_style {
                if !current.is_empty() {
                    spans.push(Span::styled(std::mem::take(&mut current), current_style));
                }
                current_style = style;
            }
            current.push_str(symbol);
        }
        if !current.is_empty() {
            spans.push(Span::styled(current, current_style));
        }
        lines.push(Line::from(spans));
    }
    lines
}

fn cell_style(cell: Option<&CellSnapshot>, palette: Palette) -> Style {
    let Some(cell) = cell else {
        return Style::default()
            .fg(palette.terminal_fg)
            .bg(palette.terminal_bg);
    };
    let mut fg = color(cell.fg, palette.terminal_fg);
    let mut bg = color(cell.bg, palette.terminal_bg);
    if cell.inverse {
        std::mem::swap(&mut fg, &mut bg);
    }
    let mut style = Style::default().fg(fg).bg(bg);
    if cell.bold {
        style = style.add_modifier(Modifier::BOLD);
    }
    if cell.italic {
        style = style.add_modifier(Modifier::ITALIC);
    }
    if cell.underline {
        style = style.add_modifier(Modifier::UNDERLINED);
    }
    if cell.dim {
        style = style.add_modifier(Modifier::DIM);
    }
    style
}

fn color(color: TerminalColor, fallback: ratatui::style::Color) -> ratatui::style::Color {
    match color {
        TerminalColor::Default => fallback,
        TerminalColor::Rgb(r, g, b) => ratatui::style::Color::Rgb(r, g, b),
    }
}

fn negotiated_key_to_bytes(
    mut key: KeyEvent,
    modes: tide_terminal::TerminalModes,
) -> Option<Vec<u8>> {
    if key.code == KeyCode::BackTab {
        key.modifiers |= KeyModifiers::SHIFT;
    }
    let modifiers = 1
        + u16::from(key.modifiers.contains(KeyModifiers::SHIFT))
        + 2 * u16::from(key.modifiers.contains(KeyModifiers::ALT))
        + 4 * u16::from(key.modifiers.contains(KeyModifiers::CONTROL))
        + 8 * u16::from(key.modifiers.contains(KeyModifiers::SUPER))
        + 32 * u16::from(key.modifiers.contains(KeyModifiers::META));
    let all = modes.kitty_flags & 8 != 0;
    let disambiguate = modes.kitty_flags & 1 != 0;
    let code = match key.code {
        KeyCode::Char(ch) => Some(
            if ch.is_ascii() && key.modifiers.contains(KeyModifiers::SHIFT) {
                ch.to_ascii_lowercase() as u32
            } else {
                ch as u32
            },
        ),
        KeyCode::Enter => Some(13),
        KeyCode::Tab | KeyCode::BackTab => Some(9),
        KeyCode::Esc => Some(27),
        KeyCode::Backspace => Some(127),
        KeyCode::F(number) if number >= 13 => Some(57376 + (number - 13) as u32),
        _ => None,
    };
    if let Some(code) = code {
        let nontext = matches!(key.code, KeyCode::Esc)
            || modifiers > 1 && !matches!(key.code, KeyCode::Char(_));
        let modified_text = matches!(key.code, KeyCode::Char(_))
            && key.modifiers.intersects(
                KeyModifiers::CONTROL
                    | KeyModifiers::ALT
                    | KeyModifiers::SUPER
                    | KeyModifiers::META,
            );
        let modify = modes.modify_other_keys > 0
            && modifiers > 1
            && (nontext || modes.modify_other_keys == 2);
        if all || disambiguate && (nontext || modified_text) || modify {
            return Some(format!("\x1b[{code};{modifiers}u").into_bytes());
        }
    }
    if modifiers > 1 || all || disambiguate {
        let final_char = match key.code {
            KeyCode::Up => Some('A'),
            KeyCode::Down => Some('B'),
            KeyCode::Right => Some('C'),
            KeyCode::Left => Some('D'),
            KeyCode::Home => Some('H'),
            KeyCode::End => Some('F'),
            KeyCode::F(1) => Some('P'),
            KeyCode::F(2) => Some('Q'),
            KeyCode::F(3) => Some('R'),
            KeyCode::F(4) => Some('S'),
            _ => None,
        };
        if let Some(final_char) = final_char {
            return Some(format!("\x1b[1;{modifiers}{final_char}").into_bytes());
        }
        let number = match key.code {
            KeyCode::Insert => Some(2),
            KeyCode::Delete => Some(3),
            KeyCode::PageUp => Some(5),
            KeyCode::PageDown => Some(6),
            KeyCode::F(number @ 5..=12) => {
                Some([15, 17, 18, 19, 20, 21, 23, 24][number as usize - 5])
            }
            _ => None,
        };
        if let Some(number) = number {
            return Some(format!("\x1b[{number};{modifiers}~").into_bytes());
        }
    }
    key_to_bytes(key, modes.application_cursor_keys)
}

pub fn key_to_bytes(mut key: KeyEvent, app_cursor: bool) -> Option<Vec<u8>> {
    let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
    let alt = key.modifiers.contains(KeyModifiers::ALT);
    let shift = key.modifiers.contains(KeyModifiers::SHIFT);

    if alt {
        key.modifiers.remove(KeyModifiers::ALT);
        let mut bytes = vec![27];
        bytes.extend(key_to_bytes(key, app_cursor)?);
        return Some(bytes);
    }

    match key.code {
        KeyCode::Char(ch) => {
            if ctrl {
                let lower = ch.to_ascii_lowercase();
                if lower.is_ascii_lowercase() {
                    return Some(vec![(lower as u8) - b'a' + 1]);
                }
                match ch {
                    '@' | ' ' => return Some(vec![0]),
                    '[' => return Some(vec![0x1b]),
                    '\\' => return Some(vec![0x1c]),
                    ']' => return Some(vec![0x1d]),
                    '^' => return Some(vec![0x1e]),
                    '_' => return Some(vec![0x1f]),
                    _ => {}
                }
            }
            let mut text = ch.to_string();
            if shift && ch.is_ascii_lowercase() {
                text = ch.to_ascii_uppercase().to_string();
            }
            Some(text.into_bytes())
        }
        KeyCode::Enter => Some(b"\r".to_vec()),
        KeyCode::Tab if shift => Some(b"\x1b[Z".to_vec()),
        KeyCode::Tab => Some(b"\t".to_vec()),
        KeyCode::BackTab => Some(b"\x1b[Z".to_vec()),
        KeyCode::Backspace => Some(vec![0x7f]),
        KeyCode::Esc => Some(vec![0x1b]),
        KeyCode::Delete => Some(b"\x1b[3~".to_vec()),
        KeyCode::Insert => Some(b"\x1b[2~".to_vec()),
        KeyCode::Home => Some(if app_cursor {
            b"\x1bOH".to_vec()
        } else {
            b"\x1b[H".to_vec()
        }),
        KeyCode::End => Some(if app_cursor {
            b"\x1bOF".to_vec()
        } else {
            b"\x1b[F".to_vec()
        }),
        KeyCode::PageUp => Some(b"\x1b[5~".to_vec()),
        KeyCode::PageDown => Some(b"\x1b[6~".to_vec()),
        KeyCode::Up => Some(if app_cursor {
            b"\x1bOA".to_vec()
        } else {
            b"\x1b[A".to_vec()
        }),
        KeyCode::Down => Some(if app_cursor {
            b"\x1bOB".to_vec()
        } else {
            b"\x1b[B".to_vec()
        }),
        KeyCode::Right => Some(if app_cursor {
            b"\x1bOC".to_vec()
        } else {
            b"\x1b[C".to_vec()
        }),
        KeyCode::Left => Some(if app_cursor {
            b"\x1bOD".to_vec()
        } else {
            b"\x1b[D".to_vec()
        }),
        KeyCode::F(number) => function_key_bytes(number),
        _ => None,
    }
}

fn function_key_bytes(number: u8) -> Option<Vec<u8>> {
    let sequence = match number {
        1 => "\x1bOP",
        2 => "\x1bOQ",
        3 => "\x1bOR",
        4 => "\x1bOS",
        5 => "\x1b[15~",
        6 => "\x1b[17~",
        7 => "\x1b[18~",
        8 => "\x1b[19~",
        9 => "\x1b[20~",
        10 => "\x1b[21~",
        11 => "\x1b[23~",
        12 => "\x1b[24~",
        _ => return None,
    };
    Some(sequence.as_bytes().to_vec())
}

#[cfg(test)]
mod tests {
    #[test]
    fn disabled_auto_start_waits_for_enter_without_spawning() {
        let root = tempfile::tempdir().unwrap();
        let marker = root.path().join("started");
        let spec = super::SpawnSpec::new(
            "/bin/sh",
            vec![
                "-c".into(),
                "printf '' > \"$1\"; sleep 0.05; printf started > \"$1\"".into(),
                "fixture".into(),
                marker.display().to_string(),
            ],
        );
        let mut terminal =
            super::TerminalTab::spawn_configured(1, root.path(), spec, false).unwrap();
        assert!(!marker.exists());
        assert!(terminal.session.is_none());
        assert!(terminal.pending.as_ref().unwrap().on_output.is_some());
        terminal.send_key(crossterm::event::KeyEvent::new(
            crossterm::event::KeyCode::Char('x'),
            crossterm::event::KeyModifiers::NONE,
        ));
        assert!(!marker.exists());
        terminal.send_key(crossterm::event::KeyEvent::new(
            crossterm::event::KeyCode::Enter,
            crossterm::event::KeyModifiers::NONE,
        ));
        for _ in 0..100 {
            terminal.poll();
            // Shell redirection creates the file before printf completes.
            if std::fs::read_to_string(&marker).is_ok_and(|text| text == "started") {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        assert_eq!(std::fs::read_to_string(marker).unwrap(), "started");
    }

    use super::*;

    #[test]
    fn child_keyboard_modes_preserve_shift_enter_ctrl_arrows_and_alt_control() {
        let legacy = tide_terminal::TerminalModes::default();
        assert_eq!(
            negotiated_key_to_bytes(KeyEvent::new(KeyCode::Left, KeyModifiers::CONTROL), legacy)
                .unwrap(),
            b"\x1b[1;5D"
        );
        assert_eq!(
            negotiated_key_to_bytes(
                KeyEvent::new(
                    KeyCode::Char('a'),
                    KeyModifiers::ALT | KeyModifiers::CONTROL
                ),
                legacy
            )
            .unwrap(),
            vec![27, 1]
        );
        let kitty = tide_terminal::TerminalModes {
            kitty_flags: 1,
            ..legacy
        };
        assert_eq!(
            negotiated_key_to_bytes(KeyEvent::new(KeyCode::Enter, KeyModifiers::SHIFT), kitty)
                .unwrap(),
            b"\x1b[13;2u"
        );
        assert_eq!(
            negotiated_key_to_bytes(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE), kitty)
                .unwrap(),
            b"\x1b[27;1u"
        );
        assert_eq!(
            negotiated_key_to_bytes(KeyEvent::new(KeyCode::Char('a'), KeyModifiers::NONE), kitty)
                .unwrap(),
            b"a"
        );
        assert_eq!(
            negotiated_key_to_bytes(
                KeyEvent::new(KeyCode::Char('a'), KeyModifiers::CONTROL),
                kitty
            )
            .unwrap(),
            b"\x1b[97;5u"
        );
        let all = tide_terminal::TerminalModes {
            kitty_flags: 9,
            ..legacy
        };
        assert_eq!(
            negotiated_key_to_bytes(KeyEvent::new(KeyCode::Char('a'), KeyModifiers::NONE), all)
                .unwrap(),
            b"\x1b[97;1u"
        );
        let modify = tide_terminal::TerminalModes {
            modify_other_keys: 2,
            ..legacy
        };
        assert_eq!(
            negotiated_key_to_bytes(KeyEvent::new(KeyCode::Enter, KeyModifiers::SHIFT), modify)
                .unwrap(),
            b"\x1b[13;2u"
        );
    }

    #[test]
    fn mouse_protocol_encodes_buttons_modifiers_motion_and_releases() {
        let area = Rect::new(10, 20, 80, 24);
        let modes = tide_terminal::TerminalModes {
            sgr_mouse: true,
            mouse_report_click: true,
            mouse_drag: true,
            ..Default::default()
        };
        let mouse = MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: 12,
            row: 23,
            modifiers: KeyModifiers::NONE,
        };
        assert_eq!(mouse_to_bytes(mouse, area, modes).unwrap(), b"\x1b[<0;3;4M");
        assert_eq!(
            mouse_to_bytes(
                MouseEvent {
                    kind: MouseEventKind::Up(MouseButton::Left),
                    ..mouse
                },
                area,
                modes
            )
            .unwrap(),
            b"\x1b[<0;3;4m"
        );
        assert_eq!(
            mouse_to_bytes(
                MouseEvent {
                    kind: MouseEventKind::Drag(MouseButton::Right),
                    modifiers: KeyModifiers::CONTROL | KeyModifiers::ALT,
                    ..mouse
                },
                area,
                modes
            )
            .unwrap(),
            b"\x1b[<58;3;4M"
        );
        assert!(
            mouse_to_bytes(
                MouseEvent {
                    kind: MouseEventKind::Moved,
                    ..mouse
                },
                area,
                modes
            )
            .is_none()
        );
        assert_eq!(
            mouse_to_bytes(
                MouseEvent {
                    kind: MouseEventKind::ScrollDown,
                    ..mouse
                },
                area,
                modes
            )
            .unwrap(),
            b"\x1b[<65;3;4M"
        );
        assert_eq!(
            mouse_to_bytes(
                mouse,
                area,
                tide_terminal::TerminalModes {
                    sgr_mouse: false,
                    ..modes
                }
            )
            .unwrap(),
            vec![27, b'[', b'M', 32, 35, 36]
        );
    }

    #[test]
    fn terminal_replies_mouse_reporting_and_scrollback_use_the_live_pty() {
        let dir = tempfile::tempdir().unwrap();
        let script = dir.path().join("protocol.py");
        std::fs::write(&script,"import os,tty\ntty.setraw(0)\nos.write(1,b'\\x1b[?1000h\\x1b[?1002h\\x1b[?1006h\\x1b[6nREADY\\r\\n')\nfor i in range(2):\n data=os.read(0,1024)\n os.write(1,b'RX'+data.hex().encode()+b'\\r\\n')\n").unwrap();
        let spec = SpawnSpec::new("/usr/bin/python3", vec![script.display().to_string()]);
        let mut terminal = TerminalTab::spawn(1, dir.path(), spec).unwrap();
        let wait = |terminal: &mut TerminalTab, needle: &str| {
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
            while std::time::Instant::now() < deadline {
                terminal.poll();
                if terminal.snapshot().text().contains(needle) {
                    return;
                }
                std::thread::sleep(std::time::Duration::from_millis(5));
            }
            panic!("missing {needle}: {}", terminal.snapshot().text());
        };
        wait(&mut terminal, "RX1b5b");
        assert!(terminal.send_mouse(
            Rect::new(10, 20, 80, 24),
            MouseEvent {
                kind: MouseEventKind::Down(MouseButton::Left),
                column: 12,
                row: 23,
                modifiers: KeyModifiers::NONE
            }
        ));
        wait(&mut terminal, "1b5b3c303b333b344d");
        terminal.emulator.feed("\x1b[?1000l\x1b[?1002l".as_bytes());
        for line in 0..80 {
            terminal
                .emulator
                .feed(format!("line {line}\r\n").as_bytes());
        }
        terminal.send_key(KeyEvent::new(KeyCode::PageUp, KeyModifiers::SHIFT));
        assert!(terminal.emulator.display_offset() > 0);
        assert!(!terminal.snapshot().cursor.visible);
        terminal.send_key(KeyEvent::new(KeyCode::Char('x'), KeyModifiers::NONE));
        assert_eq!(terminal.emulator.display_offset(), 0);
        let mut emulator = TerminalEmulator::new(10, 2, 0);
        emulator.feed("👩‍💻x".as_bytes());
        let lines = snapshot_lines(&emulator.snapshot(), Palette::ultra_dusk());
        assert_eq!(lines[0].width(), 10, "wide spacers were rendered twice");
    }

    #[test]
    fn maps_basic_terminal_keys() {
        assert_eq!(
            key_to_bytes(KeyEvent::new(KeyCode::Char('a'), KeyModifiers::NONE), false).unwrap(),
            b"a"
        );
        assert_eq!(
            key_to_bytes(
                KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL),
                false
            )
            .unwrap(),
            vec![3]
        );
        assert_eq!(
            key_to_bytes(KeyEvent::new(KeyCode::Up, KeyModifiers::NONE), true).unwrap(),
            b"\x1bOA"
        );
    }

    #[test]
    fn reports_child_exit_status() {
        let mut tab = TerminalTab::spawn(
            0,
            std::env::temp_dir(),
            SpawnSpec::new(
                "/bin/sh".to_owned(),
                vec!["-c".to_owned(), "exit 3".to_owned()],
            ),
        )
        .unwrap();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
        while std::time::Instant::now() < deadline && tab.exit_status().is_none() {
            let _ = tab.poll();
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        assert_eq!(tab.exit_status(), Some(3));
        assert!(tab.title.contains("[exit 3]"));
    }
}
