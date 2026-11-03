use std::collections::BTreeMap;

use crate::session::SessionState;
use crate::theme::{Theme, ThemeCatalog, ThemeSource};
use crate::{CommandInfo, Keybinding, Settings};

const SETTINGS_JSONC: &str = include_str!("../../../config/default-settings.jsonc");
const KEYBINDINGS_JSONC: &str = include_str!("../../../config/default-keybindings.jsonc");
const COMMANDS_JSONC: &str = include_str!("../../../config/default-commands.jsonc");
const PRODUCT_JSON: &str = include_str!("../../../config/product.json");
const RELEASE_JSON: &str = include_str!("../../../config/release.json");

const THEME_JSON: &[(&str, &str)] = &[
    (
        "catppuccin-frappe",
        include_str!("../../../config/themes/catppuccin-frappe.json"),
    ),
    (
        "catppuccin-latte",
        include_str!("../../../config/themes/catppuccin-latte.json"),
    ),
    (
        "catppuccin-macchiato",
        include_str!("../../../config/themes/catppuccin-macchiato.json"),
    ),
    (
        "catppuccin-mocha",
        include_str!("../../../config/themes/catppuccin-mocha.json"),
    ),
    (
        "dracula",
        include_str!("../../../config/themes/dracula.json"),
    ),
    (
        "gruvbox-dark",
        include_str!("../../../config/themes/gruvbox-dark.json"),
    ),
    ("nord", include_str!("../../../config/themes/nord.json")),
    (
        "one-dark",
        include_str!("../../../config/themes/one-dark.json"),
    ),
    (
        "rose-pine",
        include_str!("../../../config/themes/rose-pine.json"),
    ),
    (
        "solarized-dark",
        include_str!("../../../config/themes/solarized-dark.json"),
    ),
    (
        "solarized-light",
        include_str!("../../../config/themes/solarized-light.json"),
    ),
    (
        "tokyo-night",
        include_str!("../../../config/themes/tokyo-night.json"),
    ),
    (
        "ultra-dusk",
        include_str!("../../../config/themes/ultra-dusk.json"),
    ),
];

/// Built-in settings, parsed from the checked-in JSONC authority.
pub fn settings() -> Settings {
    crate::parse_jsonc(SETTINGS_JSONC).expect("built-in settings must parse")
}

/// Each built-in setting's description: the trailing comment on its line in the JSONC authority.
pub fn setting_descriptions() -> BTreeMap<String, String> {
    SETTINGS_JSONC
        .lines()
        .filter_map(|line| {
            let key = line.trim_start().strip_prefix('"')?.split_once("\":")?.0;
            let (_, comment) = line.split_once(" // ")?;
            Some((key.to_owned(), comment.trim().to_owned()))
        })
        .collect()
}

/// Product identity and links from `config/product.json`.
pub fn product() -> serde_json::Value {
    crate::parse_jsonc(PRODUCT_JSON).expect("product metadata must parse")
}

/// Release metadata, including install commands, from `config/release.json`.
pub fn release() -> serde_json::Value {
    crate::parse_jsonc(RELEASE_JSON).expect("release metadata must parse")
}

/// Built-in keybindings, parsed from the checked-in JSONC authority.
pub fn keybindings() -> Vec<Keybinding> {
    crate::parse_jsonc(KEYBINDINGS_JSONC).expect("built-in keybindings must parse")
}

/// Built-in command metadata, parsed from the checked-in JSONC authority.
pub fn commands() -> BTreeMap<String, CommandInfo> {
    crate::parse_jsonc(COMMANDS_JSONC).expect("built-in commands must parse")
}

/// Built-in theme catalog, parsed from the checked-in theme files.
pub fn themes() -> ThemeCatalog {
    let mut catalog = BTreeMap::new();
    for (id, text) in THEME_JSON {
        let source: ThemeSource = crate::parse_jsonc(text).expect("built-in theme must parse");
        catalog.insert((*id).to_owned(), Theme::from_source(*id, source));
    }
    ThemeCatalog::new(catalog, "ultra-dusk")
}

/// The built-in Ultra Dusk theme, used when a catalog has no themes.
pub(crate) fn fallback_theme() -> Theme {
    let (id, text) = THEME_JSON
        .iter()
        .find(|(id, _)| *id == "ultra-dusk")
        .expect("Ultra Dusk is built in");
    let source: ThemeSource = crate::parse_jsonc(text).expect("built-in theme must parse");
    Theme::from_source(*id, source)
}

/// Parse a session that a caller has already read.
pub fn parse_session(text: &str) -> Result<SessionState, crate::ConfigError> {
    let session: SessionState = crate::parse_jsonc(text)?;
    session.validate_version()?;
    Ok(session)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embedded_catalogs_have_their_expected_sizes() {
        assert_eq!(settings().len(), 57);
        assert_eq!(keybindings().len(), 209);
        assert_eq!(commands().len(), 285);
        assert_eq!(themes().len(), 13);
    }

    #[test]
    fn every_built_in_setting_has_a_description() {
        let descriptions = setting_descriptions();
        for (key, _) in settings().iter() {
            assert!(
                descriptions.get(key).is_some_and(|text| !text.is_empty()),
                "{key} has no description"
            );
        }
        assert_eq!(descriptions["editor.tabSize"], "Number of spaces per tab");
    }
}
