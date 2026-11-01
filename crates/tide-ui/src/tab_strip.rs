//! Shared main-pane and bottom-dock tab strip geometry and interactions.
use crate::palette::Palette;
use ratatui::{
    Frame,
    layout::Rect,
    style::{Modifier, Style},
    widgets::{Block, Paragraph},
};
use tide_core::num::saturating_u16;
use tide_core::{display_width, truncate_to_width};

#[derive(Debug, Default, Clone, Copy)]
pub struct TabStripState {
    pub scroll: usize,
    pub manual: bool,
    pub last_active: Option<usize>,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TabHit {
    Bar,
    Select(usize),
    Close(usize),
    Scroll(bool),
    Overflow,
}

/// Tab labels, their cell widths, whether they overflow `area`, and the cells left for tabs.
fn measure(area: Rect, tabs: &[(String, bool)]) -> (Vec<String>, Vec<usize>, bool, usize) {
    let titles = tabs
        .iter()
        .map(|(title, dirty)| {
            format!(
                " {}{} ",
                truncate_to_width(title, 24),
                if *dirty { " ●" } else { "" }
            )
        })
        .collect::<Vec<_>>();
    let widths = titles
        .iter()
        .enumerate()
        .map(|(index, title)| display_width(title) + 2 + usize::from(index + 1 < titles.len()))
        .collect::<Vec<_>>();
    let overflow = widths.iter().sum::<usize>() > area.width.saturating_sub(3) as usize;
    let available = (area.width as usize).saturating_sub(if overflow { 9 } else { 3 });
    (titles, widths, overflow, available)
}

/// Scrolls the strip so the active tab fits `area`, unless the user scrolled it by hand.
pub fn layout(area: Rect, tabs: &[(String, bool)], active: usize, state: &mut TabStripState) {
    if area.is_empty() {
        return;
    }
    let (_, widths, overflow, available) = measure(area, tabs);
    if state.last_active != Some(active) {
        state.manual = false;
    }
    state.last_active = Some(active);
    if !overflow {
        state.scroll = 0;
    }
    state.scroll = state.scroll.min(tabs.len().saturating_sub(1));
    if !state.manual {
        if active < state.scroll {
            state.scroll = active;
        }
        while state.scroll < active
            && widths[state.scroll..=active].iter().sum::<usize>() > available
        {
            state.scroll += 1;
        }
    }
}

pub fn render(
    frame: &mut Frame,
    area: Rect,
    palette: Palette,
    tabs: &[(String, bool)],
    active: usize,
    state: &TabStripState,
) -> Vec<(Rect, TabHit)> {
    if area.is_empty() {
        return Vec::new();
    }
    frame.render_widget(
        Block::default().style(Style::default().bg(palette.tab_inactive)),
        area,
    );
    let mut hits = vec![(area, TabHit::Bar)];
    let (titles, widths, overflow, available) = measure(area, tabs);
    let mut x = area.x;
    let end = area.x.saturating_add(saturating_u16(available));
    for index in state.scroll..titles.len() {
        let width = saturating_u16(widths[index]).min(end.saturating_sub(x));
        if width < 4 {
            break;
        }
        let style = if index == active {
            Style::default()
                .fg(palette.tab_active_fg)
                .bg(palette.tab_active)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default()
                .fg(palette.tab_inactive_fg)
                .bg(palette.tab_inactive)
        };
        let separator = u16::from(index + 1 < titles.len());
        let body_width = width - 2 - separator;
        let close_x = x + body_width;
        let rect = Rect::new(x, area.y, width, 1);
        frame.render_widget(
            Paragraph::new(truncate_to_width(&titles[index], usize::from(body_width))).style(style),
            Rect::new(x, area.y, body_width, 1),
        );
        frame.render_widget(
            Paragraph::new("× ").style(style),
            Rect::new(close_x, area.y, 2, 1),
        );
        if separator > 0 {
            frame.render_widget(
                Paragraph::new("│")
                    .style(Style::default().fg(palette.border).bg(palette.tab_inactive)),
                Rect::new(x + width - 1, area.y, 1, 1),
            );
        }
        hits.push((rect, TabHit::Select(index)));
        hits.push((Rect::new(close_x, area.y, 2, 1), TabHit::Close(index)));
        x += width;
    }
    if area.width >= 3 {
        for (offset, label, target) in [
            (9, " ‹ ", TabHit::Scroll(false)),
            (6, " › ", TabHit::Scroll(true)),
            (3, " ▾ ", TabHit::Overflow),
        ] {
            if offset > 3 && (!overflow || area.width < 9) {
                continue;
            }
            let rect = Rect::new(area.right() - offset, area.y, 3, 1);
            frame.render_widget(
                Paragraph::new(label).style(
                    Style::default()
                        .fg(palette.foreground)
                        .bg(palette.tab_inactive),
                ),
                rect,
            );
            hits.push((rect, target));
        }
    }
    hits
}
