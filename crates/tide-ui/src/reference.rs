//! Pure metadata export. No workspace, user profile, provider discovery or PTY is opened.
use crate::command::Command;
use crate::{app, catalog, command_aliases, preferences};
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};

#[must_use]
pub fn reference_catalog() -> Value {
    let metadata = catalog::commands();
    let mut bindings = tide_config::builtin::keybindings();
    catalog::install_bindings(&mut bindings);
    let ids = metadata
        .keys()
        .map(String::as_str)
        .chain(Command::ALL.iter().map(|command| command.as_str()))
        .chain(command_aliases::ALIASES.iter().map(|(id, _)| *id))
        .collect::<BTreeSet<_>>();
    let commands = ids
        .into_iter()
        .map(|id| {
            let canonical = command_aliases::canonical(id);
            let info = metadata.get(id).or_else(|| metadata.get(canonical));
            let visible = catalog::palette_commands(&metadata).any(|(listed, _)| listed == id);
            json!({"id":id, "canonical":canonical, "paletteVisible":visible,
            "label":info.map_or(id, |info| info.label.as_str()),
            "category":info.map_or("Keybinding actions", |info| info.category.as_str()),
            "description":info.and_then(|info| info.description.as_deref())})
        })
        .collect::<Vec<_>>();
    // Match the resolver's last entry for each key/context.
    let mut effective = BTreeMap::new();
    for binding in bindings {
        effective.insert(
            (app::canonical_key(&binding.key), binding.when.clone()),
            binding,
        );
    }
    let bindings = effective.into_values().collect::<Vec<_>>();
    let settings = catalog::settings();
    let supported_settings = preferences::SUPPORTED
        .iter()
        .map(|key| {
            (
                (*key).to_owned(),
                json!({"default":settings.get(key), "rule":preferences::rule(key)}),
            )
        })
        .collect::<BTreeMap<_, _>>();
    let aliases = command_aliases::ALIASES
        .iter()
        .copied()
        .collect::<BTreeMap<_, _>>();
    let providers = tide_ai::cli::CLI_PROVIDERS.iter().map(|provider| json!({"id":provider.id(), "name":provider.display_name(), "executable":provider.executable(), "pathOverride":provider.env_override(), "supportsRename":provider.supports_rename()})).collect::<Vec<_>>();
    let themes = tide_config::builtin::themes();
    json!({"schemaVersion":2, "implementation":"rust", "version":env!("CARGO_PKG_VERSION"),
        "commands":commands, "aliases":aliases, "keybindings":bindings,
        "settings":supported_settings,
        "externalCliProviders":providers,
        "themes":themes.iter().map(|theme| json!({"id":theme.id,"name":theme.name})).collect::<Vec<_>>()})
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn references_keep_runtime_alternates_and_defaults() {
        let reference = reference_catalog();
        assert_eq!(reference, reference_catalog());
        for command in reference["commands"].as_array().unwrap() {
            let id = command["id"].as_str().unwrap();
            assert!(app::command_supported(id), "unrouted command {id}");
        }
        for binding in reference["keybindings"].as_array().unwrap() {
            let id = binding["command"].as_str().unwrap();
            assert!(app::command_supported(id), "unrouted binding {id}");
        }
        assert!(
            reference["keybindings"]
                .as_array()
                .unwrap()
                .iter()
                .any(|b| b["key"] == "ctrl+k" && b["command"] == "workbench.commandPalette")
        );
        assert_eq!(reference["aliases"]["terminal.nextTab"], "terminal.next");
        for (key, entry) in reference["settings"].as_object().unwrap() {
            preferences::validate(key, &entry["default"])
                .unwrap_or_else(|error| panic!("Invalid exported default for {key}: {error}"));
        }
        assert_eq!(
            reference["externalCliProviders"].as_array().unwrap().len(),
            5
        );
        assert_eq!(reference["themes"].as_array().unwrap().len(), 15);
    }
}
