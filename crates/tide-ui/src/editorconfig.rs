//! A small reader for [EditorConfig](https://editorconfig.org) files.
//!
//! Tide looks for `.editorconfig` files from a file's folder up to the first one that
//! says `root = true`, or the filesystem root. Closer files win over farther ones, and
//! later sections over earlier ones. Unknown properties are ignored.

use std::path::{Path, PathBuf};

use regex::Regex;

/// Files larger than this are not read; real `.editorconfig` files are tiny.
const MAX_FILE_BYTES: u64 = 256 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IndentSize {
    Columns(usize),
    /// `indent_size = tab`: use `tab_width`.
    Tab,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EndOfLine {
    Lf,
    Crlf,
    Cr,
}

impl EndOfLine {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Lf => "\n",
            Self::Crlf => "\r\n",
            Self::Cr => "\r",
        }
    }
}

/// The properties Tide applies, as resolved for one file. `None` leaves the setting
/// or the file's own evidence in charge.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct EditorConfig {
    /// `indent_style`: `Some(true)` for spaces, `Some(false)` for tabs.
    pub indent_style_spaces: Option<bool>,
    pub indent_size: Option<IndentSize>,
    pub tab_width: Option<usize>,
    pub end_of_line: Option<EndOfLine>,
    pub insert_final_newline: Option<bool>,
    pub trim_trailing_whitespace: Option<bool>,
    /// `charset`, lowercased; Tide reads and writes UTF-8 only.
    pub charset: Option<String>,
}

impl EditorConfig {
    /// The properties for `path`, read from the `.editorconfig` files above it.
    /// Unreadable or malformed files contribute nothing.
    #[must_use]
    pub fn for_file(path: &Path) -> Self {
        let mut files = Vec::new();
        let mut directory = path.parent();
        while let Some(dir) = directory {
            let candidate = dir.join(".editorconfig");
            if let Some(text) = read_small(&candidate) {
                let parsed = parse(&text);
                let root = parsed.root;
                files.push((dir.to_path_buf(), parsed));
                if root {
                    break;
                }
            }
            directory = dir.parent();
        }
        let mut properties: Vec<(String, String)> = Vec::new();
        // Farthest first, so nearer files and later sections overwrite.
        for (dir, file) in files.iter().rev() {
            for section in &file.sections {
                if section.matches(dir, path) {
                    for (key, value) in &section.properties {
                        properties.retain(|(known, _)| known != key);
                        properties.push((key.clone(), value.clone()));
                    }
                }
            }
        }
        Self::from_properties(&properties)
    }

    fn from_properties(properties: &[(String, String)]) -> Self {
        let mut config = Self::default();
        for (key, value) in properties {
            let value = value.as_str();
            match key.as_str() {
                "indent_style" => {
                    config.indent_style_spaces = match value {
                        "space" => Some(true),
                        "tab" => Some(false),
                        _ => None,
                    }
                }
                "indent_size" => {
                    config.indent_size = match value {
                        "tab" => Some(IndentSize::Tab),
                        _ => columns(value).map(IndentSize::Columns),
                    }
                }
                "tab_width" => config.tab_width = columns(value),
                "end_of_line" => {
                    config.end_of_line = match value {
                        "lf" => Some(EndOfLine::Lf),
                        "crlf" => Some(EndOfLine::Crlf),
                        "cr" => Some(EndOfLine::Cr),
                        _ => None,
                    }
                }
                "insert_final_newline" => config.insert_final_newline = boolean(value),
                "trim_trailing_whitespace" => config.trim_trailing_whitespace = boolean(value),
                "charset" => {
                    config.charset = (value != "unset" && !value.is_empty()).then(|| value.into())
                }
                _ => {}
            }
        }
        config
    }

    /// Whether any property applies to the file.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        *self == Self::default()
    }
}

fn columns(value: &str) -> Option<usize> {
    value.parse::<usize>().ok().filter(|n| (1..=32).contains(n))
}

fn boolean(value: &str) -> Option<bool> {
    match value {
        "true" => Some(true),
        "false" => Some(false),
        _ => None,
    }
}

fn read_small(path: &Path) -> Option<String> {
    let metadata = std::fs::metadata(path).ok()?;
    if !metadata.is_file() || metadata.len() > MAX_FILE_BYTES {
        return None;
    }
    std::fs::read_to_string(path).ok()
}

#[derive(Debug, Default)]
struct ParsedFile {
    root: bool,
    sections: Vec<Section>,
}

#[derive(Debug)]
struct Section {
    pattern: Option<Regex>,
    properties: Vec<(String, String)>,
}

impl Section {
    fn matches(&self, directory: &Path, path: &Path) -> bool {
        let Some(pattern) = &self.pattern else {
            return false;
        };
        let Ok(relative) = path.strip_prefix(directory) else {
            return false;
        };
        let relative: PathBuf = relative.components().collect();
        let Some(relative) = relative.to_str() else {
            return false;
        };
        pattern.is_match(&relative.replace('\\', "/"))
    }
}

fn parse(text: &str) -> ParsedFile {
    let mut file = ParsedFile::default();
    let mut current: Option<Section> = None;
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') || line.starts_with(';') {
            continue;
        }
        if let Some(header) = line.strip_prefix('[') {
            if let Some(section) = current.take() {
                file.sections.push(section);
            }
            let glob = header.strip_suffix(']').unwrap_or(header);
            current = Some(Section {
                pattern: glob_to_regex(glob),
                properties: Vec::new(),
            });
            continue;
        }
        let Some((key, value)) = line.split_once(['=', ':']) else {
            continue;
        };
        let key = key.trim().to_ascii_lowercase();
        let value = value.trim();
        // Values are case-insensitive for every property Tide reads.
        let value = value.to_ascii_lowercase();
        match &mut current {
            Some(section) => section.properties.push((key, value)),
            None if key == "root" => file.root = value == "true",
            None => {}
        }
    }
    if let Some(section) = current {
        file.sections.push(section);
    }
    file
}

/// Translates an EditorConfig section glob to a regex over `/`-separated paths
/// relative to the `.editorconfig` file's folder.
fn glob_to_regex(glob: &str) -> Option<Regex> {
    let glob = glob.trim();
    if glob.is_empty() {
        return None;
    }
    // A glob without a slash matches the file name in any folder below.
    let (anchored, glob) = match glob.strip_prefix('/') {
        Some(rest) => (true, rest),
        None => (glob.contains('/'), glob),
    };
    let mut out = String::from("^");
    if !anchored {
        out.push_str("(?:.*/)?");
    }
    let chars: Vec<char> = glob.chars().collect();
    let mut index = 0;
    let mut braces = 0usize;
    while index < chars.len() {
        let ch = chars[index];
        match ch {
            '\\' if index + 1 < chars.len() => {
                index += 1;
                out.push_str(&regex::escape(&chars[index].to_string()));
            }
            '*' if chars.get(index + 1) == Some(&'*') => {
                index += 1;
                out.push_str(".*");
            }
            '*' => out.push_str("[^/]*"),
            '?' => out.push_str("[^/]"),
            '[' => {
                let close = chars[index + 1..]
                    .iter()
                    .position(|&c| c == ']')
                    .map(|offset| index + 1 + offset);
                let Some(close) = close.filter(|&close| close > index + 1) else {
                    out.push_str("\\[");
                    index += 1;
                    continue;
                };
                let body: String = chars[index + 1..close].iter().collect();
                if body.contains('/') {
                    out.push_str("\\[");
                    index += 1;
                    continue;
                }
                let (negated, body) = match body.strip_prefix('!') {
                    Some(rest) => (true, rest.to_owned()),
                    None => (false, body),
                };
                out.push('[');
                if negated {
                    out.push('^');
                }
                for c in body.chars() {
                    if matches!(c, '\\' | '[' | ']' | '^' | '&' | '~') {
                        out.push('\\');
                    }
                    out.push(c);
                }
                out.push(']');
                index = close;
            }
            '{' => {
                if let Some((range, end)) = numeric_range(&chars, index) {
                    out.push_str(&range);
                    index = end;
                } else if has_alternation(&chars, index) {
                    braces += 1;
                    out.push_str("(?:");
                } else {
                    out.push_str("\\{");
                }
            }
            ',' if braces > 0 => out.push('|'),
            '}' if braces > 0 => {
                braces -= 1;
                out.push(')');
            }
            other => out.push_str(&regex::escape(&other.to_string())),
        }
        index += 1;
    }
    if braces > 0 {
        return None;
    }
    out.push('$');
    Regex::new(&out).ok()
}

/// Whether the brace at `open` has a matching close with a comma at its own level.
fn has_alternation(chars: &[char], open: usize) -> bool {
    let mut depth = 0usize;
    let mut comma = false;
    let mut index = open;
    while index < chars.len() {
        match chars[index] {
            '\\' => index += 1,
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    return comma;
                }
            }
            ',' if depth == 1 => comma = true,
            _ => {}
        }
        index += 1;
    }
    false
}

/// `{3..12}`: any whole number in the range. Returns the regex and the index of the
/// closing brace.
fn numeric_range(chars: &[char], open: usize) -> Option<(String, usize)> {
    let close = chars[open..].iter().position(|&c| c == '}')? + open;
    let body: String = chars[open + 1..close].iter().collect();
    let (low, high) = body.split_once("..")?;
    let (low, high) = (low.parse::<i64>().ok()?, high.parse::<i64>().ok()?);
    let (low, high) = (low.min(high), low.max(high));
    // Large ranges would make a huge pattern; editors only need small ones in practice.
    if high - low > 10_000 {
        return None;
    }
    let numbers = (low..=high)
        .map(|n| n.to_string())
        .collect::<Vec<_>>()
        .join("|");
    Some((format!("(?:{numbers})"), close))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn matches(glob: &str, path: &str) -> bool {
        glob_to_regex(glob).is_some_and(|regex| regex.is_match(path))
    }

    #[test]
    fn globs_follow_the_editorconfig_rules() {
        assert!(matches("*", "a.rs"));
        assert!(matches("*", "src/deep/a.rs"));
        assert!(matches("*.rs", "src/a.rs"));
        assert!(!matches("*.rs", "src/a.rsx"));
        assert!(matches("Makefile", "sub/Makefile"));
        assert!(matches("*.{js,ts}", "a.ts"));
        assert!(!matches("*.{js,ts}", "a.tsx"));
        assert!(matches("{package.json,.travis.yml}", ".travis.yml"));
        assert!(matches("lib/**.js", "lib/a/b/c.js"));
        assert!(!matches("lib/**.js", "other/lib/c.js"));
        assert!(matches("/top.txt", "top.txt"));
        assert!(!matches("/top.txt", "sub/top.txt"));
        assert!(matches("src/*.c", "src/a.c"));
        assert!(!matches("src/*.c", "src/x/a.c"));
        assert!(matches("*.[ch]", "a.h"));
        assert!(!matches("*.[!ch]", "a.h"));
        assert!(matches("*.[!ch]", "a.x"));
        assert!(matches("file{1..3}.txt", "file2.txt"));
        assert!(!matches("file{1..3}.txt", "file4.txt"));
        assert!(matches("a?c", "abc"));
        assert!(matches("{*.md,docs/**}", "docs/x/y.txt"));
        assert!(matches("\\{literal}", "{literal}"));
        assert!(matches("{single}", "{single}"));
    }

    #[test]
    fn parses_sections_properties_comments_and_root() {
        let file = parse(
            "# comment\nroot = TRUE\n\n[*]\nindent_style = space\n; other comment\nindent_size = 4\n\n[*.go]\nindent_style=Tab\n",
        );
        assert!(file.root);
        assert_eq!(file.sections.len(), 2);
        assert_eq!(
            file.sections[1].properties,
            vec![("indent_style".into(), "tab".into())]
        );
    }

    #[test]
    fn nearer_files_and_later_sections_win() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::write(
            root.join(".editorconfig"),
            "root = true\n[*]\nindent_style = space\nindent_size = 2\nend_of_line = crlf\ntrim_trailing_whitespace = true\n[*.rs]\nindent_size = 4\n",
        )
        .unwrap();
        std::fs::create_dir_all(root.join("go/sub")).unwrap();
        std::fs::write(
            root.join("go/.editorconfig"),
            "[*.go]\nindent_style = tab\ntab_width = 8\ntrim_trailing_whitespace = unset\n",
        )
        .unwrap();
        let rust = EditorConfig::for_file(&root.join("src/main.rs"));
        assert_eq!(rust.indent_style_spaces, Some(true));
        assert_eq!(rust.indent_size, Some(IndentSize::Columns(4)));
        assert_eq!(rust.end_of_line, Some(EndOfLine::Crlf));
        assert_eq!(rust.trim_trailing_whitespace, Some(true));
        let go = EditorConfig::for_file(&root.join("go/sub/main.go"));
        assert_eq!(go.indent_style_spaces, Some(false));
        assert_eq!(go.tab_width, Some(8));
        assert_eq!(go.indent_size, Some(IndentSize::Columns(2)));
        assert_eq!(
            go.trim_trailing_whitespace, None,
            "unset clears the property"
        );
        let text = EditorConfig::for_file(&root.join("notes.txt"));
        assert_eq!(text.indent_size, Some(IndentSize::Columns(2)));
    }

    #[test]
    fn root_stops_the_search_upward() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join(".editorconfig"),
            "[*]\nindent_size = 8\ninsert_final_newline = true\n",
        )
        .unwrap();
        std::fs::create_dir_all(dir.path().join("project")).unwrap();
        std::fs::write(
            dir.path().join("project/.editorconfig"),
            "root = true\n[*]\nindent_style = tab\n",
        )
        .unwrap();
        let config = EditorConfig::for_file(&dir.path().join("project/a.c"));
        assert_eq!(config.indent_style_spaces, Some(false));
        assert_eq!(config.indent_size, None);
        assert_eq!(config.insert_final_newline, None);
        let outside = EditorConfig::for_file(&dir.path().join("b.c"));
        assert_eq!(outside.indent_size, Some(IndentSize::Columns(8)));
        assert_eq!(outside.insert_final_newline, Some(true));
    }

    #[test]
    fn bad_values_and_malformed_lines_are_ignored() {
        let config = EditorConfig::from_properties(&[
            ("indent_size".into(), "zero".into()),
            ("tab_width".into(), "0".into()),
            ("indent_style".into(), "both".into()),
            ("end_of_line".into(), "native".into()),
            ("charset".into(), "latin1".into()),
        ]);
        assert_eq!(
            config,
            EditorConfig {
                charset: Some("latin1".into()),
                ..EditorConfig::default()
            }
        );
        let file = parse("[*\nno equals sign here\n[]\nindent_size = 2\n");
        assert_eq!(file.sections.len(), 2);
        assert!(file.sections[1].pattern.is_none());
        assert!(glob_to_regex("{a,b").is_none() || matches("{a,b", "{a,b"));
    }
}
