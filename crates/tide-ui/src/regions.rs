//! Responsive host geometry. Sidebars keep their full height above the status bar.
use ratatui::layout::Rect;
#[derive(Clone, Copy, Debug)]
pub struct Sidebar {
    pub visible: bool,
    pub left: bool,
    pub width: u16,
    pub focused: bool,
}
#[derive(Debug)]
pub struct Regions {
    pub sidebar: Option<Rect>,
    pub agents: Option<Rect>,
    pub main: Rect,
    pub dock: Option<Rect>,
    pub status: Rect,
}
pub fn layout(
    area: Rect,
    sidebar: Sidebar,
    agents: Sidebar,
    dock_visible: bool,
    dock_height: u16,
    main_minimum: u16,
) -> Regions {
    let status = Rect::new(
        area.x,
        area.bottom().saturating_sub(u16::from(area.height > 0)),
        area.width,
        u16::from(area.height > 0),
    );
    let body = Rect::new(area.x, area.y, area.width, area.height.saturating_sub(1));
    let min_main = if body.width < 24
        && ((sidebar.focused && sidebar.visible) || (agents.focused && agents.visible))
    {
        0
    } else {
        body.width.min(main_minimum.clamp(24, 200))
    };
    let mut widths = [sidebar, agents].map(|side| {
        if side.visible && (body.width >= 60 || side.focused && body.width > 0) {
            side.width.clamp(12, 120)
        } else {
            0
        }
    });
    if widths[0].saturating_add(widths[1]).saturating_add(min_main) > body.width && !agents.focused
    {
        widths[1] = 0;
    }
    if widths[0].saturating_add(widths[1]).saturating_add(min_main) > body.width && !sidebar.focused
    {
        widths[0] = 0;
    }
    let available = body.width.saturating_sub(min_main);
    let total = widths[0].saturating_add(widths[1]);
    if total > available && total > 0 {
        // The scaled width is at most `widths[0]`, so it always fits.
        let scaled = u32::from(widths[0]) * u32::from(available) / u32::from(total);
        widths[0] = u16::try_from(scaled).unwrap_or(available);
        widths[1] = available.saturating_sub(widths[0]);
    }
    let mut main = body;
    let mut places = [None, None];
    for (index, side) in [sidebar, agents].into_iter().enumerate() {
        let width = widths[index];
        if width == 0 {
            continue;
        }
        places[index] = Some(Rect::new(
            if side.left {
                main.x
            } else {
                main.right() - width
            },
            body.y,
            width,
            body.height,
        ));
        if side.left {
            main.x += width;
        }
        main.width = main.width.saturating_sub(width);
    }
    let dock = if dock_visible && main.height >= 4 {
        let height = dock_height.max(3).min(main.height.saturating_sub(3).max(1));
        let dock = Rect::new(main.x, main.bottom() - height, main.width, height);
        main.height -= height;
        Some(dock)
    } else {
        None
    };
    Regions {
        sidebar: places[0],
        agents: places[1],
        main,
        dock,
        status,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn hosts_fit_small_surfaces_and_sidebars_span_the_dock() {
        for (width, height) in [(1, 1), (20, 7), (40, 12), (82, 26), (120, 40), (160, 50)] {
            for left in [true, false] {
                for agents_left in [true, false] {
                    let area = Rect::new(0, 0, width, height);
                    let regions = layout(
                        area,
                        Sidebar {
                            visible: true,
                            left,
                            width: 36,
                            focused: width < 24,
                        },
                        Sidebar {
                            visible: true,
                            left: agents_left,
                            width: 36,
                            focused: false,
                        },
                        true,
                        10,
                        56,
                    );
                    let rects = [
                        regions.sidebar,
                        regions.agents,
                        Some(regions.main),
                        regions.dock,
                        Some(regions.status),
                    ]
                    .into_iter()
                    .flatten()
                    .collect::<Vec<_>>();
                    for (index, rect) in rects.iter().enumerate() {
                        assert_eq!(rect.intersection(area), *rect);
                        for other in &rects[index + 1..] {
                            assert!(rect.intersection(*other).is_empty(), "overlap: {regions:?}");
                        }
                    }
                    if let Some(sidebar) = regions.sidebar {
                        assert_eq!(sidebar.height, height.saturating_sub(1));
                    }
                    if let Some(dock) = regions.dock {
                        assert_eq!(dock.y, regions.main.bottom());
                        assert_eq!(dock.width, regions.main.width);
                    }
                }
            }
        }
    }
}
