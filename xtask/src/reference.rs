//! Generated command, keyboard and configuration references from the compiled runtime.
use crate::ojson::Json;
use crate::util::{capture, read_json};
use anyhow::{Result, bail, ensure};
use std::collections::HashMap;
use std::path::Path;
use std::process::Command;

pub const GENERATED_NOTE: &str =
    "Regenerate with `cargo xtask generate-rust-reference --write`; CI verifies with `--check`.";

pub fn export_reference(root: &Path) -> Result<String> {
    capture(
        Command::new("cargo")
            .args([
                "run",
                "--locked",
                "--quiet",
                "--release",
                "-p",
                "tide",
                "--example",
                "export_reference",
            ])
            .current_dir(root),
    )
}

fn cell(value: &str) -> String {
    value.replace('|', "\\|").replace('\n', "<br>")
}

fn code(value: &str) -> String {
    format!("`{}`", cell(value).replace('`', "\\`"))
}

fn rules(rule: &Json) -> String {
    let text = |key: &str| rule.get(key).map(Json::display);
    let mut parts = vec![rule.str_at(&["type"]).to_owned()];
    if let Some(values) = rule.get("enum") {
        parts.push(
            values
                .as_array()
                .iter()
                .map(|value| code(&value.display()))
                .collect::<Vec<_>>()
                .join(", "),
        );
    }
    if let Some(minimum) = text("minimum") {
        let maximum = text("maximum").unwrap_or_else(|| "undefined".into());
        parts.push(format!("{minimum}–{maximum}"));
    }
    if let Some(length) = text("maxLength") {
        parts.push(format!("up to {length} characters"));
    }
    if let Some(items) = text("maxItems") {
        let kind = text("items").unwrap_or_else(|| "undefined".into());
        parts.push(format!("up to {items} {kind}"));
    }
    if let Some(keys) = text("maxProperties") {
        let values = text("values").unwrap_or_else(|| "undefined".into());
        parts.push(format!("up to {keys} keys; {values} values"));
    }
    parts.retain(|part| !part.is_empty());
    parts.join("; ")
}

/// Renders every generated reference file from the exporter's JSON.
pub fn render(raw: &str, product_version: &str) -> Result<Vec<(&'static str, String)>> {
    let reference = Json::parse(raw)?;
    ensure!(
        reference.get("schemaVersion").and_then(Json::as_f64) == Some(1.0)
            && reference.str_at(&["implementation"]) == "rust"
            && reference.str_at(&["version"]) == product_version,
        "Rust reference identity does not match product metadata"
    );
    let version = reference.str_at(&["version"]);
    let commands = reference
        .get("commands")
        .map(Json::as_array)
        .unwrap_or_default();
    let supported: Vec<&Json> = commands
        .iter()
        .filter(|command| command.get("supported").and_then(Json::as_bool) == Some(true))
        .collect();
    let by_id: HashMap<&str, &Json> = commands
        .iter()
        .map(|command| (command.str_at(&["id"]), command))
        .collect();
    let command_rows = supported
        .iter()
        .map(|command| {
            format!(
                "| {} | {} | {} | {} |",
                code(command.str_at(&["id"])),
                cell(command.str_at(&["label"])),
                cell(command.str_at(&["category"])),
                if command.get("paletteVisible").and_then(Json::as_bool) == Some(true) {
                    "Yes"
                } else {
                    "Keybinding action"
                }
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    let aliases = reference
        .get("aliases")
        .map(Json::entries)
        .unwrap_or_default()
        .iter()
        .filter(|(id, _)| {
            by_id
                .get(id.as_str())
                .and_then(|command| command.get("supported"))
                .and_then(Json::as_bool)
                == Some(true)
        })
        .map(|(id, target)| format!("| {} | {} |", code(id), code(&target.display())))
        .collect::<Vec<_>>()
        .join("\n");
    let generated =
        format!("Generated from the compiled Rust runtime for Tide {version}. {GENERATED_NOTE}");
    let commands_md = format!(
        "# Commands

Open the command palette with `ctrl+k`. Enhanced terminal key reporting also enables the bindings listed in the [keyboard reference](keyboard-shortcuts.md).

The table includes supported command IDs and compatibility aliases. “Keybinding action” means the command is routed but has no standalone palette entry. Provider-specific launch entries appear when the corresponding external CLI is detected.

| Command ID | Label | Category | Palette |
| --- | --- | --- | --- |
{command_rows}

## Compatibility aliases

Aliases route to the same handler. Existing user bindings can retain these IDs.

| Alias | Canonical command |
| --- | --- |
{aliases}

{generated}
"
    );
    let keybindings = reference
        .get("keybindings")
        .map(Json::as_array)
        .unwrap_or_default();
    let binding_rows = keybindings
        .iter()
        .map(|binding| {
            let command = binding.str_at(&["command"]);
            let label = by_id
                .get(command)
                .map_or(command, |found| found.str_at(&["label"]));
            let context = binding
                .get("when")
                .map(Json::display)
                .filter(|when| !when.is_empty())
                .map_or_else(|| "Any".to_owned(), |when| code(&when));
            format!(
                "| {} | {} | {} | {context} |",
                code(binding.str_at(&["key"])),
                cell(label),
                code(command)
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    let bindings_md = format!(
        "# Keyboard Shortcuts

These are the compiled Rust defaults, including terminal-safe alternatives. User bindings belong in `keybindings.jsonc` under the Tide configuration directory.

Context-specific matches take precedence over unconditional bindings. Within the same context, the last matching entry wins. An empty command or a command beginning with `-` explicitly removes a binding. Some terminal emulators cannot transmit Ctrl+Shift or Cmd combinations; use a listed terminal-safe alternative such as `ctrl+k` for the command palette.

| Keys | Action | Command ID | Context |
| --- | --- | --- | --- |
{binding_rows}

Modal buttons, list navigation and editor input also have controls local to their surfaces. See the feature guides for Find/Replace options, database results and pane navigation.

{generated}
"
    );
    let settings = reference
        .get("settings")
        .map(Json::entries)
        .unwrap_or_default();
    let setting_rows = settings
        .iter()
        .map(|(key, value)| {
            let default = value.get("default").unwrap_or(&Json::Null).compact();
            let rule = value.get("rule").unwrap_or(&Json::Null);
            format!("| {} | {} | {} |", code(key), code(&default), rules(rule))
        })
        .collect::<Vec<_>>()
        .join("\n");
    let configuration_md = format!(
        "# Configuration and Storage

Rust Tide loads embedded defaults and then user `settings.jsonc`. Supported settings are validated by the runtime. Unsupported and unknown values are preserved for compatibility; preserving a value does not enable a feature.

| Category | Default root | Contents |
| --- | --- | --- |
| Configuration | `$XDG_CONFIG_HOME/tide` or `~/.config/tide` | Settings, keybindings, themes, saved connections |
| Data | `$XDG_DATA_HOME/tide` or `~/.local/share/tide` | Encrypted secrets and database history |
| State | `$XDG_STATE_HOME/tide` or `~/.local/state/tide` | Rust sessions, recovery and logs |
| Cache | `$XDG_CACHE_HOME/tide` or `~/.cache/tide` | Rebuildable application data |
| Workspace | `<workspace>/.tide` | Project-scoped database connection records |

XDG overrides must be absolute. Rust keeps its session snapshots separate from TypeScript writes. Back up configuration/state before switching implementations; see [migration and installation](../getting-started.md).

## Supported settings

Open Settings from the command palette. Values below come from the same defaults and constraints used by the Rust workbench. An empty terminal shell selects the user's normal shell. Theme names must match a built-in or loaded user theme.

| Key | Default | Accepted value |
| --- | --- | --- |
{setting_rows}

## Keybindings

User keybindings are JSONC entries with `key`, `command`, and an optional `when` context. Matching key/context entries replace defaults; new entries append. Command IDs and supported defaults are listed in the [keyboard reference](keyboard-shortcuts.md).

## Permissions and removal

Tide creates its application storage roots with owner-only permissions. Managed installation leaves settings, sessions, credentials and history intact. Purging that data requires explicit confirmation. Project directories and external providers' native session stores are not purged by Tide.

{generated}
"
    );
    Ok(vec![
        (
            "docs/reference/runtime.json",
            format!("{}\n", reference.pretty()),
        ),
        ("docs/reference/commands.md", commands_md),
        ("docs/reference/keyboard-shortcuts.md", bindings_md),
        ("docs/reference/configuration.md", configuration_md),
    ])
}

/// `generate-rust-reference --write|--check`.
pub fn generate(root: &Path, write: bool) -> Result<()> {
    let product: serde_json::Value = read_json(&root.join("config/product.json"))?;
    let version = product["version"].as_str().unwrap_or_default();
    let raw = export_reference(root)?;
    let outputs = render(&raw, version)?;
    for (name, contents) in &outputs {
        let path = root.join(name);
        if write {
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::write(&path, contents)?;
        } else if std::fs::read_to_string(&path).ok().as_deref() != Some(contents.as_str()) {
            bail!("Generated Rust reference is stale: {name}");
        }
    }
    let reference = Json::parse(&raw)?;
    let supported = reference
        .get("commands")
        .map(Json::as_array)
        .unwrap_or_default()
        .iter()
        .filter(|command| command.get("supported").and_then(Json::as_bool) == Some(true))
        .count();
    println!(
        "{} Rust reference: {supported} command IDs, {} effective bindings, {} settings.",
        if write { "Generated" } else { "Verified" },
        reference
            .get("keybindings")
            .map(Json::as_array)
            .unwrap_or_default()
            .len(),
        reference
            .get("settings")
            .map(Json::entries)
            .unwrap_or_default()
            .len(),
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"{"schemaVersion":1,"version":"0.10.0","implementation":"rust",
      "commands":[{"id":"a.run","label":"Run | all","category":"Run","supported":true,"paletteVisible":true},
                  {"id":"a.hidden","label":"Hidden","category":"Run","supported":true,"paletteVisible":false},
                  {"id":"a.unsupported","label":"Unsupported","category":"Run","supported":false,"paletteVisible":false}],
      "aliases":{"a.run":"a.run2","a.unsupported":"x"},
      "keybindings":[{"key":"ctrl+k","command":"a.run"},{"key":"f1","command":"unknown","when":"editorFocus"}],
      "settings":{"editor.tabSize":{"default":4,"rule":{"type":"integer","minimum":1,"maximum":16}},
                  "theme":{"default":"Ultra Dusk","rule":{"type":"string","enum":["a","b"],"maxLength":64}}},
      "preservedSettings":[],"externalCliProviders":[],"themes":[]}"#;

    #[test]
    fn renders_supported_commands_aliases_bindings_and_rules() {
        let outputs = render(SAMPLE, "0.10.0").unwrap();
        let file = |name: &str| {
            outputs
                .iter()
                .find(|(path, _)| path.ends_with(name))
                .unwrap()
                .1
                .clone()
        };
        let commands = file("commands.md");
        assert!(commands.contains("| `a.run` | Run \\| all | Run | Yes |"));
        assert!(commands.contains("| `a.hidden` | Hidden | Run | Keybinding action |"));
        assert!(!commands.contains("a.unsupported"));
        assert!(commands.contains("| `a.run` | `a.run2` |"));
        let bindings = file("keyboard-shortcuts.md");
        assert!(bindings.contains("| `ctrl+k` | Run \\| all | `a.run` | Any |"));
        assert!(bindings.contains("| `f1` | unknown | `unknown` | `editorFocus` |"));
        let configuration = file("configuration.md");
        assert!(configuration.contains("| `editor.tabSize` | `4` | integer; 1–16 |"));
        assert!(
            configuration
                .contains("| `theme` | `\"Ultra Dusk\"` | string; `a`, `b`; up to 64 characters |")
        );
        assert!(configuration.contains(GENERATED_NOTE));
        let runtime = file("runtime.json");
        assert!(runtime.starts_with("{\n  \"schemaVersion\": 1,\n  \"version\": \"0.10.0\""));
    }

    #[test]
    fn rejects_a_reference_for_another_version() {
        assert!(render(SAMPLE, "0.11.0").is_err());
    }
}
