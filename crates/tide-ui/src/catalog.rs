//! Shared runtime metadata used by the workbench and generated references.
use crate::app::canonical_key;
use std::collections::BTreeMap;
use tide_config::{CommandInfo, Keybinding, Settings};

pub fn settings() -> Settings {
    tide_config::builtin::settings()
}

pub fn commands() -> BTreeMap<String, CommandInfo> {
    let mut commands = tide_config::builtin::commands();
    commands
        .entry("workbench.selectTheme".to_owned())
        .or_insert(CommandInfo {
            label: "Select Color Theme".to_owned(),
            category: "Preferences".to_owned(),
            description: Some("Switch the active semantic theme".to_owned()),
        });
    commands.insert(
        "agents.associateWorkspace".into(),
        CommandInfo {
            label: "Agents: Associate Previous Workspace...".into(),
            category: "AI".into(),
            description: Some(
                "Explicitly associate native sessions after moving a workspace".into(),
            ),
        },
    );
    for (id, label) in [
        ("view.closePane", "View: Close Pane"),
        ("editor.switchTab", "Editor: Switch Tab"),
        (
            "editor.switchTabAllPanes",
            "Editor: Switch Tab Across Panes",
        ),
        ("editor.moveTabToNextPane", "Editor: Move Tab to Next Pane"),
        (
            "editor.moveTabToPreviousPane",
            "Editor: Move Tab to Previous Pane",
        ),
        ("editor.moveTabToPane", "Editor: Move Tab to Pane..."),
        ("workbench.moveSidebarLeft", "Workbench: Move Sidebar Left"),
        (
            "workbench.moveSidebarRight",
            "Workbench: Move Sidebar Right",
        ),
        (
            "workbench.moveAgentsSidebarLeft",
            "Workbench: Move Agents Left",
        ),
        (
            "workbench.moveAgentsSidebarRight",
            "Workbench: Move Agents Right",
        ),
        ("workbench.resizeLeft", "Workbench: Resize Left"),
        ("workbench.resizeRight", "Workbench: Resize Right"),
        ("workbench.resizeUp", "Workbench: Resize Up"),
        ("workbench.resizeDown", "Workbench: Resize Down"),
        ("workbench.statusHistory", "Workbench: Status History"),
        ("sidebar.addFileTree", "Sidebar: Add Explorer"),
        ("sidebar.addGitPanel", "Sidebar: Add Source Control"),
        ("sidebar.addOutline", "Sidebar: Add Outline"),
        ("sidebar.addTimeline", "Sidebar: Add Timeline"),
    ] {
        commands.entry(id.into()).or_insert(CommandInfo {
            label: label.into(),
            category: "Workbench".into(),
            description: None,
        });
    }
    for (id, label, category) in [
        ("workbench.restart", "Restart Tide", "Workbench"),
        (
            "workbench.restartAndRebuild",
            "Restart and Rebuild Tide (source checkout)",
            "Workbench",
        ),
        ("theme.next", "Next Color Theme", "Preferences"),
        ("theme.previous", "Previous Color Theme", "Preferences"),
        (
            "theme.toggleDarkMode",
            "Toggle Light/Dark Theme",
            "Preferences",
        ),
        ("terminal.interrupt", "Interrupt Terminal", "Terminal"),
        ("database.addRow", "Add Row", "Database"),
    ] {
        commands.entry(id.into()).or_insert(CommandInfo {
            label: label.into(),
            category: category.into(),
            description: None,
        });
    }
    commands
}

/// Commands kept out of the command palette, each with the security reason. Only a
/// command whose palette entry would reveal secret values or skip a security
/// confirmation belongs here; every other routed command is in the palette.
pub const PALETTE_PRIVATE: &[(&str, &str)] = &[];

/// The command palette's entries: every catalog entry under its own ID, except
/// [`PALETTE_PRIVATE`] commands. Aliases have no entry, so each action is listed once.
pub fn palette_commands(
    commands: &BTreeMap<String, CommandInfo>,
) -> impl Iterator<Item = (&String, &CommandInfo)> {
    commands.iter().filter(|(id, _)| {
        !crate::command_aliases::is_alias(id)
            && !PALETTE_PRIVATE.iter().any(|(private, _)| private == id)
    })
}

/// Terminal-safe alternates, each for a key no default binding claims.
const ALTERNATES: &[(&str, &str)] = &[
    ("ctrl+alt+s", "file.saveAs"),
    ("ctrl+alt+f", "search.findInFiles"),
    ("ctrl+b", "workbench.toggleSidebar"),
    ("ctrl+alt+b", "workbench.toggleSidebar"),
    ("ctrl+alt+h", "view.splitHorizontal"),
    ("ctrl+alt+d", "editor.duplicateLine"),
    ("ctrl+alt+t", "terminal.new"),
    ("ctrl+alt+m", "terminal.newInPane"),
    ("ctrl+alt+g", "git.focusPanel"),
    ("ctrl+alt+e", "workbench.focusExplorer"),
    ("ctrl+alt+u", "workbench.focusOutline"),
    ("ctrl+alt+l", "workbench.focusTimeline"),
    ("ctrl+alt+k", "workbench.openKeybindings"),
    ("ctrl+alt+o", "workbench.openSettings"),
    ("ctrl+alt+r", "workbench.selectTheme"),
    ("alt+right", "workbench.focusPaneRight"),
    ("alt+left", "workbench.focusPaneLeft"),
    ("alt+up", "workbench.focusPaneUp"),
    ("alt+down", "workbench.focusPaneDown"),
    ("ctrl+alt+right", "workbench.focusPaneRight"),
    ("ctrl+alt+left", "workbench.focusPaneLeft"),
    ("alt+1", "workbench.focusEditor1"),
    ("alt+2", "workbench.focusEditor2"),
    ("alt+3", "workbench.focusEditor3"),
    ("f7", "workbench.focusNextPane"),
    ("f8", "workbench.focusPreviousPane"),
    // Without the kitty keyboard protocol, Ctrl+Shift+letter arrives as Ctrl+letter and
    // Ctrl+/ as Ctrl+_, so the Ctrl+Shift defaults need keys every terminal can send.
    ("ctrl+_", "editor.toggleComment"),
    ("f12", "lsp.goToDefinition"),
    ("shift+f12", "lsp.findReferences"),
    ("ctrl+f2", "edit.selectAllOccurrences"),
];

pub fn install_bindings(bindings: &mut Vec<Keybinding>) {
    for (key, command) in ALTERNATES {
        let canonical = canonical_key(key);
        // Existing bindings, including user removals, own their keys.
        if bindings
            .iter()
            .any(|binding| canonical_key(&binding.key) == canonical)
        {
            continue;
        }
        bindings.push(Keybinding {
            key: (*key).to_owned(),
            command: (*command).to_owned(),
            // In an editor, Alt+Left and Alt+Right (Option on macOS) move by word.
            when: matches!(*key, "alt+left" | "alt+right").then(|| "!editorFocus".to_owned()),
            args: None,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_catalog_entry_has_a_runtime_consumer() {
        for id in commands().keys() {
            assert!(crate::app::command_supported(id), "{id} is not routed");
        }
        let settings = settings();
        for (key, value) in settings.iter() {
            crate::preferences::validate(key, value)
                .unwrap_or_else(|error| panic!("{key}: {error}"));
        }
        for key in crate::preferences::SUPPORTED {
            assert!(settings.get(key).is_some(), "{key} has no default");
        }
    }

    #[test]
    fn every_routed_command_is_in_the_palette_unless_private() {
        let commands = commands();
        let palette = palette_commands(&commands)
            .map(|(id, _)| id.as_str())
            .collect::<std::collections::HashSet<_>>();
        let missing = crate::command::Command::ALL
            .iter()
            .map(|command| command.as_str())
            .filter(|id| !palette.contains(id))
            .filter(|id| !PALETTE_PRIVATE.iter().any(|(private, _)| private == id))
            .collect::<Vec<_>>();
        assert!(missing.is_empty(), "not in the palette: {missing:?}");
        for (id, reason) in PALETTE_PRIVATE {
            assert!(crate::app::command_supported(id), "{id} is not routed");
            assert!(!palette.contains(id), "{id} is private but listed");
            assert!(!reason.is_empty(), "{id} needs a security reason");
        }
    }

    #[test]
    fn every_palette_entry_routes_and_no_alias_has_its_own_entry() {
        let commands = commands();
        for (id, info) in palette_commands(&commands) {
            assert!(crate::app::command_supported(id), "{id} is not routed");
            assert!(!info.label.trim().is_empty(), "{id} has no label");
            assert!(!info.category.trim().is_empty(), "{id} has no category");
        }
        for (alias, canonical) in crate::command_aliases::ALIASES {
            assert!(
                crate::app::command_supported(alias),
                "{alias} does not route"
            );
            assert!(
                !commands.contains_key(*alias),
                "{alias} runs {canonical}; list it under {canonical} instead"
            );
        }
    }

    #[test]
    fn the_palette_lists_each_action_once() {
        let commands = commands();
        let mut actions = std::collections::HashMap::new();
        let mut labels = std::collections::HashMap::new();
        for (id, info) in palette_commands(&commands) {
            let command = id.parse::<crate::command::Command>().unwrap();
            if let Some(other) = actions.insert(command, id) {
                panic!("{id} and {other} both run {command}");
            }
            if let Some(other) = labels.insert(info.label.to_lowercase(), id) {
                panic!("{id} and {other} are both labelled {:?}", info.label);
            }
        }
    }

    #[test]
    fn every_alternate_claims_a_key_no_default_binds() {
        let mut bindings = tide_config::builtin::keybindings();
        let defaults = bindings.len();
        install_bindings(&mut bindings);
        let added = bindings.split_off(defaults);
        assert_eq!(added.len(), ALTERNATES.len());
        for binding in &added {
            assert!(crate::app::command_supported(&binding.command));
            let key = canonical_key(&binding.key);
            assert!(
                !bindings
                    .iter()
                    .any(|existing| canonical_key(&existing.key) == key),
                "{key} is already bound"
            );
        }
    }
}
