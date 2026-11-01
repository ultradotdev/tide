# Keyboard Shortcuts

These are the compiled Rust defaults, including terminal-safe alternatives. User bindings belong in `keybindings.jsonc` under the Tide configuration directory.

Context-specific matches take precedence over unconditional bindings. Within the same context, the last matching entry wins. An empty command or a command beginning with `-` explicitly removes a binding. Some terminal emulators cannot transmit Ctrl+Shift or Cmd combinations; use a listed terminal-safe alternative such as `ctrl+k` for the command palette.

| Keys | Action | Command ID | Context |
| --- | --- | --- | --- |
| `alt+1` | Focus First Main Pane | `workbench.focusEditor1` | Any |
| `alt+2` | Focus Second Main Pane | `workbench.focusEditor2` | Any |
| `alt+3` | Focus Third Main Pane | `workbench.focusEditor3` | Any |
| `alt+down` | workbench.focusPaneDown | `workbench.focusPaneDown` | Any |
| `alt+left` | workbench.focusPaneLeft | `workbench.focusPaneLeft` | Any |
| `alt+right` | workbench.focusPaneRight | `workbench.focusPaneRight` | Any |
| `alt+up` | workbench.focusPaneUp | `workbench.focusPaneUp` | Any |
| `ctrl+alt+[` | Focus Previous Workbench Region | `workbench.focusPreviousRegion` | Any |
| `ctrl+alt+]` | Focus Next Workbench Region | `workbench.focusNextRegion` | Any |
| `ctrl+alt+a` | Agents: Open Session... | `agents.openSession` | Any |
| `ctrl+alt+down` | Add Cursor Below | `editor.addCursorBelow` | Any |
| `ctrl+alt+down` | workbench.focusPaneDown | `workbench.focusPaneDown` | `!editorFocus` |
| `ctrl+alt+left` | workbench.focusPaneLeft | `workbench.focusPaneLeft` | Any |
| `ctrl+alt+right` | workbench.focusPaneRight | `workbench.focusPaneRight` | Any |
| `ctrl+alt+up` | Add Cursor Above | `editor.addCursorAbove` | Any |
| `ctrl+alt+up` | workbench.focusPaneUp | `workbench.focusPaneUp` | `!editorFocus` |
| `ctrl+alt+b` | Toggle Sidebar | `workbench.toggleSidebar` | Any |
| `ctrl+alt+d` | Duplicate Line | `editor.duplicateLine` | Any |
| `ctrl+alt+e` | workbench.focusExplorer | `workbench.focusExplorer` | Any |
| `ctrl+alt+f` | Find in Files | `search.findInFiles` | Any |
| `ctrl+alt+g` | Focus Git Panel | `git.focusPanel` | Any |
| `ctrl+alt+h` | Split Main Pane Down | `view.splitHorizontal` | Any |
| `ctrl+alt+k` | Open Keyboard Shortcuts | `workbench.openKeybindings` | Any |
| `ctrl+alt+l` | workbench.focusTimeline | `workbench.focusTimeline` | Any |
| `ctrl+alt+m` | Open Terminal Tab in Main Pane | `terminal.newInPane` | Any |
| `ctrl+alt+o` | Open Settings | `workbench.openSettings` | Any |
| `ctrl+alt+r` | Select Color Theme | `workbench.selectTheme` | Any |
| `ctrl+alt+s` | Save As... | `file.saveAs` | Any |
| `ctrl+alt+t` | Open Bottom Terminal Tab | `terminal.new` | Any |
| `ctrl+alt+u` | workbench.focusOutline | `workbench.focusOutline` | Any |
| `meta+alt+left` | Previous Tab | `editor.previousTab` | Any |
| `meta+alt+right` | Next Tab | `editor.nextTab` | Any |
| `ArrowDown` | fileTree.moveDown | `fileTree.moveDown` | `fileTreeFocus` |
| `ArrowDown` | gitPanel.moveDown | `gitPanel.moveDown` | `gitPanelFocus` |
| `ArrowDown` | outlinePanel.moveDown | `outlinePanel.moveDown` | `outlinePanelFocus` |
| `ArrowDown` | queryResults.moveDown | `queryResults.moveDown` | `queryResultsFocus` |
| `ArrowDown` | timelinePanel.moveDown | `timelinePanel.moveDown` | `timelinePanelFocus` |
| `ArrowLeft` | fileTree.collapse | `fileTree.collapse` | `fileTreeFocus` |
| `ArrowLeft` | outlinePanel.collapse | `outlinePanel.collapse` | `outlinePanelFocus` |
| `ArrowLeft` | queryResults.moveLeft | `queryResults.moveLeft` | `queryResultsFocus` |
| `ArrowRight` | fileTree.expand | `fileTree.expand` | `fileTreeFocus` |
| `ArrowRight` | outlinePanel.expand | `outlinePanel.expand` | `outlinePanelFocus` |
| `ArrowRight` | queryResults.moveRight | `queryResults.moveRight` | `queryResultsFocus` |
| `ArrowUp` | fileTree.moveUp | `fileTree.moveUp` | `fileTreeFocus` |
| `ArrowUp` | gitPanel.moveUp | `gitPanel.moveUp` | `gitPanelFocus` |
| `ArrowUp` | outlinePanel.moveUp | `outlinePanel.moveUp` | `outlinePanelFocus` |
| `ArrowUp` | queryResults.moveUp | `queryResults.moveUp` | `queryResultsFocus` |
| `ArrowUp` | timelinePanel.moveUp | `timelinePanel.moveUp` | `timelinePanelFocus` |
| `c` | gitPanel.commit | `gitPanel.commit` | `gitPanelFocus` |
| `ctrl+,` | Open Settings | `workbench.openSettings` | Any |
| `ctrl+.` | Show Code Actions | `lsp.codeActions` | `editorFocus` |
| `ctrl+/` | Toggle Line Comment | `editor.toggleComment` | Any |
| `ctrl+1` | Focus First Main Pane | `workbench.focusEditor1` | Any |
| `ctrl+2` | Focus Second Main Pane | `workbench.focusEditor2` | Any |
| `ctrl+3` | Focus Third Main Pane | `workbench.focusEditor3` | Any |
| `ctrl+[` | Focus Previous Main Pane | `workbench.focusPreviousPane` | Any |
| `ctrl+\` | Split Main Pane Right | `view.splitVertical` | Any |
| `ctrl+]` | Focus Next Main Pane | `workbench.focusNextPane` | Any |
| `ctrl+\`` | Toggle Bottom Terminal | `workbench.toggleTerminal` | Any |
| `ctrl+a` | Select All | `edit.selectAll` | Any |
| `ctrl+b` | Toggle Sidebar | `workbench.toggleSidebar` | Any |
| `ctrl+c` | Copy | `edit.copy` | Any |
| `ctrl+d` | Add Selection to Next Match | `edit.selectNextMatch` | Any |
| `ctrl+e` | Command Palette | `workbench.commandPalette` | Any |
| `ctrl+enter` | Execute Query | `database.executeQuery` | `sqlEditorFocus` |
| `ctrl+f` | Find | `search.find` | Any |
| `ctrl+g` | Go to Line... | `editor.gotoLine` | Any |
| `ctrl+h` | Find and Replace | `search.replace` | Any |
| `ctrl+i` | Show Hover | `lsp.showHover` | Any |
| `ctrl+k` | Command Palette | `workbench.commandPalette` | Any |
| `ctrl+l` | Select Line | `editor.selectLine` | Any |
| `ctrl+n` | New File | `file.new` | Any |
| `ctrl+o` | Open File... | `file.open` | Any |
| `ctrl+p` | Quick Open | `workbench.quickOpen` | Any |
| `ctrl+pagedown` | Next Tab | `editor.nextTab` | Any |
| `ctrl+pageup` | Previous Tab | `editor.previousTab` | Any |
| `ctrl+q` | Quit | `workbench.quit` | Any |
| `ctrl+r` | Go to Symbol in Editor... | `editor.gotoSymbol` | Any |
| `ctrl+r` | Execute Query | `database.executeQuery` | `sqlEditorFocus` |
| `ctrl+s` | Save | `file.save` | Any |
| `ctrl+shift+,` | Open Keyboard Shortcuts | `workbench.openKeybindings` | Any |
| `ctrl+shift+[` | Fold | `editor.fold` | Any |
| `ctrl+shift+\` | Split Main Pane Down | `view.splitHorizontal` | Any |
| `ctrl+shift+]` | Unfold | `editor.unfold` | Any |
| `ctrl+shift+\`` | Open Bottom Terminal Tab | `terminal.new` | Any |
| `ctrl+shift+a` | Add Tab | `pane.addPanel` | Any |
| `ctrl+shift+b` | Toggle Sidebar | `workbench.toggleSidebar` | Any |
| `ctrl+shift+c` | Connect to Database | `database.connect` | `sqlEditorFocus` |
| `ctrl+shift+d` | Duplicate Selection | `editor.duplicateSelection` | Any |
| `ctrl+shift+f` | Find in Files | `search.findInFiles` | Any |
| `ctrl+shift+g` | Focus Git Panel | `git.focusPanel` | Any |
| `ctrl+shift+i` | AI CLI: Toggle | `aiCli.toggle` | Any |
| `ctrl+shift+j` | Agents: Open Session... | `agents.openSession` | Any |
| `ctrl+shift+k` | Go to Definition | `lsp.goToDefinition` | Any |
| `ctrl+shift+l` | Select All Occurrences | `edit.selectAllOccurrences` | Any |
| `ctrl+shift+p` | Command Palette | `workbench.commandPalette` | Any |
| `ctrl+shift+r` | Go to Symbol in Workspace... | `editor.gotoWorkspaceSymbol` | Any |
| `ctrl+shift+s` | Save As... | `file.saveAs` | Any |
| `ctrl+shift+space` | Trigger Signature Help | `lsp.triggerSignatureHelp` | Any |
| `ctrl+shift+t` | Open Terminal Tab in Main Pane | `terminal.newInPane` | Any |
| `ctrl+shift+tab` | Previous Tab | `editor.previousTab` | Any |
| `ctrl+shift+z` | Redo | `edit.redo` | Any |
| `ctrl+space` | Trigger Completion | `lsp.triggerCompletion` | Any |
| `ctrl+space` | Complete SQL | `database.complete` | `sqlEditorFocus` |
| `ctrl+t` | Command Palette | `workbench.commandPalette` | Any |
| `ctrl+tab` | Next Tab | `editor.nextTab` | Any |
| `ctrl+v` | Paste | `edit.paste` | Any |
| `ctrl+w` | Close Tab | `file.close` | Any |
| `ctrl+x` | Cut | `edit.cut` | Any |
| `ctrl+y` | Redo | `edit.redo` | Any |
| `ctrl+z` | Undo | `edit.undo` | Any |
| `d` | fileTree.delete | `fileTree.delete` | `fileTreeFocus` |
| `d` | gitPanel.discard | `gitPanel.discard` | `gitPanelFocus` |
| `Delete` | fileTree.delete | `fileTree.delete` | `fileTreeFocus` |
| `e` | queryResults.export | `queryResults.export` | `queryResultsFocus` |
| `End` | fileTree.goToLast | `fileTree.goToLast` | `fileTreeFocus` |
| `End` | outlinePanel.goToLast | `outlinePanel.goToLast` | `outlinePanelFocus` |
| `End` | queryResults.goToLast | `queryResults.goToLast` | `queryResultsFocus` |
| `End` | timelinePanel.goToLast | `timelinePanel.goToLast` | `timelinePanelFocus` |
| `Enter` | fileTree.open | `fileTree.open` | `fileTreeFocus` |
| `Enter` | gitPanel.openDiff | `gitPanel.openDiff` | `gitPanelFocus` |
| `Enter` | outlinePanel.select | `outlinePanel.select` | `outlinePanelFocus` |
| `Enter` | queryResults.showDetails | `queryResults.showDetails` | `queryResultsFocus` |
| `Enter` | timelinePanel.viewDiff | `timelinePanel.viewDiff` | `timelinePanelFocus` |
| `escape` | Clear Cursors | `editor.clearCursors` | `editorHasMultipleCursors` |
| `f1` | Command Palette | `workbench.commandPalette` | Any |
| `F2` | fileTree.rename | `fileTree.rename` | `fileTreeFocus` |
| `f3` | Find Next | `search.findNext` | Any |
| `F5` | queryResults.refresh | `queryResults.refresh` | `queryResultsFocus` |
| `F5` | Execute Query | `database.executeQuery` | `sqlEditorFocus` |
| `f6` | Focus Next Workbench Region | `workbench.focusNextRegion` | Any |
| `f7` | Focus Next Main Pane | `workbench.focusNextPane` | Any |
| `f8` | Focus Previous Main Pane | `workbench.focusPreviousPane` | Any |
| `g` | timelinePanel.goToFirst | `timelinePanel.goToFirst` | `timelinePanelFocus` |
| `h` | fileTree.collapse | `fileTree.collapse` | `fileTreeFocus` |
| `h` | outlinePanel.collapse | `outlinePanel.collapse` | `outlinePanelFocus` |
| `h` | queryResults.moveLeft | `queryResults.moveLeft` | `queryResultsFocus` |
| `Home` | fileTree.goToFirst | `fileTree.goToFirst` | `fileTreeFocus` |
| `Home` | outlinePanel.goToFirst | `outlinePanel.goToFirst` | `outlinePanelFocus` |
| `Home` | queryResults.goToFirst | `queryResults.goToFirst` | `queryResultsFocus` |
| `Home` | timelinePanel.goToFirst | `timelinePanel.goToFirst` | `timelinePanelFocus` |
| `j` | fileTree.moveDown | `fileTree.moveDown` | `fileTreeFocus` |
| `j` | gitPanel.moveDown | `gitPanel.moveDown` | `gitPanelFocus` |
| `j` | outlinePanel.moveDown | `outlinePanel.moveDown` | `outlinePanelFocus` |
| `j` | queryResults.moveDown | `queryResults.moveDown` | `queryResultsFocus` |
| `j` | timelinePanel.moveDown | `timelinePanel.moveDown` | `timelinePanelFocus` |
| `k` | fileTree.moveUp | `fileTree.moveUp` | `fileTreeFocus` |
| `k` | gitPanel.moveUp | `gitPanel.moveUp` | `gitPanelFocus` |
| `k` | outlinePanel.moveUp | `outlinePanel.moveUp` | `outlinePanelFocus` |
| `k` | queryResults.moveUp | `queryResults.moveUp` | `queryResultsFocus` |
| `k` | timelinePanel.moveUp | `timelinePanel.moveUp` | `timelinePanelFocus` |
| `l` | fileTree.expand | `fileTree.expand` | `fileTreeFocus` |
| `l` | outlinePanel.expand | `outlinePanel.expand` | `outlinePanelFocus` |
| `l` | queryResults.moveRight | `queryResults.moveRight` | `queryResultsFocus` |
| `meta+,` | Open Settings | `workbench.openSettings` | Any |
| `meta+/` | Toggle Line Comment | `editor.toggleComment` | Any |
| `meta+[` | Focus Previous Main Pane | `workbench.focusPreviousPane` | Any |
| `meta+\` | Split Main Pane Right | `view.splitVertical` | Any |
| `meta+]` | Focus Next Main Pane | `workbench.focusNextPane` | Any |
| `meta+a` | Select All | `edit.selectAll` | Any |
| `meta+c` | Copy | `edit.copy` | Any |
| `meta+d` | Add Selection to Next Match | `edit.selectNextMatch` | Any |
| `meta+e` | Command Palette | `workbench.commandPalette` | Any |
| `meta+f` | Find | `search.find` | Any |
| `meta+h` | Find and Replace | `search.replace` | Any |
| `meta+l` | Select Line | `editor.selectLine` | Any |
| `meta+n` | New File | `file.new` | Any |
| `meta+o` | Open File... | `file.open` | Any |
| `meta+p` | Quick Open | `workbench.quickOpen` | Any |
| `meta+q` | Quit | `workbench.quit` | Any |
| `meta+r` | Go to Symbol in Editor... | `editor.gotoSymbol` | Any |
| `meta+s` | Save | `file.save` | Any |
| `shift+meta+[` | Focus Previous Workbench Region | `workbench.focusPreviousRegion` | Any |
| `shift+meta+\` | Split Main Pane Down | `view.splitHorizontal` | Any |
| `shift+meta+]` | Focus Next Workbench Region | `workbench.focusNextRegion` | Any |
| `shift+meta+a` | Add Tab | `pane.addPanel` | Any |
| `shift+meta+f` | Find in Files | `search.findInFiles` | Any |
| `shift+meta+i` | AI CLI: Toggle | `aiCli.toggle` | Any |
| `shift+meta+j` | Agents: Open Session... | `agents.openSession` | Any |
| `shift+meta+p` | Command Palette | `workbench.commandPalette` | Any |
| `shift+meta+r` | Go to Symbol in Workspace... | `editor.gotoWorkspaceSymbol` | Any |
| `shift+meta+s` | Save As... | `file.saveAs` | Any |
| `shift+meta+z` | Redo | `edit.redo` | Any |
| `meta+v` | Paste | `edit.paste` | Any |
| `meta+w` | Close Tab | `file.close` | Any |
| `meta+x` | Cut | `edit.cut` | Any |
| `meta+z` | Undo | `edit.undo` | Any |
| `n` | fileTree.newFile | `fileTree.newFile` | `fileTreeFocus` |
| `o` | gitPanel.openFile | `gitPanel.openFile` | `gitPanelFocus` |
| `o` | timelinePanel.openFileAtCommit | `timelinePanel.openFileAtCommit` | `timelinePanelFocus` |
| `PageDown` | fileTree.pageDown | `fileTree.pageDown` | `fileTreeFocus` |
| `PageDown` | outlinePanel.pageDown | `outlinePanel.pageDown` | `outlinePanelFocus` |
| `PageDown` | queryResults.pageDown | `queryResults.pageDown` | `queryResultsFocus` |
| `PageDown` | timelinePanel.pageDown | `timelinePanel.pageDown` | `timelinePanelFocus` |
| `PageUp` | fileTree.pageUp | `fileTree.pageUp` | `fileTreeFocus` |
| `PageUp` | outlinePanel.pageUp | `outlinePanel.pageUp` | `outlinePanelFocus` |
| `PageUp` | queryResults.pageUp | `queryResults.pageUp` | `queryResultsFocus` |
| `PageUp` | timelinePanel.pageUp | `timelinePanel.pageUp` | `timelinePanelFocus` |
| `r` | fileTree.rename | `fileTree.rename` | `fileTreeFocus` |
| `r` | gitPanel.refresh | `gitPanel.refresh` | `gitPanelFocus` |
| `r` | queryResults.refresh | `queryResults.refresh` | `queryResultsFocus` |
| `s` | gitPanel.stage | `gitPanel.stage` | `gitPanelFocus` |
| `s` | queryResults.sort | `queryResults.sort` | `queryResultsFocus` |
| `shift+f3` | Find Previous | `search.findPrevious` | Any |
| `shift+f6` | Focus Previous Workbench Region | `workbench.focusPreviousRegion` | Any |
| `shift+g` | timelinePanel.goToLast | `timelinePanel.goToLast` | `timelinePanelFocus` |
| `shift+n` | fileTree.newFolder | `fileTree.newFolder` | `fileTreeFocus` |
| `shift+s` | gitPanel.stageAll | `gitPanel.stageAll` | `gitPanelFocus` |
| ` ` | fileTree.open | `fileTree.open` | `fileTreeFocus` |
| ` ` | gitPanel.toggleSection | `gitPanel.toggleSection` | `gitPanelFocus` |
| ` ` | outlinePanel.toggleExpand | `outlinePanel.toggleExpand` | `outlinePanelFocus` |
| `Tab` | queryResults.toggleViewMode | `queryResults.toggleViewMode` | `queryResultsFocus` |
| `Tab` | timelinePanel.toggleMode | `timelinePanel.toggleMode` | `timelinePanelFocus` |
| `u` | gitPanel.unstage | `gitPanel.unstage` | `gitPanelFocus` |
| `y` | timelinePanel.copyHash | `timelinePanel.copyHash` | `timelinePanelFocus` |

Modal buttons, list navigation and editor input also have controls local to their surfaces. See the feature guides for Find/Replace options, database results and pane navigation.

Generated from the compiled Rust runtime for Tide 0.10.0. Regenerate with `cargo xtask generate-rust-reference --write`; CI verifies with `--check`.
