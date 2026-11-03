use std::cell::RefCell;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::Result;
use crossterm::event::{
    Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
};
use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};

use tide_ai::cli::{self as ai_cli, CliProvider, CliSession};
use tide_ai::secrets::SecretService;
use tide_config::{
    CommandInfo, Keybinding, Paths, SessionAICLIPaneState, SessionBottomTerminalState,
    SessionDocumentState, SessionLayoutNode, SessionSQLEditorState, SessionState,
    SessionTerminalState, SessionUIState, Settings, ThemeCatalog,
};
use tide_core::Workspace;
use tide_core::num::{saturating_u16, saturating_u32, saturating_usize};
use tide_core::{Document, Position, display_width, truncate_to_width};
use tide_lsp::{language_id_for_path, path_to_uri};
use tide_services::SyntaxHighlighter;

use crate::accordion::{AccordionAction, AccordionState, SectionId};
use crate::command::Command;
pub use crate::controllers::ai::{AgentDiscovery, discover_agent_sessions};
use crate::controllers::ai::{AgentRow, AiController, AiEvent, canonical_session_path};
use crate::controllers::db::{DbController, DbEvent};
use crate::controllers::git::{GitController, GitEvent};
use crate::controllers::lsp::{LspController, LspEvent, LspRequestKind};
use crate::controllers::{Effect, Ui};
use crate::editor::{CaseTransform, CommentMode, DocumentRegistry, EditorState, EditorView};
use crate::file_tree::FileTreeState;
use crate::jobs::{Jobs, Tracked};
use crate::modal::{
    self, ConfirmAction, Modal, ModalItem, ModalKind, ModalOutcome, PromptAction, Submitted,
    command_items, file_items,
};
use crate::palette::Palette;
use crate::session_store::SessionStore;
use crate::sql_tab::SqlTab;
use crate::terminal_tab::{SpawnSpec, TerminalTab};
use crate::workbench::{
    DiffTab, FocusDirection, LayoutNode, Pane, PaneId, SplitDirection, Tab, Workbench,
};

/// Runs `$body` with a [`Ui`] borrowed from the app's workbench fields, then applies the
/// effects it collected. Other app fields stay borrowable inside `$body`.
macro_rules! with_ui {
    ($app:ident, |$ui:ident| $body:expr) => {{
        let mut ui = Ui {
            workspace: &$app.workspace,
            settings: &$app.settings,
            workbench: &mut $app.workbench,
            documents: &mut $app.documents,
            overlay: &mut $app.overlay,
            focus: &mut $app.focus,
            session_dirty: &mut $app.session_dirty,
            effects: Vec::new(),
        };
        let $ui = &mut ui;
        let result = $body;
        let effects = std::mem::take(&mut ui.effects);
        $app.apply(effects);
        result
    }};
}

mod commands;
mod editing;
mod files;
mod help;
mod input;
mod layout;
mod mouse;
mod overlays;
mod panes;
mod preferences;
mod render;
mod replace;
mod session;
mod terminals;

pub(crate) use self::files::resolve_path;
use self::files::{collect_files, write_file_atomic};
pub(crate) use self::input::{TERMINAL_CONTROL_KEYS, canonical_key};
use self::layout::{AgentsLayout, Divider, DockLayout, MainItem, Painted, PaneLayout};
pub(crate) use self::layout::{LayoutState, PointerState};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Focus {
    Sidebar,
    Main,
    Agents,
    Dock,
    Status,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PaneControl {
    Add,
    SplitVertical,
    SplitHorizontal,
    Close,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResizeTarget {
    PaneBoundary(PaneId, PaneId, SplitDirection, u16),
    Sidebar,
    Agents,
    Dock,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScrollOwner {
    Section(SectionId),
    Agents,
    SqlResults(PaneId),
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HitTarget {
    Sidebar,
    ScrollBar(ScrollOwner, Rect),
    Command(Command),
    PaneControl(PaneId, PaneControl),
    Resize(ResizeTarget),
    Agents,
    AccordionHeader(usize),
    AccordionContent(usize),
    Pane(PaneId),
    Tab(PaneId, usize),
    TabClose(PaneId, usize),
    TabBar(PaneId),
    TabScroll(PaneId, bool),
    TabOverflow(PaneId),
    AgentProvider(CliProvider),
    AgentSession(usize),
    AgentSearch,
    AgentStatus(CliProvider),
    EditorContent(PaneId, Rect),
    SqlContent(PaneId, Rect),
    DiffContent(PaneId, Rect),
    Dock,
    DockTab(usize),
    DockTerminal(Rect),
    Status,
}

const DOCK_PANE: PaneId = usize::MAX;

/// A resolved key binding. `Unbound` consumes the key without running anything.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BindingAction {
    Run(Command),
    Unbound,
}

#[derive(Debug)]
pub struct DockState {
    pub visible: bool,
    pub height: u16,
    pub tabs: Vec<TerminalTab>,
    pub active: usize,
    next_id: usize,
    strip: crate::tab_strip::TabStripState,
}

impl DockState {
    #[must_use]
    pub fn new() -> Self {
        Self {
            visible: false,
            height: 10,
            tabs: Vec::new(),
            active: 0,
            next_id: 1,
            strip: crate::tab_strip::TabStripState::default(),
        }
    }

    pub fn open(&mut self, workspace: &Path, shell: Option<&str>, scrollback: usize) -> Result<()> {
        let id = self.next_id;
        self.next_id += 1;
        let mut terminal = TerminalTab::new(id, workspace, 80, 10, scrollback, shell)?;
        terminal.title = format!("Terminal {id}");
        self.tabs.push(terminal);
        self.active = self.tabs.len() - 1;
        self.visible = true;
        Ok(())
    }

    pub fn active_mut(&mut self) -> Option<&mut TerminalTab> {
        self.tabs.get_mut(self.active)
    }

    #[must_use]
    pub fn active(&self) -> Option<&TerminalTab> {
        self.tabs.get(self.active)
    }

    pub fn next(&mut self) {
        if !self.tabs.is_empty() {
            self.active = (self.active + 1) % self.tabs.len();
        }
    }

    pub fn previous(&mut self) {
        if !self.tabs.is_empty() {
            self.active = (self.active + self.tabs.len() - 1) % self.tabs.len();
        }
    }

    pub fn close_active(&mut self) {
        self.close(self.active);
    }

    pub fn close(&mut self, index: usize) {
        if index >= self.tabs.len() {
            return;
        }
        self.tabs.remove(index);
        if index < self.active {
            self.active = self.active.saturating_sub(1);
        }
        if self.active >= self.tabs.len() {
            self.active = self.tabs.len().saturating_sub(1);
        }
        if self.tabs.is_empty() {
            self.visible = false;
        }
    }
}

impl Default for DockState {
    fn default() -> Self {
        Self::new()
    }
}

pub struct AppConfig {
    pub workspace: PathBuf,
    pub initial_file: Option<PathBuf>,
    pub session_name: Option<String>,
    pub restore_session: bool,
    /// `--no-session`: leave the saved session alone until the user saves one.
    pub ephemeral_session: bool,
    pub paths: Option<Paths>,
    pub jobs: Jobs,
    pub agent_discovery: AgentDiscovery,
    /// How this executable was installed, for the help pages' update instructions.
    pub install: crate::help::Install,
}

#[derive(Debug)]
pub enum AppEvent {
    ClipboardWritten {
        result: Result<(), String>,
        done: Option<String>,
        fallback: Option<&'static str>,
    },
    ClipboardRead {
        generation: u64,
        target: Option<ModalKind>,
        result: Result<String, String>,
    },
    /// A background recovery snapshot finished.
    SessionAutosaved(Result<(), String>),
    /// Titles of open editors whose files changed on disk.
    /// Open files whose contents on disk no longer match what Tide last read or wrote.
    ExternalChanges(Vec<PathBuf>),
    QuickOpen {
        generation: u64,
        files: Vec<PathBuf>,
    },
    SearchResults {
        generation: u64,
        query: String,
        items: Vec<ModalItem>,
    },
    /// The latest release tag, or why it could not be read, and whether the daily check
    /// asked rather than Help: Check for Updates.
    LatestRelease(Result<String, String>, bool),
    /// Named sessions saved for this workspace, for listing or deleting.
    SavedSessions {
        generation: u64,
        delete: bool,
        sessions: Vec<crate::session_store::SavedSession>,
    },
    /// The preview of a Replace in Files.
    ReplacePlanned {
        generation: u64,
        result: Result<crate::workspace_replace::Plan, String>,
    },
    /// Replace in Files finished; `buffers` open files changed in their editors.
    ReplaceWritten {
        written: crate::workspace_replace::Written,
        buffers: usize,
    },
    Git(GitEvent),
    Lsp(LspEvent),
    Db(DbEvent),
    Ai(AiEvent),
    FilesChanged,
    Error(String),
}

#[derive(Clone)]
enum TabIdentity {
    Document(crate::editor::DocumentHandle),
    Terminal(usize, String, Option<String>, Option<String>),
    Diff(u64),
    Other(String),
}
impl TabIdentity {
    fn of(tab: &Tab) -> Self {
        match tab {
            Tab::Editor(editor) => Self::Document(editor.document.clone()),
            Tab::Sql(sql) => Self::Document(sql.editor.document.clone()),
            Tab::Terminal(terminal) => Self::Terminal(
                terminal.id,
                terminal.cwd.display().to_string(),
                terminal.ai_provider.clone(),
                terminal.ai_session_id.clone(),
            ),
            Tab::Diff(tab) => Self::Diff(tab.id),
            _ => Self::Other(tab.title()),
        }
    }
    fn matches(&self, tab: &Tab) -> bool {
        match (self, tab) {
            (Self::Document(document), Tab::Editor(editor)) => document.same_view(&editor.document),
            (Self::Document(document), Tab::Sql(sql)) => document.same_view(&sql.editor.document),
            (Self::Terminal(id, cwd, provider, session), Tab::Terminal(terminal)) => {
                *id == terminal.id
                    && *cwd == terminal.cwd.display().to_string()
                    && *provider == terminal.ai_provider
                    && *session == terminal.ai_session_id
            }
            (Self::Diff(id), Tab::Diff(tab)) => *id == tab.id,
            (Self::Other(title), tab) => *title == tab.title(),
            _ => false,
        }
    }
}

struct SaveBeforeAction {
    action: ConfirmAction,
    title: String,
    message: String,
    documents: Vec<crate::editor::DocumentHandle>,
    waiting_hunk: Option<(crate::editor::DocumentHandle, u64)>,
    origin: Option<(PaneId, TabIdentity)>,
    focus: Focus,
}

/// The regions zen mode hid, restored when it turns off.
#[derive(Debug, Clone, Copy)]
struct ZenLayout {
    sidebar: bool,
    dock: bool,
    agents: bool,
    focus: Focus,
}

struct CachedOutline {
    document: crate::editor::DocumentHandle,
    revision: u64,
    items: Vec<(usize, String)>,
}

pub struct App {
    pub paths: Paths,
    pub workspace: PathBuf,
    pub settings: Settings,
    pub keybindings: Vec<Keybinding>,
    /// The entries from the user's keybindings file, which help pages list first.
    user_keybindings: Vec<Keybinding>,
    install: crate::help::Install,
    /// Opens a URL in the desktop browser; tests substitute a recorder.
    url_opener: fn(&str) -> Result<(), String>,
    /// Reads the latest release tag on demand; tests substitute one without network access.
    latest_release: crate::help::LatestRelease,
    /// Shows a path in the desktop file manager; tests substitute a recorder.
    path_revealer: fn(&Path) -> Result<(), String>,
    pub themes: ThemeCatalog,
    pub commands: BTreeMap<String, CommandInfo>,
    pub palette: Palette,
    pub workbench: Workbench,
    documents: DocumentRegistry,
    pub file_tree: FileTreeState,
    pub dock: DockState,
    pub overlay: Option<Box<dyn Modal>>,
    pub focus: Focus,
    pub accordion: AccordionState,
    pub outline_selected: usize,
    outline_collapsed: std::collections::HashSet<usize>,
    outline_scroll: usize,
    outline_document: Option<crate::editor::DocumentHandle>,
    outline_cache: RefCell<Option<CachedOutline>>,
    pub sidebar_visible: bool,
    pub sidebar_width: u16,
    sidebar_left: bool,
    agents_left: bool,
    window_focused: bool,
    status_history: Vec<String>,
    status_action: usize,
    pending_move: Option<(PaneId, TabIdentity)>,
    pending_close: Option<(PaneId, TabIdentity)>,
    /// The tab Close Others keeps while its confirmation is open.
    pending_keep: Option<(PaneId, TabIdentity)>,
    save_before: Option<SaveBeforeAction>,
    save_as_form: Option<Box<dyn Modal>>,
    git: GitController,
    jobs: Jobs,
    pub status: String,
    pub should_quit: bool,
    pub(crate) restart: Option<bool>,
    /// Where the last frame drew everything; mouse input hit-tests against it.
    pub(crate) layout: LayoutState,
    pointer: PointerState,
    pub session_name: Option<String>,
    session_store: SessionStore,
    session_write_blocked: bool,
    /// No snapshot is written until the user saves or opens a session.
    session_ephemeral: bool,
    sessions_generation: u64,
    events_tx: tokio::sync::mpsc::Sender<AppEvent>,
    events_rx: Option<tokio::sync::mpsc::Receiver<AppEvent>>,
    watcher: Option<notify::RecommendedWatcher>,
    watch_filter: Arc<crate::watch::WatchFilter>,
    /// Directories created since the last refresh that still need a watch.
    watch_created: Arc<std::sync::Mutex<Vec<PathBuf>>>,
    lsp: LspController,
    ai: AiController,
    db: DbController,
    quick_open_generation: u64,
    search_generation: u64,
    search_cancel: Arc<std::sync::atomic::AtomicBool>,
    replace_generation: u64,
    replace_cancel: Arc<std::sync::atomic::AtomicBool>,
    /// The previewed Replace in Files awaiting confirmation, by generation.
    pending_replace: Option<(u64, crate::workspace_replace::Plan)>,
    instance_id: String,
    syntax: SyntaxHighlighter,
    keymap: HashMap<String, Command>,
    pending_chord: Option<(String, Instant)>,
    clipboard: String,
    clipboard_commands: crate::clipboard::Commands,
    /// System-clipboard work runs in order on one helper thread.
    clipboard_jobs: Option<std::sync::mpsc::Sender<(ClipboardJob, Tracked)>>,
    paste_generation: u64,
    last_find_query: String,
    last_replace_text: String,
    search_options: crate::search::SearchOptions,
    status_until: Option<Instant>,
    last_autosave: Instant,
    /// The newest session snapshot written so far, shared with background saves.
    session_saves: Arc<std::sync::Mutex<u64>>,
    session_ticket: u64,
    autosave_running: bool,
    last_external_check: Instant,
    external_check_running: bool,
    /// Changed-on-disk files already reported, so the status says it once per change.
    external_reported: HashSet<PathBuf>,
    files_pending: Option<Instant>,
    session_dirty: bool,
    zen: Option<ZenLayout>,
}

impl App {
    pub fn new(mut config: AppConfig) -> Result<Self> {
        config.workspace = std::fs::canonicalize(&config.workspace)?;
        let paths = config.paths.unwrap_or_else(Paths::discover);
        if let Err(error) = paths.ensure_roots() {
            tracing::warn!(%error, "could not create Tide XDG roots");
        }

        // Problems with the user's own files, shown once the workbench is up: otherwise a
        // typo in settings.jsonc would leave Tide on its defaults without a word.
        let mut config_errors = Vec::new();
        let mut settings = crate::catalog::settings();
        if paths.settings_file.exists()
            && let Some(user) = tide_config::load_jsonc::<Settings>(&paths.settings_file)
                .inspect_err(|error| {
                    tracing::warn!(%error, "user settings were not loaded");
                    config_errors.push(format!("{error}. Tide is using the default settings."));
                })
                .ok()
        {
            let defaults = settings.clone();
            settings.merge(&user);
            let invalid = settings
                .iter()
                .filter(|(key, value)| {
                    crate::preferences::supported(key)
                        && crate::preferences::validate(key, value).is_err()
                })
                .map(|(key, _)| key.clone())
                .collect::<Vec<_>>();
            for key in invalid {
                config_errors.push(format!(
                    "{}: “{key}” has an invalid value, so Tide is using its default.",
                    paths.settings_file.display()
                ));
                if let Some(value) = defaults.get(&key) {
                    settings.set(key, value.clone());
                }
            }
        }

        let mut themes = tide_config::builtin::themes();
        for diagnostic in themes.load_user_themes(&paths.themes_dir) {
            tracing::warn!(%diagnostic, "User theme was not loaded");
            config_errors.push(format!("Theme not loaded: {diagnostic}"));
        }
        if let Some(theme_id) = settings
            .get("workbench.colorTheme")
            .and_then(serde_json::Value::as_str)
        {
            themes.select(theme_id);
        }

        let mut keybindings = tide_config::builtin::keybindings();
        let mut user_keybindings = Vec::new();
        if paths.keybindings_file.exists()
            && let Some(user) = tide_config::load_jsonc::<Vec<Keybinding>>(&paths.keybindings_file)
                .inspect_err(|error| {
                    tracing::warn!(%error, "user keybindings were not loaded");
                    config_errors.push(format!("{error}. Tide is using the default keys."));
                })
                .ok()
        {
            user_keybindings.clone_from(&user);
            for binding in user {
                let canonical = canonical_key(&binding.key);
                if let Some(existing) = keybindings.iter_mut().find(|existing| {
                    canonical_key(&existing.key) == canonical && existing.when == binding.when
                }) {
                    *existing = binding;
                } else {
                    keybindings.push(binding);
                }
            }
        }

        let mut commands = crate::catalog::commands();
        if cfg!(target_os = "macos")
            && let Some(reveal) = commands.get_mut(Command::FileRevealInFinder.as_str())
        {
            "Reveal in Finder".clone_into(&mut reveal.label);
        }
        let palette = Palette::from_theme(themes.selected());
        let syntax = SyntaxHighlighter::new(themes.selected());
        let file_tree = FileTreeState::new(&config.workspace);
        let session_store = SessionStore::new(paths.clone(), config.workspace.clone());
        let (events_tx, events_rx) = tokio::sync::mpsc::channel(64);
        let watch_filter = Arc::new(crate::watch::WatchFilter::new(&config.workspace));
        let git = GitController::new(&config.jobs, events_tx.clone());
        let lsp = LspController::new(&config.jobs, events_tx.clone());
        let instance_id = format!("{}-{}", std::process::id(), timestamp_millis());
        let secrets = Arc::new(SecretService::local(paths.secrets_file.clone()));
        let ai = AiController::load(config.agent_discovery, &config.jobs, events_tx.clone());
        let db = DbController::load(
            &paths,
            &config.workspace,
            &settings,
            secrets,
            &config.jobs,
            events_tx.clone(),
        );
        let mut app = Self {
            paths,
            workspace: config.workspace,
            settings,
            keybindings,
            user_keybindings,
            install: config.install,
            url_opener: crate::help::open_url,
            latest_release: crate::help::latest_release,
            path_revealer: crate::help::reveal_path,
            themes,
            commands,
            palette,
            workbench: Workbench::new(),
            documents: DocumentRegistry::default(),
            file_tree,
            dock: DockState::new(),
            overlay: None,
            focus: Focus::Main,
            accordion: AccordionState::new(),
            outline_selected: 0,
            outline_collapsed: Default::default(),
            outline_scroll: 0,
            outline_document: None,
            outline_cache: RefCell::default(),
            sidebar_visible: true,
            sidebar_width: 36,
            sidebar_left: true,
            agents_left: false,
            window_focused: true,
            status_history: Vec::new(),
            status_action: 0,
            pending_move: None,
            pending_close: None,
            pending_keep: None,
            save_before: None,
            save_as_form: None,
            git,
            jobs: config.jobs,
            status: String::new(),
            should_quit: false,
            restart: None,
            layout: LayoutState::default(),
            pointer: PointerState::default(),
            session_name: config.session_name.clone(),
            session_store,
            session_write_blocked: false,
            session_ephemeral: config.ephemeral_session,
            sessions_generation: 0,
            events_tx,
            events_rx: Some(events_rx),
            watcher: None,
            watch_filter,
            watch_created: Arc::default(),
            lsp,
            ai,
            db,
            quick_open_generation: 0,
            search_generation: 0,
            search_cancel: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            replace_generation: 0,
            replace_cancel: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            pending_replace: None,
            instance_id,
            syntax,
            keymap: HashMap::new(),
            pending_chord: None,
            clipboard: String::new(),
            clipboard_commands: crate::clipboard::command,
            clipboard_jobs: None,
            paste_generation: 0,
            last_find_query: String::new(),
            last_replace_text: String::new(),
            search_options: crate::search::SearchOptions::default(),
            status_until: None,
            last_autosave: Instant::now(),
            session_saves: Arc::default(),
            session_ticket: 0,
            autosave_running: false,
            last_external_check: Instant::now(),
            external_check_running: false,
            external_reported: HashSet::new(),
            files_pending: None,
            session_dirty: false,
            zen: None,
        };
        app.apply_preferences();
        for (section, key) in [
            (SectionId::Outline, "tui.outline.collapsedOnStartup"),
            (SectionId::Timeline, "tui.timeline.collapsedOnStartup"),
        ] {
            if let Some(row) = app.accordion.sections.iter_mut().find(|s| s.id == section) {
                row.expanded =
                    app.settings.get(key).and_then(serde_json::Value::as_bool) == Some(false);
            }
        }
        app.git.timeline.file = app
            .settings
            .get("tui.timeline.mode")
            .and_then(serde_json::Value::as_str)
            == Some("file");
        app.sidebar_left = app
            .settings
            .get("tui.sidebar.location")
            .and_then(serde_json::Value::as_str)
            != Some("right");
        app.install_alternate_bindings();
        app.rebuild_keymap();
        app.with_ai(|ai, ui| ai.refresh_sessions(ui));
        app.start_watcher();
        with_ui!(app, |ui| app.git.refresh(ui));
        with_ui!(app, |ui| app.git.refresh_timeline(ui));

        let loaded = if let Some(name) = &config.session_name {
            app.session_store.load_named(name)
        } else if config.restore_session && app.setting_bool("session.restoreOnStartup", true) {
            app.session_store.load_last()
        } else {
            Ok(None)
        };
        let restored = match loaded {
            Ok(session) => session,
            Err(error) => {
                app.session_write_blocked = true;
                app.set_status(format!(
                    "Session load failed; session writes disabled: {error:#}"
                ));
                None
            }
        };
        let had_session = if let Some(session) = restored {
            match app.restore_session(session) {
                Ok(()) => true,
                Err(error) => {
                    app.session_write_blocked = true;
                    app.set_status(format!("Session restore failed: {error:#}"));
                    false
                }
            }
        } else {
            false
        };
        if !had_session {
            let mode = app
                .settings
                .get("workbench.startupEditor")
                .and_then(serde_json::Value::as_str)
                .unwrap_or_default();
            let first_run =
                mode == "welcomeOnFirstRun" && !crate::help::welcome_marker(&app.paths).exists();
            if let Some(file) = config.initial_file {
                app.open_file(file);
                if first_run {
                    app.set_status("New to Tide? Run “Help: Welcome” from the command palette");
                }
            } else if first_run || mode == "welcome" {
                app.open_help(
                    crate::help::for_command(Command::HelpWelcome).expect("Welcome page"),
                );
                crate::help::record_welcome(&app.paths);
            } else {
                app.workbench.insert_tab_focused(Tab::Welcome);
            }
        }
        if app.workbench.active_tab().is_none() {
            app.workbench.insert_tab_focused(Tab::Welcome);
        }
        if !config_errors.is_empty() {
            app.overlay = Some(modal::info(
                "Configuration Problems",
                &format!(
                    "{}\n\nFix the file, then restart Tide. Changes made from the Settings and Keyboard Shortcuts lists leave a file with errors unchanged.",
                    config_errors.join("\n")
                ),
            ));
        }
        Ok(app)
    }

    fn start_watcher(&mut self) {
        use notify::Watcher;
        let sender = self.events_tx.clone();
        let filter = self.watch_filter.clone();
        let created = self.watch_created.clone();
        let watcher = notify::recommended_watcher(move |result: notify::Result<notify::Event>| {
            let Ok(event) = result else { return };
            if matches!(event.kind, notify::EventKind::Access(_))
                || (!event.paths.is_empty() && event.paths.iter().all(|path| filter.ignores(path)))
            {
                return;
            }
            if crate::watch::PER_DIRECTORY && matches!(event.kind, notify::EventKind::Create(_)) {
                crate::lock(&created).extend(event.paths.into_iter().filter(|path| path.is_dir()));
            }
            let _ = sender.try_send(AppEvent::FilesChanged);
        });
        let mut watcher = match watcher {
            Ok(watcher) => watcher,
            Err(error) => {
                tracing::warn!(%error, "file watcher unavailable");
                return;
            }
        };
        let watched = if crate::watch::PER_DIRECTORY {
            let mut failure = None;
            for directory in crate::watch::directories(&self.workspace, &self.watch_filter) {
                if let Err(error) = watcher.watch(&directory, notify::RecursiveMode::NonRecursive) {
                    failure.get_or_insert(error);
                }
            }
            failure.map_or(Ok(()), Err)
        } else {
            watcher.watch(&self.workspace, notify::RecursiveMode::Recursive)
        };
        if let Err(error) = watched {
            tracing::warn!(%error, "file watcher is incomplete");
            self.set_status(format!("File watching is incomplete: {error}"));
        }
        self.watcher = Some(watcher);
    }

    /// Adds watches for directories created since the last refresh.
    fn watch_created_directories(&mut self) {
        use notify::Watcher;
        let created = std::mem::take(&mut *crate::lock(&self.watch_created));
        let Some(watcher) = self.watcher.as_mut() else {
            return;
        };
        for directory in created {
            for directory in crate::watch::walk(&directory, &self.watch_filter) {
                let _ = watcher.watch(&directory, notify::RecursiveMode::NonRecursive);
            }
        }
    }

    /// Runs queued background work and applies its events until nothing is left.
    #[cfg(test)]
    fn settle(&mut self) {
        let deadline = Instant::now() + Duration::from_secs(60);
        loop {
            let busy = self.jobs.advance();
            let mut handled = false;
            while let Some(event) = self.events_rx.as_mut().and_then(|rx| rx.try_recv().ok()) {
                self.handle_background(event);
                handled = true;
            }
            if !busy && !handled {
                return;
            }
            assert!(Instant::now() < deadline, "background work did not settle");
        }
    }

    pub fn take_event_receiver(&mut self) -> Option<tokio::sync::mpsc::Receiver<AppEvent>> {
        self.events_rx.take()
    }

    pub fn handle_background(&mut self, event: AppEvent) {
        match event {
            AppEvent::ClipboardWritten {
                result,
                done,
                fallback,
            } => match result {
                Ok(()) => {
                    if let Some(done) = done {
                        self.set_status(done);
                    }
                }
                Err(error) => {
                    if let Some(fallback) = fallback {
                        self.set_status(format!("{fallback}; {error}"));
                    }
                }
            },
            AppEvent::ClipboardRead {
                generation,
                target,
                result,
            } => {
                if generation != self.paste_generation
                    || self.overlay.as_ref().map(|overlay| overlay.kind()) != target.as_ref()
                {
                    return;
                }
                if let Ok(text) = result {
                    self.clipboard = text;
                }
                if !self.clipboard.is_empty() {
                    let text = self.clipboard.clone();
                    self.handle_paste(&text);
                }
            }
            AppEvent::SessionAutosaved(result) => {
                self.autosave_running = false;
                if let Err(error) = result {
                    self.mark_session_dirty();
                    self.set_status(format!("Recovery snapshot failed: {error}"));
                }
            }
            AppEvent::ExternalChanges(titles) => {
                self.external_check_running = false;
                self.report_external_changes(&titles);
            }
            AppEvent::QuickOpen { generation, files } => {
                if generation == self.quick_open_generation
                    && let Some(overlay) = self
                        .overlay
                        .as_mut()
                        .filter(|modal| *modal.kind() == ModalKind::QuickOpen)
                {
                    let base = overlay.base_mut();
                    base.items = file_items(&files, &self.workspace);
                    base.hints = None;
                    base.loading = false;
                    base.refilter();
                    let submit =
                        std::mem::take(&mut base.pending_submit) && base.selected_item().is_some();
                    self.set_status(format!("{} files", files.len()));
                    if submit {
                        self.handle_modal_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
                    }
                }
            }
            AppEvent::SearchResults {
                generation,
                query,
                items,
            } => {
                if generation == self.search_generation
                    && !self
                        .search_cancel
                        .load(std::sync::atomic::Ordering::Acquire)
                    && let Some(overlay) = self
                        .overlay
                        .as_mut()
                        .filter(|m| *m.kind() == ModalKind::SearchResults)
                {
                    let count = items.len();
                    let base = overlay.base_mut();
                    base.items = items;
                    base.hints = None;
                    base.refilter();
                    self.set_status(format!("{count} match(es) for {query}"));
                }
            }
            AppEvent::LatestRelease(result, automatic) => {
                self.show_latest_release(result, automatic)
            }
            AppEvent::SavedSessions {
                generation,
                delete,
                sessions,
            } => {
                if generation == self.sessions_generation {
                    self.show_workspace_sessions(delete, &sessions);
                }
            }
            AppEvent::ReplacePlanned { generation, result } => {
                self.show_replace_plan(generation, result);
            }
            AppEvent::ReplaceWritten { written, buffers } => {
                self.report_replace_in_files(&written, buffers);
            }
            AppEvent::Git(event) => {
                with_ui!(self, |ui| self.git.handle_event(
                    event,
                    ui,
                    &self.lsp.diagnostics
                ))
            }
            AppEvent::Lsp(event) => with_ui!(self, |ui| self.lsp.handle_event(event, ui)),
            AppEvent::Db(event) => self.with_db(|db, ui| db.handle_event(event, ui)),
            AppEvent::Ai(event) => self.with_ai(|ai, ui| ai.handle_event(event, ui)),
            AppEvent::FilesChanged => {
                self.files_pending.get_or_insert_with(Instant::now);
            }
            AppEvent::Error(message) => self.set_status(message),
        }
    }

    pub async fn shutdown(&mut self) {
        self.search_cancel
            .store(true, std::sync::atomic::Ordering::Release);
        self.replace_cancel
            .store(true, std::sync::atomic::Ordering::Release);
        self.lsp.stop_requests().await;
        self.git.shutdown().await;
        self.ai.stop_native_tasks().await;
        // Abort outstanding requests before dropping cached connections; PostgreSQL
        // rolls unfinished row transactions back when their client is dropped.
        self.db.stop_requests().await;
        self.workbench.kill_terminals();
        self.dock.tabs.clear();
        let _ = tokio::time::timeout(Duration::from_secs(2), async {
            self.lsp.shutdown_servers().await;
            self.db.close_sessions().await;
        })
        .await;
    }

    pub fn should_quit(&self) -> bool {
        self.should_quit
    }

    fn apply(&mut self, effects: Vec<Effect>) {
        for effect in effects {
            match effect {
                Effect::Status(message) => self.set_status(message),
                Effect::OpenFile { path, line } => {
                    self.open_file(path);
                    if let Some(line) = line
                        && let Some(editor) = self.workbench.active_editor_mut()
                    {
                        editor.goto_line(line);
                    }
                }
                Effect::Copy { text, done } => {
                    self.clipboard = text;
                    self.export_clipboard(None, None);
                    self.set_status(done);
                }
                Effect::FileDecorations(decorations) => self.file_tree.set_decorations(decorations),
                Effect::Command(command) => self.execute_command(command),
                Effect::HunkSaved {
                    document,
                    version,
                    success,
                } => self.finish_hunk_save_before(&document, version, success),
            }
        }
    }

    fn with_ai<R>(&mut self, f: impl FnOnce(&mut AiController, &mut Ui<'_>) -> R) -> R {
        with_ui!(self, |ui| f(&mut self.ai, ui))
    }

    fn with_db<R>(&mut self, f: impl FnOnce(&mut DbController, &mut Ui<'_>) -> R) -> R {
        with_ui!(self, |ui| f(&mut self.db, ui))
    }

    fn lsp_request(&mut self, kind: LspRequestKind) {
        with_ui!(self, |ui| self.lsp.request(kind, ui));
    }

    /// Records that the session snapshot is out of date.
    pub(crate) fn mark_session_dirty(&mut self) {
        self.session_dirty = true;
    }

    fn set_status(&mut self, message: impl Into<String>) {
        self.status = message.into();
        if !self.status.is_empty() && self.status_history.last() != Some(&self.status) {
            self.status_history.push(self.status.clone());
            if self.status_history.len() > 1000 {
                self.status_history.remove(0);
            }
        }
        self.status_until = Some(Instant::now() + Duration::from_secs(5));
    }

    /// Whether the event may have changed visible state.
    pub fn handle_event(&mut self, event: Event) -> bool {
        match event {
            Event::Key(key) if key.kind != KeyEventKind::Release => self.handle_input_key(key),
            Event::Paste(text) => self.handle_paste(&text),
            Event::Mouse(mouse) if mouse.kind == MouseEventKind::Moved => {
                return self.forward_terminal_mouse(mouse);
            }
            Event::Mouse(mouse) => self.handle_mouse(mouse),
            Event::FocusGained => {
                self.window_focused = true;
                self.last_external_check = Instant::now() - Duration::from_secs(3);
            }
            Event::FocusLost => self.window_focused = false,
            Event::Resize(_, _) => {
                self.pointer.resize = None;
                self.pointer.terminal_capture = None;
                self.pointer.editor_drag = None;
            }
            _ => return false,
        }
        true
    }

    pub fn on_tick(&mut self) -> bool {
        let mut changed = self.workbench.poll_terminals() | self.syntax.take_changed();
        changed |= self.workbench.pane_ids().into_iter().any(|id| {
            self.workbench
                .pane(id)
                .and_then(|pane| pane.tabs.get(pane.active))
                .and_then(Tab::editor)
                .is_some_and(EditorState::fold_refresh_due)
        });
        if self
            .files_pending
            .is_some_and(|at| at.elapsed() >= Duration::from_millis(250))
        {
            self.files_pending = None;
            self.watch_created_directories();
            self.file_tree.refresh();
            self.last_external_check = Instant::now() - Duration::from_secs(3);
            if self.git.state.snapshot.is_some() || self.workspace.join(".git").is_dir() {
                with_ui!(self, |ui| self.git.refresh(ui));
            }
            changed = true;
        }
        if with_ui!(self, |ui| self.git.follow_active_file(ui)) {
            changed = true;
        }
        if with_ui!(self, |ui| self.lsp.tick(ui)) {
            changed = true;
        }
        self.db.prune_tasks();
        if self.with_ai(|ai, ui| ai.tick(ui)) {
            changed = true;
        }
        for terminal in &mut self.dock.tabs {
            changed |= terminal.poll();
        }
        let input_error = self.workbench.take_terminal_error().or_else(|| {
            self.dock
                .tabs
                .iter_mut()
                .find_map(TerminalTab::take_input_error)
        });
        if let Some(error) = input_error {
            self.set_status(error);
            changed = true;
        }

        if self
            .status_until
            .is_some_and(|until| Instant::now() >= until)
        {
            self.status = String::new();
            self.status_until = None;
            changed = true;
        }
        if self
            .pending_chord
            .as_ref()
            .is_some_and(|(_, started)| started.elapsed() > Duration::from_millis(750))
            && let Some((prefix, _)) = self.pending_chord.take()
            && let Some(action) = self.resolve_binding(&prefix)
            && (action != BindingAction::Run(Command::WorkbenchCommandPalette)
                || self.overlay.is_none())
        {
            self.run_binding(action);
            changed = true;
        }
        let watch_mode = self
            .settings
            .get("files.watchFiles")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("onFocus");
        if watch_mode != "off"
            && (watch_mode == "always" || self.window_focused)
            && !self.external_check_running
            && self.last_external_check.elapsed() > Duration::from_secs(2)
        {
            self.last_external_check = Instant::now();
            changed |= self.check_external_changes();
        }
        if self.setting_bool("session.autoSave", true)
            && self.last_autosave.elapsed()
                > Duration::from_millis(self.setting_u64("session.autoSaveInterval", 30000))
        {
            self.last_autosave = Instant::now();
            if self.session_dirty
                && !self.autosave_running
                && let Err(error) = self.autosave_session()
            {
                self.set_status(format!("Recovery snapshot failed: {error:#}"));
                changed = true;
            }
        }
        changed
    }

    /// Stats (and, when a stamp moved, hashes) every open file on a blocking
    /// thread; the result arrives as [`AppEvent::ExternalChanges`].
    fn check_external_changes(&mut self) -> bool {
        let mut documents = Vec::new();
        for tab in self.workbench.tabs() {
            if let Some(editor) = tab.editor()
                && editor.historical.is_none()
                && let Some(path) = editor.path()
                && !documents.iter().any(|(_, known)| known == path)
            {
                documents.push((editor.document.clone(), path.to_path_buf()));
            }
        }
        if documents.is_empty() {
            return false;
        }
        let check = move || {
            documents
                .into_iter()
                .filter(|(document, path)| document.disk_changed(path))
                .map(|(_, path)| path)
                .collect::<Vec<_>>()
        };
        let sender = self.events_tx.clone();
        self.external_check_running = true;
        self.jobs.spawn_blocking(move || {
            let _ = sender.blocking_send(AppEvent::ExternalChanges(check()));
        });
        false
    }

    /// Reloads open files that changed on disk and have no unsaved edits, as one undo step
    /// that keeps the cursor near its place. A file with unsaved edits, or one deleted on
    /// disk, is reported once; saving it stays blocked until it is reverted or overwritten.
    fn report_external_changes(&mut self, paths: &[PathBuf]) -> bool {
        self.external_reported.retain(|path| paths.contains(path));
        let mut reloaded = Vec::new();
        let mut notices = Vec::new();
        for path in paths {
            let Some(editor) = self.workbench.tabs_mut().find_map(|tab| match tab {
                Tab::Editor(editor)
                    if editor.historical.is_none() && editor.path() == Some(path.as_path()) =>
                {
                    Some(editor)
                }
                _ => None,
            }) else {
                continue;
            };
            let title = editor.title();
            if editor.is_dirty() {
                if self.external_reported.insert(path.clone()) {
                    notices.push(format!(
                        "{title} changed on disk and has unsaved edits; run “Revert File” to load the disk version or “Overwrite File on Disk” to keep yours"
                    ));
                }
                continue;
            }
            match editor.reload() {
                Ok(()) => reloaded.push(title),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                    if self.external_reported.insert(path.clone()) {
                        notices.push(format!(
                            "{title} was deleted on disk; run “Overwrite File on Disk” to write it again"
                        ));
                    }
                }
                Err(error) => {
                    if self.external_reported.insert(path.clone()) {
                        notices.push(format!(
                            "{title} changed on disk but could not be reloaded: {error}"
                        ));
                    }
                }
            }
        }
        if !reloaded.is_empty() {
            self.mark_session_dirty();
            with_ui!(self, |ui| self.git.load_inline_diff(ui));
            notices.insert(
                0,
                format!("Reloaded {} (changed on disk)", reloaded.join(", ")),
            );
        }
        let changed = !notices.is_empty();
        for notice in notices {
            self.set_status(notice);
        }
        changed
    }

    fn associate_agent_workspace(&mut self, path: &str) {
        let path = Path::new(path);
        if !path.is_absolute() || path.parent().is_none() {
            self.set_status("Agents: choose a project directory");
            return;
        }
        let mut aliases = self
            .settings
            .get("ai.cli.workspaceAliases")
            .and_then(serde_json::Value::as_array)
            .cloned()
            .unwrap_or_default();
        let value = serde_json::Value::String(canonical_session_path(path).display().to_string());
        if !aliases.contains(&value) {
            aliases.push(value);
        }
        self.update_setting(
            "ai.cli.workspaceAliases",
            &serde_json::Value::Array(aliases).to_string(),
        );
        self.with_ai(|ai, ui| ai.rebuild_sessions(ui));
    }
}

type ClipboardJob = Box<dyn FnOnce() -> AppEvent + Send>;

/// Whether a command ID is routed by the workbench. Used by catalog audits.
#[must_use]
pub fn command_supported(command: &str) -> bool {
    command.parse::<Command>().is_ok()
}

pub(crate) fn timestamp_millis() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
}

#[cfg(test)]
mod tests;
