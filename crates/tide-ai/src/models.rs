//! Embedded model catalog and preference handling.
//!
//! The catalog is the same JSON shipped with the TypeScript build so both
//! applications offer the same models and defaults.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// Catalog bundled into the binary.
pub const BUNDLED_MODELS_JSON: &str = include_str!("../../../config/models.json");

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ModelsConfig {
    pub version: u32,
    pub last_updated: String,
    #[serde(default)]
    pub defaults: BTreeMap<String, String>,
    #[serde(default)]
    pub provider_defaults: BTreeMap<String, String>,
    #[serde(default)]
    pub models: Vec<ModelInfo>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ModelInfo {
    pub id: String,
    pub name: String,
    pub provider: String,
    #[serde(default)]
    pub capabilities: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context_window: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_output_tokens: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pricing: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub speed: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub quality: Option<String>,
    #[serde(default)]
    pub available: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub api_transport: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub compatibility: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub deprecated_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub replaced_by: Option<String>,
}

impl ModelInfo {
    pub fn supports(&self, capability: &str) -> bool {
        self.capabilities.iter().any(|item| item == capability)
    }
}

#[derive(Debug, thiserror::Error)]
pub enum RegistryError {
    #[error("invalid models.json: {0}")]
    Invalid(String),
    #[error("model not found: {0}")]
    NotFound(String),
    #[error("io error: {0}")]
    Io(String),
}

#[derive(Debug, Clone)]
pub struct ModelRegistry {
    config: ModelsConfig,
}

impl Default for ModelRegistry {
    fn default() -> Self {
        Self::bundled().expect("bundled models.json is valid")
    }
}

impl ModelRegistry {
    pub fn bundled() -> Result<Self, RegistryError> {
        Self::parse(BUNDLED_MODELS_JSON)
    }

    pub fn parse(json: &str) -> Result<Self, RegistryError> {
        let config: ModelsConfig = serde_json::from_str(json)
            .map_err(|error| RegistryError::Invalid(error.to_string()))?;
        if config.models.is_empty() {
            return Err(RegistryError::Invalid("catalog has no models".to_owned()));
        }
        Ok(Self { config })
    }

    /// Load the user's `models.json`, falling back to the bundled catalog.
    pub fn load_or_bundled(path: &Path) -> Result<Self, RegistryError> {
        match std::fs::read_to_string(path) {
            Ok(text) => Self::parse(&text),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Self::bundled(),
            Err(error) => Err(RegistryError::Io(error.to_string())),
        }
    }

    pub fn config(&self) -> &ModelsConfig {
        &self.config
    }

    pub fn models(&self) -> &[ModelInfo] {
        &self.config.models
    }

    pub fn model(&self, id: &str) -> Option<&ModelInfo> {
        self.config
            .models
            .iter()
            .find(|model| model.id == id)
            .or_else(|| {
                self.config
                    .models
                    .iter()
                    .find(|model| model.name.eq_ignore_ascii_case(id))
            })
    }

    pub fn default_model(&self, role: &str) -> Option<&str> {
        self.config.defaults.get(role).map(String::as_str)
    }

    pub fn provider_default(&self, provider: &str) -> Option<&str> {
        self.config
            .provider_defaults
            .get(provider)
            .map(String::as_str)
            .filter(|value| !value.is_empty())
    }

    pub fn for_provider(&self, provider: &str) -> Vec<&ModelInfo> {
        self.config
            .models
            .iter()
            .filter(|model| model.provider == provider && model.available)
            .collect()
    }

    /// Merge newly shipped catalog entries without overwriting local models.
    pub fn with_catalog_merged(mut self, bundled: &ModelsConfig) -> Self {
        let existing = self
            .config
            .models
            .iter()
            .map(|model| format!("{}:{}", model.provider, model.id))
            .collect::<std::collections::BTreeSet<_>>();
        for model in &bundled.models {
            let key = format!("{}:{}", model.provider, model.id);
            if !existing.contains(&key) {
                self.config.models.push(model.clone());
            }
        }
        for (provider, model) in &bundled.provider_defaults {
            self.config
                .provider_defaults
                .entry(provider.clone())
                .or_insert_with(|| model.clone());
        }
        self
    }

    /// Basic integrity validation for `tide --check-models`.
    pub fn validate(&self) -> Vec<String> {
        let mut problems = Vec::new();
        if self.config.version == 0 {
            problems.push("catalog version is missing".to_owned());
        }
        for (role, model) in &self.config.defaults {
            if self.model(model).is_none() {
                problems.push(format!("default {role} references unknown model {model}"));
            }
        }
        for (provider, model) in &self.config.provider_defaults {
            if model.is_empty() {
                continue;
            }
            if self.for_provider(provider).is_empty() {
                problems.push(format!("provider {provider} has no available models"));
            }
            if self.model(model).is_none() {
                problems.push(format!(
                    "provider default {provider} references unknown model {model}"
                ));
            }
        }
        let mut seen = std::collections::BTreeSet::new();
        for model in &self.config.models {
            if !seen.insert(format!("{}:{}", model.provider, model.id)) {
                problems.push(format!("duplicate model {}:{}", model.provider, model.id));
            }
        }
        problems
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct ModelPreferences {
    #[serde(default)]
    pub chat: Option<String>,
    #[serde(default)]
    pub completion: Option<String>,
    #[serde(default)]
    pub review: Option<String>,
    #[serde(default)]
    pub favourite: Vec<String>,
}

impl ModelPreferences {
    pub fn load(path: &Path) -> Self {
        std::fs::read_to_string(path)
            .ok()
            .and_then(|text| serde_json::from_str(&text).ok())
            .unwrap_or_default()
    }

    pub fn save(&self, path: &Path) -> Result<(), RegistryError> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|error| RegistryError::Io(error.to_string()))?;
        }
        let text = serde_json::to_string_pretty(self)
            .map_err(|error| RegistryError::Io(error.to_string()))?;
        std::fs::write(path, text).map_err(|error| RegistryError::Io(error.to_string()))
    }
}

/// Resolve the `models.json` path inside the Tide config directory.
pub fn models_path(config_dir: &Path) -> PathBuf {
    config_dir.join("models.json")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bundled_catalog_is_valid() {
        let registry = ModelRegistry::bundled().unwrap();
        assert_eq!(registry.validate(), Vec::<String>::new());
        assert!(registry.models().len() >= 30);
        assert_eq!(
            registry.default_model("fast"),
            Some("claude-haiku-4-5-20251001")
        );
    }

    #[test]
    fn provider_lookup_and_defaults_work() {
        let registry = ModelRegistry::default();
        assert_eq!(
            registry.provider_default("anthropic"),
            Some("claude-opus-4-5-20251101")
        );
        assert!(!registry.for_provider("anthropic").is_empty());
        let model = registry.model("claude-sonnet-4-5-20250929").unwrap();
        assert!(model.supports("code"));
        assert!(!model.supports("no-such-capability"));
    }

    #[test]
    fn merging_adds_new_models_without_overwriting() {
        let registry = ModelRegistry::default();
        let count = registry.models().len();
        let mut bundled = registry.config().clone();
        bundled.models.clear();
        bundled.models.push(ModelInfo {
            id: "brand-new".to_owned(),
            name: "Brand New".to_owned(),
            provider: "openai".to_owned(),
            capabilities: vec!["chat".to_owned()],
            context_window: None,
            max_output_tokens: None,
            pricing: None,
            speed: None,
            quality: None,
            available: true,
            api_transport: None,
            compatibility: None,
            deprecated_at: None,
            replaced_by: None,
        });
        let merged = registry.with_catalog_merged(&bundled);
        assert!(merged.model("brand-new").is_some());
        assert_eq!(merged.models().len(), count + 1);
    }

    #[test]
    fn invalid_catalog_is_rejected() {
        assert!(ModelRegistry::parse("{}").is_err());
        assert!(ModelRegistry::parse("not json").is_err());
    }
}
