<!-- Generated from crates/tide-ui/src/help/pages/settings.md by `cargo xtask generate-rust-reference --write`. Edit the template, not this file. -->
# Settings and Storage

Tide starts from its built-in defaults and applies your settings file, `~/.config/tide/settings.jsonc`, on top. Open the Settings list with `Ctrl+,` or `Ctrl+Alt+O` to change a value; the selected setting's description shows under the list. Enter flips an on/off setting, opens a list of the allowed values for a setting that has them (the current one is marked, and Esc goes back), opens the theme list for `workbench.colorTheme`, and otherwise opens a prompt that shows the allowed range for numbers. Tide checks the value, saves the file and applies it immediately. You can also edit the file by hand: Tide notices within a couple of seconds of saving and applies the changes, and the status bar says which ones wait for new terminals or a restart. If the file no longer parses, Tide lists the problem in **Configuration Problems** and keeps the settings it had until you fix it. Invalid values fall back to the default, and keys Tide does not recognize are kept in the file but ignored.

Most settings apply at once. `terminal.shell` and `tui.terminal.scrollback` apply to terminals opened afterwards. `session.restoreOnStartup`, `workbench.startupEditor`, `update.checkAutomatically`, `tui.outline.collapsedOnStartup` and `tui.timeline.collapsedOnStartup` are read when Tide starts, so they take effect after a restart.

## Icons and colors

`tui.iconStyle` (`auto`) decides whether the Explorer and the edge buttons use Nerd Font glyphs. `auto` uses them in Ghostty, kitty and WezTerm, which bundle the glyphs and show them with any font, and plain text in other terminals. Choose `nerd` if your terminal font is a Nerd Font, or `ascii` to never use them. Explorer icons are drawn in the theme's terminal colours by file type, for example orange for Rust, blue for TypeScript and yellow for folders, so they follow the theme you choose.

`tui.colorMode` (`auto`) decides how colors are sent to the terminal. `auto` sends 24-bit color when the terminal reports it, through `COLORTERM=truecolor` or `24bit`, a `TERM` ending in `-direct`, or a terminal known to support it such as iTerm2, Ghostty, kitty, WezTerm, Alacritty or the VS Code terminal. Otherwise each color becomes the nearest of the 256-color palette, which every supported terminal shows, including Terminal.app before macOS 26. Choose `truecolor` or `256` if `auto` guesses wrong, for example inside tmux or over SSH.

## All settings

### Editor

| Setting | Description | Default | Accepted values |
| --- | --- | --- | --- |
| `editor.autoIndent` | Auto-indent mode: "none", "keep", or "full" | `"full"` | `none`, `keep`, `full` |
| `editor.detectIndentation` | Use each file's existing indentation instead of editor.tabSize and editor.insertSpaces when it has enough to go by; an .editorconfig wins over both | `true` | `true` or `false` |
| `editor.diagnostics.curlyUnderline` | Use squiggly underlines for errors (requires modern terminal) | `true` | `true` or `false` |
| `editor.insertSpaces` | Use spaces instead of tabs | `true` | `true` or `false` |
| `editor.lineNumbers` | Line numbers: "on", "off", "relative" | `"on"` | `on`, `off`, `relative` |
| `editor.markdown.openMode` | How Markdown files open: "preview" (formatted) or "edit" (source); the status bar switches between them | `"preview"` | `preview`, `edit` |
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
| `tui.colorMode` | Colors sent to the terminal: "auto" (24-bit color where the terminal reports it, otherwise the nearest of 256 colors), "truecolor" or "256" | `"auto"` | `auto`, `truecolor`, `256` |
| `tui.diffViewer.autoRefresh` | Auto-refresh diff when file changes | `true` | `true` or `false` |
| `tui.diffViewer.editMode` | Edit save mode: "stage-modified" or "direct-write" | `"stage-modified"` | `stage-modified`, `direct-write` |
| `tui.diffViewer.showDiagnostics` | Show diagnostics in diff viewer | `true` | `true` or `false` |
| `tui.filePicker.maxFiles` | Maximum number of files to show in file picker | `10000` | 100–100000 |
| `tui.fileTree.showGitStatus` | Show git status colors on files in file tree | `true` | `true` or `false` |
| `tui.iconStyle` | Icon style: "auto" (Nerd Font glyphs in Ghostty, kitty and WezTerm, which bundle them; plain text elsewhere), "nerd" (always, for a terminal font that includes Nerd Font glyphs) or "ascii" (never) | `"auto"` | `auto`, `nerd`, `ascii` |
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
| `terminal.copyOnSelect` | Copy text to the clipboard as soon as it is selected with the mouse in a terminal | `true` | `true` or `false` |
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

### Other

| Setting | Description | Default | Accepted values |
| --- | --- | --- | --- |
| `clipboard.osc52` | Also copy through the host terminal with OSC 52: "auto" (in an SSH session, or when the system clipboard fails), "always" or "off" | `"auto"` | `auto`, `always`, `off` |
| `update.checkAutomatically` | Check GitHub for a newer release at most once a day at startup, and say so in the status bar; Help: Check for Updates works either way | `true` | `true` or `false` |

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
