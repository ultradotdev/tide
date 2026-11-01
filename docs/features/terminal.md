# Terminal and TUI Applications

Tide can open shell sessions in the bottom dock or as ordinary main-pane tabs. External coding-agent tabs use the same terminal implementation.

## Terminal emulator

PTY output is parsed by the native `alacritty_terminal` emulator into viewport snapshots; `portable-pty` owns the child process. The renderer consumes cells, colors, styles, cursor state, active buffer, scrollback position, and terminal modes from that snapshot.

Supported interactive behavior includes:

- Main and alternate screen buffers
- Scrollback and cursor preservation across resize
- Application cursor keys
- Bracketed paste
- Focus reporting
- SGR mouse input when requested by the child
- Synchronized output
- Fragmented escape-sequence parsing

Terminal-initiated clipboard and window operations are not enabled.

## Input precedence

Visible overlays receive input first, followed by global workbench shortcuts, focused terminal content, and focused pane content. This prevents command-palette shortcuts from leaking bytes into a shell or TUI application.

Explicit ANSI backgrounds remain child-owned. Tide can communicate host focus through terminal chrome without recoloring full-screen application content.

## Commands

- ``Ctrl+` `` toggles the bottom dock.
- ``Ctrl+Shift+` `` opens another dock terminal.
- `Ctrl+Alt+T` opens a terminal; `Ctrl+Alt+M` provides a terminal-safe main-pane alternative.
- The command palette provides close, clear, next tab, previous tab, scroll, focus, and kill actions.
