<!-- Generated from crates/tide-ui/src/help/pages/keyboard-shortcuts.md by `cargo xtask generate-rust-reference --write`. Edit the template, not this file. -->
# Keyboard Shortcuts

These are Tide's default keys: the built-in bindings and the terminal-safe alternates Tide adds for keys that some terminals cannot send. Your own bindings go in `~/.config/tide/keybindings.jsonc`; inside Tide, **Help: Keyboard Shortcuts** lists the keys with yours included. Change a key with `Ctrl+Shift+,` or `Ctrl+Alt+K`; [Themes and Customization](../features/themes.md) explains the file format.

How a key is resolved:

- A binding with a `when` context, such as `gitPanelFocus`, wins over a global binding for the same key while that context is active.
- For the same key and context, the last entry wins, so your entries replace the defaults.
- A command of `""` or `-<command>` removes a key.
- While a terminal has focus, the shell keeps every Ctrl+letter key, such as `Ctrl+C`, `Ctrl+R`, `Ctrl+X` and `Ctrl+Q`, plus `Ctrl+Space`, `Ctrl+@`, `Ctrl+[`, `Ctrl+\`, `Ctrl+]`, `Ctrl+^` and `Ctrl+_`. Other workbench keys still reach Tide.

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
| `Ctrl+Alt+C`, `Alt+Cmd+C` | Copy Path | `file.copyPath` |
| `Ctrl+Alt+Shift+C`, `Alt+Shift+Cmd+C` | Copy Relative Path | `file.copyRelativePath` |
| `Alt+Shift+R`, `Alt+Cmd+R` | Reveal in File Manager | `file.revealInFinder` |

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
| `Ctrl+Alt+E` | Focus Explorer | `workbench.focusExplorer` |
| `Ctrl+Alt+U` | Focus Outline | `workbench.focusOutline` |
| `Ctrl+Alt+L` | Focus Timeline | `workbench.focusTimeline` |
| `Alt+Right`, `Ctrl+Alt+Right` | Focus Pane to the Right | `workbench.focusPaneRight` |
| `Alt+Left`, `Ctrl+Alt+Left` | Focus Pane to the Left | `workbench.focusPaneLeft` |
| `Alt+Up` | Focus Pane Above | `workbench.focusPaneUp` |
| `Alt+Down` | Focus Pane Below | `workbench.focusPaneDown` |

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
| `Ctrl+Shift+I`, `Shift+Cmd+I` | Agents: New Session... | `agents.newSession` |
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

### Editor with multiple cursors

| Keys | Action | Command |
| --- | --- | --- |
| `Esc` | Clear Cursors | `editor.clearCursors` |

### While editing

| Keys | Action | Command |
| --- | --- | --- |
| `Ctrl+Shift+Up` | Move Line Up | `edit.moveLinesUp` |
| `Ctrl+Shift+Down` | Move Line Down | `edit.moveLinesDown` |
| `Shift+Cmd+K` | Delete Line | `edit.deleteLine` |
| `Alt+Shift+A` | Toggle Block Comment | `editor.toggleBlockComment` |
| `Ctrl+F12` | Go to Implementation | `lsp.goToImplementation` |
| `Ctrl+.` | Show Code Actions | `lsp.codeActions` |

### Terminal

| Keys | Action | Command |
| --- | --- | --- |
| `Cmd+K`, `Ctrl+Shift+K` | Clear Terminal | `terminal.clear` |
| `Ctrl+Shift+Home` | Scroll Terminal to Top | `terminal.scrollToTop` |
| `Ctrl+Shift+End` | Scroll Terminal to Bottom | `terminal.scrollToBottom` |

### Source Control

| Keys | Action | Command |
| --- | --- | --- |
| `s` | Source Control: Stage Selected File | `gitPanel.stage` |
| `Shift+S` | Stage All Changes | `git.stageAll` |
| `u` | Source Control: Unstage Selected File | `gitPanel.unstage` |
| `d` | Source Control: Discard Selected Changes... | `gitPanel.discard` |
| `Enter` | Source Control: Open Selected Diff | `gitPanel.openDiff` |
| `o` | Source Control: Open Selected File | `gitPanel.openFile` |
| `r` | Refresh | `git.refresh` |
| `c` | Commit | `git.commit` |
| `Space` | Source Control: Collapse or Expand Group | `gitPanel.toggleSection` |
| `Up`, `k` | Source Control: Select Previous | `gitPanel.moveUp` |
| `Down`, `j` | Source Control: Select Next | `gitPanel.moveDown` |

### Explorer

| Keys | Action | Command |
| --- | --- | --- |
| `Up`, `k` | Explorer: Select Previous | `fileTree.moveUp` |
| `Down`, `j` | Explorer: Select Next | `fileTree.moveDown` |
| `Right`, `l` | Explorer: Expand Folder | `fileTree.expand` |
| `Left`, `h` | Explorer: Collapse Folder | `fileTree.collapse` |
| `Enter`, `Space` | Explorer: Open Selected | `fileTree.open` |
| `n` | Explorer: New File... | `fileTree.newFile` |
| `Shift+N` | Explorer: New Folder... | `fileTree.newFolder` |
| `r`, `F2` | Explorer: Rename... | `fileTree.rename` |
| `d`, `Delete` | Explorer: Delete... | `fileTree.delete` |
| `Home` | Explorer: Select First | `fileTree.goToFirst` |
| `End` | Explorer: Select Last | `fileTree.goToLast` |
| `PageUp` | Explorer: Page Up | `fileTree.pageUp` |
| `PageDown` | Explorer: Page Down | `fileTree.pageDown` |

### Outline

| Keys | Action | Command |
| --- | --- | --- |
| `Up`, `k` | Outline: Select Previous | `outlinePanel.moveUp` |
| `Down`, `j` | Outline: Select Next | `outlinePanel.moveDown` |
| `Right`, `l` | Outline: Expand Selected | `outlinePanel.expand` |
| `Left`, `h` | Outline: Collapse Selected | `outlinePanel.collapse` |
| `Space` | Outline: Toggle Selected | `outlinePanel.toggleExpand` |
| `Enter` | Outline: Go to Selected Symbol | `outlinePanel.select` |
| `PageUp` | Outline: Page Up | `outlinePanel.pageUp` |
| `PageDown` | Outline: Page Down | `outlinePanel.pageDown` |
| `Home` | Outline: Select First | `outlinePanel.goToFirst` |
| `End` | Outline: Select Last | `outlinePanel.goToLast` |

### Timeline

| Keys | Action | Command |
| --- | --- | --- |
| `Up`, `k` | Timeline: Select Previous | `timelinePanel.moveUp` |
| `Down`, `j` | Timeline: Select Next | `timelinePanel.moveDown` |
| `PageUp` | Timeline: Page Up | `timelinePanel.pageUp` |
| `PageDown` | Timeline: Page Down | `timelinePanel.pageDown` |
| `Home`, `g` | Timeline: Select First | `timelinePanel.goToFirst` |
| `End`, `Shift+G` | Timeline: Select Last | `timelinePanel.goToLast` |
| `Enter` | Timeline: View Selected Commit's Diff | `timelinePanel.viewDiff` |
| `o` | Timeline: Open File at Selected Commit | `timelinePanel.openFileAtCommit` |
| `Tab` | Timeline: Toggle File or Repository History | `timelinePanel.toggleMode` |
| `y` | Timeline: Copy Selected Commit Hash | `timelinePanel.copyHash` |

### Query results

| Keys | Action | Command |
| --- | --- | --- |
| `Up`, `k` | Query Results: Previous Row | `queryResults.moveUp` |
| `Down`, `j` | Query Results: Next Row | `queryResults.moveDown` |
| `Left`, `h` | Query Results: Previous Column | `queryResults.moveLeft` |
| `Right`, `l` | Query Results: Next Column | `queryResults.moveRight` |
| `PageUp` | Query Results: Page Up | `queryResults.pageUp` |
| `PageDown` | Query Results: Page Down | `queryResults.pageDown` |
| `Home` | Query Results: First Row | `queryResults.goToFirst` |
| `End` | Query Results: Last Row | `queryResults.goToLast` |
| `Enter` | Query Results: Show Row Details | `queryResults.showDetails` |
| `r`, `F5` | Query Results: Run Query Again | `queryResults.refresh` |
| `e` | Export Results | `database.exportResults` |
| `s` | Query Results: Sort by Selected Column | `queryResults.sort` |
| `Tab` | Query Results: Toggle View Mode | `queryResults.toggleViewMode` |

### SQL editor

| Keys | Action | Command |
| --- | --- | --- |
| `Ctrl+R`, `Ctrl+Enter`, `F5` | Execute Query | `database.executeQuery` |
| `Ctrl+Shift+C` | Connect to Database | `database.connect` |
| `Ctrl+Space` | Complete SQL | `database.complete` |

### Outside the editor

| Keys | Action | Command |
| --- | --- | --- |
| `Ctrl+Alt+Up` | Focus Pane Above | `workbench.focusPaneUp` |
| `Ctrl+Alt+Down` | Focus Pane Below | `workbench.focusPaneDown` |
