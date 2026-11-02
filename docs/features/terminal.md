<!-- Generated from crates/tide-ui/src/help/pages/terminal.md by `cargo xtask generate-rust-reference --write`. Edit the template, not this file. -->
# Terminal and TUI Applications

Tide opens shells in the bottom dock or as ordinary tabs in the main panes. AI agent tabs use the same terminal.

## Opening terminals

| To | Press |
| --- | --- |
| Show or hide the dock (opens a shell if it is empty) | `` Ctrl+` `` |
| New terminal tab in the dock | `` Ctrl+Shift+` `` or `Ctrl+Alt+T` |
| New terminal tab in the main pane | `Ctrl+Shift+T` or `Ctrl+Alt+M` |
| Focus the dock | “Focus Bottom Dock” from the command palette (`Ctrl+K`) |
| Next / previous dock tab | “Next Bottom Terminal Tab” from the command palette (`Ctrl+K`) / “Previous Bottom Terminal Tab” from the command palette (`Ctrl+K`) |
| Close the focused terminal tab | “Close Bottom Terminal Tab” from the command palette (`Ctrl+K`) |
| Send Ctrl+C to the terminal | “Interrupt Terminal” from the command palette (`Ctrl+K`) |

The dock's `+` button also opens a terminal, and **Move Active Tab to Main Pane...** moves a terminal between the dock and the panes.

New terminals start in the workspace folder with the program in `terminal.shell` (`""`); when it is empty Tide uses `$SHELL`, then `/bin/sh`. Bash, zsh and sh start as interactive login shells. Terminals report `TERM=xterm-256color`, `COLORTERM=truecolor` and `TERM_PROGRAM=tide`. Shell and scrollback changes apply to new terminals.

## Keys inside a terminal

While a terminal has focus, the shell's own control keys go to the program running there, even when Tide binds them: `Ctrl+A`, `Ctrl+C`, `Ctrl+D`, `Ctrl+E`, `Ctrl+K`, `Ctrl+L`, `Ctrl+R`, `Ctrl+U`, `Ctrl+V`, `Ctrl+W`, `Ctrl+Y`, `Ctrl+Z`, `Ctrl+Space` and `Ctrl+\`. Workbench keys with Shift, Alt or a function key still reach Tide, so open the command palette from a terminal with `Ctrl+Shift+P` or `F1`.

Escape also belongs to the program. To leave a terminal, use `F6` or `Ctrl+Alt+]`, `Alt+Left` or `Ctrl+Alt+Left` and the other pane keys, `` Ctrl+` `` to hide the dock, or click elsewhere.

## Scrollback

Each terminal keeps `tui.terminal.scrollback` (`1000`) lines. Shift+PageUp and Shift+PageDown page through them, and the mouse wheel scrolls; hold Shift while scrolling when the program has asked for mouse input.

## Compatibility

PTY output is parsed by the native `alacritty_terminal` emulator. Full-screen programs work as they do in your normal terminal:

- Main and alternate screen buffers
- Scrollback and cursor preservation across resize
- Application cursor keys and the Kitty keyboard protocol
- Bracketed paste
- Focus reporting
- SGR mouse input when the program requests it
- Synchronized output

Programs cannot read or write your clipboard or resize the window through escape sequences. Their own background colors are kept; Tide shows which terminal has focus with its tab and border instead.

See [AI CLI Agents](ai.md) for agent tabs and [Keyboard Shortcuts](../reference/keyboard-shortcuts.md) for every key.
