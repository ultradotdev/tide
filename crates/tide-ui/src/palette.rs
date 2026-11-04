use num_traits::ToPrimitive;
use ratatui::style::Color;

use tide_config::Theme;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActionRole {
    Primary,
    Confirmation,
    Destructive,
    Neutral,
}

/// Semantic colors used by the workbench.
#[derive(Debug, Clone, Copy)]
pub struct Palette {
    pub background: Color,
    pub foreground: Color,
    pub muted: Color,
    pub accent: Color,
    pub border: Color,
    pub border_focused: Color,
    pub sidebar: Color,
    pub status: Color,
    pub status_foreground: Color,
    pub tab_active: Color,
    pub tab_active_fg: Color,
    pub tab_inactive_fg: Color,
    pub input_fg: Color,
    pub button_bg: Color,
    pub button_fg: Color,
    pub confirmation_bg: Color,
    pub destructive_bg: Color,
    pub git_modified: Color,
    pub git_untracked: Color,
    pub terminal_ansi: [Option<(u8, u8, u8)>; 16],
    pub tab_inactive: Color,
    pub line_highlight: Color,
    pub bracket_match_bg: Color,
    pub bracket_match_fg: Color,
    pub selection: Color,
    pub line_number: Color,
    pub line_number_active: Color,
    pub terminal_bg: Color,
    pub terminal_fg: Color,
    pub diff_add: Color,
    pub diff_delete: Color,
    pub warning: Color,
    pub error: Color,
    pub modal_bg: Color,
    pub modal_fg: Color,
    pub modal_border: Color,
    pub input_bg: Color,
    pub selection_bg: Color,
    pub selection_fg: Color,
    pub description: Color,
    pub section_bg: Color,
    pub section_fg: Color,
    pub section_focused_bg: Color,
    pub section_focused_fg: Color,
}

impl Palette {
    #[must_use]
    pub fn from_theme(theme: &Theme) -> Self {
        let get = |role: &str, fallback: &str| {
            theme
                .color(role)
                .and_then(hex_color)
                .unwrap_or_else(|| hex_color(fallback).expect("valid fallback"))
        };
        Self {
            background: get("editor.background", "#393a47"),
            foreground: get("editor.foreground", "#e6dbd1"),
            muted: get("editorLineNumber.foreground", "#9b98a8"),
            accent: get("editorCursor.foreground", "#c0abf5"),
            border: get("panel.border", "#565666"),
            border_focused: get("focusBorder", "#c0abf5"),
            sidebar: get("sideBar.background", "#30313d"),
            status: get("statusBar.background", "#464653"),
            status_foreground: get("statusBar.foreground", "#cabab7"),
            tab_active: get("tab.activeBackground", "#393a47"),
            tab_active_fg: get("tab.activeForeground", "#e6dbd1"),
            tab_inactive_fg: get("tab.inactiveForeground", "#9b98a8"),
            input_fg: get("input.foreground", "#e6dbd1"),
            button_bg: get("button.background", "#4d4b61"),
            button_fg: get("button.foreground", "#ffffff"),
            confirmation_bg: get("successForeground", "#add991"),
            destructive_bg: theme
                .color("statusBarItem.errorBackground")
                .or_else(|| theme.color("editorError.foreground"))
                .and_then(hex_color)
                .unwrap_or_else(|| hex_color("#f28b82").unwrap()),
            git_modified: get("gitDecoration.modifiedResourceForeground", "#e5c07b"),
            git_untracked: get("gitDecoration.untrackedResourceForeground", "#7fc98a"),
            terminal_ansi: [
                "Black",
                "Red",
                "Green",
                "Yellow",
                "Blue",
                "Magenta",
                "Cyan",
                "White",
                "BrightBlack",
                "BrightRed",
                "BrightGreen",
                "BrightYellow",
                "BrightBlue",
                "BrightMagenta",
                "BrightCyan",
                "BrightWhite",
            ]
            .map(|name| {
                theme
                    .color(&format!("terminal.ansi{name}"))
                    .and_then(hex_color)
                    .and_then(|color| {
                        if let Color::Rgb(r, g, b) = color {
                            Some((r, g, b))
                        } else {
                            None
                        }
                    })
            }),
            tab_inactive: get("tab.inactiveBackground", "#30313d"),
            line_highlight: get("editor.lineHighlightBackground", "#414252"),
            bracket_match_bg: get("editorBracketMatch.background", "#4d4b61"),
            bracket_match_fg: accessible(
                get("editor.foreground", "#e6dbd1"),
                get("editorBracketMatch.background", "#4d4b61"),
            ),
            selection: get("editor.selectionBackground", "#4d4b61"),
            line_number: get("editorLineNumber.foreground", "#807d90"),
            line_number_active: get("editorLineNumber.activeForeground", "#e6dbd1"),
            terminal_bg: get("terminal.background", "#393a47"),
            terminal_fg: get("terminal.foreground", "#e6dbd1"),
            diff_add: get("gitDecoration.addedResourceForeground", "#7fc98a"),
            diff_delete: get("gitDecoration.deletedResourceForeground", "#f28b82"),
            warning: get("editorWarning.foreground", "#e5c07b"),
            error: get("editorError.foreground", "#f28b82"),
            modal_bg: get("editorWidget.background", "#30313d"),
            modal_fg: get("editorWidget.foreground", "#e6dbd1"),
            modal_border: get("editorWidget.border", "#565666"),
            input_bg: get("input.background", "#252630"),
            selection_bg: get("list.activeSelectionBackground", "#4d4b61"),
            selection_fg: get("list.activeSelectionForeground", "#ffffff"),
            description: get("descriptionForeground", "#9b98a8"),
            section_bg: get("sideBarSectionHeader.background", "#3a3b49"),
            section_fg: get("sideBarSectionHeader.foreground", "#e6dbd1"),
            section_focused_bg: get("list.activeSelectionBackground", "#4d4b61"),
            section_focused_fg: get("list.activeSelectionForeground", "#ffffff"),
        }
    }

    pub fn action(self, role: ActionRole, focused: bool, enabled: bool) -> ratatui::style::Style {
        use ratatui::style::{Modifier, Style};
        let background = match role {
            ActionRole::Confirmation => self.confirmation_bg,
            ActionRole::Destructive => self.destructive_bg,
            ActionRole::Primary => self.button_bg,
            ActionRole::Neutral => self.input_bg,
        };
        if !enabled {
            return Style::default()
                .fg(self.description)
                .bg(self.modal_bg)
                .add_modifier(Modifier::DIM);
        }
        let mut style = Style::default()
            .fg(accessible(self.button_fg, background))
            .bg(background);
        if focused {
            style = style.add_modifier(Modifier::BOLD | Modifier::UNDERLINED);
        }
        style
    }

    /// Header focus belongs to the header surface and label, not a pane box.
    #[must_use]
    pub fn host(mut self, focused: bool, terminal: bool) -> Self {
        if terminal {
            self.background = self.terminal_bg;
        }
        if focused {
            self.tab_inactive = focus_background(self.tab_inactive, self.border_focused);
            self.tab_active_fg = accessible(self.border_focused, self.tab_active);
            self.tab_inactive_fg = accessible(self.foreground, self.tab_inactive);
        }
        self
    }

    #[must_use]
    pub fn ultra_dusk() -> Self {
        let theme = tide_config::builtin::themes()
            .get("ultra-dusk")
            .cloned()
            .expect("Ultra Dusk is bundled");
        Self::from_theme(&theme)
    }
}

/// The theme colour a file icon takes, named after the terminal colour it comes from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IconTint {
    Plain,
    Red,
    Green,
    Yellow,
    Blue,
    Magenta,
    Cyan,
    /// Halfway between the theme's red and yellow, since terminals have no orange.
    Orange,
}

impl Palette {
    /// The colour for an icon of `tint` drawn on `background`. It comes from the theme's
    /// terminal colours, so it suits light and dark themes alike, and it is lightened or
    /// darkened to a 3:1 contrast where the theme's colour is too faint. `None` means the
    /// icon takes the text colour: a plain tint, or a theme without terminal colours.
    #[must_use]
    pub fn icon(&self, tint: IconTint, background: Color) -> Option<Color> {
        let ansi = |index: usize| self.terminal_ansi[index].map(|(r, g, b)| Color::Rgb(r, g, b));
        let color = match tint {
            IconTint::Plain => None,
            IconTint::Red => ansi(1),
            IconTint::Green => ansi(2),
            IconTint::Yellow => ansi(3),
            IconTint::Blue => ansi(4),
            IconTint::Magenta => ansi(5),
            IconTint::Cyan => ansi(6),
            IconTint::Orange => ansi(1)
                .zip(ansi(3))
                .map(|(red, yellow)| blend(red, yellow, 0.5)),
        };
        color.map(|color| with_contrast(color, background, 3.0))
    }
}

#[must_use]
pub fn hex_color(value: &str) -> Option<Color> {
    let value = value.trim().trim_start_matches('#');
    if !value.is_ascii() {
        return None;
    }
    let parse = |text: &str| u8::from_str_radix(text, 16).ok();
    match value.len() {
        3 => {
            let mut chars = value.chars();
            let r = u8::try_from(chars.next()?.to_digit(16)?).ok()? * 17;
            let g = u8::try_from(chars.next()?.to_digit(16)?).ok()? * 17;
            let b = u8::try_from(chars.next()?.to_digit(16)?).ok()? * 17;
            Some(Color::Rgb(r, g, b))
        }
        6 => Some(Color::Rgb(
            parse(&value[0..2])?,
            parse(&value[2..4])?,
            parse(&value[4..6])?,
        )),
        _ => None,
    }
}

fn focus_background(base: Color, accent: Color) -> Color {
    for step in 3..=20 {
        let candidate = blend(base, accent, f64::from(step) / 20.0);
        if contrast(candidate, base) >= 1.2 {
            return candidate;
        }
    }
    let endpoint =
        if contrast(Color::Rgb(0, 0, 0), base) > contrast(Color::Rgb(255, 255, 255), base) {
            Color::Rgb(0, 0, 0)
        } else {
            Color::Rgb(255, 255, 255)
        };
    for step in 1..=20 {
        let candidate = blend(base, endpoint, f64::from(step) / 20.0);
        if contrast(candidate, base) >= 1.2 {
            return candidate;
        }
    }
    endpoint
}
fn blend(a: Color, b: Color, amount: f64) -> Color {
    match (a, b) {
        (Color::Rgb(ar, ag, ab), Color::Rgb(br, bg, bb)) => {
            let mix = |x: u8, y: u8| {
                // Clamping keeps an out-of-range `amount` at the nearest
                // channel bound; NaN maps to 0.
                (f64::from(x) * (1.0 - amount) + f64::from(y) * amount)
                    .round()
                    .clamp(0.0, 255.0)
                    .to_u8()
                    .unwrap_or(0)
            };
            Color::Rgb(mix(ar, br), mix(ag, bg), mix(ab, bb))
        }
        _ => a,
    }
}
fn luminance(color: Color) -> f64 {
    if let Color::Rgb(r, g, b) = color {
        let linear = |v: u8| {
            let v = f64::from(v) / 255.0;
            if v <= 0.04045 {
                v / 12.92
            } else {
                ((v + 0.055) / 1.055).powf(2.4)
            }
        };
        0.2126 * linear(r) + 0.7152 * linear(g) + 0.0722 * linear(b)
    } else {
        0.0
    }
}
#[cfg(test)]
pub(crate) fn contrast_ratio(a: Color, b: Color) -> f64 {
    contrast(a, b)
}
fn contrast(a: Color, b: Color) -> f64 {
    let (a, b) = (luminance(a), luminance(b));
    (a.max(b) + 0.05) / (a.min(b) + 0.05)
}
pub(crate) fn accessible(foreground: Color, background: Color) -> Color {
    with_contrast(foreground, background, 4.5)
}
/// Moves `foreground` toward black or white, whichever contrasts more with `background`,
/// until the two reach `ratio`.
fn with_contrast(foreground: Color, background: Color, ratio: f64) -> Color {
    if contrast(foreground, background) >= ratio {
        return foreground;
    }
    let endpoint = if contrast(Color::Rgb(0, 0, 0), background)
        > contrast(Color::Rgb(255, 255, 255), background)
    {
        Color::Rgb(0, 0, 0)
    } else {
        Color::Rgb(255, 255, 255)
    };
    for step in 1..=20 {
        let candidate = blend(foreground, endpoint, f64::from(step) / 20.0);
        if contrast(candidate, background) >= ratio {
            return candidate;
        }
    }
    endpoint
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn action_roles_keep_meaning_and_legible_focus_in_every_bundled_theme() {
        let catalog = tide_config::builtin::themes();
        for theme in catalog.iter() {
            let name = &theme.name;
            let palette = Palette::from_theme(theme);
            assert!(
                contrast(palette.bracket_match_fg, palette.bracket_match_bg) >= 4.5,
                "{name} matching delimiters"
            );
            let focused = palette.host(true, false);
            assert!(
                contrast(focused.tab_inactive, palette.tab_inactive) >= 1.2,
                "{name} header focus"
            );
            assert!(
                contrast(focused.tab_active_fg, focused.tab_active) >= 4.5,
                "{name} active label"
            );
            for role in [
                ActionRole::Primary,
                ActionRole::Confirmation,
                ActionRole::Destructive,
                ActionRole::Neutral,
            ] {
                let normal = palette.action(role, false, true);
                let focused = palette.action(role, true, true);
                assert_eq!(normal.bg, focused.bg, "{name} {role:?}");
                assert!(
                    contrast(normal.fg.unwrap(), normal.bg.unwrap()) >= 4.5,
                    "{name} {role:?}"
                );
                assert_ne!(normal.add_modifier, focused.add_modifier);
            }
            assert_ne!(
                palette.action(ActionRole::Confirmation, false, true).bg,
                palette.action(ActionRole::Destructive, false, true).bg,
                "{name}"
            );
        }
    }

    #[test]
    fn parses_short_and_long_hex() {
        assert_eq!(hex_color("#fff"), Some(Color::Rgb(255, 255, 255)));
        assert_eq!(hex_color("c0abf5"), Some(Color::Rgb(192, 171, 245)));
        assert_eq!(hex_color("nope"), None);
        assert_eq!(hex_color("#€abc"), None);
    }

    #[test]
    fn ultra_dusk_palette_loads() {
        let palette = Palette::ultra_dusk();
        assert_ne!(palette.background, palette.foreground);
    }
}
