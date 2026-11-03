use std::path::Path;

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// One keybinding entry.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Keybinding {
    pub key: String,
    pub command: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub when: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub args: Option<Value>,
}

/// Command metadata exposed by the palette.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct CommandInfo {
    pub label: String,
    pub category: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Other words the palette finds the command by, such as "new terminal".
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub keywords: Vec<String>,
}

pub fn load_keybindings(path: impl AsRef<Path>) -> Result<Vec<Keybinding>, crate::ConfigError> {
    crate::load_jsonc(path)
}

pub fn load_commands(
    path: impl AsRef<Path>,
) -> Result<std::collections::BTreeMap<String, CommandInfo>, crate::ConfigError> {
    crate::load_jsonc(path)
}

pub use crate::theme::ThemeCatalog;
