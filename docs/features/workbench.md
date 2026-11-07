<!-- Generated from crates/tide-ui/src/help/pages/workbench.md by `cargo xtask generate-rust-reference --write`. Edit the template, not this file. -->
# Workbench, Panes, and Navigation

Tide's workbench arranges two optional sidebars, one or more main panes, a bottom terminal dock and a status bar under one layout and focus controller.

## Regions

- The **workspace sidebar** holds the Explorer, Source Control, Outline and Timeline sections as an accordion. Click a section header to collapse or expand it. If a saved session is missing a section, add it back with `Ctrl+Shift+A` or commands such as **Sidebar: Add Source Control**.
- The **main region** is a tree of split panes. Each pane has its own tab strip and can hold files, diffs, terminals, SQL editors with results, and AI agent sessions.
- The **Agents sidebar** lists the sessions of the AI CLI agents installed on this machine. Toggle it with “Agents: Toggle Sidebar” from the command palette (`Ctrl+K`).
- The **bottom dock** holds terminal tabs and spans the main region between the sidebars. Toggle it with `` Ctrl+` ``.
- The **status bar** shows messages and the current Git branch with its ahead and behind counts. **Workbench: Status History** lists earlier messages.

Each sidebar can sit on the left or the right: use the `▾` menu in its header, or **Move Sidebar Right** and **View: Move Agents Sidebar Left** from the palette. Both sidebars may share a side.

## Keyboard focus

Exactly one region has keyboard focus at a time.

| To | Press |
| --- | --- |
| Next region: sidebar, main, Agents, dock, status bar | `F6` or `Ctrl+Alt+]` |
| Previous region | `Shift+F6` or `Ctrl+Alt+[` |
| Next main pane | `Ctrl+]` or `F7` |
| Previous main pane | `Ctrl+[` or `F8` |
| First, second or third main pane | `Ctrl+1` or `Alt+1`, `Ctrl+2` or `Alt+2`, `Ctrl+3` or `Alt+3` |
| The pane to the left or right | `Alt+Left` or `Ctrl+Alt+Left`, `Alt+Right` or `Ctrl+Alt+Right` |
| The pane above or below | `Ctrl+Alt+Up` or `Alt+Up`, `Ctrl+Alt+Down` or `Alt+Down` |
| Explorer | `Ctrl+Alt+E` |
| Source Control | `Ctrl+Shift+G` or `Ctrl+Alt+G` |
| Outline | `Ctrl+Alt+U` |
| Timeline | `Ctrl+Alt+L` |

Region traversal skips hidden regions. When it enters the workspace sidebar, focus lands on a section header:

- Up and Down, or `j` and `k`, move between headers; Home and End jump to the first or last.
- Enter or Space expands or collapses the section.
- Right expands a collapsed section; pressed again, it enters the section's content. Left collapses.
- Escape in a section's content returns to its header; Escape on a header returns to the main region.

## Panes and tabs

| To | Press |
| --- | --- |
| Split the pane right | `Ctrl+\` |
| Split the pane down | `Ctrl+Shift+\` or `Ctrl+Alt+H` |
| Add a tab or sidebar section | `Ctrl+Shift+A` |
| Next or previous tab | `Ctrl+Tab` or `Ctrl+PageDown`, `Ctrl+Shift+Tab` or `Ctrl+PageUp` |
| Switch to a tab by name | “Editor: Switch Tab” from the command palette (`Ctrl+K`) |
| Close the tab | `Ctrl+W` |
| Close the pane | “Close Main Pane” from the command palette (`Ctrl+K`) |
| Move the tab to the next or previous pane | “Move Active Tab to Next Main Pane” from the command palette (`Ctrl+K`), “Move Active Tab to Previous Main Pane” from the command palette (`Ctrl+K`) |
| Move the tab to a chosen pane or the dock | “Move Active Tab to Main Pane...” from the command palette (`Ctrl+K`) |

**Close Other Tabs** closes every tab in the pane except the active one, asking first about unsaved changes.

Add Tab offers a new file, Quick Open, a terminal, a SQL editor or an AI agent in the focused pane, and can add any sidebar section back. Tabs keep their state when they move. Closing the last pane is refused, and closing a pane with unsaved changes asks first. An empty pane stays until you close it.

Each tab has a `×` close button. When the tabs do not fit, `‹` and `›` scroll the strip and the mouse wheel scrolls it too; `▾` lists every tab, and ends with **Split Right** and **Split Down**, which split that pane without taking room in its tab bar.

## Responsive layout

Sidebar widths and the dock height are saved with the session. When the terminal is too small, the Agents sidebar hides first, then the workspace sidebar; below 60 columns the sidebars hide unless focused, and the dock hides when the main region would be shorter than four rows. Their visibility and sizes are remembered, so they come back when there is room. `tui.mainRegion.minimumWidth` (`56` columns) is how much width the main region keeps before a sidebar collapses.

Toggling a sidebar that collapsed for lack of space focuses it instead. **Reset Workbench Region Sizes** restores the default sizes. **Toggle Zen Mode** hides both sidebars and the dock to leave only the main region; running it again brings back exactly what it hid.

## Mouse

Drag a region divider to resize it. Arrow buttons at the main region's top corners show or hide the sidebars, and the one at its bottom right shows or hides the dock. The dock's `+` opens another terminal. Click tabs, accordion headers and list rows to select them.

## Dialogs

Dialogs wrap their text and line up their buttons. Save and confirm actions and destructive discard or delete actions use distinct colors, and a destructive confirmation starts on Cancel. Keyboard selection and mouse clicks run the same action. Clicking outside closes ordinary pickers; confirmations, Save As and forms stay open until you choose. A prompt that opens with a value, such as Go to Line or Rename, has it selected: typing replaces it, and Left, Right, Home or End keeps it so you can edit it.

See [Keyboard Shortcuts](../reference/keyboard-shortcuts.md) for every key.
