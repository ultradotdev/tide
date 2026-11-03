# Terminal and TUI Applications

Tide opens shells in the bottom dock or as ordinary tabs in the main panes. AI agent tabs use the same terminal.

## Opening terminals

| To | Press |
| --- | --- |
| Show or hide the dock (opens a shell if it is empty) | {{key:workbench.toggleTerminal}} |
| New terminal tab in the dock | {{key:terminal.new}} |
| New terminal tab in the main pane | {{key:terminal.newInPane}} |
| Focus the dock | {{key:workbench.focusBottomDock}} |
| Next / previous dock tab | {{key:terminal.next}} / {{key:terminal.previous}} |
| Close the focused terminal tab | {{key:terminal.close}} |
| Send Ctrl+C to the terminal | {{key:terminal.interrupt}} |
| Clear the scrollback and screen | {{key:terminal.clear}} |

The dock's `+` button also opens a terminal, and **{{label:editor.moveTabToPane}}** moves a terminal between the dock and the panes.

**{{label:terminal.kill}}** stops every process in a terminal and leaves its tab marked with the exit status, as when the shell exits; it asks first while a command or agent is running. **{{label:terminal.closeAll}}** closes the shell terminals in the dock and the panes, asking first if one is busy; agent tabs stay open. Quitting or restarting Tide also asks first while any terminal or agent is still running a program. Terminal commands act on the focused terminal, or on the dock's active terminal when focus is elsewhere.

New terminals start in the workspace folder with the program in `terminal.shell` ({{setting:terminal.shell}}); when it is empty Tide uses `$SHELL`, then `/bin/sh`. Bash, zsh and sh start as interactive login shells. Terminals report `TERM=xterm-256color`, `COLORTERM=truecolor` and `TERM_PROGRAM=tide`. Shell and scrollback changes apply to new terminals.

## Keys inside a terminal

While a terminal has focus, the program running there gets {{terminalkeys}}, even where Tide binds them, so editors such as nano and emacs, tmux and AI agents work as they do outside Tide. Workbench keys with Shift, Alt or a function key still reach Tide, so open the command palette from a terminal with {{terminalkey:workbench.commandPalette}}.

Escape also belongs to the program. To leave a terminal, use {{key:workbench.focusNextRegion}}, {{key:workbench.focusPaneLeft}} and the other pane keys, {{key:workbench.toggleTerminal}} to hide the dock, or click elsewhere.

## Scrollback

Each terminal keeps `tui.terminal.scrollback` ({{setting:tui.terminal.scrollback}}) lines. Shift+PageUp and Shift+PageDown page through them, and the mouse wheel scrolls; hold Shift while scrolling when the program has asked for mouse input. {{key:terminal.scrollToTop}} jumps to the oldest line and {{key:terminal.scrollToBottom}} back to the live screen.

{{key:terminal.clear}} clears the scrollback and the screen like a terminal's Clear Buffer: the prompt line moves to the top, and nothing is typed into the shell. A full-screen program such as an editor keeps its screen.

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

See {{help:agents}} for agent tabs and {{help:keyboard-shortcuts}} for every key.
