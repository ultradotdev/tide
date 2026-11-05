use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, List, ListItem};

use crate::palette::{IconTint, Palette};

#[derive(Debug, Clone)]
pub struct FileEntry {
    pub path: PathBuf,
    pub name: String,
    pub depth: usize,
    pub is_dir: bool,
    pub is_symlink: bool,
    pub expanded: bool,
}

#[derive(Debug, Clone)]
pub struct FileTreeState {
    pub root: PathBuf,
    pub entries: Vec<FileEntry>,
    pub selected: usize,
    expanded: HashSet<PathBuf>,
    show_hidden: bool,
    scroll_top: usize,
    view_height: usize,
    exclusions: Vec<String>,
    ascii_icons: bool,
    decorations: HashMap<PathBuf, String>,
    pub errors: Vec<String>,
}

impl FileTreeState {
    #[must_use]
    pub fn new(root: impl Into<PathBuf>) -> Self {
        let mut tree = Self {
            root: root.into(),
            entries: Vec::new(),
            selected: 0,
            expanded: HashSet::new(),
            show_hidden: false,
            scroll_top: 0,
            view_height: 0,
            ascii_icons: false,
            exclusions: vec![
                "**/.git".into(),
                "**/node_modules".into(),
                "**/.DS_Store".into(),
            ],
            decorations: HashMap::new(),
            errors: Vec::new(),
        };
        tree.refresh();
        tree
    }

    pub fn refresh(&mut self) {
        let selected_path = self
            .entries
            .get(self.selected)
            .map(|entry| entry.path.clone());
        self.entries.clear();
        self.errors.clear();
        self.walk(&self.root.clone(), 0);
        if let Some(path) = selected_path
            && let Some(index) = self.entries.iter().position(|entry| entry.path == path)
        {
            self.selected = index;
        }
        self.selected = self.selected.min(self.entries.len().saturating_sub(1));
    }

    fn walk(&mut self, directory: &Path, depth: usize) {
        if depth > 64 || self.entries.len() >= 50_000 {
            return;
        }
        let read_dir = match std::fs::read_dir(directory) {
            Ok(entries) => entries,
            Err(error) => {
                self.errors
                    .push(format!("{}: {error}", directory.display()));
                return;
            }
        };
        let mut children: Vec<_> = read_dir.take(50_001).flatten().collect();
        children.sort_by(|a, b| {
            let a_dir = a.file_type().map(|kind| kind.is_dir()).unwrap_or(false);
            let b_dir = b.file_type().map(|kind| kind.is_dir()).unwrap_or(false);
            b_dir.cmp(&a_dir).then_with(|| {
                a.file_name()
                    .to_string_lossy()
                    .to_lowercase()
                    .cmp(&b.file_name().to_string_lossy().to_lowercase())
            })
        });
        let excludes = exclude_rules(&self.root, &self.exclusions);
        for child in children {
            if self.entries.len() >= 50_000 {
                break;
            }
            let path = child.path();
            let name = child.file_name().to_string_lossy().to_string();
            if !self.show_hidden && name.starts_with('.') {
                continue;
            }
            let is_dir = child.file_type().map(|kind| kind.is_dir()).unwrap_or(false);
            if excludes
                .as_ref()
                .is_some_and(|rules| rules.matched(&path, is_dir).is_ignore())
            {
                continue;
            }
            let expanded = is_dir && self.expanded.contains(&path);
            self.entries.push(FileEntry {
                path: path.clone(),
                name,
                depth,
                is_dir,
                is_symlink: child.file_type().is_ok_and(|t| t.is_symlink()),
                expanded,
            });
            if expanded {
                self.walk(&path, depth + 1);
            }
        }
    }

    pub fn set_ascii_icons(&mut self, ascii: bool) {
        self.ascii_icons = ascii;
    }

    pub fn configure(&mut self, exclusions: Vec<String>) {
        if self.exclusions != exclusions {
            self.exclusions = exclusions;
            self.refresh();
        }
    }

    pub fn set_decorations(&mut self, items: impl IntoIterator<Item = (PathBuf, String)>) {
        self.decorations = items.into_iter().collect();
    }

    pub fn snapshot(&self) -> serde_json::Value {
        serde_json::json!({"expanded": self.expanded.iter().filter_map(|p| p.strip_prefix(&self.root).ok()).collect::<Vec<_>>(),
            "selected": self.selected().and_then(|e| e.path.strip_prefix(&self.root).ok()), "scroll": self.scroll_top, "hidden": self.show_hidden})
    }

    pub fn restore(&mut self, value: &serde_json::Value) {
        self.expanded.clear();
        if let Some(paths) = value.get("expanded").and_then(serde_json::Value::as_array) {
            for path in paths
                .iter()
                .take(10_000)
                .filter_map(serde_json::Value::as_str)
            {
                let path = Path::new(path);
                if !path.is_absolute()
                    && path
                        .components()
                        .all(|part| matches!(part, std::path::Component::Normal(_)))
                {
                    self.expanded.insert(self.root.join(path));
                }
            }
        }
        self.show_hidden = value
            .get("hidden")
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(false);
        self.refresh();
        if let Some(path) = value.get("selected").and_then(serde_json::Value::as_str) {
            let path = self.root.join(path);
            if let Some(index) = self.entries.iter().position(|entry| entry.path == path) {
                self.selected = index;
            }
        }
        self.scroll_top = value
            .get("scroll")
            .and_then(serde_json::Value::as_u64)
            .map_or(0, tide_core::num::saturating_usize);
    }

    pub fn select_first(&mut self) {
        self.selected = 0;
    }

    pub fn select_last(&mut self) {
        self.selected = self.entries.len().saturating_sub(1);
    }

    pub fn page_up(&mut self, amount: usize) {
        self.selected = self.selected.saturating_sub(amount);
    }

    pub fn page_down(&mut self, amount: usize) {
        self.selected = (self.selected + amount).min(self.entries.len().saturating_sub(1));
    }

    pub fn move_up(&mut self) {
        if self.selected > 0 {
            self.selected -= 1;
        }
    }

    pub fn move_down(&mut self) {
        if self.selected + 1 < self.entries.len() {
            self.selected += 1;
        }
    }

    #[must_use]
    pub fn selected(&self) -> Option<&FileEntry> {
        self.entries.get(self.selected)
    }

    pub fn expand_selected(&mut self) {
        let Some(entry) = self.entries.get(self.selected).cloned() else {
            return;
        };
        if entry.is_dir {
            self.expanded.insert(entry.path);
            self.refresh();
        }
    }

    pub fn collapse_selected(&mut self) {
        let Some(entry) = self.entries.get(self.selected).cloned() else {
            return;
        };
        if entry.is_dir && entry.expanded {
            self.expanded.remove(&entry.path);
            self.refresh();
        } else if let Some(parent) = entry.path.parent()
            && let Some(index) = self.entries.iter().position(|item| item.path == parent)
        {
            self.selected = index;
        }
    }

    /// Expands the folders down to `path`, `path` too when it is a folder, and selects it.
    /// Returns false when `path` is outside the tree or hidden by its filters.
    pub fn reveal(&mut self, path: &Path) -> bool {
        let Ok(relative) = path.strip_prefix(&self.root) else {
            return false;
        };
        let mut folder = self.root.clone();
        for part in relative.components() {
            folder.push(part);
            if folder.is_dir() {
                self.expanded.insert(folder.clone());
            }
        }
        self.refresh();
        let Some(index) = self.entries.iter().position(|entry| entry.path == path) else {
            return false;
        };
        self.selected = index;
        true
    }

    /// Returns the selected file path when the entry is a file.
    pub fn activate_selected(&mut self) -> Option<PathBuf> {
        let entry = self.entries.get(self.selected)?.clone();
        if entry.is_dir {
            if entry.expanded {
                self.expanded.remove(&entry.path);
                self.refresh();
            } else {
                self.expanded.insert(entry.path);
                self.refresh();
            }
            None
        } else {
            Some(entry.path)
        }
    }

    pub fn toggle_hidden(&mut self) {
        self.show_hidden = !self.show_hidden;
        self.refresh();
    }

    pub fn select_at_row(&mut self, row: usize) -> bool {
        let index = self.scroll_top.saturating_add(row);
        if row >= self.view_height || index >= self.entries.len() {
            return false;
        }
        self.selected = index;
        true
    }

    pub fn scroll_by(&mut self, amount: isize) {
        let height = self.view_height.max(1);
        self.scroll_top = self
            .scroll_top
            .saturating_add_signed(amount)
            .min(self.entries.len().saturating_sub(height));
        self.selected = self
            .selected
            .max(self.scroll_top)
            .min(self.scroll_top + height - 1)
            .min(self.entries.len().saturating_sub(1));
    }

    pub fn scroll_metrics(&self) -> (usize, usize, usize) {
        (self.entries.len(), self.view_height, self.scroll_top)
    }
    pub fn jump_scroll(&mut self, offset: usize) {
        let height = self.view_height.max(1);
        self.scroll_top = offset.min(self.entries.len().saturating_sub(height));
        self.selected = self
            .selected
            .clamp(self.scroll_top, self.scroll_top + height - 1)
            .min(self.entries.len().saturating_sub(1));
    }
    /// Lays out and draws the tree with its own border.
    pub fn draw(&mut self, frame: &mut Frame, area: Rect, palette: Palette, focused: bool) {
        let block = Block::bordered()
            .title(" Explorer ")
            .border_style(if focused {
                Style::default().fg(palette.border_focused)
            } else {
                Style::default().fg(palette.border)
            })
            .style(Style::default().bg(palette.sidebar));
        let inner = block.inner(area);
        frame.render_widget(block, area);
        self.layout_content(inner);
        self.render_content(frame, inner, palette, focused);
    }

    /// Fits the scroll position to `area`, the content area the next render draws into.
    pub fn layout_content(&mut self, area: Rect) {
        self.view_height = area.height.saturating_sub(u16::from(area.height > 2)) as usize;
        let height = self.view_height.max(1);
        if self.selected < self.scroll_top {
            self.scroll_top = self.selected;
        } else if self.selected >= self.scroll_top + height {
            self.scroll_top = self.selected + 1 - height;
        }
        self.scroll_top = self
            .scroll_top
            .min(self.entries.len().saturating_sub(height));
    }

    /// Render the tree without its own border. Used by the workspace accordion.
    pub fn render_content(&self, frame: &mut Frame, area: Rect, palette: Palette, focused: bool) {
        let _ = focused;
        let content_area = Rect::new(
            area.x,
            area.y,
            area.width,
            area.height.saturating_sub(u16::from(area.height > 2)),
        );
        if area.height > 2 {
            let hint = self.errors.first().map_or(
                "Enter open · ←/→ folders · . hidden · r refresh",
                String::as_str,
            );
            frame.render_widget(
                ratatui::widgets::Paragraph::new(hint)
                    .style(Style::default().fg(palette.description)),
                Rect::new(area.x, area.bottom() - 1, area.width, 1),
            );
        }
        let area = content_area;
        let items = self
            .entries
            .iter()
            .enumerate()
            .skip(self.scroll_top)
            .take(self.view_height)
            .map(|(index, entry)| {
                let icon = if entry.is_dir {
                    if entry.expanded {
                        if self.ascii_icons { "v" } else { "▾" }
                    } else if self.ascii_icons {
                        ">"
                    } else {
                        "▸"
                    }
                } else {
                    " "
                };
                let (kind, tint) = file_icon(entry, self.ascii_icons);
                let decoration = self.decorations.get(&entry.path).map_or("", String::as_str);
                let lead = format!("{}{} ", "  ".repeat(entry.depth), icon);
                let text = format!(" {} {}", entry.name, decoration);
                let style = if index == self.selected {
                    Style::default()
                        .fg(palette.selection_fg)
                        .bg(palette.selection_bg)
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default()
                        .fg(match decoration {
                            "M" | "T" => palette.git_modified,
                            "?" => palette.git_untracked,
                            "A" => palette.diff_add,
                            "D" => palette.diff_delete,
                            _ => palette.foreground,
                        })
                        .bg(palette.sidebar)
                };
                let background = style.bg.unwrap_or(palette.sidebar);
                let kind_style = palette
                    .icon(tint, background)
                    .map_or(style, |color| style.fg(color));
                ListItem::new(Line::from(vec![
                    Span::styled(lead, style),
                    Span::styled(kind, kind_style),
                    Span::styled(text, style),
                ]))
            })
            .collect::<Vec<_>>();
        if items.is_empty() {
            frame.render_widget(
                ratatui::widgets::Paragraph::new(if self.errors.is_empty() {
                    "No files · . hidden · Enter expand/open".to_owned()
                } else {
                    self.errors.join("\n")
                })
                .style(Style::default().fg(palette.description)),
                area,
            );
            return;
        }
        frame.render_widget(
            List::new(items).style(Style::default().bg(palette.sidebar)),
            area,
        );
        crate::scrollbar::paint(
            frame,
            area,
            self.entries.len(),
            self.view_height,
            self.scroll_top,
            palette,
        );
    }
}

/// The Nerd Font glyph and theme colour for an Explorer row, or an empty glyph in ASCII
/// mode. Each glyph takes one cell. In a non-Mono Nerd Font they are drawn wider and
/// spill into the space after them, so every glyph here stays under 1.5 cells wide there:
/// Font Awesome's folders and link (1.5 to 1.8 cells) were cut off in some terminals.
fn file_icon(entry: &FileEntry, ascii: bool) -> (&'static str, IconTint) {
    use IconTint::{Blue, Cyan, Green, Magenta, Orange, Plain, Red, Yellow};
    if ascii {
        return ("", Plain);
    }
    if entry.is_symlink {
        return ("\u{eaee}", Cyan); // cod-file_symlink_file
    }
    if entry.is_dir {
        return if entry.expanded {
            ("\u{e5fe}", Yellow) // custom-folder_open
        } else {
            ("\u{e5ff}", Yellow) // custom-folder
        };
    }
    let name = entry.name.to_ascii_lowercase();
    let by_name = match name.as_str() {
        "dockerfile"
        | "containerfile"
        | "docker-compose.yml"
        | "docker-compose.yaml"
        | "compose.yml"
        | "compose.yaml" => Some(("\u{e7b0}", Blue)), // dev-docker
        "makefile" | "gnumakefile" | "justfile" => Some(("\u{e673}", Plain)), // seti-makefile
        ".gitignore" | ".gitattributes" | ".gitmodules" | ".gitkeep" => {
            Some(("\u{e702}", Orange)) // dev-git
        }
        "license" | "license.md" | "license.txt" | "licence" | "copying" => {
            Some(("\u{e60a}", Yellow)) // seti-license
        }
        "cargo.toml" | "cargo.lock" => Some(("\u{e7a8}", Orange)), // dev-rust
        "package.json" | "package-lock.json" | ".npmrc" => Some(("\u{e71e}", Red)), // dev-npm
        "tsconfig.json" | "jsconfig.json" => Some(("\u{e69d}", Blue)), // seti-tsconfig
        ".editorconfig" => Some(("\u{e652}", Plain)),              // seti-editorconfig
        _ if name == ".env" || name.starts_with(".env.") => Some(("\u{e615}", Yellow)), // seti-config
        _ => None,
    };
    if let Some(icon) = by_name {
        return icon;
    }
    let extension = name.rsplit_once('.').map_or("", |(_, extension)| extension);
    match extension {
        "ts" | "mts" | "cts" => ("\u{e628}", Blue), // seti-typescript
        "tsx" => ("\u{e7ba}", Blue),                // dev-react
        "jsx" => ("\u{e7ba}", Yellow),
        "js" | "mjs" | "cjs" => ("\u{e74e}", Yellow), // dev-javascript_alt
        "json" | "jsonc" | "json5" => ("\u{e60b}", Green), // seti-json
        "md" | "mdx" | "markdown" => ("\u{e73e}", Plain), // dev-markdown
        "rs" => ("\u{e7a8}", Orange),                 // dev-rust
        "py" | "pyi" => ("\u{e73c}", Blue),           // dev-python
        "rb" => ("\u{e739}", Red),                    // dev-ruby
        "go" => ("\u{e626}", Cyan),                   // custom-go
        "java" => ("\u{e738}", Orange),               // dev-java
        "kt" | "kts" => ("\u{e634}", Magenta),        // seti-kotlin
        "swift" => ("\u{e755}", Orange),              // dev-swift
        "c" => ("\u{e61e}", Blue),                    // custom-c
        "h" => ("\u{e61e}", Magenta),
        "cpp" | "cc" | "cxx" | "hpp" | "hh" | "hxx" => ("\u{e61d}", Blue), // custom-cpp
        "cs" => ("\u{e648}", Magenta),                                     // seti-c_sharp
        "php" => ("\u{e608}", Magenta),                                    // seti-php
        "dart" => ("\u{e64c}", Blue),                                      // seti-dart
        "ex" | "exs" => ("\u{e62d}", Magenta),                             // seti-elixir
        "hs" => ("\u{e61f}", Magenta),                                     // seti-haskell
        "scala" => ("\u{e68e}", Red),                                      // seti-scala
        "zig" => ("\u{e6a9}", Orange),                                     // seti-zig
        "lua" => ("\u{e620}", Blue),                                       // seti-lua
        "r" => ("\u{e68a}", Blue),                                         // seti-r
        "vim" => ("\u{e62b}", Green),                                      // custom-vim
        "html" | "htm" => ("\u{e736}", Orange),                            // dev-html5
        "css" => ("\u{e749}", Blue),                                       // dev-css3
        "scss" | "sass" => ("\u{e74b}", Magenta),                          // dev-sass
        "less" => ("\u{e758}", Magenta),                                   // dev-less
        "vue" => ("\u{e6a0}", Green),                                      // seti-vue
        "svelte" => ("\u{e697}", Orange),                                  // seti-svelte
        "graphql" | "gql" => ("\u{e662}", Magenta),                        // seti-graphql
        "sql" => ("\u{e706}", Yellow),                                     // dev-database
        "sh" | "bash" | "zsh" | "fish" => ("\u{e795}", Green),             // dev-terminal
        "yaml" | "yml" => ("\u{e6a8}", Magenta),                           // seti-yml
        "toml" => ("\u{e6b2}", Plain),                                     // custom-toml
        "xml" => ("\u{e619}", Orange),                                     // seti-xml
        "csv" | "tsv" => ("\u{e64a}", Green),                              // seti-csv
        "tf" | "tfvars" => ("\u{e69a}", Magenta),                          // seti-terraform
        "tex" => ("\u{e69b}", Plain),                                      // seti-tex
        "txt" | "log" => ("\u{e64e}", Plain),                              // seti-text
        "lock" => ("\u{e672}", Plain),                                     // seti-lock
        "svg" => ("\u{e698}", Yellow),                                     // seti-svg
        "png" | "jpg" | "jpeg" | "gif" | "webp" | "ico" | "bmp" | "avif" => {
            ("\u{e60d}", Green) // seti-image
        }
        "mp4" | "mov" | "webm" | "mkv" => ("\u{e69f}", Magenta), // seti-video
        "mp3" | "wav" | "flac" | "ogg" | "m4a" => ("\u{e638}", Magenta), // seti-audio
        "ttf" | "otf" | "woff" | "woff2" => ("\u{e659}", Plain), // seti-font
        "pdf" => ("\u{e67d}", Red),                              // seti-pdf
        "zip" | "tar" | "gz" | "tgz" | "xz" | "zst" | "bz2" | "7z" | "rar" => {
            ("\u{e6aa}", Yellow) // seti-zip
        }
        _ => ("\u{e612}", Plain), // custom-default
    }
}

/// Matches the paths `files.exclude` hides, as globs relative to `root`. The Explorer,
/// Quick Open and Find in Files all leave them out.
pub(crate) fn exclude_rules(
    root: &Path,
    patterns: &[String],
) -> Option<ignore::overrides::Override> {
    let mut rules = ignore::overrides::OverrideBuilder::new(root);
    for pattern in patterns {
        let _ = rules.add(&format!("!{pattern}"));
    }
    rules.build().ok()
}

/// A walk of the workspace for file lists and searches: it honours `.gitignore`, the
/// repository's exclude file, the user's global gitignore and `files.exclude`, and never
/// enters `.git`.
pub(crate) fn workspace_walk(
    root: &Path,
    start: &Path,
    patterns: &[String],
) -> ignore::WalkBuilder {
    let excludes = exclude_rules(root, patterns);
    let mut builder = ignore::WalkBuilder::new(start);
    builder
        .hidden(false)
        .git_ignore(true)
        .git_global(true)
        .git_exclude(true)
        .filter_entry(move |entry| {
            entry.file_name() != ".git"
                && !excludes.as_ref().is_some_and(|rules| {
                    rules
                        .matched(
                            entry.path(),
                            entry.file_type().is_some_and(|kind| kind.is_dir()),
                        )
                        .is_ignore()
                })
        });
    builder
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reveal_expands_the_folders_down_to_a_path_and_selects_it() {
        let root = tempfile::tempdir().unwrap();
        let folder = root.path().join("crates/app/src");
        std::fs::create_dir_all(&folder).unwrap();
        std::fs::write(folder.join("main.rs"), "").unwrap();
        std::fs::create_dir(root.path().join(".hidden")).unwrap();
        let mut tree = FileTreeState::new(root.path());
        assert!(tree.reveal(&root.path().join("crates/app")));
        assert_eq!(
            tree.selected().unwrap().path,
            root.path().join("crates/app")
        );
        assert!(tree.selected().unwrap().expanded);
        assert!(tree.entries.iter().any(|entry| entry.path == folder));
        assert!(!tree.reveal(&root.path().join(".hidden")));
        assert!(!tree.reveal(Path::new("/elsewhere")));
    }

    #[test]
    fn native_folder_type_and_symlink_icons_have_one_cell_and_ascii_remains_available() {
        let root = tempfile::tempdir().unwrap();
        std::fs::create_dir(root.path().join("folder")).unwrap();
        for name in [
            "main.rs",
            "main.ts",
            "script.js",
            "data.json",
            "README.md",
            "plain.txt",
        ] {
            std::fs::write(root.path().join(name), "").unwrap();
        }
        #[cfg(unix)]
        std::os::unix::fs::symlink("plain.txt", root.path().join("link")).unwrap();
        let mut tree = FileTreeState::new(root.path());
        let icons = tree
            .entries
            .iter()
            .map(|entry| file_icon(entry, false).0)
            .collect::<Vec<_>>();
        assert!(icons.contains(&"\u{e5ff}"));
        assert!(icons.contains(&"\u{e628}"));
        assert!(icons.contains(&"\u{e7a8}"));
        assert!(icons.contains(&"\u{e74e}"));
        assert!(icons.contains(&"\u{e60b}"));
        assert!(icons.contains(&"\u{e73e}"));
        assert!(icons.iter().all(|icon| tide_core::display_width(icon) == 1));
        assert!(
            tree.entries
                .iter()
                .all(|entry| file_icon(entry, true).0.is_empty())
        );
        tree.select_first();
        tree.expand_selected();
        assert_eq!(file_icon(tree.selected().unwrap(), false).0, "\u{e5fe}");
    }

    fn file(name: &str) -> FileEntry {
        FileEntry {
            path: PathBuf::from(name),
            name: name.to_owned(),
            depth: 0,
            is_dir: false,
            is_symlink: false,
            expanded: false,
        }
    }

    #[test]
    fn special_names_win_over_extensions_and_matching_ignores_case() {
        assert_eq!(
            file_icon(&file("Dockerfile"), false),
            ("\u{e7b0}", IconTint::Blue)
        );
        assert_eq!(file_icon(&file("Makefile"), false).0, "\u{e673}");
        assert_eq!(
            file_icon(&file("Cargo.toml"), false),
            ("\u{e7a8}", IconTint::Orange)
        );
        assert_eq!(file_icon(&file("package.json"), false).0, "\u{e71e}");
        assert_eq!(file_icon(&file(".env.local"), false).0, "\u{e615}");
        assert_eq!(file_icon(&file("LICENSE"), false).0, "\u{e60a}");
        assert_eq!(
            file_icon(&file("App.TSX"), false),
            ("\u{e7ba}", IconTint::Blue)
        );
        assert_eq!(file_icon(&file("other.toml"), false).0, "\u{e6b2}");
        assert_eq!(file_icon(&file("archive.tar.gz"), false).0, "\u{e6aa}");
        assert_eq!(
            file_icon(&file("no_extension"), false),
            ("\u{e612}", IconTint::Plain)
        );
    }

    #[test]
    fn every_icon_is_one_cell_wide() {
        for name in [
            "a.ts",
            "a.tsx",
            "a.jsx",
            "a.js",
            "a.json",
            "a.md",
            "a.rs",
            "a.py",
            "a.rb",
            "a.go",
            "a.java",
            "a.kt",
            "a.swift",
            "a.c",
            "a.h",
            "a.cpp",
            "a.cs",
            "a.php",
            "a.dart",
            "a.ex",
            "a.hs",
            "a.scala",
            "a.zig",
            "a.lua",
            "a.r",
            "a.vim",
            "a.html",
            "a.css",
            "a.scss",
            "a.less",
            "a.vue",
            "a.svelte",
            "a.graphql",
            "a.sql",
            "a.sh",
            "a.yml",
            "a.toml",
            "a.xml",
            "a.csv",
            "a.tf",
            "a.tex",
            "a.txt",
            "a.lock",
            "a.svg",
            "a.png",
            "a.mp4",
            "a.mp3",
            "a.ttf",
            "a.pdf",
            "a.zip",
            "Dockerfile",
            "Makefile",
            ".gitignore",
            "LICENSE",
            "Cargo.toml",
            "package.json",
            "tsconfig.json",
            ".editorconfig",
            ".env",
            "plain",
        ] {
            let (icon, _) = file_icon(&file(name), false);
            assert_eq!(tide_core::display_width(icon), 1, "{name}");
        }
    }

    #[test]
    fn icon_colours_come_from_the_theme_and_stay_visible() {
        for theme in tide_config::builtin::themes().iter() {
            let palette = Palette::from_theme(theme);
            for background in [palette.sidebar, palette.selection_bg] {
                assert_eq!(palette.icon(IconTint::Plain, background), None);
                for tint in [
                    IconTint::Red,
                    IconTint::Green,
                    IconTint::Yellow,
                    IconTint::Blue,
                    IconTint::Magenta,
                    IconTint::Cyan,
                    IconTint::Orange,
                ] {
                    let color = palette.icon(tint, background).unwrap();
                    assert!(
                        crate::palette::contrast_ratio(color, background) >= 3.0,
                        "{} {tint:?}",
                        theme.name
                    );
                }
            }
        }
        let mut bare = Palette::ultra_dusk();
        bare.terminal_ansi = [None; 16];
        assert_eq!(bare.icon(IconTint::Blue, bare.sidebar), None);
    }

    #[test]
    fn exclusions_and_expanded_selection_round_trip_without_escaping_workspace() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("src/nested")).unwrap();
        std::fs::write(dir.path().join("src/nested/main.rs"), "fn main(){}").unwrap();
        std::fs::write(dir.path().join("secret.log"), "private").unwrap();
        let mut tree = FileTreeState::new(dir.path());
        tree.configure(vec!["**/*.log".into()]);
        assert!(!tree.entries.iter().any(|entry| entry.name == "secret.log"));
        tree.selected = 0;
        tree.expand_selected();
        tree.selected = 1;
        tree.expand_selected();
        tree.selected = 2;
        let state = tree.snapshot();
        let mut restored = FileTreeState::new(dir.path());
        restored.configure(vec!["**/*.log".into()]);
        restored.restore(&state);
        assert_eq!(restored.selected().unwrap().name, "main.rs");
        assert_eq!(restored.entries.len(), 3);
        restored.restore(&serde_json::json!({"expanded":["../elsewhere", "/tmp"]}));
        assert!(restored.expanded.is_empty());
        tree.expanded.clear();
        let collapsed = tree.snapshot();
        restored.restore(&collapsed);
        assert!(restored.expanded.is_empty());
    }

    #[test]
    fn lists_and_expands_directories() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join("src")).unwrap();
        std::fs::write(dir.path().join("src/main.ts"), "fn").unwrap();
        std::fs::write(dir.path().join("README.md"), "#").unwrap();

        let mut tree = FileTreeState::new(dir.path());
        assert!(tree.entries.iter().any(|entry| entry.name == "src"));
        assert!(tree.entries.iter().any(|entry| entry.name == "README.md"));
        assert!(!tree.entries.iter().any(|entry| entry.name == "main.ts"));

        let index = tree
            .entries
            .iter()
            .position(|entry| entry.name == "src")
            .unwrap();
        tree.selected = index;
        tree.expand_selected();
        assert!(tree.entries.iter().any(|entry| entry.name == "main.ts"));
    }

    #[test]
    fn activates_files() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("file.txt"), "x").unwrap();
        let mut tree = FileTreeState::new(dir.path());
        while tree
            .selected()
            .is_some_and(|entry| entry.name != "file.txt")
        {
            tree.move_down();
        }
        assert!(tree.activate_selected().is_some());
    }
}
