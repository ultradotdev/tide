# Rendering and Input Geometry

Tide uses Ratatui and Crossterm. The app renders pane state, sidebars, dock and dialogs into a terminal-cell buffer. Ratatui emits differences between frames; event processing requests redraws as state changes.

Rendering records hit targets from the same rectangles used to draw controls. Mouse routing resolves tabs, close buttons, sidebar menus, accordion headers, editor content, proportional scrollbars and modal buttons against those targets. Captured drags keep operating on their owner until release.

Main editor and terminal panes have inset chrome. Sidebar sections use their own headers and action rows; the global status area supplies navigation and active-document information. Semantic theme roles distinguish structural borders, focus, selection and action intent without changing geometry.

Editor views map graphemes to terminal cells, account for wrapping and folding, and render a compact density minimap. Background syntax work returns revision-tagged spans and schedules redraws when ready. Hosted applications are parsed by `alacritty_terminal`; their explicit ANSI foreground/background colors are preserved.

Dialogs wrap body text and share action-intent styling, keyboard selection and mouse targets. Outside clicks dismiss ordinary dialogs. Save/discard guards remain explicit so an accidental click cannot discard a buffer or authorize exit.
