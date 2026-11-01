//! Hierarchical schema navigation over a bounded server catalog snapshot.
use crate::modal::{ModalOutcome, Submitted, default_key, resolve};
use crate::{Modal, ModalBase, ModalItem, ModalKind, palette::Palette};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::{Frame, layout::Rect};
use std::collections::{BTreeSet, HashSet};
use tide_db::CatalogItem;
struct Node {
    id: String,
    parent: Option<String>,
    branch: bool,
    item: ModalItem,
    catalog: Option<CatalogItem>,
}
pub struct SchemaBrowser {
    base: ModalBase,
    items: Vec<CatalogItem>,
    expanded: HashSet<String>,
    nodes: Vec<Node>,
    last_click: Option<(usize, std::time::Instant)>,
}
impl SchemaBrowser {
    pub fn new(items: Vec<CatalogItem>) -> Self {
        let mut value = Self {
            base: ModalBase::new(ModalKind::SchemaTables, "Database Schema", vec![])
                .with_hints("←/→ collapse/expand · Enter open · type to filter"),
            items,
            expanded: HashSet::new(),
            nodes: vec![],
            last_click: None,
        };
        value.rebuild();
        value
    }
    fn rebuild(&mut self) {
        let selected = self.nodes.get(self.base.selected).map(|n| n.id.clone());
        let query = self.base.input.to_lowercase();
        self.nodes.clear();
        let schemas = self
            .items
            .iter()
            .map(|i| i.schema.clone())
            .collect::<BTreeSet<_>>();
        for schema in schemas {
            let relevant = self
                .items
                .iter()
                .filter(|i| {
                    i.schema == schema
                        && (query.is_empty()
                            || format!("{} {} {} {}", i.schema, i.table, i.name, i.detail)
                                .to_lowercase()
                                .contains(&query))
                })
                .cloned()
                .collect::<Vec<_>>();
            if relevant.is_empty() {
                continue;
            }
            let root = format!("schema:{schema}");
            let open = !query.is_empty() || self.expanded.contains(&root);
            self.nodes.push(Node {
                id: root.clone(),
                parent: None,
                branch: true,
                item: ModalItem::new(
                    format!("{} {schema}", if open { "▾" } else { "▸" }),
                    "schema",
                    "",
                ),
                catalog: None,
            });
            if !open {
                continue;
            }
            for (kind, label) in [
                ("table", "Tables"),
                ("view", "Views"),
                ("function", "Functions"),
                ("trigger", "Triggers"),
                ("index", "Indexes"),
                ("policy", "RLS Policies"),
            ] {
                let children = relevant
                    .iter()
                    .filter(|i| i.kind == kind)
                    .collect::<Vec<_>>();
                let id = format!("{root}:{kind}");
                let open = !query.is_empty() || self.expanded.contains(&id);
                self.nodes.push(Node {
                    id: id.clone(),
                    parent: Some(root.clone()),
                    branch: true,
                    item: ModalItem::new(
                        format!(
                            "  {} {label} ({})",
                            if open { "▾" } else { "▸" },
                            children.len()
                        ),
                        "",
                        "",
                    ),
                    catalog: None,
                });
                if open {
                    for item in children {
                        self.nodes.push(Node {
                            id: format!("{id}:{}:{}", item.table, item.name),
                            parent: Some(id.clone()),
                            branch: false,
                            item: ModalItem::new(
                                format!(
                                    "    {}{}",
                                    item.name,
                                    if item.table.is_empty() || item.table == item.name {
                                        String::new()
                                    } else {
                                        format!(" ({})", item.table)
                                    }
                                ),
                                item.detail.clone(),
                                "",
                            ),
                            catalog: Some((*item).clone()),
                        });
                    }
                }
            }
        }
        self.base.items = self.nodes.iter().map(|n| n.item.clone()).collect();
        self.base.filtered = (0..self.nodes.len()).collect();
        self.base.selected = selected
            .and_then(|id| self.nodes.iter().position(|n| n.id == id))
            .unwrap_or(self.base.selected.min(self.nodes.len().saturating_sub(1)));
    }
    fn toggle(&mut self, open: Option<bool>) {
        if let Some(node) = self.nodes.get(self.base.selected) {
            let id = node.id.clone();
            let parent = node.parent.clone();
            if node.branch {
                if open.unwrap_or(!self.expanded.contains(&id)) {
                    self.expanded.insert(id);
                } else {
                    self.expanded.remove(&id);
                }
                self.rebuild();
            } else if open == Some(false)
                && let Some(parent) = parent
                && let Some(index) = self.nodes.iter().position(|n| n.id == parent)
            {
                self.base.selected = index;
            }
        }
    }
}
impl Modal for SchemaBrowser {
    fn base(&self) -> &ModalBase {
        &self.base
    }
    fn base_mut(&mut self) -> &mut ModalBase {
        &mut self.base
    }
    fn paste(&mut self, text: &str) {
        self.base.push_str(text);
        self.rebuild();
    }
    fn submission(&self) -> Submitted {
        self.nodes
            .get(self.base.selected)
            .and_then(|node| node.catalog.clone())
            .map_or(Submitted::Nothing, |item| {
                Submitted::CatalogItem(Box::new(item))
            })
    }
    fn handle_key(&mut self, key: KeyEvent) -> ModalOutcome {
        if (key.code == KeyCode::Enter
            && self.nodes.get(self.base.selected).is_some_and(|n| n.branch))
            || (matches!(key.code, KeyCode::Left | KeyCode::Right) && self.base.input.is_empty())
        {
            self.toggle(match key.code {
                KeyCode::Left => Some(false),
                KeyCode::Right => Some(true),
                _ => None,
            });
            return ModalOutcome::None;
        }
        let before = self.base.input.clone();
        let result = default_key(&mut self.base, key);
        if self.base.input != before {
            self.rebuild();
        }
        resolve(self, result)
    }
    fn click_key(&mut self, bounds: Rect, x: u16, y: u16) -> Option<KeyEvent> {
        if self
            .base
            .list_bounds(bounds)
            .is_some_and(|r| r.contains((x, y).into()))
            && self.base.handle_mouse(bounds, x, y)
        {
            if self.nodes.get(self.base.selected).is_some_and(|n| n.branch) {
                return Some(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
            }
            let now = std::time::Instant::now();
            let double = self.last_click.is_some_and(|(n, at)| {
                n == self.base.selected
                    && now.duration_since(at) < std::time::Duration::from_millis(400)
            });
            self.last_click = Some((self.base.selected, now));
            return double.then(|| KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        }
        self.base.click_key(bounds, x, y)
    }
    fn render(&self, frame: &mut Frame, area: Rect, palette: Palette) {
        self.base.render(frame, area, palette);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn disclosure_search_and_leaf_selection_keep_native_identity() {
        let item = |kind: &str, name: &str| CatalogItem {
            kind: kind.into(),
            schema: "quoted schema".into(),
            table: "table\"name".into(),
            name: name.into(),
            detail: "definition".into(),
        };
        let mut tree = SchemaBrowser::new(vec![
            item("schema", "quoted schema"),
            item("table", "table\"name"),
            item("function", "do_work()"),
        ]);
        assert_eq!(tree.base.items.len(), 1);
        tree.handle_key(KeyEvent::new(KeyCode::Right, KeyModifiers::NONE));
        assert!(
            tree.base
                .items
                .iter()
                .any(|i| i.label.contains("Functions"))
        );
        tree.base.selected = 1;
        tree.handle_key(KeyEvent::new(KeyCode::Right, KeyModifiers::NONE));
        assert!(
            tree.base
                .items
                .iter()
                .any(|i| i.label.contains("table\"name"))
        );
        tree.paste("do_work");
        tree.base.selected = tree
            .base
            .items
            .iter()
            .position(|i| i.label.contains("do_work"))
            .unwrap();
        let ModalOutcome::Submit(Submitted::CatalogItem(item)) =
            tree.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE))
        else {
            panic!("the leaf did not submit its catalog item");
        };
        assert_eq!(item.schema, "quoted schema");
        assert_eq!(item.kind, "function");
    }
}
