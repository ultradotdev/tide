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

pub fn install_bindings(bindings: &mut Vec<Keybinding>) {
    if !bindings
        .iter()
        .any(|binding| canonical_key(&binding.key) == "ctrl+/")
    {
        bindings.push(Keybinding {
            key: "ctrl+/".to_owned(),
            command: "editor.commentLine".to_owned(),
            when: None,
            args: None,
        });
    }

    const ALTERNATES: &[(&str, &str)] = &[
        ("ctrl+k", "workbench.commandPalette"),
        ("ctrl+alt+s", "file.saveAs"),
        ("ctrl+alt+f", "search.findInFiles"),
        ("ctrl+alt+a", "pane.addPanel"),
        ("ctrl+b", "workbench.toggleSidebar"),
        ("ctrl+alt+b", "workbench.toggleSidebar"),
        ("ctrl+alt+h", "view.splitHorizontal"),
        ("ctrl+alt+d", "editor.duplicateLine"),
        ("ctrl+alt+t", "terminal.new"),
        ("ctrl+alt+m", "terminal.newInPane"),
        ("ctrl+alt+g", "git.focusPanel"),
        ("ctrl+space", "database.complete"),
        ("ctrl+alt+a", "agents.openSession"),
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
        ("ctrl+alt+up", "workbench.focusPaneUp"),
        ("ctrl+alt+down", "workbench.focusPaneDown"),
        ("alt+1", "workbench.focusEditor1"),
        ("alt+2", "workbench.focusEditor2"),
        ("alt+3", "workbench.focusEditor3"),
        ("f7", "workbench.focusNextPane"),
        ("f8", "workbench.focusPreviousPane"),
    ];
    for (key, command) in ALTERNATES {
        let canonical = canonical_key(key);
        if let Some(existing) = bindings
            .iter_mut()
            .find(|binding| canonical_key(&binding.key) == canonical)
        {
            // User bindings, including explicit removals, own their keys.
            if crate::app::command_supported(&existing.command)
                || existing.command.is_empty()
                || existing.command.starts_with('-')
            {
                continue;
            }
            existing.command = (*command).to_owned();
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
