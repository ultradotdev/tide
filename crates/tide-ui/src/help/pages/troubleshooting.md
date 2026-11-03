# Troubleshooting

## Diagnostic commands

```sh
tide --version
tide --check-catalogs
tide --check-terminal
tide --check-lsp
tide --check-ai-cli
```

`--check-lsp` lists which language servers are installed, `--check-ai-cli` which agent CLIs Tide finds and how many sessions each has, and `--check-terminal` runs a terminal smoke test. Start Tide with `--debug` to write a log to {{path:log}}; read and redact it before sharing it.

## Slow startup or input

Use a release build: `cargo build --locked --release -p tide`, then run `target/release/tide`. Debug builds can be much slower, especially while parsing syntax. First compilation is separate from application startup.

If input stalls in a release build, note the terminal emulator, window size, file size and type, and which language servers are running, and check whether a workspace search, agent session refresh or language-server request was in progress. Do not include agent histories or private files in a report.

## Shortcuts do not arrive

Tide asks the terminal for the kitty keyboard protocol, which kitty, Ghostty, WezTerm and foot support, and iTerm2 supports once “Report keys using CSI u” is on. Without it, Ctrl+Shift+letter arrives as plain Ctrl+letter and Ctrl+/ as Ctrl+_, and Cmd combinations never reach Tide; macOS Terminal.app is one such terminal. A terminal's own shortcuts can also take a key before Tide sees it. Function keys work everywhere: by default `F12` goes to a definition, `Shift+F12` finds references, `F2` renames and `Ctrl+F2` selects every occurrence. Use the command palette, {{key:workbench.commandPalette}}, and the terminal-safe alternatives in {{help:keyboard-shortcuts}}. While a terminal tab has focus, the shell keeps {{terminalkeys}}; open the palette there with {{terminalkey:workbench.commandPalette}}.

## Copying does not reach the clipboard

Tide copies with the system clipboard helper: `pbcopy` on macOS, `wl-copy` on Wayland, and `xclip` or `xsel` on X11. Over SSH that is the remote machine's clipboard, so Tide also sends each copy to your terminal with the OSC 52 escape sequence, which kitty, Ghostty, WezTerm, foot, Windows Terminal and iTerm2 turn into a local copy. iTerm2 needs “Applications in terminal may access clipboard” turned on; macOS Terminal.app does not support OSC 52. `clipboard.osc52` ({{setting:clipboard.osc52}}) chooses when Tide sends it: `auto` in an SSH session and whenever the system clipboard helper fails, `always` with every copy, or `off`.

Inside tmux, add `set -g set-clipboard on` to `~/.tmux.conf` so tmux passes the copy on. Tide also sends it wrapped for tmux passthrough, which works with `set -g allow-passthrough on` instead.

Tide never reads the clipboard through the terminal. To paste over SSH, use your terminal's own paste, such as Cmd+V or Ctrl+Shift+V.

## Icons or layout look wrong

File icons need a Nerd Font; set `tui.iconStyle` to `ascii` for plain text. Terminal font metrics and Unicode width handling affect alignment. If a sidebar or the dock disappeared in a small window, make the window larger: Tide hides regions that do not fit and brings them back with their saved sizes.

## Agents are missing

Run `tide --check-ai-cli` and confirm the agent's executable is on the `PATH` of the shell that starts Tide, or set its path override variable (see {{help:agents}}). Install and sign in to the CLI outside Tide. The Agents sidebar shows sessions for the current workspace and folders inside it; after moving a repository, run **{{label:agents.associateWorkspace}}**.

## Missing language-server features

Run `tide --check-lsp`, install the missing server and make sure it is on the `PATH`, or point `lsp.servers` at it (see {{help:editor}}). Editing and syntax highlighting do not need a language server.

## Settings or keys are ignored

If {{path:settings}} or {{path:keybindings}} cannot be read, Tide opens **Configuration Problems** at startup with the file, line and column, and uses the defaults for that file until you fix it. A setting with an invalid value is listed there too and falls back to its default. Changes made from **{{label:workbench.openSettings}}** and **{{label:workbench.openKeybindings}}** edit only the value you changed and keep your comments; they leave a file with errors unchanged.

## A theme does not load

User themes load once at startup from {{path:themes}}. A file that fails to load is skipped; start Tide with `--debug` and look in {{path:log}} for the reason.

## Installation refuses a path or launcher

The installer refuses symlinked directory components, a launcher it did not create, changed release contents and a lock held by a running installer; a lock left by an installer that is no longer running is cleared automatically. Use an absolute, canonical prefix. On macOS `/tmp` and `/var` are symlinks, so use `/private/tmp` or the resolved directory for a temporary prefix.

For rollback use the same prefix as the installation. See {{help:getting-started}}.

## PostgreSQL or credentials

Check the active connection, its TLS mode and the server's permissions. A read-only database role is the real authorization boundary; Tide's read-only mode adds client checks and a read-only session. Saved passwords depend on the keychain being available or the encrypted secrets file being unlocked. Redact passwords, connection strings and sensitive query text from reports. See {{help:postgresql}}.

## Session restoration

Sessions are saved under {{path:sessions}}. To check whether a problem comes from the saved session, start once with `--no-session`. That run leaves the saved session untouched and writes no recovery snapshot, unless you run {{label:session.save}}, open a named session or restart Tide. If a session fails to load, Tide stops writing sessions for that run so the file is not overwritten.

## Reporting a problem

**{{label:help.reportIssue}}** opens a new GitHub issue with your Tide version, platform and install method filled in: {{url:issues}}
