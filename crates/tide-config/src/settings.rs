use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// User/default settings as a flat JSON object, matching Tide's JSONC format.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Settings {
    values: BTreeMap<String, Value>,
}

impl Settings {
    #[must_use]
    pub fn new(values: BTreeMap<String, Value>) -> Self {
        Self { values }
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.values.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.values.is_empty()
    }

    #[must_use]
    pub fn get(&self, key: &str) -> Option<&Value> {
        self.values.get(key)
    }

    pub fn set(&mut self, key: impl Into<String>, value: Value) {
        self.values.insert(key.into(), value);
    }

    /// Merge a partial settings object over this one, returning the changed keys.
    pub fn merge(&mut self, other: &Settings) -> Vec<String> {
        let mut changed = Vec::new();
        for (key, value) in &other.values {
            if self.values.get(key) != Some(value) {
                self.values.insert(key.clone(), value.clone());
                changed.push(key.clone());
            }
        }
        changed
    }

    pub fn iter(&self) -> impl Iterator<Item = (&String, &Value)> {
        self.values.iter()
    }

    #[must_use]
    pub fn as_map(&self) -> &BTreeMap<String, Value> {
        &self.values
    }
}

impl From<BTreeMap<String, Value>> for Settings {
    fn from(values: BTreeMap<String, Value>) -> Self {
        Self::new(values)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn merge_reports_changed_keys() {
        let mut base = Settings::default();
        base.set("editor.tabSize", Value::from(2));
        let mut user = Settings::default();
        user.set("editor.tabSize", Value::from(4));
        user.set("editor.wordWrap", Value::from("on"));

        let changed = base.merge(&user);
        assert_eq!(changed.len(), 2);
        assert_eq!(base.get("editor.tabSize"), Some(&Value::from(4)));
    }
}
