//! Preserve extended grapheme cell widths while delegating VT behavior to Alacritty.
//! The pinned Alacritty engine measures scalar values independently. This adapter
//! joins Unicode graphemes in the existing cell, including split UTF-8 input.
use alacritty_terminal::event::{Event, EventListener};
use alacritty_terminal::grid::Dimensions;
use alacritty_terminal::index::{Column, Point};
use alacritty_terminal::term::{Term, cell::Flags};
use alacritty_terminal::vte::ansi::cursor_icon::CursorIcon;
use alacritty_terminal::vte::ansi::*;

pub(crate) struct GraphemeHandler<'a, T: EventListener> {
    pub term: &'a mut Term<T>,
    pub keyboard: &'a mut crate::keyboard::KeyboardState,
    /// Replies go through the listener so they keep the order of the queries.
    pub listener: &'a T,
    pub colors: &'a crate::emulator::HostColors,
}

impl<T: EventListener> GraphemeHandler<'_, T> {
    fn reply(&self, text: String) {
        self.listener.send_event(Event::PtyWrite(text));
    }
}

impl<T: EventListener> Handler for GraphemeHandler<'_, T> {
    fn input(&mut self, character: char) {
        if character.is_ascii() {
            self.term.input(character);
            return;
        }
        let grid = self.term.grid();
        let cursor = grid.cursor.point;
        if cursor.column.0 == 0 && !grid.cursor.input_needs_wrap {
            self.term.input(character);
            return;
        }
        let mut column = if grid.cursor.input_needs_wrap {
            cursor.column.0
        } else {
            cursor.column.0 - 1
        };
        if grid[Point::new(cursor.line, Column(column))]
            .flags
            .contains(Flags::WIDE_CHAR_SPACER)
        {
            column = column.saturating_sub(1);
        }
        let point = Point::new(cursor.line, Column(column));
        let previous = &grid[point];
        let mut cluster = previous.c.to_string();
        if let Some(extra) = previous.zerowidth() {
            cluster.extend(extra);
        }
        cluster.push(character);
        if tide_core::graphemes(&cluster).count() != 1 {
            self.term.input(character);
            return;
        }
        // Bound combining-mark storage per cell without discarding ordinary text.
        if cluster.chars().count() > 64 {
            return;
        }
        let old_width = if previous.flags.contains(Flags::WIDE_CHAR) {
            2
        } else {
            1
        };
        let new_width = tide_core::display_width(&cluster).clamp(1, 2);
        if new_width > old_width {
            if column + 1 >= grid.columns() {
                // A text-presentation base at the right edge became a wide emoji.
                // Reuse Alacritty's normal wide-character wrap and retain the cluster.
                self.term.grid_mut()[point].c = ' ';
                self.term.grid_mut()[point]
                    .flags
                    .insert(Flags::LEADING_WIDE_CHAR_SPACER);
                self.term.input('界');
                let cursor = self.term.grid().cursor.point;
                let column = if self.term.grid().cursor.input_needs_wrap {
                    cursor.column.0.saturating_sub(1)
                } else {
                    cursor.column.0.saturating_sub(2)
                };
                let cell = &mut self.term.grid_mut()[Point::new(cursor.line, Column(column))];
                let mut chars = cluster.chars();
                cell.c = chars.next().unwrap();
                for ch in chars {
                    cell.push_zerowidth(ch);
                }
                return;
            }
            self.term.input(' ');
            self.term.grid_mut()[point].flags.insert(Flags::WIDE_CHAR);
            self.term.grid_mut()[Point::new(point.line, Column(column + 1))]
                .flags
                .insert(Flags::WIDE_CHAR_SPACER);
        }
        self.term.grid_mut()[point].push_zerowidth(character);
    }
    fn set_title(&mut self, argument0: Option<String>) {
        self.term.set_title(argument0);
    }
    fn set_cursor_style(&mut self, argument0: Option<CursorStyle>) {
        self.term.set_cursor_style(argument0);
    }
    fn set_cursor_shape(&mut self, argument0: CursorShape) {
        self.term.set_cursor_shape(argument0);
    }
    fn goto(&mut self, argument0: i32, argument1: usize) {
        self.term.goto(argument0, argument1);
    }
    fn goto_line(&mut self, argument0: i32) {
        self.term.goto_line(argument0);
    }
    fn goto_col(&mut self, argument0: usize) {
        self.term.goto_col(argument0);
    }
    fn insert_blank(&mut self, argument0: usize) {
        self.term.insert_blank(argument0);
    }
    fn move_up(&mut self, argument0: usize) {
        self.term.move_up(argument0);
    }
    fn move_down(&mut self, argument0: usize) {
        self.term.move_down(argument0);
    }
    fn identify_terminal(&mut self, argument0: Option<char>) {
        self.term.identify_terminal(argument0);
    }
    fn device_status(&mut self, argument0: usize) {
        self.term.device_status(argument0);
    }
    fn move_forward(&mut self, argument0: usize) {
        self.term.move_forward(argument0);
    }
    fn move_backward(&mut self, argument0: usize) {
        self.term.move_backward(argument0);
    }
    fn move_down_and_cr(&mut self, argument0: usize) {
        self.term.move_down_and_cr(argument0);
    }
    fn move_up_and_cr(&mut self, argument0: usize) {
        self.term.move_up_and_cr(argument0);
    }
    fn put_tab(&mut self, argument0: u16) {
        self.term.put_tab(argument0);
    }
    fn backspace(&mut self) {
        self.term.backspace();
    }
    fn carriage_return(&mut self) {
        self.term.carriage_return();
    }
    fn linefeed(&mut self) {
        self.term.linefeed();
    }
    fn bell(&mut self) {
        self.term.bell();
    }
    fn substitute(&mut self) {
        self.term.substitute();
    }
    fn newline(&mut self) {
        self.term.newline();
    }
    fn set_horizontal_tabstop(&mut self) {
        self.term.set_horizontal_tabstop();
    }
    fn scroll_up(&mut self, argument0: usize) {
        self.term.scroll_up(argument0);
    }
    fn scroll_down(&mut self, argument0: usize) {
        self.term.scroll_down(argument0);
    }
    fn insert_blank_lines(&mut self, argument0: usize) {
        self.term.insert_blank_lines(argument0);
    }
    fn delete_lines(&mut self, argument0: usize) {
        self.term.delete_lines(argument0);
    }
    fn erase_chars(&mut self, argument0: usize) {
        self.term.erase_chars(argument0);
    }
    fn delete_chars(&mut self, argument0: usize) {
        self.term.delete_chars(argument0);
    }
    fn move_backward_tabs(&mut self, argument0: u16) {
        self.term.move_backward_tabs(argument0);
    }
    fn move_forward_tabs(&mut self, argument0: u16) {
        self.term.move_forward_tabs(argument0);
    }
    fn save_cursor_position(&mut self) {
        self.term.save_cursor_position();
    }
    fn restore_cursor_position(&mut self) {
        self.term.restore_cursor_position();
    }
    fn clear_line(&mut self, argument0: LineClearMode) {
        self.term.clear_line(argument0);
    }
    fn clear_screen(&mut self, argument0: ClearMode) {
        self.term.clear_screen(argument0);
    }
    fn clear_tabs(&mut self, argument0: TabulationClearMode) {
        self.term.clear_tabs(argument0);
    }
    fn set_tabs(&mut self, argument0: u16) {
        self.term.set_tabs(argument0);
    }
    fn reset_state(&mut self) {
        self.term.reset_state();
        *self.keyboard = crate::keyboard::KeyboardState::default();
    }
    fn reverse_index(&mut self) {
        self.term.reverse_index();
    }
    fn terminal_attribute(&mut self, argument0: Attr) {
        self.term.terminal_attribute(argument0);
    }
    fn set_mode(&mut self, argument0: Mode) {
        self.term.set_mode(argument0);
    }
    fn unset_mode(&mut self, argument0: Mode) {
        self.term.unset_mode(argument0);
    }
    fn report_mode(&mut self, argument0: Mode) {
        self.term.report_mode(argument0);
    }
    fn set_private_mode(&mut self, argument0: PrivateMode) {
        self.term.set_private_mode(argument0);
    }
    fn unset_private_mode(&mut self, argument0: PrivateMode) {
        self.term.unset_private_mode(argument0);
    }
    fn report_private_mode(&mut self, argument0: PrivateMode) {
        self.term.report_private_mode(argument0);
    }
    fn set_scrolling_region(&mut self, argument0: usize, argument1: Option<usize>) {
        self.term.set_scrolling_region(argument0, argument1);
    }
    fn set_keypad_application_mode(&mut self) {
        self.term.set_keypad_application_mode();
    }
    fn unset_keypad_application_mode(&mut self) {
        self.term.unset_keypad_application_mode();
    }
    fn set_active_charset(&mut self, argument0: CharsetIndex) {
        self.term.set_active_charset(argument0);
    }
    fn configure_charset(&mut self, argument0: CharsetIndex, argument1: StandardCharset) {
        self.term.configure_charset(argument0, argument1);
    }
    fn set_color(&mut self, argument0: usize, argument1: Rgb) {
        self.term.set_color(argument0, argument1);
    }
    fn dynamic_color_sequence(&mut self, prefix: String, index: usize, terminator: &str) {
        let Rgb { r, g, b } = (index < alacritty_terminal::term::color::COUNT)
            .then(|| self.term.colors()[index])
            .flatten()
            .unwrap_or_else(|| self.colors.rgb(index));
        self.reply(format!(
            "\x1b]{prefix};rgb:{r:02x}{r:02x}/{g:02x}{g:02x}/{b:02x}{b:02x}{terminator}"
        ));
    }
    fn reset_color(&mut self, argument0: usize) {
        self.term.reset_color(argument0);
    }
    fn clipboard_store(&mut self, argument0: u8, argument1: &[u8]) {
        self.term.clipboard_store(argument0, argument1);
    }
    fn clipboard_load(&mut self, argument0: u8, argument1: &str) {
        self.term.clipboard_load(argument0, argument1);
    }
    fn decaln(&mut self) {
        self.term.decaln();
    }
    fn push_title(&mut self) {
        self.term.push_title();
    }
    fn pop_title(&mut self) {
        self.term.pop_title();
    }
    fn text_area_size_pixels(&mut self) {
        // The PTY reports no pixel size either; answer so the child stops waiting.
        self.reply("\x1b[4;0;0t".to_owned());
    }
    fn text_area_size_chars(&mut self) {
        self.term.text_area_size_chars();
    }
    fn set_hyperlink(&mut self, argument0: Option<Hyperlink>) {
        self.term.set_hyperlink(argument0);
    }
    fn set_mouse_cursor_icon(&mut self, argument0: CursorIcon) {
        self.term.set_mouse_cursor_icon(argument0);
    }
    fn report_keyboard_mode(&mut self) {
        let index = usize::from(
            self.term
                .mode()
                .contains(alacritty_terminal::term::TermMode::ALT_SCREEN),
        );
        self.reply(format!("\x1b[?{}u", self.keyboard.layers[index].flags));
    }
    fn push_keyboard_mode(&mut self, mode: KeyboardModes) {
        let index = usize::from(
            self.term
                .mode()
                .contains(alacritty_terminal::term::TermMode::ALT_SCREEN),
        );
        let layer = &mut self.keyboard.layers[index];
        if layer.stack.len() >= 32 {
            layer.stack.remove(0);
        }
        layer.stack.push(layer.flags);
        layer.flags = mode.bits() & 9;
    }
    fn pop_keyboard_modes(&mut self, count: u16) {
        let index = usize::from(
            self.term
                .mode()
                .contains(alacritty_terminal::term::TermMode::ALT_SCREEN),
        );
        let layer = &mut self.keyboard.layers[index];
        for _ in 0..count.min(33) {
            layer.flags = layer.stack.pop().unwrap_or(0);
        }
    }
    fn set_keyboard_mode(&mut self, mode: KeyboardModes, behavior: KeyboardModesApplyBehavior) {
        let index = usize::from(
            self.term
                .mode()
                .contains(alacritty_terminal::term::TermMode::ALT_SCREEN),
        );
        let flags = &mut self.keyboard.layers[index].flags;
        let mode = mode.bits() & 9;
        *flags = match behavior {
            KeyboardModesApplyBehavior::Replace => mode,
            KeyboardModesApplyBehavior::Union => *flags | mode,
            KeyboardModesApplyBehavior::Difference => *flags & !mode,
        };
    }
    fn set_modify_other_keys(&mut self, mode: ModifyOtherKeys) {
        self.keyboard.modify_other_keys = match mode {
            ModifyOtherKeys::Reset => 0,
            ModifyOtherKeys::EnableExceptWellDefined => 1,
            ModifyOtherKeys::EnableAll => 2,
        };
    }
    fn report_modify_other_keys(&mut self) {
        self.reply(format!("\x1b[>4;{}m", self.keyboard.modify_other_keys));
    }
    fn set_scp(&mut self, argument0: ScpCharPath, argument1: ScpUpdateMode) {
        self.term.set_scp(argument0, argument1);
    }
}
