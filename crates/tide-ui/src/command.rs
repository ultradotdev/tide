//! Workbench commands. User-visible IDs are parsed only where they enter the app.
use std::collections::HashMap;
use std::fmt;
use std::str::FromStr;
use std::sync::LazyLock;

macro_rules! commands {
    ($($variant:ident = $id:literal,)*) => {
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
        pub enum Command {
            $($variant,)*
        }

        impl Command {
            pub const ALL: &[Self] = &[$(Self::$variant,)*];

            #[must_use]
            pub const fn as_str(self) -> &'static str {
                match self {
                    $(Self::$variant => $id,)*
                }
            }
        }
    };
}

commands! {
    FileSave = "file.save",
    FileSaveAs = "file.saveAs",
    FileSaveAll = "file.saveAll",
    FileReload = "file.reload",
    FileOverwrite = "file.overwrite",
    FileOpen = "file.open",
    FileNew = "file.new",
    FileClose = "file.close",
    FileCloseAll = "file.closeAll",
    EditUndo = "edit.undo",
    EditRedo = "edit.redo",
    EditCopy = "edit.copy",
    EditCut = "edit.cut",
    EditPaste = "edit.paste",
    EditSelectAll = "edit.selectAll",
    WorkbenchFocusWorkspaceSidebar = "workbench.focusWorkspaceSidebar",
    WorkbenchFocusExplorer = "workbench.focusExplorer",
    WorkbenchFocusOutline = "workbench.focusOutline",
    WorkbenchFocusTimeline = "workbench.focusTimeline",
    WorkbenchFocusTerminal = "workbench.focusTerminal",
    WorkbenchFocusAgentsSidebar = "workbench.focusAgentsSidebar",
    TerminalInterrupt = "terminal.interrupt",
    WorkbenchRestart = "workbench.restart",
    WorkbenchRestartAndRebuild = "workbench.restartAndRebuild",
    ThemeNext = "theme.next",
    ThemePrevious = "theme.previous",
    ThemeToggleDarkMode = "theme.toggleDarkMode",
    WorkbenchFocusMainRegion = "workbench.focusMainRegion",
    WorkbenchFocusBottomDock = "workbench.focusBottomDock",
    WorkbenchToggleWordWrap = "workbench.toggleWordWrap",
    WorkbenchResetRegionSizes = "workbench.resetRegionSizes",
    WorkbenchCommandPalette = "workbench.commandPalette",
    WorkbenchQuickOpen = "workbench.quickOpen",
    WorkbenchToggleSidebar = "workbench.toggleSidebar",
    ViewToggleSidebar = "view.toggleSidebar",
    WorkbenchToggleTerminal = "workbench.toggleTerminal",
    ViewToggleTerminal = "view.toggleTerminal",
    WorkbenchFocusNextRegion = "workbench.focusNextRegion",
    WorkbenchFocusPreviousRegion = "workbench.focusPreviousRegion",
    WorkbenchFocusNextPane = "workbench.focusNextPane",
    WorkbenchFocusPreviousPane = "workbench.focusPreviousPane",
    WorkbenchFocusPaneLeft = "workbench.focusPaneLeft",
    WorkbenchFocusPaneRight = "workbench.focusPaneRight",
    WorkbenchFocusPaneUp = "workbench.focusPaneUp",
    WorkbenchFocusPaneDown = "workbench.focusPaneDown",
    WorkbenchFocusEditor1 = "workbench.focusEditor1",
    WorkbenchFocusEditor2 = "workbench.focusEditor2",
    WorkbenchFocusEditor3 = "workbench.focusEditor3",
    ViewSplitVertical = "view.splitVertical",
    ViewSplitHorizontal = "view.splitHorizontal",
    ViewClosePane = "view.closePane",
    EditorSwitchTab = "editor.switchTab",
    EditorSwitchTabAllPanes = "editor.switchTabAllPanes",
    EditorMoveTabToPane = "editor.moveTabToPane",
    EditorMoveTabToNextPane = "editor.moveTabToNextPane",
    EditorMoveTabToPreviousPane = "editor.moveTabToPreviousPane",
    WorkbenchMoveSidebarLeft = "workbench.moveSidebarLeft",
    WorkbenchMoveSidebarRight = "workbench.moveSidebarRight",
    WorkbenchMoveAgentsSidebarLeft = "workbench.moveAgentsSidebarLeft",
    WorkbenchMoveAgentsSidebarRight = "workbench.moveAgentsSidebarRight",
    WorkbenchResizeLeft = "workbench.resizeLeft",
    WorkbenchResizeRight = "workbench.resizeRight",
    WorkbenchResizeUp = "workbench.resizeUp",
    WorkbenchResizeDown = "workbench.resizeDown",
    WorkbenchStatusHistory = "workbench.statusHistory",
    WorkbenchDocumentInfo = "workbench.documentInfo",
    WorkbenchSidebarMenu = "workbench.sidebarMenu",
    WorkbenchAgentsMenu = "workbench.agentsMenu",
    SidebarAddFileTree = "sidebar.addFileTree",
    SidebarAddGitPanel = "sidebar.addGitPanel",
    SidebarAddOutline = "sidebar.addOutline",
    SidebarAddTimeline = "sidebar.addTimeline",
    PaneAddPanel = "pane.addPanel",
    EditorNextTab = "editor.nextTab",
    EditorPreviousTab = "editor.previousTab",
    EditorGotoLine = "editor.gotoLine",
    LspTriggerCompletion = "lsp.triggerCompletion",
    LspShowHover = "lsp.showHover",
    LspGoToDefinition = "lsp.goToDefinition",
    LspTriggerSignatureHelp = "lsp.triggerSignatureHelp",
    LspFindReferences = "lsp.findReferences",
    LspRename = "lsp.rename",
    LspCodeActions = "lsp.codeActions",
    EditorFormatDocument = "editor.formatDocument",
    LspNextDiagnostic = "lsp.nextDiagnostic",
    LspPreviousDiagnostic = "lsp.previousDiagnostic",
    EditorGotoSymbol = "editor.gotoSymbol",
    EditorGotoWorkspaceSymbol = "editor.gotoWorkspaceSymbol",
    LspShowDiagnostics = "lsp.showDiagnostics",
    SearchFind = "search.find",
    SearchFindNext = "search.findNext",
    SearchReplace = "search.replace",
    SearchFindInFiles = "search.findInFiles",
    WorkbenchSelectTheme = "workbench.selectTheme",
    WorkbenchOpenSettings = "workbench.openSettings",
    WorkbenchOpenKeybindings = "workbench.openKeybindings",
    EditorCommentLine = "editor.commentLine",
    EditorUncommentLine = "editor.uncommentLine",
    EditorDuplicateLine = "editor.duplicateLine",
    EditorDuplicateSelection = "editor.duplicateSelection",
    GitFocusPanel = "git.focusPanel",
    GitDiff = "git.diff",
    GitOpenFileDiff = "git.openFileDiff",
    GitViewStagedChanges = "git.viewStagedChanges",
    GitStashApply = "git.stashApply",
    GitStashPop = "git.stashPop",
    GitStashDrop = "git.stashDrop",
    GitDeleteBranch = "git.deleteBranch",
    GitRenameBranch = "git.renameBranch",
    GitMerge = "git.merge",
    GitAbortMerge = "git.abortMerge",
    GitRefresh = "git.refresh",
    GitStage = "git.stage",
    GitStageAll = "git.stageAll",
    GitUnstage = "git.unstage",
    GitCommit = "git.commit",
    GitViewChanges = "git.viewChanges",
    GitDiffStaged = "git.diffStaged",
    GitLog = "git.log",
    GitLogFile = "git.logFile",
    GitBlame = "git.blame",
    GitBranches = "git.branches",
    GitCheckout = "git.checkout",
    GitSwitchBranch = "git.switchBranch",
    GitCreateBranch = "git.createBranch",
    GitStash = "git.stash",
    GitStashList = "git.stashList",
    GitTags = "git.tags",
    GitRemotes = "git.remotes",
    GitUnstageAll = "git.unstageAll",
    GitFetch = "git.fetch",
    GitPull = "git.pull",
    GitPush = "git.push",
    GitStatus = "git.status",
    TerminalNew = "terminal.new",
    TerminalNewInPane = "terminal.newInPane",
    TerminalClose = "terminal.close",
    TerminalNext = "terminal.next",
    TerminalPrevious = "terminal.previous",
    WorkbenchQuit = "workbench.quit",
    Quit = "quit",
    SessionSave = "session.save",
    WorkbenchSaveSession = "workbench.saveSession",
    AgentsOpenSession = "agents.openSession",
    AgentsAssociateWorkspace = "agents.associateWorkspace",
    AgentsNewSession = "agents.newSession",
    AgentsRenameSession = "agents.renameSession",
    AgentsDeleteSession = "agents.deleteSession",
    AgentsArchiveSession = "agents.archiveSession",
    AgentsRefreshSessions = "agents.refreshSessions",
    AgentsTogglePanel = "agents.togglePanel",
    SecretsManage = "secrets.manage",
    DatabaseNewQuery = "database.newQuery",
    DatabaseExecuteQuery = "database.executeQuery",
    DatabaseExecuteSelection = "database.executeSelection",
    DatabaseConnect = "database.connect",
    DatabaseNewConnection = "database.newConnection",
    DatabaseEditConnection = "database.editConnection",
    DatabaseDeleteConnection = "database.deleteConnection",
    DatabaseDisconnect = "database.disconnect",
    DatabaseTestConnection = "database.testConnection",
    DatabaseViewHistory = "database.viewHistory",
    DatabaseBrowseSchema = "database.browseSchema",
    DatabaseAddRow = "database.addRow",
    DatabaseDescribeTable = "database.describeTable",
    DatabaseExportResults = "database.exportResults",
    DatabaseComplete = "database.complete",
    FileTreeMoveDown = "fileTree.moveDown",
    FileTreeMoveUp = "fileTree.moveUp",
    FileTreeGoToFirst = "fileTree.goToFirst",
    FileTreeGoToLast = "fileTree.goToLast",
    FileTreePageDown = "fileTree.pageDown",
    FileTreePageUp = "fileTree.pageUp",
    FileTreeExpand = "fileTree.expand",
    FileTreeCollapse = "fileTree.collapse",
    FileTreeOpen = "fileTree.open",
    FileTreeRefresh = "fileTree.refresh",
    FileTreeNewFile = "fileTree.newFile",
    FileTreeNewFolder = "fileTree.newFolder",
    FileTreeRename = "fileTree.rename",
    FileTreeDelete = "fileTree.delete",
    GitPanelMoveDown = "gitPanel.moveDown",
    GitPanelMoveUp = "gitPanel.moveUp",
    GitPanelStage = "gitPanel.stage",
    GitPanelStageAll = "gitPanel.stageAll",
    GitPanelUnstage = "gitPanel.unstage",
    GitPanelCommit = "gitPanel.commit",
    GitPanelRefresh = "gitPanel.refresh",
    GitPanelOpenDiff = "gitPanel.openDiff",
    GitPanelOpenFile = "gitPanel.openFile",
    GitPanelToggleSection = "gitPanel.toggleSection",
    QueryResultsMoveDown = "queryResults.moveDown",
    QueryResultsMoveUp = "queryResults.moveUp",
    QueryResultsPageDown = "queryResults.pageDown",
    QueryResultsPageUp = "queryResults.pageUp",
    QueryResultsGoToFirst = "queryResults.goToFirst",
    QueryResultsGoToLast = "queryResults.goToLast",
    QueryResultsExport = "queryResults.export",
    QueryResultsRefresh = "queryResults.refresh",
    QueryResultsShowDetails = "queryResults.showDetails",
    QueryResultsMoveLeft = "queryResults.moveLeft",
    QueryResultsMoveRight = "queryResults.moveRight",
    QueryResultsSort = "queryResults.sort",
    QueryResultsToggleViewMode = "queryResults.toggleViewMode",
    OutlinePanelMoveDown = "outlinePanel.moveDown",
    OutlinePanelMoveUp = "outlinePanel.moveUp",
    OutlinePanelPageDown = "outlinePanel.pageDown",
    OutlinePanelPageUp = "outlinePanel.pageUp",
    OutlinePanelGoToFirst = "outlinePanel.goToFirst",
    OutlinePanelGoToLast = "outlinePanel.goToLast",
    OutlinePanelSelect = "outlinePanel.select",
    OutlinePanelExpand = "outlinePanel.expand",
    OutlinePanelCollapse = "outlinePanel.collapse",
    OutlinePanelToggleExpand = "outlinePanel.toggleExpand",
    TimelinePanelMoveDown = "timelinePanel.moveDown",
    TimelinePanelMoveUp = "timelinePanel.moveUp",
    TimelinePanelPageDown = "timelinePanel.pageDown",
    TimelinePanelPageUp = "timelinePanel.pageUp",
    TimelinePanelGoToFirst = "timelinePanel.goToFirst",
    TimelinePanelGoToLast = "timelinePanel.goToLast",
    TimelinePanelViewDiff = "timelinePanel.viewDiff",
    TimelinePanelOpenFileAtCommit = "timelinePanel.openFileAtCommit",
    TimelinePanelCopyHash = "timelinePanel.copyHash",
    TimelinePanelToggleMode = "timelinePanel.toggleMode",
    GitPanelDiscard = "gitPanel.discard",
    AiCliToggle = "aiCli.toggle",
    EditorToggleComment = "editor.toggleComment",
    EditorFold = "editor.fold",
    EditorFoldAll = "editor.foldAll",
    EditorUnfold = "editor.unfold",
    EditorUnfoldAll = "editor.unfoldAll",
    EditorAddCursorAbove = "editor.addCursorAbove",
    EditorAddCursorBelow = "editor.addCursorBelow",
    EditorClearCursors = "editor.clearCursors",
    EditSelectAllOccurrences = "edit.selectAllOccurrences",
    EditSelectNextMatch = "edit.selectNextMatch",
    EditorSelectLine = "editor.selectLine",
    SearchFindPrevious = "search.findPrevious",
    SessionSaveAs = "session.saveAs",
    SessionOpen = "session.open",
    HelpWelcome = "help.welcome",
    HelpGettingStarted = "help.gettingStarted",
    HelpWorkbench = "help.workbench",
    HelpEditor = "help.editor",
    HelpGit = "help.git",
    HelpTerminal = "help.terminal",
    HelpDatabase = "help.database",
    HelpAgents = "help.agents",
    HelpThemes = "help.themes",
    HelpKeyboardShortcuts = "help.keyboardShortcuts",
    HelpCommands = "help.commands",
    HelpSettings = "help.settings",
    HelpTroubleshooting = "help.troubleshooting",
    HelpAbout = "help.about",
    HelpDocumentation = "help.documentation",
    HelpReportIssue = "help.reportIssue",
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("unknown command {0}")]
pub struct UnknownCommand(pub String);

impl FromStr for Command {
    type Err = UnknownCommand;

    /// Accepts canonical IDs and compatibility aliases.
    fn from_str(id: &str) -> Result<Self, Self::Err> {
        static IDS: LazyLock<HashMap<&'static str, Command>> = LazyLock::new(|| {
            Command::ALL
                .iter()
                .map(|command| (command.as_str(), *command))
                .collect()
        });
        IDS.get(crate::command_aliases::canonical(id))
            .copied()
            .ok_or_else(|| UnknownCommand(id.to_owned()))
    }
}

impl fmt::Display for Command {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn every_command_round_trips_through_its_id() {
        let mut ids = HashSet::new();
        for command in Command::ALL {
            assert!(ids.insert(command.as_str()), "duplicate id {command}");
            assert_eq!(command.as_str().parse::<Command>(), Ok(*command));
        }
    }

    #[test]
    fn aliases_parse_to_their_canonical_command() {
        for (alias, canonical) in crate::command_aliases::ALIASES {
            let command = alias.parse::<Command>().unwrap();
            assert_eq!(command.as_str(), *canonical);
        }
    }

    #[test]
    fn every_alias_is_a_catalog_command() {
        let catalog = crate::catalog::commands();
        for (alias, _) in crate::command_aliases::ALIASES {
            assert!(
                catalog.contains_key(*alias),
                "{alias} is not in the catalog"
            );
        }
    }

    #[test]
    fn unknown_ids_are_rejected() {
        for id in ["", "__unbound__", "workbench.toggleMinimap"] {
            assert_eq!(id.parse::<Command>(), Err(UnknownCommand(id.to_owned())));
        }
    }
}
