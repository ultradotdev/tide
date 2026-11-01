use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};

use syntect::easy::HighlightLines;
use syntect::highlighting::{
    Color, FontStyle, ScopeSelectors, StyleModifier, Theme, ThemeItem, ThemeSettings,
};
use syntect::parsing::SyntaxSet;
use syntect::util::LinesWithEndings;

use tide_config::Theme as TideTheme;

/// One styled run of text on a line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HighlightSpan {
    pub text: String,
    pub fg: Option<String>,
    pub bg: Option<String>,
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
}

/// Syntax highlighting adapter backed by syntect and two-face's grammar bundle.
pub struct SyntaxHighlighter {
    theme: Arc<Theme>,
    documents: crate::syntax_document::DocumentHighlighter,
    lines: Mutex<LineCache>,
}

#[derive(Default)]
struct LineCache {
    entries: HashMap<(String, String), Vec<HighlightSpan>>,
    bytes: usize,
}

const CACHE_BYTES: usize = 2 * 1024 * 1024;
const CACHE_LINES: usize = 512;
const MAX_HIGHLIGHT_LINE_BYTES: usize = 16 * 1024;

pub(crate) fn shared_grammars() -> Arc<SyntaxSet> {
    static GRAMMARS: OnceLock<Arc<SyntaxSet>> = OnceLock::new();
    GRAMMARS
        .get_or_init(|| Arc::new(two_face::syntax::extra_newlines()))
        .clone()
}

/// Full notices for the grammar assets embedded by our pinned two-face build.
#[must_use]
pub fn embedded_syntax_notices() -> String {
    use std::fmt::Write;
    let acknowledgements = two_face::acknowledgement::listing();
    let mut licenses = acknowledgements.for_syntaxes().iter().collect::<Vec<_>>();
    licenses.sort_by(|a, b| a.rel_path.cmp(&b.rel_path));
    let mut output = String::from("## Embedded syntax definitions\n\n");
    for license in licenses {
        writeln!(
            output,
            "### {}\n\n````text\n{}\n````\n",
            license.rel_path.display(),
            license.text.trim_end()
        )
        .expect("writing to a String");
    }
    output
}

impl SyntaxHighlighter {
    #[must_use]
    pub fn new(theme: &TideTheme) -> Self {
        Self {
            theme: Arc::new(syntect_theme_from_tide(theme)),
            documents: Default::default(),
            lines: Mutex::default(),
        }
    }

    pub fn take_changed(&self) -> bool {
        self.documents.take_changed()
    }
    pub fn document_revision(&self, id: u64) -> u64 {
        self.documents.revision(id)
    }
    pub fn document_line(&self, id: u64, revision: u64, line: usize) -> Option<Vec<HighlightSpan>> {
        self.documents.line(id, revision, line)
    }
    pub fn request_document(
        &self,
        id: u64,
        revision: u64,
        changed: usize,
        text: tide_core::TextSnapshot,
        extension: &str,
        lines: Vec<usize>,
    ) {
        self.documents.request(
            id,
            revision,
            changed,
            text,
            extension,
            lines,
            shared_grammars,
            self.theme.clone(),
        );
    }
    pub fn set_theme(&mut self, theme: &TideTheme) {
        self.theme = Arc::new(syntect_theme_from_tide(theme));
        self.documents.reset();
        *self.lines.get_mut().unwrap() = LineCache::default();
    }

    #[must_use]
    pub fn theme_scope_count(&self) -> usize {
        self.theme.scopes.len()
    }

    /// Highlight a single line. Syntax definitions are selected by file extension.
    #[must_use]
    pub fn highlight_line(&self, line: &str, extension: &str) -> Vec<HighlightSpan> {
        // Keep pathological/minified lines off the synchronous grammar parser.
        if line.len() > MAX_HIGHLIGHT_LINE_BYTES {
            return plain_span(line);
        }
        let key = (extension.to_owned(), line.to_owned());
        if let Some(spans) = self.lines.lock().unwrap().entries.get(&key) {
            return spans.clone();
        }
        let syntax_set = shared_grammars();
        let syntax = syntax_set
            .find_syntax_by_extension(extension)
            .unwrap_or_else(|| syntax_set.find_syntax_plain_text());
        let mut highlighter = HighlightLines::new(syntax, &self.theme);
        let spans: Vec<HighlightSpan> = highlighter.highlight_line(line, &syntax_set).map_or_else(
            |_| plain_span(line),
            |spans| spans.into_iter().map(convert_span).collect(),
        );
        let bytes = key.0.len()
            + key.1.len()
            + spans.iter().map(|span| span.text.len() + 96).sum::<usize>();
        if bytes <= CACHE_BYTES {
            let mut cache = self.lines.lock().unwrap();
            if cache.entries.len() >= CACHE_LINES || cache.bytes + bytes > CACHE_BYTES {
                *cache = LineCache::default();
            }
            cache.bytes += bytes;
            cache.entries.insert(key, spans.clone());
        }
        spans
    }

    /// Highlight a multi-line document while preserving syntax state.
    #[must_use]
    pub fn highlight(&self, text: &str, extension: &str) -> Vec<Vec<HighlightSpan>> {
        let syntax_set = shared_grammars();
        let syntax = syntax_set
            .find_syntax_by_extension(extension)
            .unwrap_or_else(|| syntax_set.find_syntax_plain_text());
        let mut highlighter = HighlightLines::new(syntax, &self.theme);
        let mut lines = Vec::new();
        for line in LinesWithEndings::from(text) {
            match highlighter.highlight_line(line, &syntax_set) {
                Ok(spans) => lines.push(spans.into_iter().map(convert_span).collect()),
                Err(_) => lines.push(plain_span(line)),
            }
        }
        lines
    }
}

pub(crate) fn convert_span((style, text): (syntect::highlighting::Style, &str)) -> HighlightSpan {
    HighlightSpan {
        text: text.to_owned(),
        fg: Some(color_hex(style.foreground)),
        bg: Some(color_hex(style.background)),
        bold: style.font_style.contains(FontStyle::BOLD),
        italic: style.font_style.contains(FontStyle::ITALIC),
        underline: style.font_style.contains(FontStyle::UNDERLINE),
    }
}

pub(crate) fn plain_span(text: &str) -> Vec<HighlightSpan> {
    vec![HighlightSpan {
        text: text.to_owned(),
        fg: None,
        bg: None,
        bold: false,
        italic: false,
        underline: false,
    }]
}

/// Convert a Tide semantic theme into a syntect theme.
///
/// Tide's theme files use TextMate-style token scopes. Syntect's selector engine
/// understands the same scope language, so the conversion is a data mapping
/// rather than a second theme implementation.
#[must_use]
pub fn syntect_theme_from_tide(theme: &TideTheme) -> Theme {
    let settings = ThemeSettings {
        foreground: theme.color("editor.foreground").and_then(parse_hex),
        background: theme.color("editor.background").and_then(parse_hex),
        caret: theme.color("editorCursor.foreground").and_then(parse_hex),
        line_highlight: theme
            .color("editor.lineHighlightBackground")
            .and_then(parse_hex),
        ..ThemeSettings::default()
    };

    let mut scopes = Vec::with_capacity(theme.token_colors.len());
    for rule in &theme.token_colors {
        let scope = rule.scope.as_slice().join(", ");
        let Ok(selectors) = scope.parse::<ScopeSelectors>() else {
            continue;
        };
        let font_style =
            rule.settings
                .font_style
                .as_deref()
                .map_or_else(FontStyle::empty, |value| {
                    let mut style = FontStyle::empty();
                    if value.contains("bold") {
                        style |= FontStyle::BOLD;
                    }
                    if value.contains("italic") {
                        style |= FontStyle::ITALIC;
                    }
                    if value.contains("underline") {
                        style |= FontStyle::UNDERLINE;
                    }
                    style
                });
        scopes.push(ThemeItem {
            scope: selectors,
            style: StyleModifier {
                foreground: rule.settings.foreground.as_deref().and_then(parse_hex),
                background: rule.settings.background.as_deref().and_then(parse_hex),
                font_style: Some(font_style),
            },
        });
    }

    Theme {
        name: Some(theme.name.clone()),
        author: None,
        settings,
        scopes,
    }
}

fn parse_hex(value: &str) -> Option<Color> {
    let value = value.trim().trim_start_matches('#');
    if !value.is_ascii() {
        return None;
    }
    let expand = |c: char| c.to_digit(16).and_then(|n| u8::try_from(n * 17).ok());
    let parse = |text: &str| u8::from_str_radix(text, 16).ok();
    match value.len() {
        3 => {
            let mut chars = value.chars();
            Some(Color {
                r: expand(chars.next()?)?,
                g: expand(chars.next()?)?,
                b: expand(chars.next()?)?,
                a: 255,
            })
        }
        6 => Some(Color {
            r: parse(&value[0..2])?,
            g: parse(&value[2..4])?,
            b: parse(&value[4..6])?,
            a: 255,
        }),
        8 => Some(Color {
            r: parse(&value[0..2])?,
            g: parse(&value[2..4])?,
            b: parse(&value[4..6])?,
            a: parse(&value[6..8])?,
        }),
        _ => None,
    }
}

fn color_hex(color: Color) -> String {
    format!("#{:02x}{:02x}{:02x}", color.r, color.g, color.b)
}

#[cfg(test)]
mod tests {
    #[test]
    fn document_worker_preserves_multiline_state_invalidates_edits_and_changes_theme() {
        let themes = tide_config::builtin::themes();
        let mut syntax = super::SyntaxHighlighter::new(themes.get("ultra-dusk").unwrap());
        let text = format!(
            "{}/* start\ninside comment\n*/\nfn main() {{}}\n",
            "fn padding() {}\n".repeat(600)
        );
        let doc = tide_core::Document::new(&text);
        let wait = |syntax: &super::SyntaxHighlighter, revision: u64, line: usize| {
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
            loop {
                if let Some(spans) = syntax.document_line(77, revision, line) {
                    return spans;
                }
                assert!(
                    std::time::Instant::now() < deadline,
                    "syntax worker did not finish"
                );
                std::thread::sleep(std::time::Duration::from_millis(5));
            }
        };
        let started = std::time::Instant::now();
        syntax.request_document(77, 1, 0, doc.text_snapshot(), "rs", vec![601, 603]);
        assert!(started.elapsed() < std::time::Duration::from_millis(100));
        let actual = wait(&syntax, 1, 601);
        assert_eq!(actual, syntax.highlight(&text, "rs")[601]);
        assert_ne!(actual, syntax.highlight_line("inside comment\n", "rs"));
        let changed = text.replace("/* start", "// start");
        let doc = tide_core::Document::new(&changed);
        syntax.request_document(77, 2, 600, doc.text_snapshot(), "rs", vec![601, 603]);
        assert_eq!(wait(&syntax, 2, 601), syntax.highlight(&changed, "rs")[601]);
        syntax.set_theme(themes.get("dracula").unwrap());
        assert!(syntax.document_line(77, 2, 601).is_none());
        syntax.request_document(77, 2, 0, doc.text_snapshot(), "rs", vec![601, 603]);
        assert_eq!(wait(&syntax, 2, 603), syntax.highlight(&changed, "rs")[603]);
    }

    use std::path::Path;

    use super::*;

    fn ultra_dusk() -> TideTheme {
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../tests/fixtures/rust-parity/themes-defaults.json");
        let catalog = tide_config::ThemeCatalog::from_path(path).unwrap();
        catalog.get("ultra-dusk").unwrap().clone()
    }

    #[test]
    fn converts_textmate_theme_into_syntect_scopes() {
        let theme = syntect_theme_from_tide(&ultra_dusk());
        assert_eq!(theme.name.as_deref(), Some("Ultra Dusk"));
        assert!(
            theme.scopes.len() >= 30,
            "expected most token rules to convert, got {}",
            theme.scopes.len()
        );
        assert!(theme.settings.foreground.is_some());
        assert!(theme.settings.background.is_some());
    }

    #[test]
    fn highlights_rust_sample() {
        let highlighter = SyntaxHighlighter::new(&ultra_dusk());
        let source = std::fs::read_to_string(
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../tests/fixtures/syntax-samples/test.rs"),
        )
        .unwrap();
        let lines = highlighter.highlight(&source, "rs");
        assert!(!lines.is_empty());
        assert!(lines.iter().flatten().any(|span| !span.text.is_empty()));
        assert!(lines.iter().flatten().any(|span| span.fg.is_some()));
    }

    #[test]
    fn highlights_typescript_sample() {
        let highlighter = SyntaxHighlighter::new(&ultra_dusk());
        let line = "const answer: number = 42;";
        let spans = highlighter.highlight_line(line, "ts");
        assert_eq!(
            spans
                .iter()
                .map(|span| span.text.as_str())
                .collect::<String>(),
            line
        );
        assert!(
            spans
                .iter()
                .any(|span| span.fg.as_deref() != Some("#ffffff"))
        );
    }

    #[test]
    fn line_cache_is_bounded_and_theme_changes_invalidate_styles() {
        let mut highlighter = SyntaxHighlighter::new(&ultra_dusk());
        let source = "pub fn answer() -> usize { 42 }";
        let original = highlighter.highlight_line(source, "rs");
        assert_eq!(highlighter.highlight_line(source, "rs"), original);
        for number in 0..600 {
            let _ = highlighter.highlight_line(&format!("let value_{number} = {number};"), "rs");
        }
        assert!(highlighter.lines.lock().unwrap().entries.len() <= CACHE_LINES);
        assert!(highlighter.lines.lock().unwrap().bytes <= CACHE_BYTES);
        let mut theme = ultra_dusk();
        theme.token_colors.clear();
        highlighter.set_theme(&theme);
        assert!(highlighter.lines.lock().unwrap().entries.is_empty());
        assert_ne!(highlighter.highlight_line(source, "rs"), original);
        let long = "x".repeat(MAX_HIGHLIGHT_LINE_BYTES + 1);
        assert_eq!(highlighter.highlight_line(&long, "rs"), plain_span(&long));
    }

    #[test]
    fn unknown_language_falls_back_to_plain_text() {
        let highlighter = SyntaxHighlighter::new(&ultra_dusk());
        let spans = highlighter.highlight_line("plain text", "not-a-language");
        assert_eq!(spans.len(), 1);
        assert_eq!(spans[0].text, "plain text");
    }
}
