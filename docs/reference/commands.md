<!-- Generated from crates/tide-ui/src/help/pages/commands.md by `cargo xtask generate-rust-reference --write`. Edit the template, not this file. -->
# Commands

Every command is in the command palette (`Ctrl+K` or `Ctrl+Shift+P`): type part of its name and press Enter. Keys are listed where a command has one, using your current bindings. The ID is what you use in `~/.config/tide/keybindings.jsonc`.

## Palette commands

### AI

| Command | Keys | ID |
| --- | --- | --- |
| AI CLI: New... | — | `aiCli.new` |
| AI CLI: Open Session... | `Ctrl+Shift+J`, `Ctrl+Alt+A` | `aiCli.openSession` |
| AI CLI: Refresh Sessions | — | `aiCli.refreshSessions` |
| AI CLI: Toggle | `Ctrl+Shift+I` | `aiCli.toggle` |
| Agents: Archive Selected Session... | — | `agents.archiveSession` |
| Agents: Delete Selected Session... | — | `agents.deleteSession` |
| Agents: New Session... | — | `agents.newSession` |
| Agents: Open Session... | `Ctrl+Shift+J`, `Ctrl+Alt+A` | `agents.openSession` |
| Agents: Refresh Sessions | — | `agents.refreshSessions` |
| Agents: Rename Selected Session... | — | `agents.renameSession` |
| Agents: Toggle Sidebar | — | `agents.togglePanel` |

### AI CLI

| Command | Keys | ID |
| --- | --- | --- |
| Agents: Associate Previous Workspace... | — | `agents.associateWorkspace` |

### Application

| Command | Keys | ID |
| --- | --- | --- |
| Quit | `Ctrl+Q` | `workbench.quit` |

### Database

| Command | Keys | ID |
| --- | --- | --- |
| Browse Schema | — | `database.browseSchema` |
| Complete SQL | `Ctrl+Space` | `database.complete` |
| Connect to Database | `Ctrl+Shift+C` | `database.connect` |
| Delete Connection | — | `database.deleteConnection` |
| Describe Table | — | `database.describeTable` |
| Disconnect | — | `database.disconnect` |
| Edit Connection... | — | `database.editConnection` |
| Execute Query | `Ctrl+R`, `Ctrl+Enter` | `database.executeQuery` |
| Execute Selection | — | `database.executeSelection` |
| Export Results | — | `database.exportResults` |
| New Connection... | — | `database.newConnection` |
| New Query | — | `database.newQuery` |
| Query History | — | `database.viewHistory` |
| Test Connection | — | `database.testConnection` |

### Edit

| Command | Keys | ID |
| --- | --- | --- |
| Add Selection to Next Match | `Ctrl+D` | `edit.selectNextMatch` |
| Copy | `Ctrl+C` | `edit.copy` |
| Cut | `Ctrl+X` | `edit.cut` |
| Paste | `Ctrl+V` | `edit.paste` |
| Redo | `Ctrl+Shift+Z`, `Ctrl+Y` | `edit.redo` |
| Select All | `Ctrl+A` | `edit.selectAll` |
| Select All Occurrences | `Ctrl+Shift+L` | `edit.selectAllOccurrences` |
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
| Format Document | — | `editor.formatDocument` |
| Remove Line Comment | — | `editor.uncommentLine` |
| Rename Symbol | — | `lsp.rename` |
| Select Line | `Ctrl+L` | `editor.selectLine` |
| Show Code Actions | `Ctrl+.` | `lsp.codeActions` |
| Show Hover | `Ctrl+I` | `lsp.showHover` |
| Toggle Line Comment | `Ctrl+/` | `editor.toggleComment` |
| Trigger Completion | `Ctrl+Space` | `lsp.triggerCompletion` |
| Trigger Signature Help | `Ctrl+Shift+Space` | `lsp.triggerSignatureHelp` |
| Unfold | `Ctrl+Shift+]` | `editor.unfold` |
| Unfold All | — | `editor.unfoldAll` |

### File

| Command | Keys | ID |
| --- | --- | --- |
| Close All | — | `file.closeAll` |
| Close Tab | `Ctrl+W` | `file.close` |
| New File | `Ctrl+N` | `file.new` |
| Open File... | `Ctrl+O` | `file.open` |
| Overwrite File on Disk | — | `file.overwrite` |
| Revert File | — | `file.reload` |
| Save | `Ctrl+S` | `file.save` |
| Save All | — | `file.saveAll` |
| Save As... | `Ctrl+Shift+S`, `Ctrl+Alt+S` | `file.saveAs` |

### Git

| Command | Keys | ID |
| --- | --- | --- |
| Abort Merge | — | `git.abortMerge` |
| Apply Stash | — | `git.stashApply` |
| Checkout... | — | `git.checkout` |
| Commit | — | `git.commit` |
| Create Branch... | — | `git.createBranch` |
| Delete Branch... | — | `git.deleteBranch` |
| Drop Stash | — | `git.stashDrop` |
| Fetch | — | `git.fetch` |
| Focus Git Panel | `Ctrl+Shift+G`, `Ctrl+Alt+G` | `git.focusPanel` |
| List Branches | — | `git.branches` |
| List Stashes | — | `git.stashList` |
| List Tags | — | `git.tags` |
| Manage Remotes | — | `git.remotes` |
| Merge Branch... | — | `git.merge` |
| Pop Stash | — | `git.stashPop` |
| Pull | — | `git.pull` |
| Push | — | `git.push` |
| Refresh | — | `git.refresh` |
| Rename Branch... | — | `git.renameBranch` |
| Show Status | — | `git.status` |
| Stage All Changes | — | `git.stageAll` |
| Stage Changes | — | `git.stage` |
| Stash | — | `git.stash` |
| Switch Branch... | — | `git.switchBranch` |
| Toggle Blame | — | `git.blame` |
| Unstage All | — | `git.unstageAll` |
| Unstage Changes | — | `git.unstage` |
| View Active File Diff | — | `git.openFileDiff` |
| View Diff | — | `git.diff` |
| View File History | — | `git.logFile` |
| View Log | — | `git.log` |
| View Staged Changes | — | `git.viewStagedChanges` |
| View Staged Diff | — | `git.diffStaged` |

### Go

| Command | Keys | ID |
| --- | --- | --- |
| Find All References | — | `lsp.findReferences` |
| Go to Definition | `Ctrl+Shift+K` | `lsp.goToDefinition` |
| Go to Line... | `Ctrl+G` | `editor.gotoLine` |
| Go to Next Problem | — | `lsp.nextDiagnostic` |
| Go to Previous Problem | — | `lsp.previousDiagnostic` |
| Go to Symbol in Editor... | `Ctrl+R` | `editor.gotoSymbol` |
| Go to Symbol in Workspace... | `Ctrl+Shift+R` | `editor.gotoWorkspaceSymbol` |

### Help

| Command | Keys | ID |
| --- | --- | --- |
| Help: AI CLI Agents | — | `help.agents` |
| Help: About Tide | — | `help.about` |
| Help: All Commands | — | `help.commands` |
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

### Preferences

| Command | Keys | ID |
| --- | --- | --- |
| Color Theme | `Ctrl+Alt+R` | `theme.select` |
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

### Secrets

| Command | Keys | ID |
| --- | --- | --- |
| Manage Secrets | — | `secrets.manage` |

### Session

| Command | Keys | ID |
| --- | --- | --- |
| Open Session... | — | `session.open` |
| Save Session | — | `session.save` |
| Save Session As... | — | `session.saveAs` |

### Terminal

| Command | Keys | ID |
| --- | --- | --- |
| Close Bottom Terminal Tab | — | `terminal.close` |
| Focus Bottom Terminal | — | `terminal.focus` |
| Interrupt Terminal | — | `terminal.interrupt` |
| Next Bottom Terminal Tab | — | `terminal.nextTab` |
| Open Bottom Terminal Tab | `` Ctrl+Shift+` ``, `Ctrl+Alt+T` | `terminal.new` |
| Open Terminal Tab in Main Pane | `Ctrl+Shift+T`, `Ctrl+Alt+M` | `terminal.newInPane` |
| Previous Bottom Terminal Tab | — | `terminal.previousTab` |

### View

| Command | Keys | ID |
| --- | --- | --- |
| Add Tab | `Ctrl+Shift+A` | `pane.addPanel` |
| Close Main Pane | — | `view.closePane` |
| Command Palette | `Ctrl+K`, `Ctrl+Shift+P` | `workbench.commandPalette` |
| Focus Agents Sidebar | — | `workbench.focusAgentsSidebar` |
| Focus Bottom Dock | — | `workbench.focusBottomDock` |
| Focus Bottom Terminal | — | `view.focusTerminal` |
| Focus File Explorer | `Ctrl+Alt+E` | `view.focusFileTree` |
| Focus First Main Pane | `Ctrl+1`, `Alt+1` | `workbench.focusEditor1` |
| Focus Main Pane | — | `view.focusEditor` |
| Focus Main Region | — | `workbench.focusMainRegion` |
| Focus Next Main Pane | `Ctrl+]`, `F7` | `workbench.focusNextPane` |
| Focus Next Workbench Region | `F6`, `Ctrl+Alt+]` | `workbench.focusNextRegion` |
| Focus Previous Main Pane | `Ctrl+[`, `F8` | `workbench.focusPreviousPane` |
| Focus Previous Workbench Region | `Shift+F6`, `Ctrl+Alt+[` | `workbench.focusPreviousRegion` |
| Focus Second Main Pane | `Ctrl+2`, `Alt+2` | `workbench.focusEditor2` |
| Focus Third Main Pane | `Ctrl+3`, `Alt+3` | `workbench.focusEditor3` |
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
| Show Problems | — | `lsp.showDiagnostics` |
| Split Main Pane Down | `Ctrl+Shift+\`, `Ctrl+Alt+H` | `view.splitHorizontal` |
| Split Main Pane Right | `Ctrl+\` | `view.splitVertical` |
| Toggle Bottom Terminal | `` Ctrl+` `` | `workbench.toggleTerminal` |
| Toggle Sidebar | `Ctrl+Shift+B`, `Ctrl+B` | `workbench.toggleSidebar` |
| Toggle Word Wrap | — | `workbench.toggleWordWrap` |
| View: Move Agents Sidebar Left | — | `workbench.moveAgentsSidebarLeft` |
| View: Move Agents Sidebar Right | — | `workbench.moveAgentsSidebarRight` |
| View: Toggle Agents Sidebar | — | `workbench.toggleAgentsSidebar` |

### Workbench

| Command | Keys | ID |
| --- | --- | --- |
| Database: Add Row | — | `database.addRow` |
| Editor: Switch Tab | — | `editor.switchTab` |
| Editor: Switch Tab Across Panes | — | `editor.switchTabAllPanes` |
| Restart Tide | — | `workbench.restart` |
| Restart and Rebuild Tide (source checkout) | — | `workbench.restartAndRebuild` |
| Sidebar: Add Explorer | — | `sidebar.addFileTree` |
| Sidebar: Add Outline | — | `sidebar.addOutline` |
| Sidebar: Add Source Control | — | `sidebar.addGitPanel` |
| Sidebar: Add Timeline | — | `sidebar.addTimeline` |
| Workbench: Status History | — | `workbench.statusHistory` |

## Keybinding actions

These actions have no palette entry; they act on a focused panel and are reached through keys.

| Action | Keys | ID |
| --- | --- | --- |
| Focus outline | `Ctrl+Alt+U` | `workbench.focusOutline` |
| Focus timeline | `Ctrl+Alt+L` | `workbench.focusTimeline` |
| Toggle sidebar | — | `view.toggleSidebar` |
| Toggle terminal | — | `view.toggleTerminal` |
| Focus pane left | `Alt+Left`, `Ctrl+Alt+Left` | `workbench.focusPaneLeft` |
| Focus pane right | `Alt+Right`, `Ctrl+Alt+Right` | `workbench.focusPaneRight` |
| Focus pane up | `Ctrl+Alt+Up`, `Alt+Up` | `workbench.focusPaneUp` |
| Focus pane down | `Ctrl+Alt+Down`, `Alt+Down` | `workbench.focusPaneDown` |
| Document info | — | `workbench.documentInfo` |
| Sidebar menu | — | `workbench.sidebarMenu` |
| Agents menu | — | `workbench.agentsMenu` |
| View changes | — | `git.viewChanges` |
| Quit | — | `quit` |
| Save session | — | `workbench.saveSession` |
| Move down | `Down`, `j` | `fileTree.moveDown` |
| Move up | `Up`, `k` | `fileTree.moveUp` |
| Go to first | `Home` | `fileTree.goToFirst` |
| Go to last | `End` | `fileTree.goToLast` |
| Page down | `PageDown` | `fileTree.pageDown` |
| Page up | `PageUp` | `fileTree.pageUp` |
| Expand | `Right`, `l` | `fileTree.expand` |
| Collapse | `Left`, `h` | `fileTree.collapse` |
| Open | `Enter`, `Space` | `fileTree.open` |
| Refresh | — | `fileTree.refresh` |
| New file | `n` | `fileTree.newFile` |
| New folder | `Shift+N` | `fileTree.newFolder` |
| Rename | `r`, `F2` | `fileTree.rename` |
| Delete | `d`, `Delete` | `fileTree.delete` |
| Move down | `Down`, `j` | `gitPanel.moveDown` |
| Move up | `Up`, `k` | `gitPanel.moveUp` |
| Stage | `s` | `gitPanel.stage` |
| Stage all | `Shift+S` | `gitPanel.stageAll` |
| Unstage | `u` | `gitPanel.unstage` |
| Commit | `c` | `gitPanel.commit` |
| Refresh | `r` | `gitPanel.refresh` |
| Open diff | `Enter` | `gitPanel.openDiff` |
| Open file | `o` | `gitPanel.openFile` |
| Toggle section | `Space` | `gitPanel.toggleSection` |
| Move down | `Down`, `j` | `queryResults.moveDown` |
| Move up | `Up`, `k` | `queryResults.moveUp` |
| Page down | `PageDown` | `queryResults.pageDown` |
| Page up | `PageUp` | `queryResults.pageUp` |
| Go to first | `Home` | `queryResults.goToFirst` |
| Go to last | `End` | `queryResults.goToLast` |
| Export | `e` | `queryResults.export` |
| Refresh | `r`, `F5` | `queryResults.refresh` |
| Show details | `Enter` | `queryResults.showDetails` |
| Move left | `Left`, `h` | `queryResults.moveLeft` |
| Move right | `Right`, `l` | `queryResults.moveRight` |
| Sort | `s` | `queryResults.sort` |
| Toggle view mode | `Tab` | `queryResults.toggleViewMode` |
| Move down | `Down`, `j` | `outlinePanel.moveDown` |
| Move up | `Up`, `k` | `outlinePanel.moveUp` |
| Page down | `PageDown` | `outlinePanel.pageDown` |
| Page up | `PageUp` | `outlinePanel.pageUp` |
| Go to first | `Home` | `outlinePanel.goToFirst` |
| Go to last | `End` | `outlinePanel.goToLast` |
| Select | `Enter` | `outlinePanel.select` |
| Expand | `Right`, `l` | `outlinePanel.expand` |
| Collapse | `Left`, `h` | `outlinePanel.collapse` |
| Toggle expand | `Space` | `outlinePanel.toggleExpand` |
| Move down | `Down`, `j` | `timelinePanel.moveDown` |
| Move up | `Up`, `k` | `timelinePanel.moveUp` |
| Page down | `PageDown` | `timelinePanel.pageDown` |
| Page up | `PageUp` | `timelinePanel.pageUp` |
| Go to first | `Home`, `g` | `timelinePanel.goToFirst` |
| Go to last | `End`, `Shift+G` | `timelinePanel.goToLast` |
| View diff | `Enter` | `timelinePanel.viewDiff` |
| Open file at commit | `o` | `timelinePanel.openFileAtCommit` |
| Copy hash | `y` | `timelinePanel.copyHash` |
| Toggle mode | `Tab` | `timelinePanel.toggleMode` |
| Discard | `d` | `gitPanel.discard` |

## Alternative IDs

These IDs run the same command as another ID. Existing keybindings can keep using them.

| Alias | Runs |
| --- | --- |
| `aiCli.new` | `agents.newSession` |
| `aiCli.openSession` | `agents.openSession` |
| `aiCli.refreshSessions` | `agents.refreshSessions` |
| `terminal.nextTab` | `terminal.next` |
| `terminal.previousTab` | `terminal.previous` |
| `terminal.focus` | `workbench.focusTerminal` |
| `view.focusTerminal` | `workbench.focusTerminal` |
| `theme.select` | `workbench.selectTheme` |
| `view.focusEditor` | `workbench.focusMainRegion` |
| `view.focusFileTree` | `workbench.focusExplorer` |
| `workbench.toggleAgentsSidebar` | `agents.togglePanel` |
