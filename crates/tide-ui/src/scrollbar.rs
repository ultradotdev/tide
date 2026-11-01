//! Shared proportional scrollbar geometry for paint and pointer input.
use crate::palette::Palette;
use ratatui::{Frame, layout::Rect, style::Style};
pub fn thumb(height: usize, total: usize, visible: usize, offset: usize) -> (usize, usize) {
    if height == 0 {
        return (0, 0);
    }
    if total <= visible {
        return (0, height);
    }
    let size = height.saturating_mul(visible.max(1)) / total.max(1);
    let size = size.clamp(1, height);
    let start =
        (height - size).saturating_mul(offset.min(total - visible)) / (total - visible).max(1);
    (start, size)
}
pub fn offset(height: usize, total: usize, visible: usize, row: usize) -> usize {
    let (_, size) = thumb(height, total, visible, 0);
    let track = height.saturating_sub(size);
    row.saturating_sub(size / 2)
        .min(track)
        .saturating_mul(total.saturating_sub(visible))
        / track.max(1)
}
pub fn paint(
    frame: &mut Frame,
    area: Rect,
    total: usize,
    visible: usize,
    top: usize,
    palette: Palette,
) -> Option<Rect> {
    if total <= visible || area.is_empty() {
        return None;
    }
    let bar = Rect::new(area.right() - 1, area.y, 1, area.height);
    let (start, size) = thumb(bar.height as usize, total, visible, top);
    for cell_row in 0..bar.height {
        let row = usize::from(cell_row);
        frame.buffer_mut()[(bar.x, bar.y + cell_row)]
            .set_symbol(if (start..start + size).contains(&row) {
                "█"
            } else {
                "│"
            })
            .set_style(
                Style::default()
                    .fg(if (start..start + size).contains(&row) {
                        palette.description
                    } else {
                        palette.border
                    })
                    .bg(palette.background),
            );
    }
    Some(bar)
}
#[cfg(test)]
mod tests {
    #[test]
    fn thumb_tracks_visible_fraction_and_reaches_both_ends() {
        assert_eq!(super::thumb(20, 100, 50, 0), (0, 10));
        assert_eq!(super::thumb(20, 100, 50, 50), (10, 10));
        assert_eq!(super::thumb(20, 1000, 10, 990), (19, 1));
        assert_eq!(super::offset(20, 100, 50, 19), 50);
        assert_eq!(super::thumb(0, 100, 10, 20), (0, 0));
    }
}
