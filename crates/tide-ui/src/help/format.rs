//! Help pages as formatted text: Markdown laid out for a width, one style per span.
//!
//! The result is plain lines (so the help view keeps cursor movement, selection, copy
//! and find) plus the ranges to style: headings, emphasis, inline code, links, list
//! markers and tables laid out as aligned columns.
use std::ops::Range;

use pulldown_cmark::{Alignment, Event, HeadingLevel, Options, Parser, Tag, TagEnd};
use tide_core::{display_width, grapheme_width, graphemes};

/// What a span of formatted text is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Kind {
    #[default]
    Text,
    Heading(u8),
    Code,
    Link,
    /// A link target shown after its text.
    Url,
    /// List bullets and numbers.
    Marker,
    /// Horizontal rules, heading underlines and table header separators.
    Rule,
    Quote,
    CodeBlock,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Mark {
    pub kind: Kind,
    pub strong: bool,
    pub emphasis: bool,
}

impl Mark {
    const fn of(kind: Kind) -> Self {
        Self {
            kind,
            strong: false,
            emphasis: false,
        }
    }
}

/// One laid-out line: its text and the styled byte ranges in it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Line {
    pub text: String,
    pub spans: Vec<(Range<usize>, Mark)>,
    /// The Markdown block the line came from, to keep the reading position when the
    /// width changes.
    pub block: usize,
}

impl Line {
    fn push(&mut self, text: &str, mark: Mark) {
        if text.is_empty() {
            return;
        }
        let start = self.text.len();
        self.text.push_str(text);
        match self.spans.last_mut() {
            Some((range, previous)) if *previous == mark && range.end == start => {
                range.end = self.text.len();
            }
            _ => self.spans.push((start..self.text.len(), mark)),
        }
    }

    fn trim_end(&mut self) {
        let end = self.text.trim_end().len();
        self.text.truncate(end);
        self.spans.retain_mut(|(range, _)| {
            range.end = range.end.min(end);
            range.start < range.end
        });
    }
}

/// A run of inline text with one style; `"\n"` alone is a hard line break.
type Run = (String, Mark);

/// The widest a help page is laid out, so prose stays readable on wide windows.
pub const MAX_WIDTH: usize = 120;

/// Lays out `markdown` in `width` cells.
#[must_use]
pub fn layout(markdown: &str, width: usize) -> Vec<Line> {
    let width = width.clamp(20, MAX_WIDTH);
    let mut writer = Writer {
        width,
        lines: Vec::new(),
        containers: Vec::new(),
        lists: Vec::new(),
        runs: Vec::new(),
        strong: 0,
        emphasis: 0,
        links: Vec::new(),
        heading: None,
        code_block: None,
        table: None,
        pending_blank: false,
        block: 0,
    };
    let options = Options::ENABLE_TABLES | Options::ENABLE_STRIKETHROUGH;
    for event in Parser::new_ext(markdown, options) {
        writer.event(event);
    }
    writer.flush_paragraph();
    while writer.lines.last().is_some_and(|line| line.text.is_empty()) {
        writer.lines.pop();
    }
    writer.lines
}

enum Container {
    Quote,
    Item { marker: String, shown: bool },
}

struct Table {
    alignments: Vec<Alignment>,
    rows: Vec<Vec<Vec<Run>>>,
    head: bool,
}

struct Writer {
    width: usize,
    lines: Vec<Line>,
    containers: Vec<Container>,
    /// The next number of each open list; `None` for bullets.
    lists: Vec<Option<u64>>,
    runs: Vec<Run>,
    strong: usize,
    emphasis: usize,
    /// Open links: their target and where their text starts in `runs`.
    links: Vec<(String, usize)>,
    heading: Option<u8>,
    code_block: Option<String>,
    table: Option<Table>,
    pending_blank: bool,
    block: usize,
}

impl Writer {
    fn mark(&self) -> Mark {
        Mark {
            kind: match (self.heading, self.links.is_empty()) {
                (Some(level), _) => Kind::Heading(level),
                (None, false) => Kind::Link,
                (None, true) if self.in_quote() => Kind::Quote,
                (None, true) => Kind::Text,
            },
            strong: self.strong > 0,
            emphasis: self.emphasis > 0,
        }
    }

    fn in_quote(&self) -> bool {
        self.containers
            .iter()
            .any(|container| matches!(container, Container::Quote))
    }

    fn text(&mut self, text: &str, mark: Mark) {
        if let Some(code) = &mut self.code_block {
            code.push_str(text);
            return;
        }
        self.runs.push((text.to_owned(), mark));
    }

    fn event(&mut self, event: Event<'_>) {
        match event {
            Event::Start(tag) => self.start(tag),
            Event::End(tag) => self.end(tag),
            Event::Text(text) => self.text(&text, self.mark()),
            Event::Code(text) => self.text(
                &text,
                Mark {
                    kind: Kind::Code,
                    ..self.mark()
                },
            ),
            Event::SoftBreak => self.text(" ", self.mark()),
            Event::HardBreak => self.runs.push(("\n".to_owned(), Mark::default())),
            Event::Rule => {
                self.flush_paragraph();
                self.begin_block();
                let width = self.available();
                self.emit(vec![("─".repeat(width), Mark::of(Kind::Rule))]);
                self.pending_blank = true;
            }
            Event::TaskListMarker(done) => {
                self.text(if done { "[x] " } else { "[ ] " }, Mark::of(Kind::Marker));
            }
            Event::InlineMath(text) | Event::DisplayMath(text) => self.text(&text, self.mark()),
            Event::Html(_) | Event::InlineHtml(_) | Event::FootnoteReference(_) => {}
        }
    }

    fn start(&mut self, tag: Tag<'_>) {
        match tag {
            Tag::Paragraph => self.flush_paragraph(),
            Tag::Heading { level, .. } => {
                self.flush_paragraph();
                self.heading = Some(match level {
                    HeadingLevel::H1 => 1,
                    HeadingLevel::H2 => 2,
                    HeadingLevel::H3 => 3,
                    HeadingLevel::H4 => 4,
                    HeadingLevel::H5 => 5,
                    HeadingLevel::H6 => 6,
                });
            }
            Tag::BlockQuote(_) => {
                self.flush_paragraph();
                self.containers.push(Container::Quote);
            }
            Tag::CodeBlock(_) => {
                self.flush_paragraph();
                self.code_block = Some(String::new());
            }
            Tag::List(start) => {
                self.flush_paragraph();
                self.lists.push(start);
            }
            Tag::Item => {
                self.flush_paragraph();
                let marker = match self.lists.last_mut() {
                    Some(Some(number)) => {
                        let marker = format!("{number}. ");
                        *number += 1;
                        marker
                    }
                    _ => ["• ", "◦ ", "▪ "][(self.lists.len().saturating_sub(1)) % 3].to_owned(),
                };
                self.containers.push(Container::Item {
                    marker,
                    shown: false,
                });
            }
            Tag::Table(alignments) => {
                self.flush_paragraph();
                self.table = Some(Table {
                    alignments,
                    rows: Vec::new(),
                    head: false,
                });
            }
            Tag::TableHead => {
                if let Some(table) = &mut self.table {
                    table.head = true;
                    table.rows.push(Vec::new());
                }
            }
            Tag::TableRow => {
                if let Some(table) = &mut self.table {
                    table.rows.push(Vec::new());
                }
            }
            Tag::TableCell => self.runs.clear(),
            Tag::Emphasis => self.emphasis += 1,
            Tag::Strong => self.strong += 1,
            Tag::Link { dest_url, .. } => {
                self.links.push((dest_url.into_string(), self.runs.len()))
            }
            Tag::Image { .. }
            | Tag::Strikethrough
            | Tag::FootnoteDefinition(_)
            | Tag::HtmlBlock
            | Tag::DefinitionList
            | Tag::DefinitionListTitle
            | Tag::DefinitionListDefinition
            | Tag::Superscript
            | Tag::Subscript
            | Tag::MetadataBlock(_) => {}
        }
    }

    fn end(&mut self, tag: TagEnd) {
        match tag {
            TagEnd::Paragraph => {
                self.flush_paragraph();
                self.pending_blank = true;
            }
            TagEnd::Heading(_) => {
                let level = self.heading.take().unwrap_or(1);
                let runs = std::mem::take(&mut self.runs);
                self.begin_heading();
                let lines = wrap(&runs, self.available());
                let mut widest = 0;
                for line in lines {
                    widest = widest.max(line.iter().map(|(text, _)| display_width(text)).sum());
                    self.emit(line);
                }
                let rule = match level {
                    1 => Some("━"),
                    2 => Some("─"),
                    _ => None,
                };
                if let Some(rule) = rule {
                    self.emit(vec![(rule.repeat(widest), Mark::of(Kind::Heading(level)))]);
                }
                self.pending_blank = true;
            }
            TagEnd::BlockQuote(_) => {
                self.flush_paragraph();
                self.containers.pop();
                self.pending_blank = true;
            }
            TagEnd::CodeBlock => {
                let code = self.code_block.take().unwrap_or_default();
                self.begin_block();
                let width = self.available().saturating_sub(2).max(1);
                for source in code.trim_end_matches('\n').split('\n') {
                    for chunk in split_width(source, width) {
                        self.emit(vec![(format!("  {chunk}"), Mark::of(Kind::CodeBlock))]);
                    }
                }
                self.pending_blank = true;
            }
            TagEnd::List(_) => {
                self.flush_paragraph();
                self.lists.pop();
                self.pending_blank = true;
            }
            TagEnd::Item => {
                self.flush_paragraph();
                self.containers.pop();
            }
            TagEnd::Table => {
                if let Some(table) = self.table.take() {
                    self.begin_block();
                    self.emit_table(table);
                    self.pending_blank = true;
                }
            }
            TagEnd::TableCell => {
                let runs = std::mem::take(&mut self.runs);
                if let Some(row) = self.table.as_mut().and_then(|table| table.rows.last_mut()) {
                    row.push(runs);
                }
            }
            TagEnd::TableHead => {
                if let Some(table) = &mut self.table {
                    table.head = false;
                }
            }
            TagEnd::Emphasis => self.emphasis = self.emphasis.saturating_sub(1),
            TagEnd::Strong => self.strong = self.strong.saturating_sub(1),
            TagEnd::Link => {
                let Some((url, start)) = self.links.pop() else {
                    return;
                };
                let text = self.runs[start.min(self.runs.len())..]
                    .iter()
                    .map(|(text, _)| text.as_str())
                    .collect::<String>();
                let external = ["http://", "https://", "mailto:"]
                    .iter()
                    .any(|scheme| url.starts_with(scheme));
                let shown = url.trim_start_matches("mailto:");
                if external && !text.contains(shown) {
                    self.runs.push((" ".to_owned(), self.mark()));
                    self.runs.push((format!("({url})"), Mark::of(Kind::Url)));
                }
            }
            TagEnd::TableRow
            | TagEnd::Image
            | TagEnd::Strikethrough
            | TagEnd::FootnoteDefinition
            | TagEnd::HtmlBlock
            | TagEnd::DefinitionList
            | TagEnd::DefinitionListTitle
            | TagEnd::DefinitionListDefinition
            | TagEnd::Superscript
            | TagEnd::Subscript
            | TagEnd::MetadataBlock(_) => {}
        }
    }

    /// Lays out any inline text gathered since the last block, such as a tight list
    /// item's text, which has no paragraph around it.
    fn flush_paragraph(&mut self) {
        if self.table.is_some() || self.heading.is_some() {
            return;
        }
        if self
            .runs
            .iter()
            .all(|(text, _)| text.trim().is_empty() && text != "\n")
        {
            self.runs.clear();
            return;
        }
        let runs = std::mem::take(&mut self.runs);
        self.begin_block();
        for line in wrap(&runs, self.available()) {
            self.emit(line);
        }
    }

    fn begin_block(&mut self) {
        self.block += 1;
        if self.pending_blank && !self.lines.is_empty() {
            self.blank();
        }
        self.pending_blank = false;
    }

    /// Headings always stand apart from what precedes them.
    fn begin_heading(&mut self) {
        self.pending_blank = true;
        self.begin_block();
    }

    fn blank(&mut self) {
        if self.lines.last().is_some_and(|line| line.text.is_empty()) {
            return;
        }
        let mut line = Line {
            block: self.block,
            ..Line::default()
        };
        for container in &self.containers {
            if matches!(container, Container::Quote) {
                line.push("│ ", Mark::of(Kind::Quote));
            }
        }
        line.trim_end();
        self.lines.push(line);
    }

    fn prefix_width(&self) -> usize {
        self.containers
            .iter()
            .map(|container| match container {
                Container::Quote => 2,
                Container::Item { marker, .. } => display_width(marker),
            })
            .sum()
    }

    fn available(&self) -> usize {
        self.width.saturating_sub(self.prefix_width()).max(8)
    }

    fn emit(&mut self, pieces: Vec<Run>) {
        let mut line = Line {
            block: self.block,
            ..Line::default()
        };
        let innermost = self
            .containers
            .iter()
            .rposition(|container| matches!(container, Container::Item { .. }));
        for (index, container) in self.containers.iter_mut().enumerate() {
            match container {
                Container::Quote => line.push("│ ", Mark::of(Kind::Quote)),
                Container::Item { marker, shown } => {
                    if *shown || Some(index) != innermost {
                        line.push(&" ".repeat(display_width(marker)), Mark::default());
                    } else {
                        line.push(marker, Mark::of(Kind::Marker));
                        *shown = true;
                    }
                }
            }
        }
        for (text, mark) in pieces {
            line.push(&text, mark);
        }
        line.trim_end();
        self.lines.push(line);
    }

    fn emit_table(&mut self, table: Table) {
        const GAP: usize = 2;
        let columns = table.rows.iter().map(Vec::len).max().unwrap_or(0);
        if columns == 0 {
            return;
        }
        let available = self.available().saturating_sub(GAP * (columns - 1));
        let words = |cell: &Vec<Run>| words(cell);
        let mut natural = vec![0; columns];
        let mut longest = vec![0; columns];
        for row in &table.rows {
            for (column, cell) in row.iter().enumerate() {
                let cell_words = words(cell);
                let width = cell_words.iter().map(|word| word.width).sum::<usize>()
                    + cell_words
                        .iter()
                        .skip(1)
                        .filter(|word| word.space_before)
                        .count();
                natural[column] = natural[column].max(width);
                longest[column] = longest[column]
                    .max(cell_words.iter().map(|word| word.width).max().unwrap_or(0));
            }
        }
        let widths = column_widths(&natural, &longest, available);
        let header_rows = usize::from(table.rows.len() > 1);
        let laid_out = table
            .rows
            .iter()
            .enumerate()
            .map(|(index, row)| {
                (0..columns)
                    .map(|column| {
                        let mut cell = row.get(column).cloned().unwrap_or_default();
                        if index < header_rows {
                            for (_, mark) in &mut cell {
                                mark.strong = true;
                            }
                        }
                        wrap(&cell, widths[column].max(1))
                    })
                    .collect::<Vec<_>>()
            })
            .collect::<Vec<_>>();
        // A row that wraps is set apart by blank lines so its lines read as one row;
        // single-line rows stay together.
        let tall = |row: &Vec<Vec<Vec<Run>>>| row.iter().any(|cell| cell.len() > 1);
        for (index, row) in laid_out.iter().enumerate() {
            if index > header_rows && (tall(row) || tall(&laid_out[index - 1])) {
                self.emit(Vec::new());
            }
            let height = row.iter().map(Vec::len).max().unwrap_or(1).max(1);
            for line_index in 0..height {
                let mut pieces = Vec::new();
                for (column, cell) in row.iter().enumerate() {
                    if column > 0 {
                        pieces.push((" ".repeat(GAP), Mark::default()));
                    }
                    let content = cell.get(line_index).cloned().unwrap_or_default();
                    let used = content
                        .iter()
                        .map(|(text, _)| display_width(text))
                        .sum::<usize>();
                    let spare = widths[column].saturating_sub(used);
                    let (before, after) = match table.alignments.get(column) {
                        Some(Alignment::Right) => (spare, 0),
                        Some(Alignment::Center) => (spare / 2, spare - spare / 2),
                        _ => (0, spare),
                    };
                    pieces.push((" ".repeat(before), Mark::default()));
                    pieces.extend(content);
                    pieces.push((" ".repeat(after), Mark::default()));
                }
                self.emit(pieces);
            }
            if index + 1 == header_rows {
                let rule = widths
                    .iter()
                    .map(|width| "─".repeat(*width))
                    .collect::<Vec<_>>()
                    .join(&" ".repeat(GAP));
                self.emit(vec![(rule, Mark::of(Kind::Rule))]);
            }
        }
    }
}

/// Fits columns into `available` cells: natural widths when they fit, otherwise each
/// column keeps room for its longest word (within a fair share) and the rest is shared
/// out by how much more each column wants.
fn column_widths(natural: &[usize], longest: &[usize], available: usize) -> Vec<usize> {
    let columns = natural.len();
    if natural.iter().sum::<usize>() <= available {
        return natural.to_vec();
    }
    let share = (available / columns).max(4);
    let minimum = natural
        .iter()
        .zip(longest)
        .map(|(natural, longest)| (*natural).min((*longest).min(share)).max(1))
        .collect::<Vec<_>>();
    let floor = minimum.iter().sum::<usize>();
    if floor >= available {
        let mut widths = vec![(available / columns).max(1); columns];
        for width in widths.iter_mut().take(available % columns) {
            *width += 1;
        }
        return widths;
    }
    let extra = available - floor;
    let wants = natural
        .iter()
        .zip(&minimum)
        .map(|(natural, minimum)| natural - minimum)
        .collect::<Vec<_>>();
    let wanted = wants.iter().sum::<usize>().max(1);
    let mut widths = minimum
        .iter()
        .zip(&wants)
        .map(|(minimum, want)| minimum + extra * want / wanted)
        .collect::<Vec<_>>();
    let mut left = available.saturating_sub(widths.iter().sum());
    for (width, natural) in widths.iter_mut().zip(natural) {
        let room = natural.saturating_sub(*width).min(left);
        *width += room;
        left -= room;
    }
    widths
}

struct Word {
    pieces: Vec<Run>,
    width: usize,
    space_before: bool,
    /// A hard line break rather than a word.
    newline: bool,
}

/// Splits runs into words. Inline code keeps its spaces, so a key chord such as
/// `Ctrl+K Ctrl+S` stays on one line when it fits; bare web addresses become links.
fn words(runs: &[Run]) -> Vec<Word> {
    let mut words = Vec::new();
    let mut current: Option<Word> = None;
    let mut space = false;
    let finish = |current: &mut Option<Word>, words: &mut Vec<Word>| {
        if let Some(mut word) = current.take() {
            let text = word
                .pieces
                .iter()
                .map(|(text, _)| text.as_str())
                .collect::<String>();
            if (text.starts_with("https://") || text.starts_with("http://"))
                && word.pieces.iter().all(|(_, mark)| mark.kind == Kind::Text)
            {
                for (_, mark) in &mut word.pieces {
                    mark.kind = Kind::Link;
                }
            }
            words.push(word);
        }
    };
    for (text, mark) in runs {
        if text == "\n" {
            finish(&mut current, &mut words);
            words.push(Word {
                pieces: Vec::new(),
                width: 0,
                space_before: false,
                newline: true,
            });
            space = false;
            continue;
        }
        if mark.kind == Kind::Code {
            let word = current.get_or_insert_with(|| Word {
                pieces: Vec::new(),
                width: 0,
                space_before: space,
                newline: false,
            });
            word.pieces.push((text.clone(), *mark));
            word.width += display_width(text);
            space = false;
            continue;
        }
        let mut rest = text.as_str();
        while !rest.is_empty() {
            let blank = rest.starts_with(char::is_whitespace);
            let end = rest
                .find(|ch: char| ch.is_whitespace() != blank)
                .unwrap_or(rest.len());
            let (piece, tail) = rest.split_at(end);
            rest = tail;
            if blank {
                finish(&mut current, &mut words);
                space = true;
            } else {
                let word = current.get_or_insert_with(|| Word {
                    pieces: Vec::new(),
                    width: 0,
                    space_before: space,
                    newline: false,
                });
                word.pieces.push((piece.to_owned(), *mark));
                word.width += display_width(piece);
                space = false;
            }
        }
    }
    finish(&mut current, &mut words);
    words
}

/// Wraps runs at word boundaries into lines of at most `width` cells. A word wider
/// than a line is broken between graphemes.
fn wrap(runs: &[Run], width: usize) -> Vec<Vec<Run>> {
    let width = width.max(1);
    let mut lines: Vec<Vec<Run>> = Vec::new();
    let mut line: Vec<Run> = Vec::new();
    let mut used = 0;
    let push = |line: &mut Vec<Run>, text: &str, mark: Mark| match line.last_mut() {
        Some((previous, previous_mark)) if *previous_mark == mark => previous.push_str(text),
        _ => line.push((text.to_owned(), mark)),
    };
    for word in words(runs) {
        if word.newline {
            lines.push(std::mem::take(&mut line));
            used = 0;
            continue;
        }
        let gap = usize::from(word.space_before && used > 0);
        if used > 0 && used + gap + word.width > width {
            lines.push(std::mem::take(&mut line));
            used = 0;
        } else if gap > 0 {
            // A space takes the style around it only inside one styled phrase, so a
            // link is underlined across its words but not into its neighbours.
            let next = word.pieces.first().map(|(_, mark)| *mark);
            let mark = match line.last() {
                Some((_, previous)) if Some(*previous) == next && previous.kind != Kind::Code => {
                    *previous
                }
                _ => Mark::default(),
            };
            push(&mut line, " ", mark);
            used += 1;
        }
        if word.width <= width {
            for (text, mark) in &word.pieces {
                push(&mut line, text, *mark);
            }
            used += word.width;
            continue;
        }
        for (text, mark) in &word.pieces {
            for grapheme in graphemes(text) {
                let cells = grapheme_width(grapheme);
                if used > 0 && used + cells > width {
                    lines.push(std::mem::take(&mut line));
                    used = 0;
                }
                push(&mut line, grapheme, *mark);
                used += cells;
            }
        }
    }
    if !line.is_empty() || lines.is_empty() {
        lines.push(line);
    }
    lines
}

/// `text` in chunks of at most `width` cells, for code that must not reflow.
fn split_width(text: &str, width: usize) -> Vec<String> {
    let mut chunks = vec![String::new()];
    let mut used = 0;
    for grapheme in graphemes(text) {
        let cells = if grapheme == "\t" {
            4
        } else {
            grapheme_width(grapheme)
        };
        if used > 0 && used + cells > width {
            chunks.push(String::new());
            used = 0;
        }
        let chunk = chunks.last_mut().expect("one chunk");
        if grapheme == "\t" {
            chunk.push_str("    ");
        } else {
            chunk.push_str(grapheme);
        }
        used += cells;
    }
    chunks
}

/// How `mark` looks in `palette`.
#[must_use]
pub fn style(mark: Mark, palette: &crate::palette::Palette) -> ratatui::style::Style {
    use ratatui::style::{Modifier, Style};
    let base = Style::default().fg(palette.foreground);
    let mut style = match mark.kind {
        Kind::Text => base,
        Kind::Heading(level) if level <= 3 => base.fg(palette.accent).add_modifier(Modifier::BOLD),
        Kind::Heading(_) => base.add_modifier(Modifier::BOLD),
        Kind::Code => base.fg(palette.git_modified),
        Kind::Link => base
            .fg(palette.border_focused)
            .add_modifier(Modifier::UNDERLINED),
        Kind::Url | Kind::Rule => base.fg(palette.description),
        Kind::Marker => base.fg(palette.accent),
        Kind::Quote => base.fg(palette.description).add_modifier(Modifier::ITALIC),
        Kind::CodeBlock => base.fg(palette.git_untracked),
    };
    if mark.strong {
        style = style.add_modifier(Modifier::BOLD);
    }
    if mark.emphasis {
        style = style.add_modifier(Modifier::ITALIC);
    }
    style
}

/// The formatted text of `lines`, one per line.
#[must_use]
pub fn text(lines: &[Line]) -> String {
    let mut text = lines
        .iter()
        .map(|line| line.text.as_str())
        .collect::<Vec<_>>()
        .join("\n");
    text.push('\n');
    text
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rendered(markdown: &str, width: usize) -> Vec<String> {
        layout(markdown, width)
            .into_iter()
            .map(|line| line.text)
            .collect()
    }

    fn marks(line: &Line) -> Vec<(&str, Mark)> {
        line.spans
            .iter()
            .map(|(range, mark)| (&line.text[range.clone()], *mark))
            .collect()
    }

    #[test]
    fn headings_are_styled_and_underlined_without_markup() {
        let lines = layout("# Welcome\n\nText here.\n\n## Next\n\n### Small\n", 40);
        let text = lines
            .iter()
            .map(|line| line.text.as_str())
            .collect::<Vec<_>>();
        assert_eq!(
            text,
            [
                "Welcome",
                "━━━━━━━",
                "",
                "Text here.",
                "",
                "Next",
                "────",
                "",
                "Small"
            ]
        );
        assert_eq!(marks(&lines[0]), [("Welcome", Mark::of(Kind::Heading(1)))]);
        assert_eq!(marks(&lines[8]), [("Small", Mark::of(Kind::Heading(3)))]);
    }

    #[test]
    fn paragraphs_wrap_at_word_boundaries() {
        let text = rendered(
            "Tide is a keyboard-driven IDE that runs inside your terminal window.",
            24,
        );
        assert_eq!(
            text,
            [
                "Tide is a",
                "keyboard-driven IDE that",
                "runs inside your",
                "terminal window."
            ]
        );
        for line in &text {
            assert!(display_width(line) <= 24, "{line}");
        }
    }

    #[test]
    fn inline_code_strong_and_links_are_marked_without_their_syntax() {
        let lines = layout(
            "Press `Ctrl+K Ctrl+S` for **keys**, see [the docs](https://example.com/docs) or https://example.com.",
            200,
        );
        assert_eq!(
            lines[0].text,
            "Press Ctrl+K Ctrl+S for keys, see the docs (https://example.com/docs) or https://example.com."
        );
        let found = marks(&lines[0]);
        assert!(
            found.contains(&("Ctrl+K Ctrl+S", Mark::of(Kind::Code))),
            "{found:?}"
        );
        assert!(found.contains(&(
            "keys",
            Mark {
                strong: true,
                ..Mark::default()
            }
        )));
        assert!(found.contains(&("the docs", Mark::of(Kind::Link))));
        assert!(found.contains(&("(https://example.com/docs)", Mark::of(Kind::Url))));
        assert!(found.contains(&("https://example.com.", Mark::of(Kind::Link))));
    }

    #[test]
    fn code_spans_stay_whole_when_they_fit() {
        let text = rendered("Bind it as `ctrl+k ctrl+s` from today", 20);
        assert_eq!(text, ["Bind it as", "ctrl+k ctrl+s from", "today"]);
    }

    #[test]
    fn lists_hang_their_text_under_the_marker() {
        let text = rendered(
            "- **Settings:** every setting is listed here for you\n- Second\n  - Nested item\n\n1. One\n2. Two\n",
            24,
        );
        assert_eq!(
            text,
            [
                "• Settings: every",
                "  setting is listed here",
                "  for you",
                "• Second",
                "  ◦ Nested item",
                "",
                "1. One",
                "2. Two"
            ]
        );
    }

    #[test]
    fn tables_become_aligned_columns_that_fit_the_width() {
        let markdown = "| To | Press |\n| --- | --- |\n| Open the command palette | `Ctrl+K` or `Ctrl+Shift+P` |\n| Save | `Ctrl+S` |\n";
        let wide = rendered(markdown, 80);
        assert_eq!(
            wide,
            [
                "To                        Press",
                "────────────────────────  ──────────────────────",
                "Open the command palette  Ctrl+K or Ctrl+Shift+P",
                "Save                      Ctrl+S"
            ]
        );
        assert!(!wide.iter().any(|line| line.contains('|')));
        let narrow = rendered(markdown, 30);
        for line in &narrow {
            assert!(display_width(line) <= 30, "{line:?} in {narrow:#?}");
        }
        assert!(
            narrow.iter().any(|line| line.ends_with("Ctrl+Shift+P")),
            "a key is never split: {narrow:#?}"
        );
        assert_eq!(
            narrow,
            [
                "To             Press",
                "─────────────  ───────────────",
                "Open the       Ctrl+K or",
                "command        Ctrl+Shift+P",
                "palette",
                "",
                "Save           Ctrl+S",
            ],
            "rows that wrap are kept apart"
        );
    }

    #[test]
    fn only_rows_that_wrap_are_set_apart() {
        let markdown = "| A | B |\n| --- | --- |\n| one | x |\n| two | x |\n| a row whose text is much too long to fit | x |\n| three | x |\n| four | x |\n";
        let text = rendered(markdown, 24);
        assert_eq!(
            text,
            [
                "A                      B",
                "─────────────────────  ─",
                "one                    x",
                "two                    x",
                "",
                "a row whose text is    x",
                "much too long to fit",
                "",
                "three                  x",
                "four                   x",
            ]
        );
    }

    #[test]
    fn header_cells_are_strong_and_code_blocks_keep_their_lines() {
        let lines = layout(
            "| A | B |\n|---|---|\n| x | y |\n\n```\nlet a = 1;\n  indented\n```\n",
            40,
        );
        assert!(
            lines[0]
                .spans
                .iter()
                .all(|(range, mark)| mark.strong || lines[0].text[range.clone()].trim().is_empty())
        );
        let text = lines
            .iter()
            .map(|line| line.text.as_str())
            .collect::<Vec<_>>();
        assert_eq!(&text[4..], ["  let a = 1;", "    indented"]);
        assert_eq!(lines[4].spans[0].1.kind, Kind::CodeBlock);
    }

    #[test]
    fn every_line_fits_and_blocks_are_numbered_in_order() {
        for page in crate::help::PAGES {
            for width in [24, 60, 100] {
                let lines = layout(page.template, width);
                let mut block = 0;
                for line in &lines {
                    assert!(
                        display_width(&line.text) <= width.max(20),
                        "{}: {:?} is wider than {width}",
                        page.id,
                        line.text
                    );
                    assert!(line.block >= block);
                    block = line.block;
                }
            }
        }
    }
}
