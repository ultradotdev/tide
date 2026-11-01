//! Compatibility IDs used by command dispatch and generated references.
pub const ALIASES: &[(&str, &str)] = &[
    ("file.nextTab", "editor.nextTab"),
    ("file.previousTab", "editor.previousTab"),
    ("aiCli.new", "agents.newSession"),
    ("aiCli.openSession", "agents.openSession"),
    ("aiCli.refreshSessions", "agents.refreshSessions"),
    ("terminal.nextTab", "terminal.next"),
    ("terminal.previousTab", "terminal.previous"),
    ("terminal.focus", "workbench.focusTerminal"),
    ("view.focusTerminal", "workbench.focusTerminal"),
    ("theme.select", "workbench.selectTheme"),
    ("view.focusEditor", "workbench.focusMainRegion"),
    ("view.focusFileTree", "workbench.focusExplorer"),
    ("workbench.toggleAgentsSidebar", "agents.togglePanel"),
];

pub fn canonical(id: &str) -> &str {
    static MAP: std::sync::LazyLock<std::collections::HashMap<&str, &str>> =
        std::sync::LazyLock::new(|| ALIASES.iter().copied().collect());
    MAP.get(id).copied().unwrap_or(id)
}
