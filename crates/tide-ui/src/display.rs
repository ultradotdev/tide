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
}
