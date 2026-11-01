# Configuration and Storage

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
| `ai.autoStartCli` | `true` | boolean |
| `ai.cli.defaultProvider` | `"claude-code"` | string; `claude-code`, `codex`, `gemini`, `antigravity`, `opencode` |
| `ai.cli.workspaceAliases` | `[]` | array; up to 1000 absolute workspace paths |
| `editor.autoIndent` | `"full"` | string; `none`, `keep`, `full` |
| `editor.diagnostics.curlyUnderline` | `true` | boolean |
| `editor.insertSpaces` | `true` | boolean |
| `editor.lineNumbers` | `"on"` | string; `on`, `off`, `relative` |
| `editor.minimap.enabled` | `true` | boolean |
| `editor.tabSize` | `2` | integer; 1–16 |
| `editor.undoHistoryLimit` | `1000` | integer; 10–10000 |
| `editor.wordWrap` | `"on"` | string; `off`, `on`, `bounded`, `wordWrapColumn` |
| `editor.wordWrapColumn` | `80` | integer; 10–1000 |
| `files.exclude` | `{"**/.DS_Store":true,"**/.git":true,"**/node_modules":true}` | object; up to 1000 keys; boolean values |
| `files.watchFiles` | `"onFocus"` | string; `onFocus`, `always`, `off` |
| `git.inlineDiff.contextLines` | `3` | integer; 0–10 |
| `git.inlineDiff.maxHeight` | `15` | integer; 3–100 |
| `lsp.completionDebounceMs` | `250` | integer; 0–5000 |
| `lsp.diagnostics.enabled` | `true` | boolean |
| `lsp.diagnostics.showInGutter` | `true` | boolean |
| `lsp.diagnostics.underlineErrors` | `true` | boolean |
| `lsp.enabled` | `true` | boolean |
| `lsp.hover.enabled` | `true` | boolean |
| `lsp.signatureHelp.display` | `"popup"` | string; `popup`, `inline`, `statusBar` |
| `lsp.signatureHelp.enabled` | `true` | boolean |
| `lsp.triggerCharacters` | `".:/<@("` | string; up to 1024 characters |
| `session.autoSave` | `true` | boolean |
| `session.autoSaveInterval` | `30000` | integer; 1000–3600000 |
| `session.databaseQueryHistory` | `true` | boolean |
| `session.restoreOnStartup` | `true` | boolean |
| `terminal.shell` | `""` | string; up to 1024 characters |
| `tui.agentsSidebar.visible` | `true` | boolean |
| `tui.agentsSidebar.width` | `36` | integer; 24–200 |
| `tui.diffViewer.autoRefresh` | `true` | boolean |
| `tui.diffViewer.editMode` | `"stage-modified"` | string; `stage-modified`, `direct-write` |
| `tui.diffViewer.showDiagnostics` | `true` | boolean |
| `tui.filePicker.maxFiles` | `10000` | integer; 100–100000 |
| `tui.fileTree.showGitStatus` | `true` | boolean |
| `tui.iconStyle` | `"nerd"` | string; `nerd`, `ascii` |
| `tui.mainRegion.minimumWidth` | `56` | integer; 24–200 |
| `tui.outline.autoFollow` | `true` | boolean |
| `tui.outline.collapsedOnStartup` | `true` | boolean |
| `tui.sidebar.location` | `"left"` | string; `left`, `right` |
| `tui.sidebar.visible` | `true` | boolean |
| `tui.sidebar.width` | `36` | integer; 12–200 |
| `tui.tabBar.scrollAmount` | `1` | integer; 1–10 |
| `tui.terminal.height` | `10` | integer; 3–200 |
| `tui.terminal.scrollback` | `1000` | integer; 0–100000 |
| `tui.timeline.collapsedOnStartup` | `true` | boolean |
| `tui.timeline.commitCount` | `50` | integer; 10–500 |
| `tui.timeline.mode` | `"file"` | string; `file`, `repo` |
| `workbench.colorTheme` | `"ultra-dusk"` | string; up to 1024 characters |

## Keybindings

User keybindings are JSONC entries with `key`, `command`, and an optional `when` context. Matching key/context entries replace defaults; new entries append. Command IDs and supported defaults are listed in the [keyboard reference](keyboard-shortcuts.md).

## Permissions and removal

Tide creates its application storage roots with owner-only permissions. Managed installation leaves settings, sessions, credentials and history intact. Purging that data requires explicit confirmation. Project directories and external providers' native session stores are not purged by Tide.

Generated from the compiled Rust runtime for Tide 0.10.0. Regenerate with `cargo xtask generate-rust-reference --write`; CI verifies with `--check`.
