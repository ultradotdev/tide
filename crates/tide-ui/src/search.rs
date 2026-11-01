//! One match model for navigation, highlighting and replacement.
use regex::{Regex, RegexBuilder};
use tide_core::{Document, Range};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SearchOptions {
    pub case_sensitive: bool,
    pub whole_word: bool,
    pub regex: bool,
    pub selection: Option<Range>,
}

pub fn expression(query: &str, options: SearchOptions) -> Result<Regex, String> {
    if query.len() > 64 * 1024 {
        return Err("Search pattern exceeds 64 KiB".into());
    }
    let query = if options.regex {
        query.to_owned()
    } else {
        regex::escape(query)
    };
    let query = if options.whole_word {
        format!(r"\b(?:{query})\b")
    } else {
        query
    };
    RegexBuilder::new(&query)
        .case_insensitive(!options.case_sensitive)
        .size_limit(1024 * 1024)
        .build()
        .map_err(|error| error.to_string())
}

pub fn matches(
    document: &Document,
    query: &str,
    options: SearchOptions,
    replacement: Option<&str>,
) -> Result<Vec<(Range, String)>, String> {
    if query.is_empty() {
        return Ok(Vec::new());
    }
    let expression = expression(query, options)?;
    let text = document.text();
    let mut last_byte = 0;
    let mut offset = 0;
    let mut result = Vec::new();
    let mut replacement_bytes = 0usize;
    for captures in expression.captures_iter(&text).take(100_001) {
        let hit = captures.get(0).unwrap();
        offset += text[last_byte..hit.start()].chars().count();
        let start = document.char_to_position(offset);
        offset += hit.as_str().chars().count();
        let end = document.char_to_position(offset);
        last_byte = hit.end();
        let range = Range::new(start, end);
        if options.selection.is_some_and(|selection| {
            let selection = selection.normalized();
            (start.line, start.column) < (selection.start.line, selection.start.column)
                || (end.line, end.column) > (selection.end.line, selection.end.column)
        }) {
            continue;
        }
        let mut value = String::new();
        if let Some(replacement) = replacement {
            if options.regex {
                captures.expand(replacement, &mut value);
            } else {
                value.push_str(replacement);
            }
        }
        replacement_bytes = replacement_bytes.saturating_add(value.len());
        if replacement_bytes > 32 * 1024 * 1024 {
            return Err("Replacement exceeds 32 MiB; narrow the search".into());
        }
        if result.len() >= 100_000 {
            return Err("More than 100,000 matches; narrow the search".into());
        }
        result.push((range, value));
    }
    Ok(result)
}
