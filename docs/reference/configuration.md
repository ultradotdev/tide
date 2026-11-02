<!-- Generated from crates/tide-ui/src/help/pages/settings.md by `cargo xtask generate-rust-reference --write`. Edit the template, not this file. -->
# Settings and Storage

Tide starts from its built-in defaults and applies your settings file, `~/.config/tide/settings.jsonc`, on top. Open the Settings list with `Ctrl+,` or `Ctrl+Alt+O` to change a value; Tide checks it, saves the file and applies most changes immediately. You can also edit the file by hand and restart Tide. Invalid values fall back to the default, and keys Tide does not recognize are kept in the file but ignored.

## All settings

### Editor

| Setting | Description | Default | Accepted values |
| --- | --- | --- | --- |
| `editor.autoIndent` | Auto-indent mode: "none", "keep", or "full" | `"full"` | `none`, `keep`, `full` |
| `editor.diagnostics.curlyUnderline` | Use squiggly underlines for errors (requires modern terminal) | `true` | `true` or `false` |
| `editor.insertSpaces` | Use spaces instead of tabs | `true` | `true` or `false` |
| `editor.lineNumbers` | Line numbers: "on", "off", "relative" | `"on"` | `on`, `off`, `relative` |
| `editor.minimap.enabled` | Show the minimap | `true` | `true` or `false` |
| `editor.tabSize` | Number of spaces per tab | `2` | 1–16 |
| `editor.undoHistoryLimit` | Maximum undo actions per document | `1000` | 10–10000 |
| `editor.wordWrap` | Word wrap: "off", "on", "wordWrapColumn", "bounded" | `"on"` | `off`, `on`, `bounded`, `wordWrapColumn` |
| `editor.wordWrapColumn` | Wrap column for "wordWrapColumn" and "bounded" word wrap (10-1000) | `80` | 10–1000 |

### Files

| Setting | Description | Default | Accepted values |
| --- | --- | --- | --- |
| `files.exclude` | Patterns to exclude from file tree | `{"**/.DS_Store":true,"**/.git":true,"**/node_modules":true}` | Glob pattern → `true` or `false` |
| `files.watchFiles` | Watch for external changes: "onFocus", "always", "off" | `"onFocus"` | `onFocus`, `always`, `off` |

### Workbench

| Setting | Description | Default | Accepted values |
| --- | --- | --- | --- |
| `workbench.colorTheme` | Color theme name | `"ultra-dusk"` | Text |
| `workbench.startupEditor` | Opening Welcome: "welcomeOnFirstRun" (the first start without a file or session), "welcome" (every such start) or "none" | `"welcomeOnFirstRun"` | `welcomeOnFirstRun`, `welcome`, `none` |

### Layout and panels

| Setting | Description | Default | Accepted values |
| --- | --- | --- | --- |
| `tui.agentsSidebar.visible` | Agents sidebar visibility | `true` | `true` or `false` |
| `tui.agentsSidebar.width` | Agents sidebar width in characters (24-200) | `36` | 24–200 |
| `tui.diffViewer.autoRefresh` | Auto-refresh diff when file changes | `true` | `true` or `false` |
| `tui.diffViewer.editMode` | Edit save mode: "stage-modified" or "direct-write" | `"stage-modified"` | `stage-modified`, `direct-write` |
| `tui.diffViewer.showDiagnostics` | Show diagnostics in diff viewer | `true` | `true` or `false` |
| `tui.filePicker.maxFiles` | Maximum number of files to show in file picker | `10000` | 100–100000 |
| `tui.fileTree.showGitStatus` | Show git status colors on files in file tree | `true` | `true` or `false` |
| `tui.iconStyle` | Icon style: "nerd" (Nerd Font glyphs, width-1, default) or "ascii" (no icons, for terminals without a Nerd Font) | `"nerd"` | `nerd`, `ascii` |
| `tui.mainRegion.minimumWidth` | Minimum main-region width retained before the sidebar responsively collapses (24-200 columns) | `56` | 24–200 |
| `tui.outline.autoFollow` | Auto-follow cursor position in outline | `true` | `true` or `false` |
| `tui.outline.collapsedOnStartup` | Collapse outline panel on startup | `true` | `true` or `false` |
| `tui.sidebar.location` | Sidebar position: "left", "right" | `"left"` | `left`, `right` |
| `tui.sidebar.visible` | Sidebar visibility | `true` | `true` or `false` |
| `tui.sidebar.width` | Sidebar width in characters | `36` | 12–200 |
| `tui.tabBar.scrollAmount` | Number of tabs to scroll when using scroll buttons | `1` | 1–10 |
| `tui.terminal.height` | Bottom terminal dock height in rows | `10` | 3–200 |
| `tui.terminal.scrollback` | Terminal scrollback buffer size in lines | `1000` | 0–100000 |
| `tui.timeline.collapsedOnStartup` | Collapse timeline panel on startup | `true` | `true` or `false` |
| `tui.timeline.commitCount` | Number of commits to show in timeline | `50` | 10–500 |
| `tui.timeline.mode` | Timeline mode: "file" (current file) or "repo" (all commits) | `"file"` | `file`, `repo` |

### Terminal

| Setting | Description | Default | Accepted values |
| --- | --- | --- | --- |
| `terminal.shell` | Shell program for new terminals; empty uses $SHELL, then /bin/sh | `""` | Text |

### Git

| Setting | Description | Default | Accepted values |
| --- | --- | --- | --- |
| `git.inlineDiff.contextLines` | Context lines in inline diffs | `3` | 0–10 |
| `git.inlineDiff.maxHeight` | Max height of inline diff expander in lines | `15` | 3–100 |

### AI CLI agents

| Setting | Description | Default | Accepted values |
| --- | --- | --- | --- |
| `ai.autoStartCli` | Start the external AI CLI automatically when its pane receives focus | `true` | `true` or `false` |
| `ai.cli.defaultProvider` | Default registered external AI CLI provider | `"claude-code"` | `claude-code`, `codex`, `gemini`, `antigravity`, `opencode` |
| `ai.cli.workspaceAliases` | Earlier absolute workspace paths whose agent sessions belong to this workspace | `[]` | List of absolute paths |

### Sessions

| Setting | Description | Default | Accepted values |
| --- | --- | --- | --- |
| `session.autoSave` | Auto-save session state | `true` | `true` or `false` |
| `session.autoSaveInterval` | Auto-save interval in milliseconds | `30000` | 1000–3600000 |
| `session.databaseQueryHistory` | Record database queries and failures in local history | `true` | `true` or `false` |
| `session.restoreOnStartup` | Restore previous session on startup | `true` | `true` or `false` |

### Language servers

| Setting | Description | Default | Accepted values |
| --- | --- | --- | --- |
| `lsp.completionDebounceMs` | Delay before triggering completion (ms) | `250` | 0–5000 |
| `lsp.diagnostics.enabled` | Show diagnostics (errors, warnings) | `true` | `true` or `false` |
| `lsp.diagnostics.showInGutter` | Show diagnostic icons in gutter | `true` | `true` or `false` |
| `lsp.diagnostics.underlineErrors` | Underline errors in editor | `true` | `true` or `false` |
| `lsp.enabled` | Enable language server features | `true` | `true` or `false` |
| `lsp.hover.enabled` | Show hover information | `true` | `true` or `false` |
| `lsp.signatureHelp.display` | Signature display: "popup", "inline", "statusBar" | `"popup"` | `popup`, `inline`, `statusBar` |
| `lsp.signatureHelp.enabled` | Show function signature help | `true` | `true` or `false` |
| `lsp.triggerCharacters` | Characters that trigger completion | `".:/<@("` | Text |

## Language server overrides

`lsp.servers` is not in the Settings list; add it to `~/.config/tide/settings.jsonc` by hand. Each key is a language ID (`typescript`, `javascript`, `typescriptreact`, `javascriptreact`, `rust`, `python`, `go`, `ruby`, `c`, `cpp`, `json`, `html`, `css` or `sql`). The value is the server to start, `{ "command": "...", "args": [...] }`, or `null` to turn off language-server features for that language:

```jsonc
{
  "lsp.servers": {
    "python": { "command": "pyright-langserver", "args": ["--stdio"] },
    "sql": null
  }
}
```

Languages you leave out use the defaults listed in [Editor and Language Servers](../features/editor.md).

## Where Tide keeps your files

| What | Default location |
| --- | --- |
| Settings | `~/.config/tide/settings.jsonc` |
| Keybindings | `~/.config/tide/keybindings.jsonc` |
| User themes | `~/.config/tide/themes` |
| Global database connections | `~/.config/tide/connections.json` |
| Encrypted secrets | `~/.local/share/tide/secrets.enc` |
| Query history | `~/.local/share/tide/database/query-history` |
| Sessions | `~/.local/state/tide/sessions` |
| Debug log (with `--debug`) | `~/.local/state/tide/logs/debug.log` |
| Cache | `~/.cache/tide` |
| Project database connections | `<workspace>/.tide/connections.json` |

The configuration, data, state and cache roots follow `$XDG_CONFIG_HOME`, `$XDG_DATA_HOME`, `$XDG_STATE_HOME` and `$XDG_CACHE_HOME` when they are set to absolute paths, and otherwise `~/.config`, `~/.local/share`, `~/.local/state` and `~/.cache`. Tide creates them readable only by you.

## Permissions and removal

Installing, updating and uninstalling Tide leave all of these files in place. `tide --uninstall --purge` removes Tide's own roots after asking; project `.tide` directories and the agents' own session stores are never removed by Tide. See [Getting Started](../getting-started.md).
