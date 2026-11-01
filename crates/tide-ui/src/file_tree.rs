use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, List, ListItem};

use crate::palette::Palette;

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
        let mut excludes = ignore::overrides::OverrideBuilder::new(&self.root);
        for pattern in &self.exclusions {
            let _ = excludes.add(&format!("!{pattern}"));
        }
        let excludes = excludes.build().ok();
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
                let kind = file_icon(entry, self.ascii_icons);
                let decoration = self.decorations.get(&entry.path).map_or("", String::as_str);
                let text = format!(
                    "{}{} {} {} {}",
                    "  ".repeat(entry.depth),
                    icon,
                    kind,
                    entry.name,
                    decoration
                );
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
                ListItem::new(Line::from(Span::styled(text, style)))
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

fn file_icon(entry: &FileEntry, ascii: bool) -> &'static str {
    if ascii {
        return "";
    }
    if entry.is_symlink {
        return "\u{f0c1}";
    }
    if entry.is_dir {
        return if entry.expanded {
            "\u{f07c}"
        } else {
            "\u{f07b}"
        };
    }
    match entry
        .path
        .extension()
        .and_then(|x| x.to_str())
        .unwrap_or("")
        .to_ascii_lowercase()
        .as_str()
    {
        "ts" | "tsx" => "\u{e628}",
        "js" | "jsx" | "mjs" | "cjs" => "\u{e74e}",
        "json" | "jsonc" => "\u{e60b}",
        "md" | "mdx" => "\u{e609}",
        "rs" => "\u{e7a8}",
        "py" | "pyi" => "\u{e73c}",
        "html" | "htm" => "\u{e736}",
        "css" | "scss" => "\u{e749}",
        "go" => "\u{e627}",
        "sh" | "bash" | "zsh" => "\u{e795}",
        "toml" | "yaml" | "yml" => "\u{e615}",
        _ => "\u{f15b}",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
            .map(|entry| file_icon(entry, false))
            .collect::<Vec<_>>();
        assert!(icons.contains(&"\u{f07b}"));
        assert!(icons.contains(&"\u{e628}"));
        assert!(icons.contains(&"\u{e7a8}"));
        assert!(icons.contains(&"\u{e74e}"));
        assert!(icons.contains(&"\u{e60b}"));
        assert!(icons.contains(&"\u{e609}"));
        assert!(icons.iter().all(|icon| tide_core::display_width(icon) == 1));
        assert!(
            tree.entries
                .iter()
                .all(|entry| file_icon(entry, true).is_empty())
        );
        tree.select_first();
        tree.expand_selected();
        assert_eq!(file_icon(tree.selected().unwrap(), false), "\u{f07c}");
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
