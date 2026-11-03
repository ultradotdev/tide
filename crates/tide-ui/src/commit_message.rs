//! The commit message box: a summary line and an optional body.
//!
//! Enter starts a new line, so a message can have a body. Ctrl+S commits, as does Alt+Enter
//! (or Ctrl+Enter in terminals that report it); Tab reaches the Commit and Cancel buttons.
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Clear, Paragraph};
use tide_core::num::saturating_u16;
use tide_core::{display_width, grapheme_boundaries, truncate_to_width};

use crate::modal::{Modal, ModalBase, ModalKind, ModalOutcome, PromptAction, resolve};
use crate::palette::{ActionRole, Palette};

/// Git's conventional summary length; longer summaries are flagged, never refused.
const SUMMARY_LIMIT: usize = 72;
const HINT: &str = "Enter new line · Ctrl+S commit · Tab buttons · Esc cancel";

pub struct CommitMessageModal {
    base: ModalBase,
    /// The first message line shown, kept so the cursor line stays visible.
    scroll: std::cell::Cell<usize>,
    /// The display column Up and Down aim for, kept across short lines.
    goal: Option<usize>,
}

/// The parts of the box, shared by rendering and clicking.
struct Areas {
    text: Rect,
    actions: Vec<(Rect, usize, &'static str)>,
    footer: Rect,
}

impl CommitMessageModal {
    #[must_use]
    pub fn new(action: PromptAction, title: &str, initial: &str) -> Self {
        let mut base = ModalBase::prompt(ModalKind::Prompt(action), title, "");
        base.push_str(&normalize(initial));
        Self {
            base,
            scroll: std::cell::Cell::new(0),
            goal: None,
        }
    }

    fn areas(&self, area: Rect) -> Areas {
        let inner = Block::bordered().inner(self.base.geometry(area));
        let footer_rows = u16::from(inner.height > 4);
        let text_height = inner.height.saturating_sub(1 + footer_rows).max(1);
        let text = Rect::new(inner.x, inner.y, inner.width, text_height.min(inner.height));
        let row = inner.y + text.height;
        let labels = [" Commit [Ctrl+S] ", " Cancel [Esc] "];
        let total = labels
            .iter()
            .map(|label| display_width(label))
            .sum::<usize>()
            + 1;
        let mut x = inner.x + inner.width.saturating_sub(saturating_u16(total)) / 2;
        let actions = if row < inner.bottom() {
            labels
                .into_iter()
                .enumerate()
                .map(|(index, label)| {
                    let width =
                        saturating_u16(display_width(label)).min(inner.right().saturating_sub(x));
                    let rect = Rect::new(x, row, width, 1);
                    x = x.saturating_add(width + 1);
                    (rect, index, label)
                })
                .collect()
        } else {
            Vec::new()
        };
        let footer = if footer_rows == 1 {
            Rect::new(inner.x, inner.bottom() - 1, inner.width, 1)
        } else {
            Rect::default()
        };
        Areas {
            text,
            actions,
            footer,
        }
    }

    /// The line the cursor is on and the cursor's character offset within it.
    fn cursor_line(&self) -> (usize, usize) {
        let before = self.base.input.chars().take(self.base.cursor);
        let mut line = 0;
        let mut column = 0;
        for ch in before {
            if ch == '\n' {
                line += 1;
                column = 0;
            } else {
                column += 1;
            }
        }
        (line, column)
    }

    /// The character index where line `line` starts.
    fn line_start(&self, line: usize) -> usize {
        if line == 0 {
            return 0;
        }
        let mut seen = 0;
        for (index, ch) in self.base.input.chars().enumerate() {
            if ch == '\n' {
                seen += 1;
                if seen == line {
                    return index + 1;
                }
            }
        }
        self.base.input.chars().count()
    }

    fn line_count(&self) -> usize {
        self.base.input.matches('\n').count() + 1
    }

    /// Moves the cursor to `line`, as near the display column `column` as the line allows.
    fn move_to(&mut self, line: usize, column: usize) {
        let line = line.min(self.line_count() - 1);
        let text = self.base.input.split('\n').nth(line).unwrap_or_default();
        let mut offset = 0;
        for boundary in grapheme_boundaries(text) {
            let prefix: String = text.chars().take(boundary).collect();
            if display_width(&prefix) > column {
                break;
            }
            offset = boundary;
        }
        self.base.cursor = self.line_start(line) + offset;
    }

    fn cursor_column(&self) -> usize {
        let (line, column) = self.cursor_line();
        let text = self.base.input.split('\n').nth(line).unwrap_or_default();
        display_width(&text.chars().take(column).collect::<String>())
    }

    fn move_vertically(&mut self, delta: isize) {
        let (line, _) = self.cursor_line();
        let column = self.goal.unwrap_or_else(|| self.cursor_column());
        self.goal = Some(column);
        match line.checked_add_signed(delta) {
            Some(target) if target < self.line_count() => self.move_to(target, column),
            // Up on the first line or Down on the last goes to its start or end.
            _ if delta < 0 => self.base.cursor = 0,
            _ => self.base.cursor = self.base.input.chars().count(),
        }
    }

    fn text_key(&mut self, key: KeyEvent) {
        let plain = !key.modifiers.intersects(
            KeyModifiers::CONTROL | KeyModifiers::ALT | KeyModifiers::SUPER | KeyModifiers::META,
        );
        let (line, _) = self.cursor_line();
        if !matches!(
            key.code,
            KeyCode::Up | KeyCode::Down | KeyCode::PageUp | KeyCode::PageDown
        ) {
            self.goal = None;
        }
        match key.code {
            KeyCode::Enter => self.base.push_char('\n'),
            KeyCode::Left => self.base.move_left(),
            KeyCode::Right => self.base.move_right(),
            KeyCode::Up => self.move_vertically(-1),
            KeyCode::Down => self.move_vertically(1),
            KeyCode::PageUp => self.move_vertically(-10),
            KeyCode::PageDown => self.move_vertically(10),
            KeyCode::Home if key.modifiers.contains(KeyModifiers::CONTROL) => self.base.cursor = 0,
            KeyCode::End if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.base.cursor = self.base.input.chars().count();
            }
            KeyCode::Home => self.base.cursor = self.line_start(line),
            KeyCode::End => {
                self.base.cursor = if line + 1 < self.line_count() {
                    self.line_start(line + 1) - 1
                } else {
                    self.base.input.chars().count()
                };
            }
            KeyCode::Backspace => self.base.backspace(),
            KeyCode::Delete => self.base.delete(),
            KeyCode::Char(ch) if plain => self.base.push_char(ch),
            _ => {}
        }
    }

    /// Scrolls so the cursor line is inside a text area `height` rows tall.
    fn visible_from(&self, height: usize) -> usize {
        let (line, _) = self.cursor_line();
        let mut scroll = self.scroll.get();
        if line < scroll {
            scroll = line;
        } else if line >= scroll + height {
            scroll = line + 1 - height;
        }
        self.scroll.set(scroll);
        scroll
    }
}

/// Line endings become `\n` and tabs become spaces, so every character takes a cell.
fn normalize(text: &str) -> String {
    text.replace("\r\n", "\n")
        .replace('\r', "\n")
        .replace('\t', "    ")
        .chars()
        .filter(|ch| *ch == '\n' || !ch.is_control())
        .collect()
}

/// The part of `line` from display column `start`, at most `width` columns wide.
fn columns(line: &str, start: usize, width: usize) -> String {
    let chars = line.chars().collect::<Vec<_>>();
    let skip = grapheme_boundaries(line)
        .into_iter()
        .find(|boundary| display_width(&chars[..*boundary].iter().collect::<String>()) >= start)
        .unwrap_or(chars.len());
    let rest = chars[skip..].iter().collect::<String>();
    truncate_to_width(&rest, width).to_owned()
}

impl Modal for CommitMessageModal {
    fn base(&self) -> &ModalBase {
        &self.base
    }

    fn base_mut(&mut self) -> &mut ModalBase {
        &mut self.base
    }

    fn handle_key(&mut self, key: KeyEvent) -> ModalOutcome {
        let commit = (key.code == KeyCode::Char('s') && key.modifiers == KeyModifiers::CONTROL)
            || (key.code == KeyCode::Enter
                && key
                    .modifiers
                    .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT | KeyModifiers::SUPER));
        let outcome = if key.code == KeyCode::Esc {
            crate::modal::KeyOutcome::Close
        } else if commit {
            crate::modal::KeyOutcome::Submit
        } else if matches!(key.code, KeyCode::Tab | KeyCode::BackTab) {
            // Text, Commit, Cancel.
            let zone = if self.base.actions_focused {
                self.base.action_index + 1
            } else {
                0
            };
            let next = if key.code == KeyCode::BackTab {
                (zone + 2) % 3
            } else {
                (zone + 1) % 3
            };
            self.base.actions_focused = next != 0;
            self.base.action_index = next.saturating_sub(1);
            crate::modal::KeyOutcome::None
        } else if self.base.actions_focused {
            match key.code {
                KeyCode::Left | KeyCode::Right | KeyCode::Up | KeyCode::Down => {
                    self.base.action_index = 1 - self.base.action_index.min(1);
                    crate::modal::KeyOutcome::None
                }
                KeyCode::Enter | KeyCode::Char(' ') if self.base.action_index == 0 => {
                    crate::modal::KeyOutcome::Submit
                }
                KeyCode::Enter | KeyCode::Char(' ') => crate::modal::KeyOutcome::Close,
                _ => crate::modal::KeyOutcome::None,
            }
        } else {
            self.text_key(key);
            crate::modal::KeyOutcome::None
        };
        resolve(self, outcome)
    }

    fn paste(&mut self, text: &str) {
        self.base.actions_focused = false;
        self.goal = None;
        self.base.push_str(&normalize(text));
    }

    fn handle_mouse(&mut self, _area: Rect, _x: u16, _y: u16) -> bool {
        false
    }

    fn click_key(&mut self, area: Rect, x: u16, y: u16) -> Option<KeyEvent> {
        let areas = self.areas(area);
        for (rect, index, _) in &areas.actions {
            if rect.contains((x, y).into()) {
                self.base.actions_focused = true;
                self.base.action_index = *index;
                return Some(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
            }
        }
        if areas.text.contains((x, y).into()) {
            self.base.actions_focused = false;
            self.goal = None;
            let scroll = self.visible_from(areas.text.height as usize);
            let column = self.cursor_column();
            let offset = column.saturating_sub(areas.text.width.saturating_sub(1) as usize);
            self.move_to(
                scroll + (y - areas.text.y) as usize,
                offset + (x - areas.text.x) as usize,
            );
        }
        None
    }

    fn render(&self, frame: &mut Frame, area: Rect, palette: Palette) {
        let popup = self.base.geometry(area);
        frame.render_widget(Clear, popup);
        let block = Block::bordered()
            .title(format!(" {} ", self.base.title))
            .border_style(Style::default().fg(palette.modal_border))
            .style(Style::default().bg(palette.modal_bg));
        frame.render_widget(block, popup);
        let areas = self.areas(area);
        let text = areas.text;
        let input = Style::default().fg(palette.input_fg).bg(palette.input_bg);
        let height = text.height as usize;
        let width = text.width as usize;
        let scroll = self.visible_from(height);
        let column = self.cursor_column();
        // Long lines scroll sideways together so the cursor column stays in view.
        let offset = column.saturating_sub(width.saturating_sub(1));
        let lines = if self.base.input.is_empty() {
            vec![Line::from(Span::styled(
                "Summary, then a blank line and the details",
                Style::default()
                    .fg(palette.description)
                    .bg(palette.input_bg),
            ))]
        } else {
            self.base
                .input
                .split('\n')
                .enumerate()
                .skip(scroll)
                .take(height)
                .map(|(index, line)| {
                    let visible = columns(line, offset, width);
                    if index == 0 && display_width(line) > SUMMARY_LIMIT {
                        // Mark the part of the summary past the conventional length.
                        let keep = SUMMARY_LIMIT.saturating_sub(offset);
                        let head = truncate_to_width(&visible, keep).to_owned();
                        let tail = visible[head.len()..].to_owned();
                        Line::from(vec![
                            Span::styled(head, input),
                            Span::styled(tail, input.fg(palette.warning)),
                        ])
                    } else {
                        Line::from(Span::styled(visible, input))
                    }
                })
                .collect()
        };
        frame.render_widget(Paragraph::new(lines).style(input), text);
        if !self.base.actions_focused {
            let (line, _) = self.cursor_line();
            frame.set_cursor_position((
                text.x
                    .saturating_add(saturating_u16(column - offset))
                    .min(text.right().saturating_sub(1)),
                text.y + saturating_u16(line - scroll),
            ));
        }
        for (rect, index, label) in &areas.actions {
            let role = if *index == 0 {
                ActionRole::Primary
            } else {
                ActionRole::Neutral
            };
            let focused = self.base.actions_focused && self.base.action_index == *index;
            frame.render_widget(
                Paragraph::new(truncate_to_width(label, rect.width as usize))
                    .style(palette.action(role, focused, true)),
                *rect,
            );
        }
        if !areas.footer.is_empty() {
            let summary = self.base.input.split('\n').next().unwrap_or_default();
            let count = format!("{}/{SUMMARY_LIMIT}", display_width(summary));
            let hint = truncate_to_width(
                HINT,
                (areas.footer.width as usize).saturating_sub(count.len() + 1),
            );
            let gap =
                (areas.footer.width as usize).saturating_sub(display_width(hint) + count.len());
            frame.render_widget(
                Paragraph::new(Line::from(vec![
                    Span::raw(hint.to_owned()),
                    Span::raw(" ".repeat(gap)),
                    Span::raw(count),
                ]))
                .style(
                    Style::default()
                        .fg(palette.description)
                        .bg(palette.modal_bg),
                ),
                areas.footer,
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(code: KeyCode, modifiers: KeyModifiers) -> KeyEvent {
        KeyEvent::new(code, modifiers)
    }

    fn typed(modal: &mut CommitMessageModal, text: &str) {
        for ch in text.chars() {
            let code = if ch == '\n' {
                KeyCode::Enter
            } else {
                KeyCode::Char(ch)
            };
            assert!(matches!(
                modal.handle_key(key(code, KeyModifiers::NONE)),
                ModalOutcome::None
            ));
        }
    }

    #[test]
    fn enter_starts_a_new_line_and_ctrl_s_or_alt_enter_commits() {
        for commit in [
            key(KeyCode::Char('s'), KeyModifiers::CONTROL),
            key(KeyCode::Enter, KeyModifiers::ALT),
            key(KeyCode::Enter, KeyModifiers::CONTROL),
        ] {
            let mut modal = CommitMessageModal::new(PromptAction::Commit, "Commit", "");
            typed(&mut modal, "Fix the parser\n\nIt dropped the last token.");
            assert_eq!(
                modal.base.input,
                "Fix the parser\n\nIt dropped the last token."
            );
            match modal.handle_key(commit) {
                ModalOutcome::Submit(crate::modal::Submitted::Prompt(
                    PromptAction::Commit,
                    message,
                )) => assert_eq!(message, "Fix the parser\n\nIt dropped the last token."),
                other => panic!("{commit:?} gave {other:?}"),
            }
        }
    }

    #[test]
    fn tab_reaches_the_buttons_and_esc_cancels() {
        let mut modal = CommitMessageModal::new(PromptAction::Commit, "Commit", "summary");
        modal.handle_key(key(KeyCode::Tab, KeyModifiers::NONE));
        assert!(matches!(
            modal.handle_key(key(KeyCode::Enter, KeyModifiers::NONE)),
            ModalOutcome::Submit(_)
        ));
        modal.handle_key(key(KeyCode::Tab, KeyModifiers::NONE));
        assert!(matches!(
            modal.handle_key(key(KeyCode::Enter, KeyModifiers::NONE)),
            ModalOutcome::Close
        ));
        modal.handle_key(key(KeyCode::Tab, KeyModifiers::NONE));
        assert!(!modal.base.actions_focused);
        assert!(matches!(
            modal.handle_key(key(KeyCode::Esc, KeyModifiers::NONE)),
            ModalOutcome::Close
        ));
    }

    #[test]
    fn arrows_move_between_lines_keeping_the_column() {
        let mut modal =
            CommitMessageModal::new(PromptAction::Commit, "Commit", "summary\n\nbody line");
        assert_eq!(modal.cursor_line(), (2, 9));
        modal.handle_key(key(KeyCode::Up, KeyModifiers::NONE));
        assert_eq!(modal.cursor_line(), (1, 0));
        modal.handle_key(key(KeyCode::Up, KeyModifiers::NONE));
        assert_eq!(
            modal.cursor_line(),
            (0, 7),
            "as near column 9 as the line goes"
        );
        modal.handle_key(key(KeyCode::Home, KeyModifiers::NONE));
        modal.handle_key(key(KeyCode::End, KeyModifiers::NONE));
        assert_eq!(modal.cursor_line(), (0, 7));
        modal.handle_key(key(KeyCode::Down, KeyModifiers::NONE));
        modal.handle_key(key(KeyCode::Down, KeyModifiers::NONE));
        assert_eq!(modal.cursor_line(), (2, 7));
        modal.handle_key(key(KeyCode::Home, KeyModifiers::NONE));
        typed(&mut modal, "the ");
        assert_eq!(modal.base.input, "summary\n\nthe body line");
        modal.handle_key(key(KeyCode::Down, KeyModifiers::NONE));
        assert_eq!(modal.base.cursor, modal.base.input.chars().count());
    }

    #[test]
    fn pasted_text_keeps_its_lines_without_carriage_returns() {
        let mut modal = CommitMessageModal::new(PromptAction::Commit, "Commit", "");
        modal.paste("one\r\ntwo\tthree\u{7}");
        assert_eq!(modal.base.input, "one\ntwo    three");
    }

    #[test]
    fn renders_every_line_and_clicks_place_the_cursor() {
        use ratatui::{Terminal, backend::TestBackend};
        let mut modal = CommitMessageModal::new(
            PromptAction::Commit,
            "Commit",
            "summary\n\nfirst body line\nsecond body line",
        );
        let area = Rect::new(0, 0, 100, 30);
        let mut terminal = Terminal::new(TestBackend::new(100, 30)).unwrap();
        terminal
            .draw(|frame| modal.render(frame, area, Palette::ultra_dusk()))
            .unwrap();
        let screen = terminal
            .backend()
            .buffer()
            .content()
            .chunks(100)
            .map(|row| row.iter().map(|cell| cell.symbol()).collect::<String>())
            .collect::<Vec<_>>();
        let text = screen.join("\n");
        for expected in [
            "summary",
            "first body line",
            "second body line",
            "Commit [Ctrl+S]",
            "Enter new line",
        ] {
            assert!(text.contains(expected), "{expected} missing:\n{text}");
        }
        let row = screen
            .iter()
            .position(|row| row.contains("first body line"))
            .unwrap();
        let column = |row: &str, text: &str| row[..row.find(text).unwrap()].chars().count();
        let first = column(&screen[row], "first");
        modal.click_key(area, saturating_u16(first + 6), saturating_u16(row));
        assert_eq!(modal.cursor_line(), (2, 6));
        let row = screen
            .iter()
            .position(|row| row.contains("Commit [Ctrl+S]"))
            .unwrap();
        let button = column(&screen[row], "Commit [Ctrl+S]");
        let click = modal.click_key(area, saturating_u16(button + 2), saturating_u16(row));
        assert_eq!(
            click,
            Some(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE))
        );
        assert!(matches!(
            modal.handle_key(click.unwrap()),
            ModalOutcome::Submit(_)
        ));
    }
}
