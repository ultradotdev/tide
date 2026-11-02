# Workbench, Panes, and Navigation

Tide's workbench arranges two optional sidebars, one or more main panes, a bottom terminal dock and a status bar under one layout and focus controller.

## Regions

- The **workspace sidebar** holds the Explorer, Source Control, Outline and Timeline sections as an accordion. Click a section header to collapse or expand it. If a saved session is missing a section, add it back with {{key:pane.addPanel}} or commands such as **{{label:sidebar.addGitPanel}}**.
- The **main region** is a tree of split panes. Each pane has its own tab strip and can hold files, diffs, terminals, SQL editors with results, and AI agent sessions.
- The **Agents sidebar** lists the sessions of the AI CLI agents installed on this machine. Toggle it with {{key:agents.togglePanel}}.
- The **bottom dock** holds terminal tabs and spans the main region between the sidebars. Toggle it with {{key:workbench.toggleTerminal}}.
- The **status bar** shows messages and the current Git branch with its ahead and behind counts. **{{label:workbench.statusHistory}}** lists earlier messages.

Each sidebar can sit on the left or the right: use the `▾` menu in its header, or **{{label:workbench.moveSidebarRight}}** and **{{label:workbench.moveAgentsSidebarLeft}}** from the palette. Both sidebars may share a side.

## Keyboard focus

Exactly one region has keyboard focus at a time.

| To | Press |
| --- | --- |
| Next region: sidebar, main, Agents, dock, status bar | {{key:workbench.focusNextRegion}} |
| Previous region | {{key:workbench.focusPreviousRegion}} |
| Next main pane | {{key:workbench.focusNextPane}} |
| Previous main pane | {{key:workbench.focusPreviousPane}} |
| First, second or third main pane | {{key:workbench.focusEditor1}}, {{key:workbench.focusEditor2}}, {{key:workbench.focusEditor3}} |
| The pane to the left or right | {{key:workbench.focusPaneLeft}}, {{key:workbench.focusPaneRight}} |
| The pane above or below | {{key:workbench.focusPaneUp}}, {{key:workbench.focusPaneDown}} |
| Explorer | {{key:workbench.focusExplorer}} |
| Source Control | {{key:git.focusPanel}} |
| Outline | {{key:workbench.focusOutline}} |
| Timeline | {{key:workbench.focusTimeline}} |

Region traversal skips hidden regions. When it enters the workspace sidebar, focus lands on a section header:

- Up and Down, or `j` and `k`, move between headers; Home and End jump to the first or last.
- Enter or Space expands or collapses the section.
- Right expands a collapsed section; pressed again, it enters the section's content. Left collapses.
- Escape in a section's content returns to its header; Escape on a header returns to the main region.

## Panes and tabs

| To | Press |
| --- | --- |
| Split the pane right | {{key:view.splitVertical}} |
| Split the pane down | {{key:view.splitHorizontal}} |
| Add a tab or sidebar section | {{key:pane.addPanel}} |
| Next or previous tab | {{key:editor.nextTab}}, {{key:editor.previousTab}} |
| Switch to a tab by name | {{key:editor.switchTab}} |
| Close the tab | {{key:file.close}} |
| Close the pane | {{key:view.closePane}} |
| Move the tab to the next or previous pane | {{key:editor.moveTabToNextPane}}, {{key:editor.moveTabToPreviousPane}} |
| Move the tab to a chosen pane or the dock | {{key:editor.moveTabToPane}} |

**{{label:file.closeOthers}}** closes every tab in the pane except the active one, asking first about unsaved changes.

Add Tab offers a new file, Quick Open, a terminal, a SQL editor or an AI agent in the focused pane, and can add any sidebar section back. Tabs keep their state when they move. Closing the last pane is refused, and closing a pane with unsaved changes asks first. An empty pane stays until you close it.

Each tab has a `×` close button. When the tabs do not fit, `‹` and `›` scroll the strip and the mouse wheel scrolls it too; `▾` lists every tab.

## Responsive layout

Sidebar widths and the dock height are saved with the session. When the terminal is too small, the Agents sidebar hides first, then the workspace sidebar; below 60 columns the sidebars hide unless focused, and the dock hides when the main region would be shorter than four rows. Their visibility and sizes are remembered, so they come back when there is room. `tui.mainRegion.minimumWidth` ({{setting:tui.mainRegion.minimumWidth}} columns) is how much width the main region keeps before a sidebar collapses.

Toggling a sidebar that collapsed for lack of space focuses it instead. **{{label:workbench.resetRegionSizes}}** restores the default sizes. **{{label:workbench.toggleZenMode}}** hides both sidebars and the dock to leave only the main region; running it again brings back exactly what it hid.

## Mouse

Drag a region divider to resize it. Arrow buttons at the main region's top corners show or hide the sidebars, and the one at its bottom right shows or hides the dock. The dock's `+` opens another terminal. Click tabs, accordion headers and list rows to select them.

## Dialogs

Dialogs wrap their text and line up their buttons. Save and confirm actions and destructive discard or delete actions use distinct colors, and a destructive confirmation starts on Cancel. Keyboard selection and mouse clicks run the same action. Clicking outside closes ordinary pickers; confirmations, Save As and forms stay open until you choose.

See {{help:keyboard-shortcuts}} for every key.
