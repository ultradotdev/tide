use std::collections::BTreeMap;
use std::path::Path;

use serde::{Deserialize, Serialize};

/// A token-color scope. TextMate themes allow either one scope or a list.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Scope {
    One(String),
    Many(Vec<String>),
}

impl Scope {
    #[must_use]
    pub fn as_slice(&self) -> &[String] {
        match self {
            Self::One(scope) => std::slice::from_ref(scope),
            Self::Many(scopes) => scopes,
        }
    }
}

/// Style settings for a token rule.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct TokenSettings {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub foreground: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub background: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub font_style: Option<String>,
}

/// One syntax token-color rule.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TokenColor {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    pub scope: Scope,
    #[serde(default)]
    pub settings: TokenSettings,
}

/// Theme data as stored in `config/themes/*.json`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ThemeSource {
    pub name: String,
    #[serde(rename = "type")]
    pub theme_type: String,
    #[serde(default)]
    pub colors: BTreeMap<String, String>,
    #[serde(default, rename = "tokenColors")]
    pub token_colors: Vec<TokenColor>,
}

/// A resolved theme with its catalog id.
#[derive(Debug, Clone, PartialEq)]
pub struct Theme {
    pub id: String,
    pub name: String,
    pub theme_type: String,
    pub colors: BTreeMap<String, String>,
    pub token_colors: Vec<TokenColor>,
}

impl Theme {
    #[must_use]
    pub fn from_source(id: impl Into<String>, source: ThemeSource) -> Self {
        Self {
            id: id.into(),
            name: source.name,
            theme_type: source.theme_type,
            colors: source.colors,
            token_colors: source.token_colors,
        }
    }

    #[must_use]
    pub fn color(&self, role: &str) -> Option<&str> {
        self.colors.get(role).map(String::as_str)
    }

    /// Resolve a role or a deterministic fallback.
    #[must_use]
    pub fn color_or<'a>(&'a self, role: &str, fallback: &'a str) -> &'a str {
        self.color(role).unwrap_or(fallback)
    }
}

/// All available themes and the selected theme id.
#[derive(Debug, Clone)]
pub struct ThemeCatalog {
    themes: BTreeMap<String, Theme>,
    selected: String,
}

impl ThemeCatalog {
    #[must_use]
    pub fn new(themes: BTreeMap<String, Theme>, selected: impl Into<String>) -> Self {
        let selected = selected.into();
        let selected = if themes.contains_key(&selected) {
            selected
        } else {
            "ultra-dusk".to_owned()
        };
        Self { themes, selected }
    }

    pub fn from_path(path: impl AsRef<Path>) -> Result<Self, crate::ConfigError> {
        let sources: BTreeMap<String, ThemeSource> = crate::load_jsonc(path)?;
        Ok(Self::from_sources(sources))
    }

    pub fn parse(text: &str) -> Result<Self, crate::ConfigError> {
        Ok(Self::from_sources(crate::parse_jsonc(text)?))
    }

    fn from_sources(sources: BTreeMap<String, ThemeSource>) -> Self {
        let themes = sources
            .into_iter()
            .map(|(id, source)| {
                let theme = Theme::from_source(id.clone(), source);
                (id, theme)
            })
            .collect();
        Self::new(themes, "ultra-dusk")
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.themes.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.themes.is_empty()
    }

    #[must_use]
    pub fn get(&self, id: &str) -> Option<&Theme> {
        self.themes.get(id)
    }

    /// The selected theme. An empty catalog falls back to the built-in Ultra Dusk.
    #[must_use]
    pub fn selected(&self) -> &Theme {
        static FALLBACK: std::sync::LazyLock<Theme> =
            std::sync::LazyLock::new(crate::builtin::fallback_theme);
        self.themes
            .get(&self.selected)
            .or_else(|| self.themes.get("ultra-dusk"))
            .or_else(|| self.themes.values().next())
            .unwrap_or(&FALLBACK)
    }

    #[must_use]
    pub fn selected_id(&self) -> &str {
        &self.selected
    }

    pub fn select(&mut self, id: impl Into<String>) -> bool {
        let id = id.into();
        if self.themes.contains_key(&id) {
            self.selected = id;
            true
        } else {
            false
        }
    }

    pub fn iter(&self) -> impl Iterator<Item = &Theme> {
        self.themes.values()
    }

    /// Load bounded, regular user JSON/JSONC theme files. Invalid files leave
    /// the built-in catalog intact and return diagnostics without file contents.
    pub fn load_user_themes(&mut self, directory: &Path) -> Vec<String> {
        let mut diagnostics = Vec::new();
        let entries = match std::fs::read_dir(directory) {
            Ok(entries) => entries,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return diagnostics,
            Err(_) => return vec!["Could not read user themes directory".into()],
        };
        let mut paths = entries
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .filter(|path| {
                matches!(
                    path.extension().and_then(|value| value.to_str()),
                    Some("json" | "jsonc")
                )
            })
            .take(101)
            .collect::<Vec<_>>();
        if paths.len() > 100 {
            return vec!["User theme limit exceeded (100 files)".into()];
        }
        paths.sort();
        for path in paths {
            let Some(id) = path.file_stem().and_then(|value| value.to_str()) else {
                continue;
            };
            if id.is_empty()
                || id.len() > 128
                || !id
                    .bytes()
                    .all(|c| c.is_ascii_alphanumeric() || b"-_".contains(&c))
            {
                diagnostics.push("Invalid user theme filename".into());
                continue;
            }
            let load = || -> anyhow::Result<ThemeSource> {
                use std::io::Read;
                let metadata = std::fs::symlink_metadata(&path)?;
                anyhow::ensure!(
                    metadata.is_file() && metadata.len() <= 1024 * 1024,
                    "Invalid theme file"
                );
                let mut text = String::new();
                std::fs::File::open(&path)?
                    .take(1024 * 1024 + 1)
                    .read_to_string(&mut text)?;
                anyhow::ensure!(text.len() <= 1024 * 1024, "Theme file is too large");
                let source: ThemeSource = crate::parse_jsonc(&text)?;
                anyhow::ensure!(
                    !source.name.is_empty()
                        && source.name.len() <= 256
                        && !source.name.chars().any(char::is_control),
                    "Invalid theme name"
                );
                anyhow::ensure!(
                    matches!(source.theme_type.as_str(), "dark" | "light"),
                    "Invalid theme type"
                );
                anyhow::ensure!(
                    source.colors.len() <= 2000 && source.token_colors.len() <= 2000,
                    "Too many theme rules"
                );
                Ok(source)
            };
            match load() {
                Ok(source) => {
                    self.themes
                        .insert(id.to_owned(), Theme::from_source(id, source));
                }
                Err(error) => diagnostics.push(format!("Could not load user theme {id}: {error}")),
            }
        }
        diagnostics
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scope_accepts_string_and_array() {
        let one: Scope = serde_json::from_str(r#""comment""#).unwrap();
        let many: Scope = serde_json::from_str(r#"["comment", "punctuation"]"#).unwrap();
        assert_eq!(one.as_slice().len(), 1);
        assert_eq!(many.as_slice().len(), 2);
    }

    #[test]
    fn user_themes_load_without_replacing_builtins_on_invalid_input() {
        let directory = tempfile::tempdir().unwrap();
        std::fs::write(
            directory.path().join("personal.jsonc"),
            r##"{
            // user theme
            "name":"Personal", "type":"dark", "colors":{"editor.background":"#101218"}
        }"##,
        )
        .unwrap();
        std::fs::write(directory.path().join("ultra-dusk.json"), "invalid").unwrap();
        std::fs::write(
            directory.path().join("oversize.json"),
            vec![b' '; 1024 * 1024 + 1],
        )
        .unwrap();
        #[cfg(unix)]
        std::os::unix::fs::symlink(
            directory.path().join("personal.jsonc"),
            directory.path().join("linked.json"),
        )
        .unwrap();
        let mut catalog = crate::builtin::themes();
        let original = catalog.selected().clone();
        let diagnostics = catalog.load_user_themes(directory.path());
        assert!(
            diagnostics
                .iter()
                .any(|message| message.contains("ultra-dusk"))
        );
        assert!(
            diagnostics
                .iter()
                .any(|message| message.contains("oversize"))
        );
        assert_eq!(catalog.selected(), &original);
        assert!(catalog.get("oversize").is_none());
        assert!(catalog.get("linked").is_none());
        assert!(catalog.select("personal"));
        assert_eq!(
            catalog.selected().color("editor.background"),
            Some("#101218")
        );
    }

    #[test]
    fn user_theme_diagnostics_keep_the_parse_reason() {
        let directory = tempfile::tempdir().unwrap();
        std::fs::write(directory.path().join("broken.json"), "{\"name\": }").unwrap();
        std::fs::write(
            directory.path().join("sepia.json"),
            r#"{"name":"Sepia","type":"beige"}"#,
        )
        .unwrap();
        let mut catalog = crate::builtin::themes();
        let diagnostics = catalog.load_user_themes(directory.path());
        assert_eq!(diagnostics.len(), 2, "{diagnostics:?}");
        assert!(
            diagnostics[0].starts_with("Could not load user theme broken: ")
                && diagnostics[0].contains("line 1"),
            "{diagnostics:?}"
        );
        assert_eq!(
            diagnostics[1],
            "Could not load user theme sepia: Invalid theme type"
        );
    }

    #[test]
    fn the_user_theme_limit_counts_only_theme_files() {
        let directory = tempfile::tempdir().unwrap();
        for index in 0..150 {
            std::fs::write(directory.path().join(format!("notes-{index}.txt")), "").unwrap();
        }
        std::fs::write(
            directory.path().join("personal.json"),
            r#"{"name":"Personal","type":"dark"}"#,
        )
        .unwrap();
        let mut catalog = crate::builtin::themes();
        assert_eq!(
            catalog.load_user_themes(directory.path()),
            Vec::<String>::new()
        );
        assert!(catalog.get("personal").is_some());

        for index in 0..100 {
            std::fs::write(
                directory.path().join(format!("theme-{index}.json")),
                r#"{"name":"Extra","type":"dark"}"#,
            )
            .unwrap();
        }
        let mut catalog = crate::builtin::themes();
        assert_eq!(
            catalog.load_user_themes(directory.path()),
            vec!["User theme limit exceeded (100 files)".to_owned()]
        );
    }

    #[test]
    fn an_empty_catalog_still_selects_a_theme() {
        let catalog = ThemeCatalog::parse("{}").unwrap();
        assert!(catalog.is_empty());
        assert_eq!(catalog.selected().id, "ultra-dusk");
    }
}
