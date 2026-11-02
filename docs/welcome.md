<!-- Generated from crates/tide-ui/src/help/pages/welcome.md by `cargo xtask generate-rust-reference --write`. Edit the template, not this file. -->
# Welcome to Tide 0.11.0

Tide is a keyboard-driven IDE that runs inside your terminal. Code editing with language servers, Git, shells, PostgreSQL and AI coding agents such as Claude Code and Codex share one workbench, so you can do a whole task without leaving the window you started in.

This guide is also built into Tide, where it shows your own keybindings, settings and installed agents: run **Help: Welcome** from the command palette. The keys below are the defaults.

## The one key to remember

Press `Ctrl+K` or `Ctrl+Shift+P` to open the **command palette**. Every command in Tide is there: start typing a few letters of what you want, such as `split`, `commit` or `theme`, and press Enter. The palette is also how you reach anything that has no key, including all the help guides (type `help`).

While a terminal has focus, the shell keeps its own control keys such as `Ctrl+C`, `Ctrl+R` and `Ctrl+K`. Open the palette from a terminal with `Ctrl+Shift+P` or `F1`.

## The essentials

| To | Press |
| --- | --- |
| Open the command palette | `Ctrl+K` or `Ctrl+Shift+P` |
| Open a file by name | `Ctrl+P` |
| Save | `Ctrl+S` |
| Find in this file | `Ctrl+F` |
| Find and replace | `Ctrl+H` |
| Find in all files | `Ctrl+Shift+F` or `Ctrl+Alt+F` |
| Go to a symbol in this file | `Ctrl+R` |
| Split the pane right | `Ctrl+\` |
| Split the pane down | `Ctrl+Shift+\` or `Ctrl+Alt+H` |
| Add a tab: file, terminal, SQL editor or AI agent | `Ctrl+Shift+A` |
| Move focus to the next region (sidebar, panes, agents, terminal, status bar) | `F6` or `Ctrl+Alt+]` |
| Show or hide the bottom terminal | `` Ctrl+` `` |
| Open a terminal tab in the main pane | `Ctrl+Shift+T` or `Ctrl+Alt+M` |
| Go to Source Control | `Ctrl+Shift+G` or `Ctrl+Alt+G` |
| Start a new SQL query | “New Query” from the command palette (`Ctrl+K`) |
| Start an AI agent session | `Ctrl+Shift+I` |
| Reopen an earlier agent session | `Ctrl+Shift+J` or `Ctrl+Alt+A` |
| Close the tab | `Ctrl+W` |
| Settings | `Ctrl+,` or `Ctrl+Alt+O` |
| Keyboard shortcuts | `Ctrl+Shift+,` or `Ctrl+Alt+K` |
| Color theme | `Ctrl+Alt+R` |
| Quit | `Ctrl+Q` |

Many terminals cannot send `Ctrl+Shift` or `Cmd` combinations, so Tide adds `Ctrl+Alt` or function-key alternatives for most of them; they are shown as the second key. If a key does nothing in your terminal, run the command from the palette. The mouse works everywhere too: click tabs, drag dividers, and use the small arrow buttons at the edges of the main area to show or hide the sidebars and the terminal.

## A quick tour

### The workbench

The screen is split into **regions**: the workspace sidebar (Explorer, Source Control, Outline and Timeline), the main area of panes and tabs, the Agents sidebar, the bottom terminal dock and the status bar. `F6` or `Ctrl+Alt+]` moves between them and `Shift+F6` or `Ctrl+Alt+[` goes back. Each pane has its own tabs: files, diffs, terminals, SQL editors and agent sessions can sit side by side. See [Workbench and Panes](features/workbench.md).

### Editing

Open files with `Ctrl+P` or from the Explorer. The editor has syntax highlighting, multiple cursors (`Ctrl+D` adds the next match), folding, comments (`Ctrl+/`), find and replace with regular expressions, and language-server features such as completion, hover, go to definition and rename when a server is installed for the language. See [Editor and Language Servers](features/editor.md).

### Git

Source Control lists conflicted, staged, changed and untracked files. With it focused, `s` stages the selected file, `u` unstages it, `Enter` opens its diff and `c` commits. Push, pull, branches, stashes, merges, blame and history are in the palette under Git. See [Git](features/git.md).

### Terminals

`` Ctrl+` `` shows the terminal dock at the bottom of the window, and `Ctrl+Shift+T` or `Ctrl+Alt+M` opens a terminal as a tab beside your files. They are full terminals: TUI programs, mouse reporting and colors work as they do in your normal terminal. See [Terminals](features/terminal.md).

### PostgreSQL

Run **New Connection...** from the palette, then “New Query” from the command palette (`Ctrl+K`) for a SQL tab. In a SQL editor `Ctrl+R` or `Ctrl+Enter` runs the query and the results appear below it. New connections are read-only until you choose otherwise. See [PostgreSQL](features/postgresql.md).

### AI coding agents

Tide hosts the agent CLIs you already use in its own tabs and lists their saved sessions for this workspace in the Agents sidebar. `Ctrl+Shift+I` starts a new session and `Ctrl+Shift+J` or `Ctrl+Alt+A` reopens an earlier one. Tide supports these agents:

| Agent | Executable | Path override |
| --- | --- | --- |
| Claude Code | `claude` | `TIDE_CLAUDE_PATH` |
| Codex | `codex` | `TIDE_CODEX_PATH` |
| Gemini CLI | `gemini` | `TIDE_GEMINI_PATH` |
| Antigravity | `agy` | `TIDE_ANTIGRAVITY_PATH` |
| OpenCode | `opencode` | `TIDE_OPENCODE_PATH` |

Install and sign in to an agent outside Tide. Tide finds it on the `PATH` it was started with, or at the path in its override variable. See [AI CLI Agents](features/ai.md).

## Make it yours

- **Settings:** `Ctrl+,` or `Ctrl+Alt+O` lists every setting. They are saved in `~/.config/tide/settings.jsonc`, which you can also edit directly; [Settings Reference](reference/configuration.md) describes each one.
- **Keybindings:** `Ctrl+Shift+,` or `Ctrl+Alt+K` lets you change a key. Your own bindings live in `~/.config/tide/keybindings.jsonc` as entries such as `{ "key": "ctrl+alt+p", "command": "workbench.commandPalette" }`. [Keyboard Shortcuts](reference/keyboard-shortcuts.md) lists every active key and command ID.
- **Themes:** `Ctrl+Alt+R` picks one of the built-in themes; your own go in `~/.config/tide/themes`. See [Themes and Customization](features/themes.md).
- **This page:** `workbench.startupEditor` (`welcomeOnFirstRun`) decides when Welcome opens by itself: `welcomeOnFirstRun` the first time Tide starts without a file or session, `welcome` every time it does, and `none` never.

## Updating and uninstalling

- **Homebrew:** Update with `brew upgrade tide` and uninstall with `brew uninstall tide`. Homebrew owns this copy, so `tide --uninstall` refuses and names the `brew` command to use instead.
- **Install script:** Update by running the install command again: `curl -fsSL https://github.com/ultradotdev/tide/releases/latest/download/install.sh | sh`. Uninstall with `tide --uninstall`; add `--purge` to remove your settings and data as well.
- **Managed install from source** (`./install-from-source.sh`): Update by pulling the latest source and running `./install-from-source.sh` again from the checkout; `./install.sh --rollback` returns to the previous build. Uninstall with `tide --uninstall`.
- **Source build** (`./target/release/tide`): Update with `git pull` and then `cargo build --locked --release -p tide` in the checkout. To remove it, delete the build; nothing else was installed.

Updates and uninstalls keep your settings, keybindings, sessions and connections; only `tide --uninstall --purge` removes them.

## Getting help

- Type `help` in the command palette for the guides: [Getting Started](getting-started.md), [Workbench and Panes](features/workbench.md), [Editor and Language Servers](features/editor.md), [Git](features/git.md), [Terminals](features/terminal.md), [PostgreSQL](features/postgresql.md), [AI CLI Agents](features/ai.md), [Themes and Customization](features/themes.md), [Keyboard Shortcuts](reference/keyboard-shortcuts.md), [All Commands](reference/commands.md), [Settings Reference](reference/configuration.md) and [Troubleshooting](troubleshooting.md). Each one is rebuilt from your current setup when it opens.
- **Help: Online Documentation** opens the online documentation: https://github.com/ultradotdev/tide/tree/main/docs
- **Help: Report an Issue** starts a GitHub issue with your Tide version and platform filled in: https://github.com/ultradotdev/tide/issues

Happy hacking.
