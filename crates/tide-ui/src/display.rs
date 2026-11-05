//! Final display boundary: source text is data, never terminal instructions.
use ratatui::{
    buffer::{Buffer, Cell},
    style::{Color, Modifier},
};
pub fn sanitize(buffer: &mut Buffer) {
    for cell in &mut buffer.content {
        if cell.symbol().chars().any(char::is_control) {
            let safe = cell
                .symbol()
                .chars()
                .map(|c| match c {
                    '\0'..='\u{1f}' => char::from_u32(0x2400 + c as u32).unwrap_or('�'),
                    '\u{7f}' => '␡',
                    c if c.is_control() => '�',
                    c => c,
                })
                .collect::<String>();
            cell.set_symbol(&safe);
        }
    }
}
/// The first eight characters of a revision or session identifier.
pub fn short_id(id: &str) -> &str {
    id.char_indices().nth(8).map_or(id, |(end, _)| &id[..end])
}
pub fn diagnostic_underlines(
    buffer: &Buffer,
    error: Color,
    warning: Color,
) -> Vec<(u16, u16, Cell)> {
    buffer
        .content
        .iter()
        .enumerate()
        .filter(|(_, cell)| {
            cell.modifier.contains(Modifier::UNDERLINED)
                && (cell.underline_color == error || cell.underline_color == warning)
        })
        .map(|(index, cell)| {
            let (x, y) = buffer.pos_of(index);
            (x, y, cell.clone())
        })
        .collect()
}
fn terminal_color(color: Color) -> crossterm::style::Color {
    use crossterm::style::Color as C;
    match color {
        Color::Reset => C::Reset,
        Color::Black => C::Black,
        Color::Red => C::DarkRed,
        Color::Green => C::DarkGreen,
        Color::Yellow => C::DarkYellow,
        Color::Blue => C::DarkBlue,
        Color::Magenta => C::DarkMagenta,
        Color::Cyan => C::DarkCyan,
        Color::Gray => C::Grey,
        Color::DarkGray => C::DarkGrey,
        Color::LightRed => C::Red,
        Color::LightGreen => C::Green,
        Color::LightYellow => C::Yellow,
        Color::LightBlue => C::Blue,
        Color::LightMagenta => C::Magenta,
        Color::LightCyan => C::Cyan,
        Color::White => C::White,
        Color::Rgb(r, g, b) => C::Rgb { r, g, b },
        Color::Indexed(n) => C::AnsiValue(n),
    }
}
pub fn paint_curly<W: std::io::Write>(
    writer: &mut W,
    cells: &[(u16, u16, Cell)],
) -> std::io::Result<()> {
    use crossterm::{
        cursor::{MoveTo, RestorePosition, SavePosition},
        style::{
            Attribute, Print, ResetColor, SetAttribute, SetBackgroundColor, SetForegroundColor,
            SetUnderlineColor,
        },
    };
    if cells.is_empty() {
        return Ok(());
    }
    crossterm::queue!(writer, SavePosition)?;
    for (x, y, cell) in cells {
        crossterm::queue!(
            writer,
            MoveTo(*x, *y),
            SetAttribute(Attribute::Reset),
            SetForegroundColor(terminal_color(cell.fg)),
            SetBackgroundColor(terminal_color(cell.bg)),
            SetUnderlineColor(terminal_color(cell.underline_color)),
            SetAttribute(Attribute::Undercurled)
        )?;
        for (flag, attribute) in [
            (Modifier::BOLD, Attribute::Bold),
            (Modifier::ITALIC, Attribute::Italic),
            (Modifier::DIM, Attribute::Dim),
            (Modifier::CROSSED_OUT, Attribute::CrossedOut),
        ] {
            if cell.modifier.contains(flag) {
                crossterm::queue!(writer, SetAttribute(attribute))?;
            }
        }
        crossterm::queue!(writer, Print(cell.symbol()))?;
    }
    crossterm::queue!(
        writer,
        SetAttribute(Attribute::Reset),
        ResetColor,
        RestorePosition
    )?;
    writer.flush()
}

/// What the outer terminal can show, read from its environment at startup.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TerminalTraits {
    /// 24-bit colour. Without it, colours go out as the nearest of the 256-colour palette.
    pub truecolor: bool,
    /// Nerd Font glyphs render even when the chosen font lacks them, because the
    /// terminal bundles them as a fallback (Ghostty, kitty and WezTerm do).
    pub nerd_font: bool,
    /// Ctrl+Shift with a letter, digit or punctuation key reaches Tide as itself, through
    /// the kitty keyboard protocol. Claimed only where `TERM` names kitty or Ghostty
    /// directly: inside tmux or screen the outer terminal's variables are inherited but
    /// the protocol does not pass through.
    pub keyboard_protocol: bool,
}

impl TerminalTraits {
    /// Everything available: the state before detection, and what tests draw with.
    pub const FULL: Self = Self {
        truecolor: true,
        nerd_font: true,
        keyboard_protocol: true,
    };

    /// Reads `COLORTERM`, `TERM`, `TERM_PROGRAM` and the variables terminals set for
    /// their own sessions. Unknown terminals get 256 colours and no Nerd Font glyphs,
    /// which every terminal Tide supports can show.
    pub fn detect(env: impl Fn(&str) -> Option<String>) -> Self {
        let var = |name: &str| env(name).filter(|value| !value.is_empty());
        let term = var("TERM").unwrap_or_default().to_ascii_lowercase();
        let program = var("TERM_PROGRAM").unwrap_or_default();
        let colorterm = var("COLORTERM").unwrap_or_default().to_ascii_lowercase();
        let ghostty = program == "ghostty"
            || term == "xterm-ghostty"
            || var("GHOSTTY_RESOURCES_DIR").is_some();
        let kitty = term == "xterm-kitty" || var("KITTY_WINDOW_ID").is_some();
        let wezterm = program == "WezTerm" || term == "wezterm" || var("WEZTERM_PANE").is_some();
        let truecolor = matches!(colorterm.as_str(), "truecolor" | "24bit")
            || term.contains("direct")
            || ghostty
            || kitty
            || wezterm
            || matches!(
                program.as_str(),
                "iTerm.app" | "vscode" | "WarpTerminal" | "Hyper" | "Tabby" | "rio" | "zed"
            )
            || matches!(term.as_str(), "alacritty" | "foot" | "contour" | "rio")
            || var("VTE_VERSION")
                .and_then(|version| version.parse::<u32>().ok())
                .is_some_and(|version| version >= 3600);
        Self {
            truecolor,
            nerd_font: ghostty || kitty || wezterm,
            keyboard_protocol: matches!(term.as_str(), "xterm-kitty" | "xterm-ghostty"),
        }
    }
}

/// The colours the terminal is sent.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ColorDepth {
    TrueColor,
    /// The 256-colour palette, for terminals such as Terminal.app before macOS 26.
    Indexed,
}

/// Replaces every 24-bit colour in `cells` with the nearest 256-palette colour. The 16
/// themeable ANSI colours are never chosen, since the user's terminal decides them.
pub fn reduce_colors<'a>(cells: impl Iterator<Item = &'a mut Cell>) {
    let reduce = |color: &mut Color| {
        if let Color::Rgb(r, g, b) = *color {
            *color = Color::Indexed(nearest_indexed(r, g, b));
        }
    };
    for cell in cells {
        reduce(&mut cell.fg);
        reduce(&mut cell.bg);
        reduce(&mut cell.underline_color);
    }
}

/// The 256-palette index closest to an RGB colour: the nearer of the 6×6×6 colour
/// cube entry and the grey ramp entry.
#[must_use]
pub fn nearest_indexed(r: u8, g: u8, b: u8) -> u8 {
    const LEVELS: [u8; 6] = [0, 95, 135, 175, 215, 255];
    let level = |value: u8| -> usize {
        match value {
            0..48 => 0,
            48..115 => 1,
            _ => usize::from((value - 35) / 40),
        }
    };
    let distance = |(r2, g2, b2): (u8, u8, u8)| {
        let d = |a: u8, b: u8| (i32::from(a) - i32::from(b)).pow(2);
        // Weighted for how strongly the eye sees each channel.
        2 * d(r, r2) + 4 * d(g, g2) + 3 * d(b, b2)
    };
    let (ri, gi, bi) = (level(r), level(g), level(b));
    let cube = (LEVELS[ri], LEVELS[gi], LEVELS[bi]);
    let average = (u16::from(r) + u16::from(g) + u16::from(b)) / 3;
    let grey_index = u8::try_from(average.saturating_sub(3) / 10)
        .unwrap_or(23)
        .min(23);
    let grey_level = 8 + 10 * grey_index;
    let grey = (grey_level, grey_level, grey_level);
    let cube_index = u8::try_from(16 + 36 * ri + 6 * gi + bi).unwrap_or(16);
    if distance(grey) < distance(cube) {
        232 + grey_index
    } else {
        cube_index
    }
}
#[cfg(test)]
mod tests {
    #[test]
    fn short_ids_cut_on_a_char_boundary() {
        assert_eq!(super::short_id("0123456789"), "01234567");
        assert_eq!(super::short_id("ééééééééé"), "éééééééé");
        assert_eq!(super::short_id("abc"), "abc");
    }
    #[test]
    fn untrusted_cell_controls_never_reach_the_terminal_and_unicode_is_preserved() {
        let mut buffer = ratatui::buffer::Buffer::empty(ratatui::layout::Rect::new(0, 0, 2, 1));
        buffer[(0, 0)].set_symbol("\x1b]52;c;PRIVATE\x07");
        buffer[(1, 0)].set_symbol("👩‍💻");
        super::sanitize(&mut buffer);
        assert_eq!(buffer[(0, 0)].symbol(), "␛]52;c;PRIVATE␇");
        assert_eq!(buffer[(1, 0)].symbol(), "👩‍💻");
        buffer[(0, 0)].modifier = ratatui::style::Modifier::UNDERLINED;
        buffer[(0, 0)].underline_color = ratatui::style::Color::Red;
        let cells = super::diagnostic_underlines(
            &buffer,
            ratatui::style::Color::Red,
            ratatui::style::Color::Yellow,
        );
        let mut bytes = Vec::new();
        super::paint_curly(&mut bytes, &cells).unwrap();
        assert!(!bytes.windows(4).any(|b| b == b"\x1b]52"));
        assert!(bytes.windows(6).any(|b| b == b"\x1b[4:3m"));
    }

    #[test]
    fn terminals_without_truecolor_or_bundled_glyphs_are_recognised() {
        use super::TerminalTraits;
        let detect = |vars: &[(&str, &str)]| {
            TerminalTraits::detect(|name| {
                vars.iter()
                    .find(|(key, _)| *key == name)
                    .map(|(_, value)| (*value).to_owned())
            })
        };
        let apple = detect(&[
            ("TERM_PROGRAM", "Apple_Terminal"),
            ("TERM", "xterm-256color"),
        ]);
        assert!(!apple.truecolor && !apple.nerd_font);
        let apple_truecolor = detect(&[
            ("TERM_PROGRAM", "Apple_Terminal"),
            ("COLORTERM", "truecolor"),
        ]);
        assert!(apple_truecolor.truecolor);
        assert!(detect(&[("TERM_PROGRAM", "iTerm.app")]).truecolor);
        assert!(!detect(&[("TERM_PROGRAM", "iTerm.app")]).nerd_font);
        assert!(detect(&[("COLORTERM", "24bit")]).truecolor);
        assert!(detect(&[("TERM", "xterm-direct")]).truecolor);
        assert!(detect(&[("TERM_PROGRAM", "vscode")]).truecolor);
        for terminal in [
            &[("TERM_PROGRAM", "ghostty")][..],
            &[("TERM", "xterm-kitty")],
            &[("TERM_PROGRAM", "WezTerm")],
            &[("TERM", "tmux-256color"), ("KITTY_WINDOW_ID", "1")],
        ] {
            let traits = detect(terminal);
            assert!(traits.truecolor && traits.nerd_font, "{terminal:?}");
        }
        let unknown = detect(&[("TERM", "xterm-256color")]);
        assert!(!unknown.truecolor && !unknown.nerd_font);
        assert!(!unknown.keyboard_protocol);
        assert!(detect(&[("TERM", "xterm-kitty")]).keyboard_protocol);
        assert!(detect(&[("TERM", "xterm-ghostty")]).keyboard_protocol);
        assert!(
            !detect(&[("TERM", "tmux-256color"), ("TERM_PROGRAM", "ghostty")]).keyboard_protocol,
            "not through tmux"
        );
        assert!(!detect(&[]).truecolor);
    }

    #[test]
    fn colours_reduce_to_the_nearest_palette_entry_outside_the_ansi_sixteen() {
        use super::nearest_indexed;
        assert_eq!(nearest_indexed(0, 0, 0), 16);
        assert_eq!(nearest_indexed(255, 255, 255), 231);
        assert_eq!(nearest_indexed(255, 0, 0), 196);
        assert_eq!(nearest_indexed(0x39, 0x3a, 0x47), 237);
        assert_eq!(nearest_indexed(128, 128, 128), 244);
        for (r, g, b) in [
            (1, 2, 3),
            (200, 100, 50),
            (0xc0, 0xab, 0xf5),
            (250, 250, 240),
        ] {
            assert!(nearest_indexed(r, g, b) >= 16);
        }
        let mut buffer = ratatui::buffer::Buffer::empty(ratatui::layout::Rect::new(0, 0, 2, 1));
        buffer[(0, 0)].fg = ratatui::style::Color::Rgb(255, 0, 0);
        buffer[(0, 0)].bg = ratatui::style::Color::Reset;
        buffer[(1, 0)].fg = ratatui::style::Color::Red;
        super::reduce_colors(buffer.content.iter_mut());
        assert_eq!(buffer[(0, 0)].fg, ratatui::style::Color::Indexed(196));
        assert_eq!(buffer[(0, 0)].bg, ratatui::style::Color::Reset);
        assert_eq!(buffer[(1, 0)].fg, ratatui::style::Color::Red);
    }
}
