//! Shared runtime metadata used by the workbench and generated references.
use crate::app::canonical_key;
use std::collections::BTreeMap;
use tide_config::{CommandInfo, Keybinding, Settings};

pub fn settings() -> Settings {
    let mut settings = tide_config::builtin::settings();
    settings.set("terminal.shell", serde_json::json!(""));
    settings.set("ai.cli.workspaceAliases", serde_json::json!([]));
    settings.set("editor.wordWrapColumn", serde_json::json!(80));
    settings
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
            category: "AI CLI".into(),
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
        ("database.addRow", "Database: Add Row"),
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
        ("terminal.focus", "Focus Terminal", "Terminal"),
        (
            "workbench.focusAgentsSidebar",
            "Focus Agents Sidebar",
            "AI CLI",
        ),
        ("git.openFileDiff", "View Active File Diff", "Git"),
        ("git.viewStagedChanges", "View Staged Changes", "Git"),
    ] {
        commands.entry(id.into()).or_insert(CommandInfo {
            label: label.into(),
            category: category.into(),
            description: None,
        });
    }
    commands
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
            when: None,
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
