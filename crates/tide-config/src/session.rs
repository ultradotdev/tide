use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Recursive pane layout persisted in a session.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionLayoutNode {
    #[serde(rename = "type")]
    pub node_type: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pane_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub children: Option<Vec<Self>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ratios: Option<Vec<f64>>,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

/// Persisted document state.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionDocumentState {
    pub file_path: String,
    #[serde(default)]
    pub scroll_top: u32,
    #[serde(default)]
    pub scroll_left: u32,
    #[serde(default)]
    pub cursor_line: u32,
    #[serde(default)]
    pub cursor_column: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub selection_anchor_line: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub selection_anchor_column: Option<u32>,
    #[serde(default)]
    pub folded_regions: Vec<u32>,
    pub pane_id: String,
    #[serde(default)]
    pub tab_order: u32,
    #[serde(default)]
    pub is_active_in_pane: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unsaved_content: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub undo_history: Option<Value>,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

/// Persisted terminal-pane state.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionTerminalState {
    pub element_id: String,
    pub pane_id: String,
    #[serde(default)]
    pub tab_order: u32,
    #[serde(default)]
    pub is_active_in_pane: bool,
    #[serde(default)]
    pub cwd: String,
    #[serde(default)]
    pub title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub semantic_kind: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub durability: Option<String>,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

/// Persisted bottom-dock terminal state.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionBottomTerminalState {
    #[serde(default)]
    pub tab_id: String,
    #[serde(default)]
    pub tab_order: u32,
    #[serde(default)]
    pub is_active: bool,
    #[serde(default)]
    pub cwd: String,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub semantic_kind: String,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

/// Persisted external AI CLI pane state.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionAICLIPaneState {
    pub element_id: String,
    pub pane_id: String,
    #[serde(default)]
    pub tab_order: u32,
    #[serde(default)]
    pub is_active_in_pane: bool,
    pub provider: String,
    #[serde(default)]
    pub session_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_key: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(default)]
    pub cwd: String,
    #[serde(default)]
    pub title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input_history: Option<Vec<String>>,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

/// Persisted built-in AI Chat pane state.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionAIChatPaneState {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub conversation_id: Option<String>,
    pub element_id: String,
    pub pane_id: String,
    #[serde(default)]
    pub tab_order: u32,
    #[serde(default)]
    pub is_active_in_pane: bool,
    pub provider: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(default)]
    pub messages: Vec<Value>,
    #[serde(default)]
    pub title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub composer_draft: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scroll_top: Option<u32>,
    #[serde(default)]
    pub interrupted_turn: bool,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

/// Persisted SQL editor state.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionSQLEditorState {
    pub element_id: String,
    pub pane_id: String,
    #[serde(default)]
    pub tab_order: u32,
    #[serde(default)]
    pub is_active_in_pane: bool,
    #[serde(default)]
    pub file_path: Option<String>,
    #[serde(default)]
    pub content: String,
    #[serde(default)]
    pub connection_id: Option<String>,
    #[serde(default)]
    pub cursor_line: u32,
    #[serde(default)]
    pub cursor_column: u32,
    #[serde(default)]
    pub scroll_top: u32,
    #[serde(default)]
    pub title: String,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

/// Persisted workbench UI state.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionUIState {
    #[serde(default)]
    pub sidebar_visible: bool,
    #[serde(default)]
    pub sidebar_width: u16,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sidebar_location: Option<String>,
    #[serde(default)]
    pub agents_sidebar_visible: bool,
    #[serde(default)]
    pub agents_sidebar_width: u16,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agents_sidebar_location: Option<String>,
    #[serde(default)]
    pub agents_sidebar_expanded_providers: Vec<String>,
    #[serde(default)]
    pub agents_sidebar_expanded_sections: Vec<String>,
    #[serde(default)]
    pub terminal_visible: bool,
    #[serde(default)]
    pub terminal_height: u16,
    #[serde(default)]
    pub git_panel_visible: bool,
    #[serde(default)]
    pub git_panel_width: u16,
    #[serde(default)]
    pub active_sidebar_panel: String,
    #[serde(default)]
    pub minimap_enabled: bool,
    #[serde(default)]
    pub ai_panel_visible: bool,
    #[serde(default)]
    pub ai_panel_width: u16,
    #[serde(default)]
    pub sidebar_accordion_expanded: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub focused_region: Option<String>,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

/// Complete versioned session state.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionState {
    pub version: u32,
    pub timestamp: String,
    pub instance_id: String,
    pub workspace_root: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_name: Option<String>,
    #[serde(default)]
    pub documents: Vec<SessionDocumentState>,
    #[serde(default)]
    pub terminals: Vec<SessionTerminalState>,
    #[serde(default)]
    pub bottom_terminals: Vec<SessionBottomTerminalState>,
    #[serde(default)]
    pub ai_cli_panes: Vec<SessionAICLIPaneState>,
    #[serde(default)]
    pub ai_chat_panes: Vec<SessionAIChatPaneState>,
    #[serde(default)]
    pub sql_editors: Vec<SessionSQLEditorState>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub active_document_path: Option<String>,
    pub active_pane_id: String,
    pub layout: SessionLayoutNode,
    pub ui: SessionUIState,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

impl SessionState {
    /// True when the session contains state the Rust MVP does not render/manage.
    #[must_use]
    pub fn has_unsupported_content(&self) -> bool {
        !self.extra.is_empty()
            || !self.ui.extra.is_empty()
            || self
                .documents
                .iter()
                .any(|document| !document.extra.is_empty())
            || self
                .terminals
                .iter()
                .any(|terminal| !terminal.extra.is_empty())
            || self
                .bottom_terminals
                .iter()
                .any(|terminal| !terminal.extra.is_empty())
    }

    /// Preserve state the Rust app does not manage when refreshing a session.
    pub fn preserve_unsupported_from(&mut self, previous: &Self) {
        if self.ai_cli_panes.is_empty() {
            self.ai_cli_panes = previous.ai_cli_panes.clone();
        }
        if self.ai_chat_panes.is_empty() {
            self.ai_chat_panes = previous.ai_chat_panes.clone();
        }
        if self.sql_editors.is_empty() {
            self.sql_editors = previous.sql_editors.clone();
        }
        for (key, value) in &previous.extra {
            self.extra
                .entry(key.clone())
                .or_insert_with(|| value.clone());
        }
        for (key, value) in &previous.ui.extra {
            self.ui
                .extra
                .entry(key.clone())
                .or_insert_with(|| value.clone());
        }
    }

    /// Refuse unknown future versions rather than silently misinterpreting them.
    pub fn validate_version(&self) -> Result<(), crate::ConfigError> {
        if self.version == 7 {
            Ok(())
        } else {
            Err(crate::ConfigError::UnsupportedSessionVersion(self.version))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_v7_fixture() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../tests/fixtures/rust-parity/session-v7.json");
        let original: SessionState = crate::load_jsonc(path).unwrap();
        original.validate_version().unwrap();
        let text = serde_json::to_string(&original).unwrap();
        let round_trip: SessionState = serde_json::from_str(&text).unwrap();
        assert_eq!(original, round_trip);
    }
}
