# Welcome to Tide {{version}}

Tide is a keyboard-driven IDE that runs inside your terminal. Code editing with language servers, Git, shells, PostgreSQL and AI coding agents such as Claude Code and Codex share one workbench, so you can do a whole task without leaving the window you started in.

{{live}}This guide is built from your own keybindings and settings each time it opens, so every key below is one that works for you right now. It is read-only. Reopen it any time with {{key:help.welcome}}.{{/live}}{{docs}}This guide is also built into Tide, where it shows your own keybindings, settings and installed agents: run **{{label:help.welcome}}** from the command palette. The keys below are the defaults.{{/docs}}

## The one key to remember

Press {{key:workbench.commandPalette}} to open the **command palette**. Every command in Tide is there: start typing a few letters of what you want, such as `split`, `commit` or `theme`, and press Enter. The palette is also how you reach anything that has no key, including all the help guides (type `help`).

While a terminal has focus, every Ctrl+letter key, such as `Ctrl+C`, `Ctrl+R`, `Ctrl+X` and `Ctrl+Q`, goes to the program running there. Open the palette from a terminal with {{terminalkey:workbench.commandPalette}}.

## The essentials

| To | Press |
| --- | --- |
| Open the command palette | {{key:workbench.commandPalette}} |
| Open a file by name | {{key:workbench.quickOpen}} |
| Save | {{key:file.save}} |
| Find in this file | {{key:search.find}} |
| Find and replace | {{key:search.replace}} |
| Find in all files | {{key:search.findInFiles}} |
| Go to a symbol in this file | {{key:editor.gotoSymbol}} |
| Split the pane right | {{key:view.splitVertical}} |
| Split the pane down | {{key:view.splitHorizontal}} |
| Add a tab: file, terminal, SQL editor or AI agent | {{key:pane.addPanel}} |
| Move focus to the next region (sidebar, panes, agents, terminal, status bar) | {{key:workbench.focusNextRegion}} |
| Show or hide the bottom terminal | {{key:workbench.toggleTerminal}} |
| Open a terminal tab in the main pane | {{key:terminal.newInPane}} |
| Go to Source Control | {{key:git.focusPanel}} |
| Start a new SQL query | {{key:database.newQuery}} |
| Start an AI agent session | {{key:aiCli.toggle}} |
| Reopen an earlier agent session | {{key:agents.openSession}} |
| Close the tab | {{key:file.close}} |
| Settings | {{key:workbench.openSettings}} |
| Keyboard shortcuts | {{key:workbench.openKeybindings}} |
| Color theme | {{key:workbench.selectTheme}} |
| Quit | {{key:workbench.quit}} |

Many terminals cannot send `Ctrl+Shift` or `Cmd` combinations, so Tide adds `Ctrl+Alt` or function-key alternatives for most of them; they are shown as the second key. If a key does nothing in your terminal, run the command from the palette. The mouse works everywhere too: click tabs, drag dividers, and use the small arrow buttons at the edges of the main area to show or hide the sidebars and the terminal.

## A quick tour

### The workbench

The screen is split into **regions**: the workspace sidebar (Explorer, Source Control, Outline and Timeline), the main area of panes and tabs, the Agents sidebar, the bottom terminal dock and the status bar. {{key:workbench.focusNextRegion}} moves between them and {{key:workbench.focusPreviousRegion}} goes back. Each pane has its own tabs: files, diffs, terminals, SQL editors and agent sessions can sit side by side. See {{help:workbench}}.

### Editing

Open files with {{key:workbench.quickOpen}} or from the Explorer. The editor has syntax highlighting, multiple cursors ({{key:edit.selectNextMatch}} adds the next match), folding, comments ({{key:editor.toggleComment}}), find and replace with regular expressions, and language-server features such as completion, hover, go to definition and rename when a server is installed for the language. See {{help:editor}}.

### Git

Source Control lists conflicted, staged, changed and untracked files. With it focused, {{key:gitPanel.stage}} stages the selected file, {{key:gitPanel.unstage}} unstages it, {{key:gitPanel.openDiff}} opens its diff and {{key:gitPanel.commit}} commits. Push, pull, branches, stashes, merges, blame and history are in the palette under Git. See {{help:git}}.

### Terminals

{{key:workbench.toggleTerminal}} shows the terminal dock at the bottom of the window, and {{key:terminal.newInPane}} opens a terminal as a tab beside your files. They are full terminals: TUI programs, mouse reporting and colors work as they do in your normal terminal. See {{help:terminal}}.

### PostgreSQL

Run **{{label:database.newConnection}}** from the palette, then {{key:database.newQuery}} for a SQL tab. In a SQL editor {{key:database.executeQuery}} runs the query and the results appear below it. New connections are read-only until you choose otherwise. See {{help:postgresql}}.

### AI coding agents

Tide hosts the agent CLIs you already use in its own tabs and lists their saved sessions for this workspace in the Agents sidebar. {{key:aiCli.toggle}} starts a new session and {{key:agents.openSession}} reopens an earlier one. {{live}}Here is what Tide found on this machine:{{/live}}{{docs}}Tide supports these agents:{{/docs}}

{{agents}}

Install and sign in to an agent outside Tide. Tide finds it on the `PATH` it was started with, or at the path in its override variable. See {{help:agents}}.

## Make it yours

- **Settings:** {{key:workbench.openSettings}} lists every setting. They are saved in {{path:settings}}, which you can also edit directly; {{help:settings}} describes each one.
- **Keybindings:** {{key:workbench.openKeybindings}} lets you change a key. Your own bindings live in {{path:keybindings}} as entries such as `{ "key": "ctrl+alt+p", "command": "workbench.commandPalette" }`. {{help:keyboard-shortcuts}} lists every active key and command ID.
- **Themes:** {{key:workbench.selectTheme}} picks one of the built-in themes; your own go in {{path:themes}}. See {{help:themes}}.
- **This page:** `workbench.startupEditor` ({{setting:workbench.startupEditor}}) decides when Welcome opens by itself: `welcomeOnFirstRun` the first time Tide starts without a file or session, `welcome` every time it does, and `none` never.

## Updating and uninstalling

{{install}}

## Getting help

- Type `help` in the command palette for the guides: {{help:getting-started}}, {{help:workbench}}, {{help:editor}}, {{help:git}}, {{help:terminal}}, {{help:postgresql}}, {{help:agents}}, {{help:themes}}, {{help:keyboard-shortcuts}}, {{help:commands}}, {{help:settings}} and {{help:troubleshooting}}. Each one is rebuilt from your current setup when it opens.
- **{{label:help.documentation}}** opens the online documentation: {{url:documentation}}
- **{{label:help.reportIssue}}** starts a GitHub issue with your Tide version and platform filled in: {{url:issues}}

Happy hacking.
