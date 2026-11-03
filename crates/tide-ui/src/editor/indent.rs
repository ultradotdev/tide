//! Per-file indentation: guessed from the text, read from `.editorconfig`, or taken
//! from settings, in that order of increasing priority.

use crate::editorconfig::{EditorConfig, IndentSize};

/// How far into a file detection looks.
const SAMPLE_LINES: usize = 1000;

/// The indentation a file's own text suggests.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Detected {
    Tabs,
    Spaces(usize),
}

/// Guesses the indentation of `text` from its first lines: tabs or spaces by the
/// majority of indented lines, and the width from the most common change in
/// indentation between neighbouring lines. `None` when the text has too little
/// indentation to tell.
#[must_use]
pub fn detect(text: &str) -> Option<Detected> {
    let mut tab_lines = 0usize;
    let mut space_lines = 0usize;
    // How often each step in space indentation (1–8 columns) occurs between neighbouring lines.
    let mut steps = [0usize; 9];
    let mut previous: Option<usize> = None;
    for line in text.lines().take(SAMPLE_LINES) {
        let content = line.trim_start_matches([' ', '\t']);
        if content.is_empty() {
            continue;
        }
        let indent = &line[..line.len() - content.len()];
        // Block-comment continuations (" * text") sit one column in, which says nothing
        // about the indent width.
        let comment_continuation = content.starts_with('*');
        if indent.starts_with('\t') {
            tab_lines += 1;
            previous = None;
            continue;
        }
        if indent.contains('\t') {
            // Mixed indentation can't be measured reliably.
            previous = None;
            continue;
        }
        if comment_continuation {
            continue;
        }
        let width = indent.len();
        if width > 0 {
            space_lines += 1;
        }
        if let Some(before) = previous {
            let step = width.abs_diff(before);
            if (1..steps.len()).contains(&step) {
                steps[step] += 1;
            }
        }
        previous = Some(width);
    }
    if tab_lines == 0 && space_lines == 0 {
        return None;
    }
    if tab_lines > space_lines {
        return Some(Detected::Tabs);
    }
    let total: usize = steps.iter().sum();
    if total == 0 {
        return None;
    }
    // Prefer common widths on a tie. A one-column step only counts when it clearly
    // dominates, since alignment and wrapped arguments produce them in any file.
    let best = [4, 2, 8, 3, 6, 5, 7]
        .into_iter()
        .max_by_key(|&width| (steps[width], std::cmp::Reverse(rank(width))))
        .filter(|&width| steps[width] > 0);
    match best {
        Some(width) if steps[1] * 2 <= total || steps[width] >= steps[1] => {
            Some(Detected::Spaces(width))
        }
        _ if steps[1] * 2 > total => Some(Detected::Spaces(1)),
        _ => None,
    }
}

fn rank(width: usize) -> usize {
    [4, 2, 8, 3, 6, 5, 7]
        .iter()
        .position(|&w| w == width)
        .unwrap_or(usize::MAX)
}

/// Where a file's effective indentation came from, as the status bar explains it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum IndentSource {
    #[default]
    Settings,
    Detected,
    EditorConfig,
}

/// The indentation an editor uses: what Tab and auto-indent insert, and how wide a
/// tab character is drawn.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Indent {
    pub insert_spaces: bool,
    /// Columns per indent level when indenting with spaces.
    pub size: usize,
    /// Columns a tab character occupies.
    pub tab_width: usize,
    pub source: IndentSource,
}

impl Indent {
    /// The settings alone: `editor.tabSize` and `editor.insertSpaces`.
    #[must_use]
    pub fn from_settings(tab_size: usize, insert_spaces: bool) -> Self {
        let tab_size = tab_size.clamp(1, 32);
        Self {
            insert_spaces,
            size: tab_size,
            tab_width: tab_size,
            source: IndentSource::Settings,
        }
    }

    /// What one indent level inserts.
    #[must_use]
    pub fn unit(&self) -> String {
        if self.insert_spaces {
            " ".repeat(self.size.clamp(1, 32))
        } else {
            "\t".into()
        }
    }

    /// The status bar label, such as "Spaces: 4" or "Tab Size: 8".
    #[must_use]
    pub fn label(&self) -> String {
        if self.insert_spaces {
            format!("Spaces: {}", self.size)
        } else {
            format!("Tab Size: {}", self.tab_width)
        }
    }
}

/// Settings that apply to every file before a file's own evidence.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct IndentDefaults {
    pub tab_size: usize,
    pub insert_spaces: bool,
    /// `editor.detectIndentation`.
    pub detect: bool,
}

impl Default for IndentDefaults {
    fn default() -> Self {
        Self {
            tab_size: 2,
            insert_spaces: true,
            detect: true,
        }
    }
}

/// Combines `.editorconfig`, the detected indentation and the settings. Each
/// `.editorconfig` property wins over detection, which wins over settings.
#[must_use]
pub fn resolve(
    defaults: IndentDefaults,
    config: &EditorConfig,
    detected: Option<Detected>,
) -> Indent {
    let mut indent = Indent::from_settings(defaults.tab_size, defaults.insert_spaces);
    if defaults.detect
        && let Some(detected) = detected
    {
        indent.source = IndentSource::Detected;
        match detected {
            Detected::Tabs => indent.insert_spaces = false,
            Detected::Spaces(width) => {
                indent.insert_spaces = true;
                indent.size = width;
                indent.tab_width = width;
            }
        }
    }
    let config_size = match config.indent_size {
        Some(IndentSize::Columns(columns)) => Some(columns),
        Some(IndentSize::Tab) | None => config.tab_width,
    };
    let config_tab = config.tab_width.or(match config.indent_size {
        Some(IndentSize::Columns(columns)) => Some(columns),
        _ => None,
    });
    if let Some(spaces) = config.indent_style_spaces {
        indent.insert_spaces = spaces;
        indent.source = IndentSource::EditorConfig;
    }
    if let Some(size) = config_size {
        indent.size = size.clamp(1, 32);
        indent.source = IndentSource::EditorConfig;
    }
    if let Some(width) = config_tab {
        indent.tab_width = width.clamp(1, 32);
        indent.source = IndentSource::EditorConfig;
    }
    indent
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lines(indents: &[&str]) -> String {
        indents
            .iter()
            .map(|indent| format!("{indent}x\n"))
            .collect()
    }

    #[test]
    fn four_space_rust_is_detected_as_four() {
        let text = "fn main() {\n    let a = 1;\n    if a {\n        call(\n            a,\n        );\n    }\n}\n";
        assert_eq!(detect(text), Some(Detected::Spaces(4)));
    }

    #[test]
    fn two_space_files_are_detected_as_two() {
        let text = lines(&["", "  ", "    ", "      ", "    ", "  ", "", "  ", "    "]);
        assert_eq!(detect(&text), Some(Detected::Spaces(2)));
    }

    #[test]
    fn tabs_win_by_majority() {
        let text = lines(&["", "\t", "\t\t", "\t", "    ", "\t\t\t"]);
        assert_eq!(detect(&text), Some(Detected::Tabs));
    }

    #[test]
    fn deep_dedents_do_not_outvote_the_indent_step() {
        // One level in at a time, then back out several levels at once.
        let text = lines(&[
            "",
            "    ",
            "        ",
            "            ",
            "",
            "    ",
            "        ",
            "",
        ]);
        assert_eq!(detect(&text), Some(Detected::Spaces(4)));
    }

    #[test]
    fn block_comment_continuations_are_ignored() {
        let text = "/**\n * Docs\n * more\n */\nfn a() {\n    b();\n    c();\n}\n";
        assert_eq!(detect(text), Some(Detected::Spaces(4)));
    }

    #[test]
    fn unindented_or_empty_text_is_undecided() {
        assert_eq!(detect(""), None);
        assert_eq!(detect("a\nb\nc\n"), None);
        assert_eq!(detect("\n\n   \n"), None);
    }

    #[test]
    fn stray_alignment_spaces_do_not_make_a_one_space_indent() {
        let text =
            "fn a(b: u8,\n     c: u8) {\n    d();\n    e();\n    if f {\n        g();\n    }\n}\n";
        assert_eq!(detect(text), Some(Detected::Spaces(4)));
    }

    #[test]
    fn editorconfig_beats_detection_which_beats_settings() {
        let defaults = IndentDefaults {
            tab_size: 2,
            insert_spaces: true,
            detect: true,
        };
        let none = EditorConfig::default();
        assert_eq!(
            resolve(defaults, &none, None),
            Indent::from_settings(2, true)
        );
        let detected = resolve(defaults, &none, Some(Detected::Spaces(4)));
        assert_eq!(
            (detected.size, detected.source),
            (4, IndentSource::Detected)
        );
        assert_eq!(detected.label(), "Spaces: 4");
        let config = EditorConfig {
            indent_style_spaces: Some(true),
            indent_size: Some(IndentSize::Columns(3)),
            ..EditorConfig::default()
        };
        let configured = resolve(defaults, &config, Some(Detected::Spaces(4)));
        assert_eq!(
            (configured.size, configured.tab_width, configured.source),
            (3, 3, IndentSource::EditorConfig)
        );
        let tabs = EditorConfig {
            indent_style_spaces: Some(false),
            tab_width: Some(8),
            ..EditorConfig::default()
        };
        let resolved = resolve(defaults, &tabs, Some(Detected::Spaces(4)));
        assert!(!resolved.insert_spaces);
        assert_eq!(resolved.label(), "Tab Size: 8");
        assert_eq!(resolved.unit(), "\t");
    }

    #[test]
    fn detection_can_be_turned_off() {
        let defaults = IndentDefaults {
            tab_size: 2,
            insert_spaces: true,
            detect: false,
        };
        let resolved = resolve(defaults, &EditorConfig::default(), Some(Detected::Tabs));
        assert_eq!(resolved, Indent::from_settings(2, true));
    }

    #[test]
    fn an_editorconfig_property_overrides_only_what_it_sets() {
        // indent_style alone keeps the detected width.
        let config = EditorConfig {
            indent_style_spaces: Some(true),
            ..EditorConfig::default()
        };
        let resolved = resolve(
            IndentDefaults::default(),
            &config,
            Some(Detected::Spaces(4)),
        );
        assert_eq!((resolved.size, resolved.insert_spaces), (4, true));
        // indent_size = tab takes tab_width.
        let config = EditorConfig {
            indent_size: Some(IndentSize::Tab),
            tab_width: Some(6),
            ..EditorConfig::default()
        };
        let resolved = resolve(IndentDefaults::default(), &config, None);
        assert_eq!((resolved.size, resolved.tab_width), (6, 6));
    }
}
