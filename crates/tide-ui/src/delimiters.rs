//! Revision-cached lexical delimiters. Cursor movement only queries this index.
use std::collections::BTreeMap;

use tide_core::Position;

#[derive(Debug, Clone)]
struct Pair {
    ch: char,
    open: Position,
    close: Option<Position>,
    parent: Option<usize>,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct Delimiters {
    pairs: Vec<Pair>,
    closing: BTreeMap<Position, usize>,
    /// Lexer state at the start of every parsed line, plus the end state, so
    /// an edit re-lexes from its first changed line instead of the top.
    starts: Vec<LineStart>,
}

#[derive(Debug, Clone, Copy, Default)]
struct LineStart {
    quote: Option<Quote>,
    comment_depth: usize,
}

#[derive(Debug, Clone, Copy)]
enum Quote {
    Normal(char),
    Triple(char),
    Raw(usize),
}

#[cfg(test)]
thread_local! {
    pub(crate) static PARSED_LINES: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

impl Delimiters {
    #[cfg(test)]
    pub(crate) fn parse(lines: &[String], extension: &str) -> Self {
        let mut result = Self::default();
        result.reparse(0, lines.len(), |line| lines[line].as_str(), extension);
        result
    }

    /// Re-lexes lines `first..line_count`, keeping what was parsed before
    /// `first`. Lines before `first` must be unchanged since the last parse.
    pub(crate) fn reparse<S: AsRef<str>>(
        &mut self,
        first: usize,
        line_count: usize,
        mut text_of: impl FnMut(usize) -> S,
        extension: &str,
    ) {
        let first = first.min(self.starts.len().saturating_sub(1));
        let LineStart {
            mut quote,
            mut comment_depth,
        } = self.starts.get(first).copied().unwrap_or_default();
        self.starts.truncate(first);
        let kept = self.pairs.partition_point(|pair| pair.open.line < first);
        self.pairs.truncate(kept);
        self.closing.split_off(&Position::new(first, 0));
        let mut stack: Vec<(char, usize)> = Vec::new();
        for (index, pair) in self.pairs.iter_mut().enumerate() {
            if pair.close.is_some_and(|close| close.line >= first) {
                pair.close = None;
            }
            if pair.close.is_none() {
                stack.push((pair.ch, index));
            }
        }
        let mut escaped = false;
        let rust = extension == "rs";
        let python = matches!(extension, "py" | "pyw");
        let sql = extension == "sql";
        let hash_comments = matches!(
            extension,
            "py" | "pyw" | "rb" | "sh" | "bash" | "zsh" | "fish" | "toml" | "yaml" | "yml" | "r"
        );
        let mut chars: Vec<char> = Vec::new();
        for line in first..line_count {
            self.starts.push(LineStart {
                quote,
                comment_depth,
            });
            #[cfg(test)]
            PARSED_LINES.set(PARSED_LINES.get() + 1);
            let text = text_of(line);
            let text = text.as_ref();
            if quote.is_none()
                && comment_depth == 0
                && !text.bytes().any(|ch| {
                    matches!(
                        ch,
                        b'{' | b'}'
                            | b'('
                            | b')'
                            | b'['
                            | b']'
                            | b'\''
                            | b'"'
                            | b'`'
                            | b'/'
                            | b'#'
                            | b'-'
                    )
                })
            {
                continue;
            }
            chars.clear();
            chars.extend(text.chars());
            let mut i = 0;
            while i < chars.len() {
                let ch = chars[i];
                let next = chars.get(i + 1).copied();
                if comment_depth > 0 {
                    if rust && ch == '/' && next == Some('*') {
                        comment_depth += 1;
                        i += 2;
                    } else if ch == '*' && next == Some('/') {
                        comment_depth -= 1;
                        i += 2;
                    } else {
                        i += 1;
                    }
                    continue;
                }
                if let Some(active) = quote {
                    match active {
                        Quote::Raw(hashes) => {
                            if ch == '"'
                                && chars
                                    .get(i + 1..i + 1 + hashes)
                                    .is_some_and(|tail| tail.iter().all(|ch| *ch == '#'))
                            {
                                quote = None;
                                i += hashes;
                            }
                        }
                        Quote::Normal(end) | Quote::Triple(end) => {
                            if escaped {
                                escaped = false;
                            } else if ch == '\\' {
                                escaped = true;
                            } else if ch == end {
                                if matches!(active, Quote::Triple(_)) {
                                    if next == Some(end) && chars.get(i + 2) == Some(&end) {
                                        quote = None;
                                        i += 2;
                                    }
                                } else if sql && next == Some(end) {
                                    i += 1;
                                } else {
                                    quote = None;
                                }
                            }
                        }
                    }
                    i += 1;
                    continue;
                }
                if (hash_comments && ch == '#')
                    || (!python && !sql && ch == '/' && next == Some('/'))
                    || (sql && ch == '-' && next == Some('-'))
                {
                    break;
                }
                if !python && ch == '/' && next == Some('*') {
                    comment_depth = 1;
                    i += 2;
                    continue;
                }
                if rust && ch == 'r' {
                    let hashes = chars[i + 1..].iter().take_while(|ch| **ch == '#').count();
                    if chars.get(i + 1 + hashes) == Some(&'"') {
                        quote = Some(Quote::Raw(hashes));
                        i += hashes + 2;
                        continue;
                    }
                }
                if matches!(ch, '\'' | '"' | '`')
                    && !(rust
                        && ch == '\''
                        && next != Some('\\')
                        && chars.get(i + 2) != Some(&'\''))
                {
                    if python && next == Some(ch) && chars.get(i + 2) == Some(&ch) {
                        quote = Some(Quote::Triple(ch));
                        i += 3;
                    } else {
                        quote = Some(Quote::Normal(ch));
                        i += 1;
                    }
                    continue;
                }
                if matches!(ch, '{' | '(' | '[') {
                    let index = self.pairs.len();
                    self.pairs.push(Pair {
                        ch,
                        open: Position::new(line, i),
                        close: None,
                        parent: stack.last().map(|(_, parent)| *parent),
                    });
                    stack.push((ch, index));
                } else if let Some(open) = match ch {
                    '}' => Some('{'),
                    ')' => Some('('),
                    ']' => Some('['),
                    _ => None,
                } && stack.last().is_some_and(|(ch, _)| *ch == open)
                {
                    let (_, index) = stack.pop().unwrap();
                    let close = Position::new(line, i);
                    self.pairs[index].close = Some(close);
                    self.closing.insert(close, index);
                }
                i += 1;
            }
            // Invalid/incomplete single-line literals must not hide subsequent code.
            if !rust && !sql && !escaped && matches!(quote, Some(Quote::Normal(ch)) if ch != '`') {
                quote = None;
            }
            escaped = false;
        }
        self.starts.push(LineStart {
            quote,
            comment_depth,
        });
    }

    fn at(&self, point: Position) -> Option<(Position, Position)> {
        let index = self
            .pairs
            .binary_search_by_key(&point, |pair| pair.open)
            .ok()
            .or_else(|| self.closing.get(&point).copied())?;
        let pair = &self.pairs[index];
        pair.close.map(|close| (pair.open, close))
    }

    pub(crate) fn enclosing(&self, cursor: Position) -> Option<(Position, Position)> {
        if let Some(pair) = self.at(cursor).or_else(|| {
            cursor
                .column
                .checked_sub(1)
                .and_then(|column| self.at(Position::new(cursor.line, column)))
        }) {
            return Some(pair);
        }
        let opened = self
            .pairs
            .partition_point(|pair| pair.open <= cursor)
            .checked_sub(1);
        // The most recent lexical event determines the active nesting level.
        // Jump directly to the parent of a closed pair instead of walking a
        // potentially deep chain after the cursor leaves a large nested block.
        let index = match (opened, self.closing.range(..=cursor).next_back()) {
            (Some(open), Some((position, closed))) if *position > self.pairs[open].open => {
                self.pairs[*closed].parent
            }
            (open, _) => open,
        }?;
        let pair = &self.pairs[index];
        pair.close
            .filter(|close| cursor <= *close)
            .map(|close| (pair.open, close))
    }

    pub(crate) fn multiline_regions(&self) -> impl Iterator<Item = (usize, usize)> + '_ {
        self.pairs.iter().filter_map(|pair| {
            pair.close
                .filter(|close| close.line > pair.open.line)
                .map(|close| (pair.open.line, close.line))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn parse(text: &str, extension: &str) -> Delimiters {
        Delimiters::parse(
            &text.lines().map(str::to_owned).collect::<Vec<_>>(),
            extension,
        )
    }
    #[test]
    fn matches_touched_and_innermost_enclosing_pairs_with_unicode_columns() {
        let index = parse(
            "fn outer() {\n  if ready {\n    café([value]);\n  }\n}",
            "rs",
        );
        assert_eq!(
            index.enclosing(Position::new(2, 12)),
            Some((Position::new(2, 9), Position::new(2, 15)))
        );
        assert_eq!(
            index.enclosing(Position::new(2, 16)),
            Some((Position::new(2, 8), Position::new(2, 16)))
        );
        assert_eq!(
            index.enclosing(Position::new(3, 2)),
            Some((Position::new(1, 11), Position::new(3, 2)))
        );
        assert_eq!(
            index.enclosing(Position::new(4, 0)),
            Some((Position::new(0, 11), Position::new(4, 0)))
        );
        assert_eq!(index.enclosing(Position::new(0, 0)), None);
    }
    #[test]
    fn comments_strings_raw_literals_and_lifetimes_do_not_create_fake_scopes() {
        let text = "fn f<'a>(value: &'a str) {\n let ch = '}'; let raw = r###\"{ \" [ }\"###;\n /* outer { /* nested } */ ) */\n // }\n let text = `fake { [ (`;\n work();\n}";
        let index = parse(text, "rs");
        assert_eq!(
            index.enclosing(Position::new(5, 1)),
            Some((Position::new(0, 25), Position::new(6, 0)))
        );
        assert_eq!(index.multiline_regions().collect::<Vec<_>>(), [(0, 6)]);
    }
    #[test]
    fn python_multiline_strings_and_sql_escaped_strings_ignore_delimiters() {
        let py = parse("data = [\n ''' } [\n ) ] ''' ,\n 12\n] # [", "py");
        assert_eq!(
            py.enclosing(Position::new(3, 1)),
            Some((Position::new(0, 7), Position::new(4, 0)))
        );
        let sql = parse("SELECT (\n 'it''s ) [', -- )\n value\n)", "sql");
        assert_eq!(
            sql.enclosing(Position::new(2, 1)),
            Some((Position::new(0, 7), Position::new(3, 0)))
        );
    }
    #[test]
    fn reparsing_from_the_first_edited_line_matches_a_full_parse() {
        let original =
            "fn a() {\n  let x = [1,\n    2];\n}\n/* c */\nfn b() {\n  call(\"}\");\n}\n";
        let edits: [(usize, &str); 5] = [
            (1, "  let x = [1, /* open"),
            (4, "/* c"),
            (6, "  call(\"}\"); {"),
            (7, ""),
            (0, "fn a() { (\n\n"),
        ];
        for (line, replacement) in edits {
            let mut lines = original.lines().map(str::to_owned).collect::<Vec<_>>();
            let mut index = Delimiters::parse(&lines, "rs");
            lines.splice(line..=line, replacement.split('\n').map(str::to_owned));
            index.reparse(line, lines.len(), |line| lines[line].as_str(), "rs");
            let full = Delimiters::parse(&lines, "rs");
            assert_eq!(format!("{index:?}"), format!("{full:?}"), "edit at {line}");
        }
    }

    #[test]
    fn unmatched_delimiters_never_invent_a_closing_statement() {
        let index = parse("fn broken() { [ )\n value\n", "rs");
        assert_eq!(index.enclosing(Position::new(1, 1)), None);
        assert_eq!(index.multiline_regions().count(), 0);
    }

    #[test]
    fn leaving_closed_nested_scopes_returns_to_the_active_parent() {
        let text = "{ (x) (y) after } outside";
        let index = parse(text, "ts");
        assert_eq!(
            index.enclosing(Position::new(0, text.find("after").unwrap() + 1)),
            Some((
                Position::new(0, 0),
                Position::new(0, text.find('}').unwrap())
            ))
        );
        assert_eq!(
            index.enclosing(Position::new(0, text.find("outside").unwrap() + 2)),
            None
        );
        let deep = format!("{}{} outside", "(".repeat(10_000), ")".repeat(10_000));
        assert_eq!(parse(&deep, "ts").enclosing(Position::new(0, 20_004)), None);
    }
}
