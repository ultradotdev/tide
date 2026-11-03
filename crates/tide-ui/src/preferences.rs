//! Supported preferences have validated types and real runtime consumers.
use serde_json::{Value, json};
pub const SUPPORTED: &[&str] = &[
    "ai.autoStartCli",
    "ai.cli.defaultProvider",
    "editor.diagnostics.curlyUnderline",
    "lsp.diagnostics.underlineErrors",
    "editor.autoIndent",
    "editor.undoHistoryLimit",
    "editor.minimap.enabled",
    "files.watchFiles",
    "tui.mainRegion.minimumWidth",
    "tui.tabBar.scrollAmount",
    "tui.filePicker.maxFiles",
    "tui.diffViewer.autoRefresh",
    "tui.diffViewer.showDiagnostics",
    "tui.outline.collapsedOnStartup",
    "tui.timeline.collapsedOnStartup",
    "tui.timeline.mode",
    "tui.timeline.commitCount",
    "git.inlineDiff.contextLines",
    "session.restoreOnStartup",
    "lsp.signatureHelp.display",
    "ai.cli.workspaceAliases",
    "editor.tabSize",
    "editor.insertSpaces",
    "editor.detectIndentation",
    "editor.lineNumbers",
    "editor.wordWrap",
    "editor.wordWrapColumn",
    "workbench.colorTheme",
    "workbench.startupEditor",
    "update.checkAutomatically",
    "tui.sidebar.width",
    "tui.sidebar.visible",
    "tui.sidebar.location",
    "tui.agentsSidebar.visible",
    "tui.agentsSidebar.width",
    "tui.terminal.height",
    "tui.terminal.scrollback",
    "terminal.shell",
    "terminal.copyOnSelect",
    "clipboard.osc52",
    "tui.fileTree.showGitStatus",
    "tui.iconStyle",
    "tui.diffViewer.editMode",
    "tui.outline.autoFollow",
    "git.inlineDiff.maxHeight",
    "files.exclude",
    "session.autoSave",
    "session.autoSaveInterval",
    "session.databaseQueryHistory",
    "lsp.enabled",
    "lsp.completionDebounceMs",
    "lsp.triggerCharacters",
    "lsp.hover.enabled",
    "lsp.signatureHelp.enabled",
    "lsp.diagnostics.enabled",
    "lsp.diagnostics.showInGutter",
];
pub fn supported(key: &str) -> bool {
    SUPPORTED.contains(&key)
}

pub fn validate(key: &str, value: &Value) -> Result<(), String> {
    if !supported(key) {
        return Err("This preference has no supported runtime control".into());
    }
    let (range, choices) = constraints(key);
    let valid = if range.1 > 0 {
        value.as_u64().is_some_and(|v| v >= range.0 && v <= range.1)
    } else if !choices.is_empty() {
        value.as_str().is_some_and(|v| choices.contains(&v))
    } else if matches!(
        key,
        "workbench.colorTheme" | "terminal.shell" | "lsp.triggerCharacters"
    ) {
        value
            .as_str()
            .is_some_and(|s| s.len() <= 1024 && !s.chars().any(char::is_control))
    } else if key == "ai.cli.workspaceAliases" {
        value.as_array().is_some_and(|a| {
            a.len() <= 1000
                && a.iter().all(|v| {
                    v.as_str()
                        .is_some_and(|s| s.len() <= 4096 && std::path::Path::new(s).is_absolute())
                })
        })
    } else if key == "files.exclude" {
        value.as_object().is_some_and(|o| {
            o.len() <= 1000 && o.iter().all(|(k, v)| k.len() <= 1024 && v.is_boolean())
        })
    } else {
        value.is_boolean()
    };
    if valid {
        Ok(())
    } else {
        Err(format!("Invalid value for {key}"))
    }
}

fn constraints(key: &str) -> ((u64, u64), &'static [&'static str]) {
    let range = match key {
        "editor.undoHistoryLimit" => (10, 10000),
        "tui.mainRegion.minimumWidth" => (24, 200),
        "tui.tabBar.scrollAmount" => (1, 10),
        "tui.filePicker.maxFiles" => (100, 100000),
        "tui.timeline.commitCount" => (10, 500),
        "git.inlineDiff.contextLines" => (0, 10),
        "editor.tabSize" => (1, 16),
        "editor.wordWrapColumn" => (10, 1000),
        "tui.sidebar.width" => (12, 200),
        "tui.agentsSidebar.width" => (24, 200),
        "tui.terminal.height" => (3, 200),
        "tui.terminal.scrollback" => (0, 100000),
        "git.inlineDiff.maxHeight" => (3, 100),
        "session.autoSaveInterval" => (1000, 3600000),
        "lsp.completionDebounceMs" => (0, 5000),
        _ => (0, 0),
    };
    let choices: &[&str] = match key {
        "ai.cli.defaultProvider" => &["claude-code", "codex", "gemini", "antigravity", "opencode"],
        "editor.autoIndent" => &["none", "keep", "full"],
        "files.watchFiles" => &["onFocus", "always", "off"],
        "tui.timeline.mode" => &["file", "repo"],
        "lsp.signatureHelp.display" => &["popup", "inline", "statusBar"],
        "editor.lineNumbers" => &["on", "off", "relative"],
        "editor.wordWrap" => &["off", "on", "bounded", "wordWrapColumn"],
        "tui.sidebar.location" => &["left", "right"],
        "tui.iconStyle" => &["nerd", "ascii"],
        "workbench.startupEditor" => &["welcomeOnFirstRun", "welcome", "none"],
        "tui.diffViewer.editMode" => &["stage-modified", "direct-write"],
        "clipboard.osc52" => &["auto", "always", "off"],
        _ => &[],
    };
    (range, choices)
}

pub fn rule(key: &str) -> Option<Value> {
    if !supported(key) {
        return None;
    }
    let (range, choices) = constraints(key);
    Some(if range.1 > 0 {
        json!({"type":"integer", "minimum":range.0, "maximum":range.1})
    } else if !choices.is_empty() {
        json!({"type":"string", "enum":choices})
    } else if matches!(
        key,
        "workbench.colorTheme" | "terminal.shell" | "lsp.triggerCharacters"
    ) {
        json!({"type":"string", "maxLength":1024})
    } else if key == "ai.cli.workspaceAliases" {
        json!({"type":"array", "maxItems":1000, "items":"absolute workspace paths"})
    } else if key == "files.exclude" {
        json!({"type":"object", "maxProperties":1000, "values":"boolean"})
    } else {
        json!({"type":"boolean"})
    })
}
