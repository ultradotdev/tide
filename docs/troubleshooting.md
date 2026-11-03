<!-- Generated from crates/tide-ui/src/help/pages/troubleshooting.md by `cargo xtask generate-rust-reference --write`. Edit the template, not this file. -->
# Troubleshooting

## Diagnostic commands

```sh
tide --version
tide --check-catalogs
tide --check-terminal
tide --check-lsp
tide --check-ai-cli
```

`--check-lsp` lists which language servers are installed, `--check-ai-cli` which agent CLIs Tide finds and how many sessions each has, and `--check-terminal` runs a terminal smoke test. Start Tide with `--debug` to write a log to `~/.local/state/tide/logs/debug.log`; read and redact it before sharing it.

## Slow startup or input

Use a release build: `cargo build --locked --release -p tide`, then run `target/release/tide`. Debug builds can be much slower, especially while parsing syntax. First compilation is separate from application startup.

If input stalls in a release build, note the terminal emulator, window size, file size and type, and which language servers are running, and check whether a workspace search, agent session refresh or language-server request was in progress. Do not include agent histories or private files in a report.

## Shortcuts do not arrive

Tide asks the terminal for the kitty keyboard protocol, which kitty, Ghostty, WezTerm and foot support, and iTerm2 supports once “Report keys using CSI u” is on. Without it, Ctrl+Shift+letter arrives as plain Ctrl+letter and Ctrl+/ as Ctrl+_, and Cmd combinations never reach Tide; macOS Terminal.app is one such terminal. A terminal's own shortcuts can also take a key before Tide sees it. Function keys work everywhere: by default `F12` goes to a definition, `Shift+F12` finds references, `F2` renames and `Ctrl+F2` selects every occurrence. Use the command palette, `Ctrl+K` or `Ctrl+Shift+P`, and the terminal-safe alternatives in [Keyboard Shortcuts](reference/keyboard-shortcuts.md). While a terminal tab has focus, the shell keeps every Ctrl+letter key, such as `Ctrl+C`, `Ctrl+R`, `Ctrl+X` and `Ctrl+Q`, plus `Ctrl+Space`, `Ctrl+@`, `Ctrl+[`, `Ctrl+\`, `Ctrl+]`, `Ctrl+^` and `Ctrl+_`; open the palette there with `F1`.

## Copying does not reach the clipboard

Tide copies with the system clipboard helper: `pbcopy` on macOS, `wl-copy` on Wayland, and `xclip` or `xsel` on X11. Over SSH that is the remote machine's clipboard, so Tide also sends each copy to your terminal with the OSC 52 escape sequence, which kitty, Ghostty, WezTerm, foot, Windows Terminal and iTerm2 turn into a local copy. iTerm2 needs “Applications in terminal may access clipboard” turned on; macOS Terminal.app does not support OSC 52. `clipboard.osc52` (`auto`) chooses when Tide sends it: `auto` in an SSH session and whenever the system clipboard helper fails, `always` with every copy, or `off`.

Inside tmux, add `set -g set-clipboard on` to `~/.tmux.conf` so tmux passes the copy on. Tide also sends it wrapped for tmux passthrough, which works with `set -g allow-passthrough on` instead.

Tide never reads the clipboard through the terminal. To paste over SSH, use your terminal's own paste, such as Cmd+V or Ctrl+Shift+V.

## Icons or layout look wrong

Nerd Font file icons show only where the glyphs are available: by default (`tui.iconStyle` set to `auto`) Tide uses them in Ghostty, kitty and WezTerm, which bundle them. If you see empty boxes, set `tui.iconStyle` to `ascii`; if your terminal font is a Nerd Font and you want the icons, set it to `nerd`. Terminal font metrics and Unicode width handling affect alignment. If a sidebar or the dock disappeared in a small window, make the window larger: Tide hides regions that do not fit and brings them back with their saved sizes.

## Colors look wrong

Tide sends 24-bit color only to terminals that report it, and the nearest of 256 colors to others such as Terminal.app before macOS 26. Inside tmux or over SSH the terminal's own variables may not get through: if colors look washed out or banded, set `tui.colorMode` to `truecolor`; if they show as the wrong colors entirely, set it to `256`. The change applies as soon as you save the settings file.

## Agents are missing

Run `tide --check-ai-cli` and confirm the agent's executable is on the `PATH` of the shell that starts Tide, or set its path override variable (see [AI CLI Agents](features/ai.md)). Install and sign in to the CLI outside Tide. An agent marked “Not signed in” is installed but asked to be signed in or set up when Tide listed its sessions; select the row and press Enter to see what it said. The Agents sidebar shows sessions for the current workspace and folders inside it; after moving a repository, run **Agents: Associate Previous Workspace...**.

## Missing language-server features

Run `tide --check-lsp`, install the missing server and make sure it is on the `PATH`, or point `lsp.servers` at it (see [Editor and Language Servers](features/editor.md)). Editing and syntax highlighting do not need a language server.

## Settings or keys are ignored

If `~/.config/tide/settings.jsonc` or `~/.config/tide/keybindings.jsonc` cannot be read, Tide opens **Configuration Problems** with the file, line and column: at startup it uses the defaults for that file, and after an edit it keeps the configuration it already had. Save the fixed file and Tide applies it without a restart. A setting with an invalid value is listed there too and falls back to its default. Changes made from **Open Settings** and **Open Keyboard Shortcuts** edit only the value you changed and keep your comments; they leave a file with errors unchanged.

## A theme does not load

User themes load once at startup from `~/.config/tide/themes`. A file that fails to load is skipped; start Tide with `--debug` and look in `~/.local/state/tide/logs/debug.log` for the reason.

## Installation refuses a path or launcher

The installer refuses symlinked directory components, a launcher it did not create, changed release contents and a lock held by a running installer; a lock left by an installer that is no longer running is cleared automatically. Use an absolute, canonical prefix. On macOS `/tmp` and `/var` are symlinks, so use `/private/tmp` or the resolved directory for a temporary prefix.

For rollback use the same prefix as the installation. See [Getting Started](getting-started.md).

## PostgreSQL or credentials

Check the active connection, its TLS mode and the server's permissions. A server with a self-signed certificate needs `require`, or `verify-ca` or `verify-full` with its CA certificate; see the TLS section of [PostgreSQL](features/postgresql.md). A read-only database role is the real authorization boundary; Tide's read-only mode adds client checks and a read-only session. Saved passwords depend on the keychain being available or the encrypted secrets file being unlocked. Redact passwords, connection strings and sensitive query text from reports. See [PostgreSQL](features/postgresql.md).

## Session restoration

Sessions are saved under `~/.local/state/tide/sessions`. To check whether a problem comes from the saved session, start once with `--no-session`. That run leaves the saved session untouched and writes no recovery snapshot, unless you run Save Session, open a named session or restart Tide. If a session fails to load, Tide stops writing sessions for that run so the file is not overwritten.

## Reporting a problem

**Help: Report an Issue** opens a new GitHub issue with your Tide version, platform and install method filled in: https://github.com/ultradotdev/tide/issues
