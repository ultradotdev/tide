<!-- Generated from crates/tide-ui/src/help/pages/keyboard-shortcuts.md by `cargo xtask generate-rust-reference --write`. Edit the template, not this file. -->
# Keyboard Shortcuts

These are Tide's default keys: the built-in bindings and the terminal-safe alternates Tide adds for keys that some terminals cannot send. Your own bindings go in `~/.config/tide/keybindings.jsonc`; inside Tide, **Help: Keyboard Shortcuts** lists the keys with yours included. Change a key with `Ctrl+Shift+,` or `Ctrl+Alt+K`; [Themes and Customization](../features/themes.md) explains the file format.

How a key is resolved:

- A binding with a `when` context, such as `gitPanelFocus`, wins over a global binding for the same key while that context is active.
- For the same key and context, the last entry wins, so your entries replace the defaults.
- A command of `""` or `-<command>` removes a key.
- While a terminal has focus, the shell keeps `Ctrl+A`, `Ctrl+C`, `Ctrl+D`, `Ctrl+E`, `Ctrl+K`, `Ctrl+L`, `Ctrl+R`, `Ctrl+U`, `Ctrl+V`, `Ctrl+W`, `Ctrl+Y`, `Ctrl+Z`, `Ctrl+Space` and `Ctrl+\`. Other workbench keys still reach Tide.

Many terminals cannot distinguish Ctrl+Shift combinations or send Cmd at all, so Tide adds Ctrl+Alt and function-key alternates for many commands. When a key does not arrive, run the command from the palette with `Ctrl+K` or `Ctrl+Shift+P`. Lists, dialogs, the find bar, diff tabs and the Agents sidebar also have keys of their own, described in each feature guide.

## All keys

### File

| Keys | Action | Command |
| --- | --- | --- |
| `Ctrl+S`, `Cmd+S` | Save | `file.save` |
| `Ctrl+Shift+S`, `Shift+Cmd+S`, `Ctrl+Alt+S` | Save As... | `file.saveAs` |
| `Ctrl+O`, `Cmd+O` | Open File... | `file.open` |
| `Ctrl+N`, `Cmd+N` | New File | `file.new` |
| `Ctrl+W`, `Cmd+W` | Close Tab | `file.close` |

### View

| Keys | Action | Command |
| --- | --- | --- |
| `Ctrl+Tab`, `Ctrl+PageDown`, `Alt+Cmd+Right` | Next Tab | `editor.nextTab` |
| `Ctrl+Shift+Tab`, `Ctrl+PageUp`, `Alt+Cmd+Left` | Previous Tab | `editor.previousTab` |
| `Ctrl+P`, `Cmd+P` | Quick Open | `workbench.quickOpen` |
| `Ctrl+K`, `Ctrl+Shift+P`, `Shift+Cmd+P`, `F1`, `Ctrl+T`, `Ctrl+E`, `Cmd+E` | Command Palette | `workbench.commandPalette` |
| `F6`, `Shift+Cmd+]`, `Ctrl+Alt+]` | Focus Next Workbench Region | `workbench.focusNextRegion` |
| `Shift+F6`, `Shift+Cmd+[`, `Ctrl+Alt+[` | Focus Previous Workbench Region | `workbench.focusPreviousRegion` |
| `Ctrl+Shift+A`, `Shift+Cmd+A` | Add Tab | `pane.addPanel` |
| `Ctrl+]`, `Cmd+]`, `F7` | Focus Next Main Pane | `workbench.focusNextPane` |
| `Ctrl+[`, `Cmd+[`, `F8` | Focus Previous Main Pane | `workbench.focusPreviousPane` |
| `Ctrl+Shift+B`, `Ctrl+B`, `Ctrl+Alt+B` | Toggle Sidebar | `workbench.toggleSidebar` |
| `` Ctrl+` `` | Toggle Bottom Terminal | `workbench.toggleTerminal` |
| `Ctrl+\`, `Cmd+\` | Split Main Pane Right | `view.splitVertical` |
| `Ctrl+Shift+\`, `Shift+Cmd+\`, `Ctrl+Alt+H` | Split Main Pane Down | `view.splitHorizontal` |
| `Ctrl+1`, `Alt+1` | Focus First Main Pane | `workbench.focusEditor1` |
| `Ctrl+2`, `Alt+2` | Focus Second Main Pane | `workbench.focusEditor2` |
| `Ctrl+3`, `Alt+3` | Focus Third Main Pane | `workbench.focusEditor3` |
| `Ctrl+Alt+E` | Focus File Explorer | `workbench.focusExplorer` |

### Edit

| Keys | Action | Command |
| --- | --- | --- |
| `Ctrl+Z`, `Cmd+Z` | Undo | `edit.undo` |
| `Ctrl+Shift+Z`, `Shift+Cmd+Z`, `Ctrl+Y` | Redo | `edit.redo` |
| `Ctrl+C`, `Cmd+C` | Copy | `edit.copy` |
| `Ctrl+X`, `Cmd+X` | Cut | `edit.cut` |
| `Ctrl+V`, `Cmd+V` | Paste | `edit.paste` |
| `Ctrl+A`, `Cmd+A` | Select All | `edit.selectAll` |
| `Ctrl+D`, `Cmd+D` | Add Selection to Next Match | `edit.selectNextMatch` |
| `Ctrl+Shift+L` | Select All Occurrences | `edit.selectAllOccurrences` |

### Editor

| Keys | Action | Command |
| --- | --- | --- |
| `Ctrl+Alt+Up` | Add Cursor Above | `editor.addCursorAbove` |
| `Ctrl+Alt+Down` | Add Cursor Below | `editor.addCursorBelow` |
| `Ctrl+L`, `Cmd+L` | Select Line | `editor.selectLine` |
| `Ctrl+Shift+D` | Duplicate Selection | `editor.duplicateSelection` |
| `Ctrl+Shift+[` | Fold | `editor.fold` |
| `Ctrl+Shift+]` | Unfold | `editor.unfold` |
| `Ctrl+/`, `Cmd+/` | Toggle Line Comment | `editor.toggleComment` |
| `Ctrl+I` | Show Hover | `lsp.showHover` |
| `Ctrl+Space` | Trigger Completion | `lsp.triggerCompletion` |
| `Ctrl+Shift+Space` | Trigger Signature Help | `lsp.triggerSignatureHelp` |
| `Ctrl+Alt+D` | Duplicate Line | `editor.duplicateLine` |

### Search

| Keys | Action | Command |
| --- | --- | --- |
| `Ctrl+F`, `Cmd+F` | Find | `search.find` |
| `Ctrl+H`, `Cmd+H` | Find and Replace | `search.replace` |
| `Ctrl+Shift+F`, `Shift+Cmd+F`, `Ctrl+Alt+F` | Find in Files | `search.findInFiles` |
| `Ctrl+Shift+H`, `Shift+Cmd+H` | Replace in Files | `search.replaceInFiles` |
| `F3` | Find Next | `search.findNext` |
| `Shift+F3` | Find Previous | `search.findPrevious` |

### Go

| Keys | Action | Command |
| --- | --- | --- |
| `Ctrl+G` | Go to Line... | `editor.gotoLine` |
| `Ctrl+R`, `Cmd+R` | Go to Symbol in Editor... | `editor.gotoSymbol` |
| `Ctrl+Shift+R`, `Shift+Cmd+R` | Go to Symbol in Workspace... | `editor.gotoWorkspaceSymbol` |
| `Ctrl+Shift+K` | Go to Definition | `lsp.goToDefinition` |

### Terminal

| Keys | Action | Command |
| --- | --- | --- |
| `` Ctrl+Shift+` ``, `Ctrl+Alt+T` | Open Bottom Terminal Tab | `terminal.new` |
| `Ctrl+Shift+T`, `Ctrl+Alt+M` | Open Terminal Tab in Main Pane | `terminal.newInPane` |

### Git

| Keys | Action | Command |
| --- | --- | --- |
| `Ctrl+Shift+G`, `Ctrl+Alt+G` | Focus Git Panel | `git.focusPanel` |

### AI

| Keys | Action | Command |
| --- | --- | --- |
| `Ctrl+Shift+I`, `Shift+Cmd+I` | AI CLI: Toggle | `aiCli.toggle` |
| `Ctrl+Shift+J`, `Shift+Cmd+J`, `Ctrl+Alt+A` | Agents: Open Session... | `agents.openSession` |

### Application

| Keys | Action | Command |
| --- | --- | --- |
| `Ctrl+Q`, `Cmd+Q` | Quit | `workbench.quit` |

### Preferences

| Keys | Action | Command |
| --- | --- | --- |
| `Ctrl+,`, `Cmd+,`, `Ctrl+Alt+O` | Open Settings | `workbench.openSettings` |
| `Ctrl+Shift+,`, `Ctrl+Alt+K` | Open Keyboard Shortcuts | `workbench.openKeybindings` |
| `Ctrl+Alt+R` | Select Color Theme | `workbench.selectTheme` |

### Other actions

| Keys | Action | Command |
| --- | --- | --- |
| `Ctrl+Alt+U` | Focus outline | `workbench.focusOutline` |
| `Ctrl+Alt+L` | Focus timeline | `workbench.focusTimeline` |
| `Alt+Right`, `Ctrl+Alt+Right` | Focus pane right | `workbench.focusPaneRight` |
| `Alt+Left`, `Ctrl+Alt+Left` | Focus pane left | `workbench.focusPaneLeft` |
| `Alt+Up` | Focus pane up | `workbench.focusPaneUp` |
| `Alt+Down` | Focus pane down | `workbench.focusPaneDown` |

### Editor with multiple cursors

| Keys | Action | Command |
| --- | --- | --- |
| `Esc` | Clear Cursors | `editor.clearCursors` |

### Terminal

| Keys | Action | Command |
| --- | --- | --- |
| `Cmd+K`, `Ctrl+Shift+K` | Clear Terminal | `terminal.clear` |
| `Ctrl+Shift+Home` | Scroll Terminal to Top | `terminal.scrollToTop` |
| `Ctrl+Shift+End` | Scroll Terminal to Bottom | `terminal.scrollToBottom` |

### Source Control

| Keys | Action | Command |
| --- | --- | --- |
| `s` | Stage | `gitPanel.stage` |
| `Shift+S` | Stage all | `gitPanel.stageAll` |
| `u` | Unstage | `gitPanel.unstage` |
| `d` | Discard | `gitPanel.discard` |
| `Enter` | Open diff | `gitPanel.openDiff` |
| `o` | Open file | `gitPanel.openFile` |
| `r` | Refresh | `gitPanel.refresh` |
| `c` | Commit | `gitPanel.commit` |
| `Space` | Toggle section | `gitPanel.toggleSection` |
| `Up`, `k` | Move up | `gitPanel.moveUp` |
| `Down`, `j` | Move down | `gitPanel.moveDown` |

### Explorer

| Keys | Action | Command |
| --- | --- | --- |
| `Up`, `k` | Move up | `fileTree.moveUp` |
| `Down`, `j` | Move down | `fileTree.moveDown` |
| `Right`, `l` | Expand | `fileTree.expand` |
| `Left`, `h` | Collapse | `fileTree.collapse` |
| `Enter`, `Space` | Open | `fileTree.open` |
| `n` | New file | `fileTree.newFile` |
| `Shift+N` | New folder | `fileTree.newFolder` |
| `r`, `F2` | Rename | `fileTree.rename` |
| `d`, `Delete` | Delete | `fileTree.delete` |
| `Home` | Go to first | `fileTree.goToFirst` |
| `End` | Go to last | `fileTree.goToLast` |
| `PageUp` | Page up | `fileTree.pageUp` |
| `PageDown` | Page down | `fileTree.pageDown` |

### Outline

| Keys | Action | Command |
| --- | --- | --- |
| `Up`, `k` | Move up | `outlinePanel.moveUp` |
| `Down`, `j` | Move down | `outlinePanel.moveDown` |
| `Right`, `l` | Expand | `outlinePanel.expand` |
| `Left`, `h` | Collapse | `outlinePanel.collapse` |
| `Space` | Toggle expand | `outlinePanel.toggleExpand` |
| `Enter` | Select | `outlinePanel.select` |
| `PageUp` | Page up | `outlinePanel.pageUp` |
| `PageDown` | Page down | `outlinePanel.pageDown` |
| `Home` | Go to first | `outlinePanel.goToFirst` |
| `End` | Go to last | `outlinePanel.goToLast` |

### Timeline

| Keys | Action | Command |
| --- | --- | --- |
| `Up`, `k` | Move up | `timelinePanel.moveUp` |
| `Down`, `j` | Move down | `timelinePanel.moveDown` |
| `PageUp` | Page up | `timelinePanel.pageUp` |
| `PageDown` | Page down | `timelinePanel.pageDown` |
| `Home`, `g` | Go to first | `timelinePanel.goToFirst` |
| `End`, `Shift+G` | Go to last | `timelinePanel.goToLast` |
| `Enter` | View diff | `timelinePanel.viewDiff` |
| `o` | Open file at commit | `timelinePanel.openFileAtCommit` |
| `Tab` | Toggle mode | `timelinePanel.toggleMode` |
| `y` | Copy hash | `timelinePanel.copyHash` |

### Query results

| Keys | Action | Command |
| --- | --- | --- |
| `Up`, `k` | Move up | `queryResults.moveUp` |
| `Down`, `j` | Move down | `queryResults.moveDown` |
| `Left`, `h` | Move left | `queryResults.moveLeft` |
| `Right`, `l` | Move right | `queryResults.moveRight` |
| `PageUp` | Page up | `queryResults.pageUp` |
| `PageDown` | Page down | `queryResults.pageDown` |
| `Home` | Go to first | `queryResults.goToFirst` |
| `End` | Go to last | `queryResults.goToLast` |
| `Enter` | Show details | `queryResults.showDetails` |
| `r`, `F5` | Refresh | `queryResults.refresh` |
| `e` | Export | `queryResults.export` |
| `s` | Sort | `queryResults.sort` |
| `Tab` | Toggle view mode | `queryResults.toggleViewMode` |

### SQL editor

| Keys | Action | Command |
| --- | --- | --- |
| `Ctrl+R`, `Ctrl+Enter`, `F5` | Execute Query | `database.executeQuery` |
| `Ctrl+Shift+C` | Connect to Database | `database.connect` |
| `Ctrl+Space` | Complete SQL | `database.complete` |

### While editing

| Keys | Action | Command |
| --- | --- | --- |
| `Ctrl+.` | Show Code Actions | `lsp.codeActions` |

### Outside the editor

| Keys | Action | Command |
| --- | --- | --- |
| `Ctrl+Alt+Up` | Focus pane up | `workbench.focusPaneUp` |
| `Ctrl+Alt+Down` | Focus pane down | `workbench.focusPaneDown` |
