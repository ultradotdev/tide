<!-- Generated from crates/tide-ui/src/help/pages/commands.md by `cargo xtask generate-rust-reference --write`. Edit the template, not this file. -->
# Commands

Every command is in the command palette (`Ctrl+K` or `Ctrl+Shift+P`): type part of its name and press Enter. Commands for the Explorer, Source Control, Outline and Timeline panels and for query results act on that panel's selection, revealing the panel first. Keys are listed where a command has one, using your current bindings. The ID is what you use in `~/.config/tide/keybindings.jsonc`.

## Palette commands

### AI

| Command | Keys | ID |
| --- | --- | --- |
| Agents: Archive Selected Session... | — | `agents.archiveSession` |
| Agents: Associate Previous Workspace... | — | `agents.associateWorkspace` |
| Agents: Delete Selected Session... | — | `agents.deleteSession` |
| Agents: New Session... | `Ctrl+Shift+I` | `agents.newSession` |
| Agents: Open Session... | `Ctrl+Shift+J`, `Ctrl+Alt+A` | `agents.openSession` |
| Agents: Refresh Sessions | — | `agents.refreshSessions` |
| Agents: Rename Selected Session... | — | `agents.renameSession` |
| Agents: Toggle Sidebar | — | `agents.togglePanel` |

### Application

| Command | Keys | ID |
| --- | --- | --- |
| Quit | `Ctrl+Q` | `workbench.quit` |

### Database

| Command | Keys | ID |
| --- | --- | --- |
| Add Row | — | `database.addRow` |
| Browse Schema | — | `database.browseSchema` |
| Complete SQL | `Ctrl+Space` | `database.complete` |
| Connect to Database | `Ctrl+Shift+C` | `database.connect` |
| Delete Connection | — | `database.deleteConnection` |
| Describe Table | — | `database.describeTable` |
| Disconnect | — | `database.disconnect` |
| Edit Connection... | — | `database.editConnection` |
| Execute Query | `Ctrl+R`, `Ctrl+Enter` | `database.executeQuery` |
| Execute Selection | — | `database.executeSelection` |
| Export Results | `e` | `database.exportResults` |
| New Connection... | — | `database.newConnection` |
| New Query | — | `database.newQuery` |
| Query History | — | `database.viewHistory` |
| Query Results: First Row | `Home` | `queryResults.goToFirst` |
| Query Results: Last Row | `End` | `queryResults.goToLast` |
| Query Results: Next Column | `Right`, `l` | `queryResults.moveRight` |
| Query Results: Next Row | `Down`, `j` | `queryResults.moveDown` |
| Query Results: Page Down | `PageDown` | `queryResults.pageDown` |
| Query Results: Page Up | `PageUp` | `queryResults.pageUp` |
| Query Results: Previous Column | `Left`, `h` | `queryResults.moveLeft` |
| Query Results: Previous Row | `Up`, `k` | `queryResults.moveUp` |
| Query Results: Run Query Again | `r`, `F5` | `queryResults.refresh` |
| Query Results: Show Row Details | `Enter` | `queryResults.showDetails` |
| Query Results: Sort by Selected Column | `s` | `queryResults.sort` |
| Query Results: Toggle View Mode | `Tab` | `queryResults.toggleViewMode` |
| Test Connection | — | `database.testConnection` |

### Edit

| Command | Keys | ID |
| --- | --- | --- |
| Add Selection to Next Match | `Ctrl+D` | `edit.selectNextMatch` |
| Copy | `Ctrl+C` | `edit.copy` |
| Cut | `Ctrl+X` | `edit.cut` |
| Delete Line | `Shift+Cmd+K` | `edit.deleteLine` |
| Indent Line | — | `edit.indentLine` |
| Join Lines | — | `edit.joinLines` |
| Move Line Down | `Ctrl+Shift+Down` | `edit.moveLinesDown` |
| Move Line Up | `Ctrl+Shift+Up` | `edit.moveLinesUp` |
| Outdent Line | — | `edit.outdentLine` |
| Paste | `Ctrl+V` | `edit.paste` |
| Redo | `Ctrl+Shift+Z`, `Ctrl+Y` | `edit.redo` |
| Select All | `Ctrl+A` | `edit.selectAll` |
| Select All Occurrences | `Ctrl+Shift+L` | `edit.selectAllOccurrences` |
| Transform to Lowercase | — | `edit.transformToLowercase` |
| Transform to Title Case | — | `edit.transformToTitleCase` |
| Transform to Uppercase | — | `edit.transformToUppercase` |
| Undo | `Ctrl+Z` | `edit.undo` |

### Editor

| Command | Keys | ID |
| --- | --- | --- |
| Add Cursor Above | `Ctrl+Alt+Up` | `editor.addCursorAbove` |
| Add Cursor Below | `Ctrl+Alt+Down` | `editor.addCursorBelow` |
| Add Line Comment | — | `editor.commentLine` |
| Clear Cursors | `Esc` | `editor.clearCursors` |
| Duplicate Line | `Ctrl+Alt+D` | `editor.duplicateLine` |
| Duplicate Selection | `Ctrl+Shift+D` | `editor.duplicateSelection` |
| Fold | `Ctrl+Shift+[` | `editor.fold` |
| Fold All | — | `editor.foldAll` |
| Fold Level 1 | — | `editor.foldLevel1` |
| Fold Level 2 | — | `editor.foldLevel2` |
| Fold Level 3 | — | `editor.foldLevel3` |
| Format Document | — | `editor.formatDocument` |
| Format Selection | — | `editor.formatSelection` |
| Remove Line Comment | — | `editor.uncommentLine` |
| Rename Symbol | — | `lsp.rename` |
| Restart Language Server | — | `lsp.restartServer` |
| Select Line | `Ctrl+L` | `editor.selectLine` |
| Show Code Actions | `Ctrl+.` | `lsp.codeActions` |
| Show Hover | `Ctrl+I` | `lsp.showHover` |
| Sort Lines Ascending | — | `editor.sortLinesAscending` |
| Sort Lines Descending | — | `editor.sortLinesDescending` |
| Toggle Block Comment | `Alt+Shift+A` | `editor.toggleBlockComment` |
| Toggle Line Comment | `Ctrl+/` | `editor.toggleComment` |
| Trigger Completion | `Ctrl+Space` | `lsp.triggerCompletion` |
| Trigger Signature Help | `Ctrl+Shift+Space` | `lsp.triggerSignatureHelp` |
| Trim Trailing Whitespace | — | `editor.trimTrailingWhitespace` |
| Unfold | `Ctrl+Shift+]` | `editor.unfold` |
| Unfold All | — | `editor.unfoldAll` |

### Explorer

| Command | Keys | ID |
| --- | --- | --- |
| Explorer: Collapse Folder | `Left`, `h` | `fileTree.collapse` |
| Explorer: Delete... | `d`, `Delete` | `fileTree.delete` |
| Explorer: Expand Folder | `Right`, `l` | `fileTree.expand` |
| Explorer: New File... | `n` | `fileTree.newFile` |
| Explorer: New Folder... | `Shift+N` | `fileTree.newFolder` |
| Explorer: Open Selected | `Enter`, `Space` | `fileTree.open` |
| Explorer: Page Down | `PageDown` | `fileTree.pageDown` |
| Explorer: Page Up | `PageUp` | `fileTree.pageUp` |
| Explorer: Refresh | — | `fileTree.refresh` |
| Explorer: Rename... | `r`, `F2` | `fileTree.rename` |
| Explorer: Select First | `Home` | `fileTree.goToFirst` |
| Explorer: Select Last | `End` | `fileTree.goToLast` |
| Explorer: Select Next | `Down`, `j` | `fileTree.moveDown` |
| Explorer: Select Previous | `Up`, `k` | `fileTree.moveUp` |

### File

| Command | Keys | ID |
| --- | --- | --- |
| Close All | — | `file.closeAll` |
| Close Other Tabs | — | `file.closeOthers` |
| Close Tab | `Ctrl+W` | `file.close` |
| Copy Path | `Ctrl+Alt+C` | `file.copyPath` |
| Copy Relative Path | `Ctrl+Alt+Shift+C` | `file.copyRelativePath` |
| Delete File... | — | `file.delete` |
| New File | `Ctrl+N` | `file.new` |
| New Folder... | — | `file.newFolder` |
| Open File... | `Ctrl+O` | `file.open` |
| Overwrite File on Disk | — | `file.overwrite` |
| Rename File... | — | `file.rename` |
| Reveal in File Manager | `Alt+Shift+R` | `file.revealInFinder` |
| Revert File | — | `file.reload` |
| Save | `Ctrl+S` | `file.save` |
| Save All | — | `file.saveAll` |
| Save As... | `Ctrl+Shift+S`, `Ctrl+Alt+S` | `file.saveAs` |

### Git

| Command | Keys | ID |
| --- | --- | --- |
| Abort Merge | — | `git.abortMerge` |
| Abort Merge, Rebase, Cherry-Pick or Revert | — | `git.abort` |
| Abort Rebase | — | `git.abortRebase` |
| Apply Stash | — | `git.stashApply` |
| Checkout... | — | `git.checkout` |
| Cherry-Pick Commit... | — | `git.cherryPick` |
| Clean Untracked Files... | — | `git.clean` |
| Commit | `c` | `git.commit` |
| Commit (Amend)... | — | `git.commitAmend` |
| Continue Rebase | — | `git.continueRebase` |
| Continue Rebase, Cherry-Pick or Revert | — | `git.continue` |
| Create Branch... | — | `git.createBranch` |
| Create Tag... | — | `git.createTag` |
| Delete Branch... | — | `git.deleteBranch` |
| Discard All Changes | — | `git.discardAll` |
| Discard Changes in Active File | — | `git.discard` |
| Drop Stash | — | `git.stashDrop` |
| Fetch | — | `git.fetch` |
| Fetch All Remotes | — | `git.fetchAll` |
| Focus Git Panel | `Ctrl+Shift+G`, `Ctrl+Alt+G` | `git.focusPanel` |
| List Branches | — | `git.branches` |
| List Stashes | — | `git.stashList` |
| List Tags | — | `git.tags` |
| Manage Remotes | — | `git.remotes` |
| Merge Branch... | — | `git.merge` |
| Open Conflicted File... | — | `git.conflicts` |
| Pop Stash | — | `git.stashPop` |
| Pull | — | `git.pull` |
| Pull (Rebase) | — | `git.pullRebase` |
| Push | — | `git.push` |
| Push (Force with Lease) | — | `git.pushForce` |
| Rebase Branch... | — | `git.rebase` |
| Refresh | `r` | `git.refresh` |
| Rename Branch... | — | `git.renameBranch` |
| Reset to Commit... | — | `git.reset` |
| Revert Commit... | — | `git.revert` |
| Show Status | — | `git.status` |
| Stage All Changes | `Shift+S` | `git.stageAll` |
| Stage Changes | — | `git.stage` |
| Stash | — | `git.stash` |
| Switch Branch... | — | `git.switchBranch` |
| Toggle Blame | — | `git.blame` |
| Unstage All | — | `git.unstageAll` |
| Unstage Changes | — | `git.unstage` |
| View Diff | — | `git.diff` |
| View File History | — | `git.logFile` |
| View Log | — | `git.log` |
| View Staged Diff | — | `git.diffStaged` |

### Go

| Command | Keys | ID |
| --- | --- | --- |
| Find All References | — | `lsp.findReferences` |
| Go to Declaration | — | `lsp.goToDeclaration` |
| Go to Definition | `Ctrl+Shift+K` | `lsp.goToDefinition` |
| Go to Implementation | `Ctrl+F12` | `lsp.goToImplementation` |
| Go to Line... | `Ctrl+G` | `editor.gotoLine` |
| Go to Next Problem | — | `lsp.nextDiagnostic` |
| Go to Previous Problem | — | `lsp.previousDiagnostic` |
| Go to Symbol in Editor... | `Ctrl+R` | `editor.gotoSymbol` |
| Go to Symbol in Workspace... | `Ctrl+Shift+R` | `editor.gotoWorkspaceSymbol` |
| Go to Type Definition | — | `lsp.goToTypeDefinition` |

### Help

| Command | Keys | ID |
| --- | --- | --- |
| Help: AI CLI Agents | — | `help.agents` |
| Help: About Tide | — | `help.about` |
| Help: All Commands | — | `help.commands` |
| Help: Check for Updates | — | `help.checkForUpdates` |
| Help: Editor and Language Servers | — | `help.editor` |
| Help: Getting Started | — | `help.gettingStarted` |
| Help: Git | — | `help.git` |
| Help: Keyboard Shortcuts | — | `help.keyboardShortcuts` |
| Help: Online Documentation | — | `help.documentation` |
| Help: PostgreSQL | — | `help.database` |
| Help: Report an Issue | — | `help.reportIssue` |
| Help: Settings Reference | — | `help.settings` |
| Help: Terminals | — | `help.terminal` |
| Help: Themes and Customization | — | `help.themes` |
| Help: Troubleshooting | — | `help.troubleshooting` |
| Help: Welcome | — | `help.welcome` |
| Help: Workbench and Panes | — | `help.workbench` |

### Outline

| Command | Keys | ID |
| --- | --- | --- |
| Outline: Collapse Selected | `Left`, `h` | `outlinePanel.collapse` |
| Outline: Expand Selected | `Right`, `l` | `outlinePanel.expand` |
| Outline: Go to Selected Symbol | `Enter` | `outlinePanel.select` |
| Outline: Page Down | `PageDown` | `outlinePanel.pageDown` |
| Outline: Page Up | `PageUp` | `outlinePanel.pageUp` |
| Outline: Select First | `Home` | `outlinePanel.goToFirst` |
| Outline: Select Last | `End` | `outlinePanel.goToLast` |
| Outline: Select Next | `Down`, `j` | `outlinePanel.moveDown` |
| Outline: Select Previous | `Up`, `k` | `outlinePanel.moveUp` |
| Outline: Toggle Selected | `Space` | `outlinePanel.toggleExpand` |

### Preferences

| Command | Keys | ID |
| --- | --- | --- |
| Next Theme | — | `theme.next` |
| Open Keyboard Shortcuts | `Ctrl+Shift+,`, `Ctrl+Alt+K` | `workbench.openKeybindings` |
| Open Settings | `Ctrl+,`, `Ctrl+Alt+O` | `workbench.openSettings` |
| Previous Theme | — | `theme.previous` |
| Select Color Theme | `Ctrl+Alt+R` | `workbench.selectTheme` |
| Toggle Dark Mode | — | `theme.toggleDarkMode` |

### Search

| Command | Keys | ID |
| --- | --- | --- |
| Find | `Ctrl+F` | `search.find` |
| Find Next | `F3` | `search.findNext` |
| Find Previous | `Shift+F3` | `search.findPrevious` |
| Find and Replace | `Ctrl+H` | `search.replace` |
| Find in Files | `Ctrl+Shift+F`, `Ctrl+Alt+F` | `search.findInFiles` |
| Replace in Files | `Ctrl+Shift+H` | `search.replaceInFiles` |

### Secrets

| Command | Keys | ID |
| --- | --- | --- |
| Manage Secrets | — | `secrets.manage` |

### Session

| Command | Keys | ID |
| --- | --- | --- |
| Delete Session... | — | `session.delete` |
| List Sessions for This Workspace... | — | `session.list` |
| Open Session... | — | `session.open` |
| Save Session | — | `session.save` |
| Save Session As... | — | `session.saveAs` |

### Source Control

| Command | Keys | ID |
| --- | --- | --- |
| Source Control: Collapse or Expand Group | `Space` | `gitPanel.toggleSection` |
| Source Control: Discard Selected Changes... | `d` | `gitPanel.discard` |
| Source Control: Open Selected Diff | `Enter` | `gitPanel.openDiff` |
| Source Control: Open Selected File | `o` | `gitPanel.openFile` |
| Source Control: Select Next | `Down`, `j` | `gitPanel.moveDown` |
| Source Control: Select Previous | `Up`, `k` | `gitPanel.moveUp` |
| Source Control: Stage Selected File | `s` | `gitPanel.stage` |
| Source Control: Unstage Selected File | `u` | `gitPanel.unstage` |

### Terminal

| Command | Keys | ID |
| --- | --- | --- |
| Clear Terminal | `Ctrl+Shift+K` | `terminal.clear` |
| Close All Terminals | — | `terminal.closeAll` |
| Close Bottom Terminal Tab | — | `terminal.close` |
| Interrupt Terminal | — | `terminal.interrupt` |
| Kill Terminal | — | `terminal.kill` |
| Next Bottom Terminal Tab | — | `terminal.next` |
| Open Bottom Terminal Tab | `` Ctrl+Shift+` ``, `Ctrl+Alt+T` | `terminal.new` |
| Open Terminal Tab in Main Pane | `Ctrl+Shift+T`, `Ctrl+Alt+M` | `terminal.newInPane` |
| Previous Bottom Terminal Tab | — | `terminal.previous` |
| Scroll Terminal to Bottom | `Ctrl+Shift+End` | `terminal.scrollToBottom` |
| Scroll Terminal to Top | `Ctrl+Shift+Home` | `terminal.scrollToTop` |

### Timeline

| Command | Keys | ID |
| --- | --- | --- |
| Timeline: Copy Selected Commit Hash | `y` | `timelinePanel.copyHash` |
| Timeline: Open File at Selected Commit | `o` | `timelinePanel.openFileAtCommit` |
| Timeline: Page Down | `PageDown` | `timelinePanel.pageDown` |
| Timeline: Page Up | `PageUp` | `timelinePanel.pageUp` |
| Timeline: Select First | `Home`, `g` | `timelinePanel.goToFirst` |
| Timeline: Select Last | `End`, `Shift+G` | `timelinePanel.goToLast` |
| Timeline: Select Next | `Down`, `j` | `timelinePanel.moveDown` |
| Timeline: Select Previous | `Up`, `k` | `timelinePanel.moveUp` |
| Timeline: Toggle File or Repository History | `Tab` | `timelinePanel.toggleMode` |
| Timeline: View Selected Commit's Diff | `Enter` | `timelinePanel.viewDiff` |

### View

| Command | Keys | ID |
| --- | --- | --- |
| Add Tab | `Ctrl+Shift+A` | `pane.addPanel` |
| Agents Sidebar Menu... | — | `workbench.agentsMenu` |
| Close Main Pane | — | `view.closePane` |
| Command Palette | `Ctrl+K`, `Ctrl+Shift+P` | `workbench.commandPalette` |
| Focus Agents Sidebar | — | `workbench.focusAgentsSidebar` |
| Focus Bottom Dock | — | `workbench.focusBottomDock` |
| Focus Bottom Terminal | — | `workbench.focusTerminal` |
| Focus Explorer | `Ctrl+Alt+E` | `workbench.focusExplorer` |
| Focus First Main Pane | `Ctrl+1`, `Alt+1` | `workbench.focusEditor1` |
| Focus Main Region | — | `workbench.focusMainRegion` |
| Focus Next Main Pane | `Ctrl+]`, `F7` | `workbench.focusNextPane` |
| Focus Next Workbench Region | `F6`, `Ctrl+Alt+]` | `workbench.focusNextRegion` |
| Focus Outline | `Ctrl+Alt+U` | `workbench.focusOutline` |
| Focus Pane Above | `Ctrl+Alt+Up`, `Alt+Up` | `workbench.focusPaneUp` |
| Focus Pane Below | `Ctrl+Alt+Down`, `Alt+Down` | `workbench.focusPaneDown` |
| Focus Pane to the Left | `Alt+Left`, `Ctrl+Alt+Left` | `workbench.focusPaneLeft` |
| Focus Pane to the Right | `Alt+Right`, `Ctrl+Alt+Right` | `workbench.focusPaneRight` |
| Focus Previous Main Pane | `Ctrl+[`, `F8` | `workbench.focusPreviousPane` |
| Focus Previous Workbench Region | `Shift+F6`, `Ctrl+Alt+[` | `workbench.focusPreviousRegion` |
| Focus Second Main Pane | `Ctrl+2`, `Alt+2` | `workbench.focusEditor2` |
| Focus Third Main Pane | `Ctrl+3`, `Alt+3` | `workbench.focusEditor3` |
| Focus Timeline | `Ctrl+Alt+L` | `workbench.focusTimeline` |
| Focus Workspace Sidebar | — | `workbench.focusWorkspaceSidebar` |
| Move Active Tab to Main Pane... | — | `editor.moveTabToPane` |
| Move Active Tab to Next Main Pane | — | `editor.moveTabToNextPane` |
| Move Active Tab to Previous Main Pane | — | `editor.moveTabToPreviousPane` |
| Move Focused Region Boundary Down | — | `workbench.resizeDown` |
| Move Focused Region Boundary Left | — | `workbench.resizeLeft` |
| Move Focused Region Boundary Right | — | `workbench.resizeRight` |
| Move Focused Region Boundary Up | — | `workbench.resizeUp` |
| Move Sidebar Left | — | `workbench.moveSidebarLeft` |
| Move Sidebar Right | — | `workbench.moveSidebarRight` |
| Next Tab | `Ctrl+Tab`, `Ctrl+PageDown` | `editor.nextTab` |
| Previous Tab | `Ctrl+Shift+Tab`, `Ctrl+PageUp` | `editor.previousTab` |
| Quick Open | `Ctrl+P` | `workbench.quickOpen` |
| Reset Workbench Region Sizes | — | `workbench.resetRegionSizes` |
| Show Document Information | — | `workbench.documentInfo` |
| Show Problems | — | `lsp.showDiagnostics` |
| Split Main Pane Down | `Ctrl+Shift+\`, `Ctrl+Alt+H` | `view.splitHorizontal` |
| Split Main Pane Right | `Ctrl+\` | `view.splitVertical` |
| Toggle Bottom Terminal | `` Ctrl+` `` | `workbench.toggleTerminal` |
| Toggle Minimap | — | `workbench.toggleMinimap` |
| Toggle Sidebar | `Ctrl+Shift+B`, `Ctrl+B` | `workbench.toggleSidebar` |
| Toggle Word Wrap | — | `workbench.toggleWordWrap` |
| Toggle Zen Mode | — | `workbench.toggleZenMode` |
| View: Move Agents Sidebar Left | — | `workbench.moveAgentsSidebarLeft` |
| View: Move Agents Sidebar Right | — | `workbench.moveAgentsSidebarRight` |
| Workspace Sidebar Menu... | — | `workbench.sidebarMenu` |

### Workbench

| Command | Keys | ID |
| --- | --- | --- |
| Editor: Switch Tab | — | `editor.switchTab` |
| Editor: Switch Tab Across Panes | — | `editor.switchTabAllPanes` |
| Restart Tide | — | `workbench.restart` |
| Restart and Rebuild Tide (source checkout) | — | `workbench.restartAndRebuild` |
| Sidebar: Add Explorer | — | `sidebar.addFileTree` |
| Sidebar: Add Outline | — | `sidebar.addOutline` |
| Sidebar: Add Source Control | — | `sidebar.addGitPanel` |
| Sidebar: Add Timeline | — | `sidebar.addTimeline` |
| Workbench: Status History | — | `workbench.statusHistory` |

## Alternative IDs

These IDs run the same command as another ID. Existing keybindings can keep using them.

| Alias | Runs |
| --- | --- |
| `aiCli.new` | `agents.newSession` |
| `aiCli.openSession` | `agents.openSession` |
| `aiCli.refreshSessions` | `agents.refreshSessions` |
| `aiCli.toggle` | `agents.newSession` |
| `terminal.nextTab` | `terminal.next` |
| `terminal.previousTab` | `terminal.previous` |
| `terminal.focus` | `workbench.focusTerminal` |
| `view.focusTerminal` | `workbench.focusTerminal` |
| `theme.select` | `workbench.selectTheme` |
| `view.focusEditor` | `workbench.focusMainRegion` |
| `view.focusFileTree` | `workbench.focusExplorer` |
| `view.toggleSidebar` | `workbench.toggleSidebar` |
| `view.toggleTerminal` | `workbench.toggleTerminal` |
| `workbench.toggleAgentsSidebar` | `agents.togglePanel` |
| `workbench.saveSession` | `session.save` |
| `quit` | `workbench.quit` |
| `git.openFileDiff` | `git.diff` |
| `git.viewStagedChanges` | `git.diffStaged` |
| `git.viewChanges` | `gitPanel.openDiff` |
| `gitPanel.stageAll` | `git.stageAll` |
| `gitPanel.commit` | `git.commit` |
| `gitPanel.refresh` | `git.refresh` |
| `queryResults.export` | `database.exportResults` |
