//! Alternate command IDs, routed to their canonical command by dispatch and listed in
//! generated references. Some come from the embedded command catalog; others are
//! older IDs that ran the same action as another command. The palette lists each
//! action once, under its canonical ID.
pub const ALIASES: &[(&str, &str)] = &[
    ("aiCli.new", "agents.newSession"),
    ("aiCli.openSession", "agents.openSession"),
    ("aiCli.refreshSessions", "agents.refreshSessions"),
    ("aiCli.toggle", "agents.newSession"),
    ("terminal.nextTab", "terminal.next"),
    ("terminal.previousTab", "terminal.previous"),
    ("terminal.focus", "workbench.focusTerminal"),
    ("view.focusTerminal", "workbench.focusTerminal"),
    ("theme.select", "workbench.selectTheme"),
    ("view.focusEditor", "workbench.focusMainRegion"),
    ("view.focusFileTree", "workbench.focusExplorer"),
    ("view.toggleSidebar", "workbench.toggleSidebar"),
    ("view.toggleTerminal", "workbench.toggleTerminal"),
    ("workbench.toggleAgentsSidebar", "agents.togglePanel"),
    ("workbench.saveSession", "session.save"),
    ("quit", "workbench.quit"),
    ("git.openFileDiff", "git.diff"),
    ("git.viewStagedChanges", "git.diffStaged"),
    ("git.viewChanges", "gitPanel.openDiff"),
    ("gitPanel.stageAll", "git.stageAll"),
    ("gitPanel.commit", "git.commit"),
    ("gitPanel.refresh", "git.refresh"),
    ("queryResults.export", "database.exportResults"),
];

pub fn canonical(id: &str) -> &str {
    static MAP: std::sync::LazyLock<std::collections::HashMap<&str, &str>> =
        std::sync::LazyLock::new(|| ALIASES.iter().copied().collect());
    MAP.get(id).copied().unwrap_or(id)
}

/// Whether `id` is an alternate ID rather than a command's own.
pub fn is_alias(id: &str) -> bool {
    canonical(id) != id
}
