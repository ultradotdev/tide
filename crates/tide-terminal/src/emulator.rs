use std::sync::{Arc, Mutex};

use alacritty_terminal::event::{Event, EventListener};
use alacritty_terminal::grid::Dimensions;
use alacritty_terminal::index::{Column, Point};
use alacritty_terminal::term::cell::Flags;
use alacritty_terminal::term::{Config, Term, TermMode, point_to_viewport, viewport_to_point};
use alacritty_terminal::vte::ansi::{Color, CursorShape, NamedColor, Processor, Rgb};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct TermDimensions {
    cols: usize,
    rows: usize,
}

impl Dimensions for TermDimensions {
    fn total_lines(&self) -> usize {
        self.rows
    }

    fn screen_lines(&self) -> usize {
        self.rows
    }

    fn columns(&self) -> usize {
        self.cols
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TerminalColor {
    #[default]
    Default,
    Rgb(u8, u8, u8),
}

const INLINE_SYMBOL: usize = 22;

/// A cell's grapheme cluster. Clusters up to 22 UTF-8 bytes, which covers
/// ZWJ emoji sequences, are stored inline so snapshots don't allocate per cell.
#[derive(Clone)]
pub struct CellSymbol(SymbolRepr);

#[derive(Clone)]
enum SymbolRepr {
    Inline { len: u8, bytes: [u8; INLINE_SYMBOL] },
    Heap(Box<str>),
}

impl CellSymbol {
    fn new(base: char, zerowidth: &[char]) -> Self {
        let chars = || std::iter::once(base).chain(zerowidth.iter().copied());
        let mut bytes = [0; INLINE_SYMBOL];
        let mut len = 0;
        for ch in chars() {
            if len + ch.len_utf8() > INLINE_SYMBOL {
                return Self(SymbolRepr::Heap(chars().collect::<String>().into()));
            }
            len += ch.encode_utf8(&mut bytes[len..]).len();
        }
        // `len` is at most INLINE_SYMBOL, so the conversion always succeeds.
        match u8::try_from(len) {
            Ok(len) => Self(SymbolRepr::Inline { len, bytes }),
            Err(_) => Self(SymbolRepr::Heap(chars().collect::<String>().into())),
        }
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        match &self.0 {
            SymbolRepr::Inline { len, bytes } => {
                std::str::from_utf8(&bytes[..usize::from(*len)]).unwrap_or_default()
            }
            SymbolRepr::Heap(symbol) => symbol,
        }
    }
}

impl Default for CellSymbol {
    fn default() -> Self {
        Self::new(' ', &[])
    }
}

impl std::ops::Deref for CellSymbol {
    type Target = str;

    fn deref(&self) -> &str {
        self.as_str()
    }
}

impl std::fmt::Debug for CellSymbol {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.as_str().fmt(formatter)
    }
}

impl std::fmt::Display for CellSymbol {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.as_str())
    }
}

impl PartialEq for CellSymbol {
    fn eq(&self, other: &Self) -> bool {
        self.as_str() == other.as_str()
    }
}

impl Eq for CellSymbol {}

impl PartialEq<str> for CellSymbol {
    fn eq(&self, other: &str) -> bool {
        self.as_str() == other
    }
}

impl PartialEq<&str> for CellSymbol {
    fn eq(&self, other: &&str) -> bool {
        self.as_str() == *other
    }
}

impl PartialEq<String> for CellSymbol {
    fn eq(&self, other: &String) -> bool {
        self.as_str() == other
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CellSnapshot {
    pub symbol: CellSymbol,
    pub fg: TerminalColor,
    pub bg: TerminalColor,
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
    pub dim: bool,
    pub inverse: bool,
    pub spacer: bool,
}

impl Default for CellSnapshot {
    fn default() -> Self {
        Self {
            symbol: CellSymbol::default(),
            fg: TerminalColor::Default,
            bg: TerminalColor::Default,
            bold: false,
            italic: false,
            underline: false,
            dim: false,
            inverse: false,
            spacer: false,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CursorSnapshot {
    pub x: usize,
    pub y: usize,
    pub visible: bool,
    pub shape: CursorShape,
    pub blinking: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct TerminalModes {
    pub alternate_screen: bool,
    pub application_cursor_keys: bool,
    pub application_keypad: bool,
    pub bracketed_paste: bool,
    pub focus_reporting: bool,
    pub sgr_mouse: bool,
    pub mouse_report_click: bool,
    pub mouse_motion: bool,
    pub mouse_drag: bool,
    pub insert: bool,
    pub origin: bool,
    pub line_wrap: bool,
    pub kitty_flags: u8,
    pub modify_other_keys: u8,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TerminalSnapshot {
    pub generation: u64,
    pub cols: usize,
    pub rows: usize,
    pub cells: Vec<CellSnapshot>,
    pub cursor: CursorSnapshot,
    pub modes: TerminalModes,
    pub title: Option<String>,
}

impl TerminalSnapshot {
    #[must_use]
    pub fn cell(&self, x: usize, y: usize) -> Option<&CellSnapshot> {
        if x >= self.cols || y >= self.rows {
            return None;
        }
        self.cells.get(y * self.cols + x)
    }

    /// Plain-text screen dump. Trailing spaces are trimmed per row.
    #[must_use]
    pub fn text(&self) -> String {
        let mut rows = Vec::with_capacity(self.rows);
        for y in 0..self.rows {
            let mut row = String::new();
            for x in 0..self.cols {
                if let Some(cell) = self.cell(x, y)
                    && !cell.spacer
                {
                    row.push_str(&cell.symbol);
                }
            }
            rows.push(row.trim_end().to_owned());
        }
        rows.join("\n")
    }
}

#[derive(Debug, Default)]
struct ListenerState {
    title: Option<String>,
    bell: bool,
    replies: Vec<u8>,
}

#[derive(Clone, Debug, Default)]
pub(crate) struct CapturingListener {
    state: Arc<Mutex<ListenerState>>,
}

impl EventListener for CapturingListener {
    fn send_event(&self, event: Event) {
        let Ok(mut state) = self.state.lock() else {
            return;
        };
        match event {
            Event::Title(title) => state.title = Some(title),
            Event::ResetTitle => state.title = None,
            Event::Bell => state.bell = true,
            Event::PtyWrite(reply) if state.replies.len() + reply.len() <= 64 * 1024 => {
                state.replies.extend_from_slice(reply.as_bytes())
            }
            _ => {}
        }
    }
}

/// Host colours that colour queries report until the child overrides them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) struct HostColors {
    ansi: [Option<(u8, u8, u8)>; 16],
    foreground: Option<(u8, u8, u8)>,
    background: Option<(u8, u8, u8)>,
}

impl HostColors {
    pub(crate) fn rgb(&self, index: usize) -> Rgb {
        let (r, g, b) = match u8::try_from(index) {
            Ok(indexed) if indexed < 16 => self.ansi[index].unwrap_or_else(|| indexed_rgb(indexed)),
            Ok(indexed) => indexed_rgb(indexed),
            Err(_) if index == NamedColor::Background as usize => {
                self.background.unwrap_or((0, 0, 0))
            }
            Err(_) => self.foreground.unwrap_or((229, 229, 229)),
        };
        Rgb { r, g, b }
    }
}

pub struct TerminalEmulator {
    term: Term<CapturingListener>,
    processor: Processor,
    cols: usize,
    rows: usize,
    generation: u64,
    listener: CapturingListener,
    keyboard: crate::keyboard::KeyboardState,
    colors: HostColors,
}

impl TerminalEmulator {
    #[must_use]
    pub fn new(cols: usize, rows: usize, scrollback: usize) -> Self {
        let cols = cols.max(2);
        let rows = rows.max(1);
        let listener = CapturingListener::default();
        let config = Config {
            scrolling_history: scrollback,
            ..Config::default()
        };
        let term = Term::new(config, &TermDimensions { cols, rows }, listener.clone());
        Self {
            term,
            processor: Processor::new(),
            cols,
            rows,
            generation: 0,
            listener,
            keyboard: crate::keyboard::KeyboardState::default(),
            colors: HostColors::default(),
        }
    }

    pub fn set_ansi_palette(&mut self, colors: [Option<(u8, u8, u8)>; 16]) {
        if self.colors.ansi != colors {
            self.colors.ansi = colors;
            self.generation = self.generation.wrapping_add(1);
        }
    }

    /// The theme's default foreground and background, reported to OSC 10/11
    /// queries. Cells keep `TerminalColor::Default` for these.
    pub fn set_default_colors(
        &mut self,
        foreground: Option<(u8, u8, u8)>,
        background: Option<(u8, u8, u8)>,
    ) {
        self.colors.foreground = foreground;
        self.colors.background = background;
    }
    fn resolve_color(&self, color: Color) -> TerminalColor {
        let index = match color {
            Color::Indexed(index) => Some(index as usize),
            Color::Named(name) => Some(name as usize),
            _ => None,
        };
        if let Some(index) = index {
            if let Some(rgb) = self.term.colors()[index] {
                return TerminalColor::Rgb(rgb.r, rgb.g, rgb.b);
            }
            if let Some(Some((r, g, b))) = self.colors.ansi.get(index) {
                return TerminalColor::Rgb(*r, *g, *b);
            }
        }
        resolve_color(color)
    }
    pub fn feed(&mut self, bytes: &[u8]) {
        self.processor.advance(
            &mut crate::grapheme::GraphemeHandler {
                term: &mut self.term,
                keyboard: &mut self.keyboard,
                listener: &self.listener,
                colors: &self.colors,
            },
            bytes,
        );
        self.generation = self.generation.wrapping_add(1);
    }

    pub fn resize(&mut self, cols: usize, rows: usize) {
        self.cols = cols.max(2);
        self.rows = rows.max(1);
        self.term.resize(TermDimensions {
            cols: self.cols,
            rows: self.rows,
        });
        self.generation = self.generation.wrapping_add(1);
    }

    pub fn generation(&self) -> u64 {
        self.generation
    }

    pub fn take_replies(&mut self) -> Vec<u8> {
        self.listener
            .state
            .lock()
            .map(|mut state| std::mem::take(&mut state.replies))
            .unwrap_or_default()
    }

    /// Whether the child rang the bell since the last call.
    pub fn take_bell(&mut self) -> bool {
        self.listener
            .state
            .lock()
            .is_ok_and(|mut state| std::mem::take(&mut state.bell))
    }

    pub fn scroll_display(&mut self, lines: i32) {
        self.term
            .scroll_display(alacritty_terminal::grid::Scroll::Delta(lines));
        self.generation = self.generation.wrapping_add(1);
    }

    pub fn scroll_to_bottom(&mut self) {
        if self.term.grid().display_offset() != 0 {
            self.term
                .scroll_display(alacritty_terminal::grid::Scroll::Bottom);
            self.generation = self.generation.wrapping_add(1);
        }
    }

    pub fn scroll_to_top(&mut self) {
        self.term
            .scroll_display(alacritty_terminal::grid::Scroll::Top);
        self.generation = self.generation.wrapping_add(1);
    }

    /// Clears the scrollback and the screen above the cursor, like a terminal's Clear Buffer:
    /// the cursor's line, usually the prompt, moves to the top. Nothing is sent to the child.
    /// A full-screen program owns the alternate screen, so that is left alone; returns
    /// whether anything was cleared.
    pub fn clear(&mut self) -> bool {
        if self.term.mode().contains(TermMode::ALT_SCREEN) {
            return false;
        }
        let cursor = self.term.grid().cursor.point;
        let lines = cursor.line.0.max(0);
        let column = cursor.column.0 + 1;
        // Scroll the lines above the cursor into history, then erase the history (ED 3).
        let mut sequence = String::new();
        if lines > 0 {
            sequence.push_str(&format!("\x1b[{lines}S"));
        }
        sequence.push_str(&format!("\x1b[1;{column}H\x1b[3J"));
        self.feed(sequence.as_bytes());
        true
    }

    pub fn display_offset(&self) -> usize {
        self.term.grid().display_offset()
    }

    pub fn modes(&self) -> TerminalModes {
        let mode = *self.term.mode();
        TerminalModes {
            alternate_screen: mode.contains(TermMode::ALT_SCREEN),
            application_cursor_keys: mode.contains(TermMode::APP_CURSOR),
            application_keypad: mode.contains(TermMode::APP_KEYPAD),
            bracketed_paste: mode.contains(TermMode::BRACKETED_PASTE),
            focus_reporting: mode.contains(TermMode::FOCUS_IN_OUT),
            sgr_mouse: mode.contains(TermMode::SGR_MOUSE),
            mouse_report_click: mode.contains(TermMode::MOUSE_REPORT_CLICK),
            mouse_motion: mode.contains(TermMode::MOUSE_MOTION),
            mouse_drag: mode.contains(TermMode::MOUSE_DRAG),
            insert: mode.contains(TermMode::INSERT),
            origin: mode.contains(TermMode::ORIGIN),
            line_wrap: mode.contains(TermMode::LINE_WRAP),
            kitty_flags: self.keyboard.layers[usize::from(mode.contains(TermMode::ALT_SCREEN))]
                .flags,
            modify_other_keys: self.keyboard.modify_other_keys,
        }
    }

    #[must_use]
    pub fn snapshot(&self) -> TerminalSnapshot {
        let grid = self.term.grid();
        let display_offset = grid.display_offset();
        let mut cells = Vec::with_capacity(self.cols * self.rows);

        for row in 0..self.rows {
            for col in 0..self.cols {
                let point = viewport_to_point(display_offset, Point::new(row, Column(col)));
                let cell = &grid[point];
                cells.push(CellSnapshot {
                    symbol: CellSymbol::new(cell.c, cell.zerowidth().unwrap_or_default()),
                    fg: self.resolve_color(cell.fg),
                    bg: self.resolve_color(cell.bg),
                    bold: cell.flags.contains(Flags::BOLD),
                    italic: cell.flags.contains(Flags::ITALIC),
                    underline: cell.flags.contains(Flags::UNDERLINE),
                    dim: cell.flags.contains(Flags::DIM),
                    inverse: cell.flags.contains(Flags::INVERSE),
                    spacer: cell.flags.contains(Flags::WIDE_CHAR_SPACER)
                        || cell.flags.contains(Flags::LEADING_WIDE_CHAR_SPACER),
                });
            }
        }

        let cursor_point = grid.cursor.point;
        // A cursor above the viewport clamps to the top row.
        let cursor_y =
            point_to_viewport(display_offset, cursor_point).map_or(0, |point| point.line);
        let cursor = CursorSnapshot {
            x: cursor_point.column.0.min(self.cols.saturating_sub(1)),
            y: cursor_y.min(self.rows.saturating_sub(1)),
            visible: self.term.mode().contains(TermMode::SHOW_CURSOR) && display_offset == 0,
            shape: self.term.cursor_style().shape,
            blinking: self.term.cursor_style().blinking,
        };

        let modes = self.modes();

        let title = self
            .listener
            .state
            .lock()
            .ok()
            .and_then(|state| state.title.clone());

        TerminalSnapshot {
            generation: self.generation,
            cols: self.cols,
            rows: self.rows,
            cells,
            cursor,
            modes,
            title,
        }
    }

    #[must_use]
    pub fn cols(&self) -> usize {
        self.cols
    }

    #[must_use]
    pub fn rows(&self) -> usize {
        self.rows
    }
}

fn resolve_color(color: Color) -> TerminalColor {
    match color {
        Color::Spec(Rgb { r, g, b }) => TerminalColor::Rgb(r, g, b),
        Color::Indexed(index) => {
            let (r, g, b) = indexed_rgb(index);
            TerminalColor::Rgb(r, g, b)
        }
        Color::Named(name) => named_color(name),
    }
}

const fn named_color(name: NamedColor) -> TerminalColor {
    match name {
        NamedColor::Black => TerminalColor::Rgb(0, 0, 0),
        NamedColor::Red => TerminalColor::Rgb(205, 49, 49),
        NamedColor::Green => TerminalColor::Rgb(13, 188, 121),
        NamedColor::Yellow => TerminalColor::Rgb(229, 229, 16),
        NamedColor::Blue => TerminalColor::Rgb(36, 114, 200),
        NamedColor::Magenta => TerminalColor::Rgb(188, 63, 188),
        NamedColor::Cyan => TerminalColor::Rgb(17, 168, 205),
        NamedColor::White => TerminalColor::Rgb(229, 229, 229),
        NamedColor::BrightBlack => TerminalColor::Rgb(102, 102, 102),
        NamedColor::BrightRed => TerminalColor::Rgb(241, 76, 76),
        NamedColor::BrightGreen => TerminalColor::Rgb(35, 209, 139),
        NamedColor::BrightYellow => TerminalColor::Rgb(245, 245, 67),
        NamedColor::BrightBlue => TerminalColor::Rgb(59, 142, 234),
        NamedColor::BrightMagenta => TerminalColor::Rgb(214, 112, 214),
        NamedColor::BrightCyan => TerminalColor::Rgb(41, 184, 219),
        NamedColor::BrightWhite => TerminalColor::Rgb(255, 255, 255),
        _ => TerminalColor::Default,
    }
}

fn indexed_rgb(index: u8) -> (u8, u8, u8) {
    if index < 16 {
        match index {
            0 => (0, 0, 0),
            1 => (205, 49, 49),
            2 => (13, 188, 121),
            3 => (229, 229, 16),
            4 => (36, 114, 200),
            5 => (188, 63, 188),
            6 => (17, 168, 205),
            7 => (229, 229, 229),
            8 => (102, 102, 102),
            9 => (241, 76, 76),
            10 => (35, 209, 139),
            11 => (245, 245, 67),
            12 => (59, 142, 234),
            13 => (214, 112, 214),
            14 => (41, 184, 219),
            _ => (255, 255, 255),
        }
    } else if index < 232 {
        let value = index - 16;
        let r = value / 36;
        let g = (value % 36) / 6;
        let b = value % 6;
        let step = |v: u8| if v == 0 { 0 } else { 55 + v * 40 };
        (step(r), step(g), step(b))
    } else {
        let level = 8 + (index - 232) * 10;
        (level, level, level)
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn semantic_ansi_palette_changes_live_cells_and_respects_child_color_overrides() {
        let mut emulator = super::TerminalEmulator::new(20, 2, 0);
        emulator.feed(b"\x1b[31mA\x1b[38;5;1mB\x1b[38;2;7;8;9mC");
        let mut colors = [None; 16];
        colors[1] = Some((1, 2, 3));
        emulator.set_ansi_palette(colors);
        let snapshot = emulator.snapshot();
        assert_eq!(
            snapshot.cell(0, 0).unwrap().fg,
            super::TerminalColor::Rgb(1, 2, 3)
        );
        assert_eq!(
            snapshot.cell(1, 0).unwrap().fg,
            super::TerminalColor::Rgb(1, 2, 3)
        );
        assert_eq!(
            snapshot.cell(2, 0).unwrap().fg,
            super::TerminalColor::Rgb(7, 8, 9)
        );
        emulator.feed(b"\x1b]4;1;rgb:10/20/30\x07");
        colors[1] = Some((5, 6, 7));
        emulator.set_ansi_palette(colors);
        assert_eq!(
            emulator.snapshot().cell(0, 0).unwrap().fg,
            super::TerminalColor::Rgb(16, 32, 48)
        );
        emulator.feed(b"\x1b]104;1\x07");
        assert_eq!(
            emulator.snapshot().cell(0, 0).unwrap().fg,
            super::TerminalColor::Rgb(5, 6, 7)
        );
    }

    use super::*;

    #[test]
    fn keyboard_negotiation_has_independent_bounded_stacks_and_truthful_replies() {
        let mut emulator = TerminalEmulator::new(20, 3, 0);
        emulator.feed(b"\x1b[>1u\x1b[?u");
        assert_eq!(emulator.modes().kitty_flags, 1);
        assert_eq!(emulator.take_replies(), b"\x1b[?1u");
        emulator.feed(b"\x1b[?1049h\x1b[?u");
        assert_eq!(emulator.modes().kitty_flags, 0);
        assert_eq!(emulator.take_replies(), b"\x1b[?0u");
        emulator.feed(b"\x1b[>31u\x1b[?u");
        assert_eq!(emulator.take_replies(), b"\x1b[?9u");
        for _ in 0..5000 {
            emulator.feed(b"\x1b[>1u");
        }
        assert_eq!(emulator.keyboard.layers[1].stack.len(), 32);
        emulator.feed(b"\x1b[<65535u");
        assert_eq!(emulator.modes().kitty_flags, 0);
        emulator.feed(b"\x1b[?1049l");
        assert_eq!(emulator.modes().kitty_flags, 1);
        emulator.feed(b"\x1b[>4;2m\x1b[?4m");
        assert_eq!(emulator.modes().modify_other_keys, 2);
        emulator.feed(b"\x1bc");
        assert_eq!(emulator.modes().kitty_flags, 0);
        assert_eq!(emulator.modes().modify_other_keys, 0);
    }

    #[test]
    fn clear_drops_scrollback_and_keeps_the_cursor_line_at_the_top() {
        let mut emulator = TerminalEmulator::new(20, 4, 100);
        for line in 0..10 {
            emulator.feed(format!("line {line}\r\n").as_bytes());
        }
        emulator.feed(b"$ prompt");
        emulator.scroll_to_top();
        assert!(emulator.display_offset() > 0);
        assert!(emulator.clear());
        assert_eq!(emulator.display_offset(), 0);
        emulator.scroll_to_top();
        assert_eq!(emulator.display_offset(), 0, "scrollback was kept");
        let snapshot = emulator.snapshot();
        assert_eq!(snapshot.text().trim(), "$ prompt");
        assert_eq!((snapshot.cursor.x, snapshot.cursor.y), (8, 0));
        // A full-screen program keeps its screen.
        emulator.feed(b"\x1b[?1049hfull screen");
        assert!(!emulator.clear());
        assert!(emulator.snapshot().text().contains("full screen"));
    }

    #[test]
    fn graphemes_keep_cursor_cells_stable_across_utf8_chunks_styles_and_overwrites() {
        let mut emulator = TerminalEmulator::new(40, 4, 10);
        for byte in "after marker\r\n👩‍💻 end\r\n❤️ 🇺🇸 👍🏽".as_bytes() {
            emulator.feed(&[*byte]);
        }
        let snapshot = emulator.snapshot();
        assert_eq!(snapshot.cell(0, 1).unwrap().symbol, "👩‍💻");
        assert!(snapshot.cell(1, 1).unwrap().spacer);
        assert_eq!(snapshot.cell(3, 1).unwrap().symbol, "e");
        assert_eq!(snapshot.cell(0, 2).unwrap().symbol, "❤️");
        assert_eq!(snapshot.cell(3, 2).unwrap().symbol, "🇺🇸");
        assert_eq!(snapshot.cell(6, 2).unwrap().symbol, "👍🏽");
        assert_eq!(snapshot.cursor.x, 8);
        emulator.feed(b"\x1b[1;6HX marker");
        assert!(emulator.snapshot().text().contains("afterX marker"));
        emulator.feed("\x1b[2;1Hdone        ".as_bytes());
        assert!(emulator.snapshot().text().contains("done"));
        assert!(!emulator.snapshot().text().contains("👩"));
        let mut edge = TerminalEmulator::new(4, 3, 0);
        edge.feed("abc❤️!".as_bytes());
        assert_eq!(edge.snapshot().cell(0, 1).unwrap().symbol, "❤️");
        assert_eq!(edge.snapshot().cursor.x, 3);
    }

    #[test]
    fn color_queries_are_answered_in_order_from_the_live_palette() {
        let mut emulator = TerminalEmulator::new(20, 2, 0);
        // fzf and nvim send OSC 11 followed by DA1 and stop waiting at DA1.
        emulator.feed(b"\x1b]11;?\x07\x1b[c");
        assert_eq!(
            emulator.take_replies(),
            b"\x1b]11;rgb:0000/0000/0000\x07\x1b[?6c"
        );
        let mut ansi = [None; 16];
        ansi[1] = Some((1, 2, 3));
        emulator.set_ansi_palette(ansi);
        emulator.set_default_colors(Some((0x12, 0x34, 0x56)), Some((0xab, 0xcd, 0xef)));
        emulator.feed(b"\x1b]10;?\x1b\\\x1b]11;?\x07\x1b]4;1;?\x07\x1b]4;20;?\x07");
        assert_eq!(
            String::from_utf8(emulator.take_replies()).unwrap(),
            "\x1b]10;rgb:1212/3434/5656\x1b\\\
             \x1b]11;rgb:abab/cdcd/efef\x07\
             \x1b]4;1;rgb:0101/0202/0303\x07\
             \x1b]4;20;rgb:0000/0000/d7d7\x07"
        );
        emulator.feed(b"\x1b]11;rgb:01/02/03\x07\x1b]11;?\x07");
        assert_eq!(emulator.take_replies(), b"\x1b]11;rgb:0101/0202/0303\x07");
        emulator.feed(b"\x1b]111\x07\x1b]11;?\x07");
        assert_eq!(emulator.take_replies(), b"\x1b]11;rgb:abab/cdcd/efef\x07");
    }

    #[test]
    fn size_queries_keyboard_replies_and_bell_are_reported_once_in_order() {
        let mut emulator = TerminalEmulator::new(20, 2, 0);
        emulator.feed(b"\x1b[14t\x1b[18t\x1b[?u\x1b[c");
        assert_eq!(
            emulator.take_replies(),
            b"\x1b[4;0;0t\x1b[8;2;20t\x1b[?0u\x1b[?6c"
        );
        assert!(!emulator.take_bell());
        emulator.feed(b"\x07");
        assert!(emulator.take_bell());
        assert!(!emulator.take_bell());
    }

    #[test]
    fn snapshot_cells_store_graphemes_without_heap_allocation() {
        assert!(std::mem::size_of::<CellSymbol>() <= std::mem::size_of::<String>());
        let mut emulator = TerminalEmulator::new(40, 3, 0);
        emulator.feed("plain ascii\r\n👩‍💻 ❤️ 🇺🇸 👍🏽 é".as_bytes());
        let snapshot = emulator.snapshot();
        assert!(
            snapshot
                .cells
                .iter()
                .all(|cell| matches!(cell.symbol.0, SymbolRepr::Inline { .. }))
        );
        assert_eq!(snapshot.cell(0, 1).unwrap().symbol, "👩‍💻");
        assert_eq!(snapshot.cell(0, 2).unwrap().symbol, " ");
        let long = format!("e{}", "\u{301}".repeat(20));
        emulator.feed(format!("\r\n{long}").as_bytes());
        let symbol = emulator.snapshot().cell(0, 2).unwrap().symbol.clone();
        assert_eq!(symbol.as_str(), long);
        assert!(matches!(symbol.0, SymbolRepr::Heap(_)));
    }

    #[test]
    fn renders_plain_text_and_cursor() {
        let mut emulator = TerminalEmulator::new(20, 4, 100);
        emulator.feed(b"hello\r\nworld");
        let snapshot = emulator.snapshot();
        assert_eq!(snapshot.text(), "hello\nworld\n\n");
        assert_eq!(snapshot.cursor.x, 5);
        assert_eq!(snapshot.cursor.y, 1);
        assert!(snapshot.cursor.visible);
    }

    #[test]
    fn handles_sgr_colors_and_attributes() {
        let mut emulator = TerminalEmulator::new(20, 4, 100);
        emulator.feed(b"\x1b[1;38;2;255;0;0mred\x1b[0m");
        let snapshot = emulator.snapshot();
        let cell = snapshot.cell(0, 0).unwrap();
        assert_eq!(cell.symbol, "r");
        assert!(cell.bold);
        assert_eq!(cell.fg, TerminalColor::Rgb(255, 0, 0));
    }

    #[test]
    fn alternates_screen_and_reports_modes() {
        let mut emulator = TerminalEmulator::new(20, 4, 100);
        emulator.feed(b"normal");
        emulator.feed(b"\x1b[?1049h\x1b[?25l\x1b[?2004h\x1b[?1006h");
        let snapshot = emulator.snapshot();
        assert!(snapshot.modes.alternate_screen);
        assert!(snapshot.modes.bracketed_paste);
        assert!(snapshot.modes.sgr_mouse);
        assert!(!snapshot.cursor.visible);
    }

    #[test]
    fn scrollback_keeps_visible_offset_stable() {
        let mut emulator = TerminalEmulator::new(20, 3, 100);
        for index in 0..10 {
            emulator.feed(format!("line {index}\r\n").as_bytes());
        }
        let snapshot = emulator.snapshot();
        assert!(snapshot.text().contains("line 9"));
    }

    #[test]
    fn resize_updates_snapshot_dimensions() {
        let mut emulator = TerminalEmulator::new(10, 3, 100);
        emulator.resize(20, 5);
        let snapshot = emulator.snapshot();
        assert_eq!(snapshot.cols, 20);
        assert_eq!(snapshot.rows, 5);
        assert_eq!(snapshot.cells.len(), 100);
    }
}
