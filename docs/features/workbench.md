# Workbench, Panes, and Navigation

Tide's workbench arranges two optional sidebars, one or more main panes, a bottom dock, and a status bar under one geometry and focus controller.

## Regions

The workspace sidebar contains Explorer, Source Control, Outline, and Timeline accordions. The Agents sidebar lists available external provider sessions. Either sidebar can move left or right; moving one onto the occupied side swaps them.

The main region is a recursive split tree. Each pane owns a tab strip and can host files, diffs, terminals, SQL editors with results, and agent CLIs. The bottom dock spans the entire main region between visible sidebars.

## Keyboard focus

`F6` and `Shift+F6` traverse visible regions in spatial order. `Ctrl+]` and `Ctrl+[` stay scoped to main panes. `Alt+1`, `Alt+2`, and `Alt+3` provide terminal-safe focus for the first three visible main panes.

When region traversal enters a sidebar, focus lands on an accordion header:

- Up and Down move through headers.
- Home and End jump to the first or last header.
- Enter or Space toggles the section.
- Right expands and enters content.
- Left collapses.
- Escape returns from section content to its header when the content has no local Escape action.

Only one host owns keyboard focus. The shared host-chrome system keeps an active tab visible when its host is unfocused while giving the focused region a stronger semantic treatment.

## Panes and tabs

Use `Ctrl+\` to split right and `Ctrl+Shift+\` to split down. Commands in the palette can close a main pane or move the active tab to the previous, next, or chosen pane. An empty source pane remains until explicitly closed.

Tab strips provide close buttons, overflow menus and wheel scrolling. Tabs preserve their element state when moved. The bottom dock is a separate region and is not a main-pane move destination.

## Responsive layout

Sidebar widths and dock height are fixed cell extents saved with workspace state. When the terminal becomes too small, the Agents sidebar collapses first, followed by the workspace sidebar or dock as required. Requested visibility and saved sizes remain intact so regions can restore when space returns.

`tui.mainRegion.minimumWidth` controls how much main-region width Tide retains before responsively collapsing a sidebar.

## Mouse controls

Region dividers support captured drag resizing. Directional controls on tab strips collapse or restore sidebars and the dock. Tab actions, accordion rows, and split controls share render and hit-test geometry.

## Dialogs

Dialogs wrap body text and align their action buttons consistently. Save/confirm actions and destructive discard/delete actions use semantic intent colors. Keyboard selection and mouse clicks invoke the same action. Ordinary dialogs dismiss when clicking outside; unresolved save/discard guards remain until an explicit choice.

The global status area and pane-local hints use Rust's active bindings. The [generated keyboard reference](../reference/keyboard-shortcuts.md) lists effective defaults and terminal-safe alternatives.
