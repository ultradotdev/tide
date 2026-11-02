//! Language servers: running clients, document leases, diagnostics, outlines and the
//! request/reply cycle for editor features.
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use crossterm::event::KeyCode;
use tide_config::Settings;
use tide_core::Workspace;
use tide_core::num::{saturating_i64, utf16_units};
use tide_core::{Document, Position};
use tide_lsp::{
    CodeActionItem, CompletionItem, Diagnostic, DocumentLease, Location, LspClient, LspError,
    LspEvent as ServerEvent, LspRegistry, Position as LspPosition, Range as LspRange, Result,
    ServerConfig, SymbolItem, TextEdit, command_available, language_id_for_path,
    parse_document_symbols, parse_workspace_symbols, path_to_uri, server_for_language, uri_to_path,
};
use tokio::sync::mpsc::Sender;
use tokio::task::JoinHandle;

use super::{Effect, Ui};
use crate::app::{AppEvent, Focus};
use crate::editor::DocumentHandle;
use crate::jobs::Jobs;
use crate::modal::{self, ModalItem, PromptAction, Submitted};
use crate::workbench::{PaneId, Tab, Workbench};

#[derive(Debug, Clone)]
pub enum LspRequestKind {
    Completion,
    Hover,
    Definition,
    DocumentSymbols,
    WorkspaceSymbols(String),
    References,
    SignatureHelp,
    Format,
    /// Formats the selection, or the cursor's line without one.
    FormatSelection,
    CodeActions,
    ExecuteCommand(serde_json::Value),
    Rename(String),
}

#[derive(Debug, Clone)]
pub struct LspFileEdits {
    pub uri: String,
    pub edits: Vec<TextEdit>,
}

#[derive(Debug)]
pub enum LspEvent {
    ServerEdit {
        edit: serde_json::Value,
        versions: HashMap<String, i64>,
        reply: tokio::sync::oneshot::Sender<bool>,
    },
    Status {
        language_id: String,
        message: String,
    },
    Diagnostics {
        version: Option<i64>,
        uri: String,
        diagnostics: Vec<Diagnostic>,
    },
    Completion {
        generation: u64,
        items: Vec<CompletionItem>,
    },
    Hover {
        generation: u64,
        text: String,
    },
    Definition {
        generation: u64,
        locations: Vec<Location>,
    },
    Symbols {
        generation: u64,
        items: Vec<SymbolItem>,
    },
    References {
        generation: u64,
        locations: Vec<Location>,
    },
    Signature {
        generation: u64,
        text: String,
    },
    Edits {
        generation: u64,
        files: Vec<LspFileEdits>,
    },
    Outline {
        version: i64,
        uri: String,
        items: Vec<SymbolItem>,
    },
    CodeActions {
        generation: u64,
        actions: Vec<CodeActionItem>,
    },
    CommandResult {
        generation: u64,
        message: String,
    },
    Error(String),
}

/// The editor state a reply must still match to be shown.
#[derive(Default)]
pub(crate) struct Origin {
    pub revision: u64,
    pub cursor: Position,
    pub document: Option<DocumentHandle>,
    pub pane: PaneId,
}

/// The active document as a language server sees it.
pub(crate) struct ActiveDocument {
    language: String,
    uri: String,
    text: String,
    version: i64,
    pub position: LspPosition,
    /// The selection, or the cursor's whole line without one.
    selection: LspRange,
}

/// What a request sends besides the document itself.
struct RequestInput {
    generation: u64,
    language: String,
    uri: String,
    position: LspPosition,
    selection: LspRange,
    diagnostics: Vec<Diagnostic>,
    tab_size: usize,
    insert_spaces: bool,
}

pub(crate) struct LspController {
    pub clients: Arc<LspRegistry>,
    pub leases: HashMap<String, DocumentLease>,
    /// Server status keyed by command and arguments, since one server can
    /// serve several languages.
    status: HashMap<(String, Vec<String>), String>,
    pub diagnostics: HashMap<String, Vec<Diagnostic>>,
    pub outline: HashMap<String, Vec<SymbolItem>>,
    code_actions: Vec<CodeActionItem>,
    pub generation: u64,
    pub origin: Origin,
    /// A typed trigger waiting out the completion debounce.
    pub pending: Option<(Instant, LspRequestKind)>,
    task: Option<JoinHandle<()>>,
    tasks: Vec<JoinHandle<()>>,
    jobs: Jobs,
    events: Sender<AppEvent>,
}

impl LspController {
    pub fn new(jobs: &Jobs, events: Sender<AppEvent>) -> Self {
        Self {
            clients: Arc::new(LspRegistry::new()),
            leases: HashMap::new(),
            status: HashMap::new(),
            diagnostics: HashMap::new(),
            outline: HashMap::new(),
            code_actions: Vec::new(),
            generation: 0,
            origin: Origin::default(),
            pending: None,
            task: None,
            tasks: Vec::new(),
            jobs: jobs.clone(),
            events,
        }
    }

    /// Stops in-flight requests and document notifications.
    pub async fn stop_requests(&mut self) {
        if let Some(task) = self.task.take() {
            task.abort();
            let _ = task.await;
        }
        for task in self.tasks.drain(..) {
            task.abort();
            let _ = task.await;
        }
    }

    pub async fn shutdown_servers(&self) {
        self.clients.shutdown_all().await;
    }

    /// Drops any reply still in flight, such as after the cursor moves.
    pub fn invalidate(&mut self) {
        self.generation = self.generation.wrapping_add(1);
        self.pending = None;
    }

    /// Abandons the current request entirely.
    pub fn cancel(&mut self) {
        self.invalidate();
        if let Some(task) = self.task.take() {
            task.abort();
        }
    }

    /// Whether a reply for `generation` still matches what the user is looking at.
    pub fn current(&self, generation: u64, ui: &Ui<'_>) -> bool {
        let editor = ui.workbench.active_editor();
        generation == self.generation
            && *ui.focus == Focus::Main
            && ui.overlay.as_ref().is_none_or(|o| o.is_popup())
            && editor.is_some_and(|e| e.document.cursor().position == self.origin.cursor)
            && ui.workbench.focused() == self.origin.pane
            && editor.is_some_and(|editor| {
                self.origin
                    .document
                    .as_ref()
                    .is_some_and(|handle| handle.same_view(&editor.document))
            })
            && editor.map_or(0, |editor| editor.revision()) == self.origin.revision
    }

    /// The status reported by the server for `language`.
    pub fn server_status(&self, settings: &Settings, language: &str) -> Option<&str> {
        server_for(settings, language)
            .and_then(|server| self.status.get(&status_key(&server)))
            .map(String::as_str)
    }

    /// Sends a debounced typed request once it is due; true when one was sent.
    pub fn tick(&mut self, ui: &mut Ui<'_>) -> bool {
        let mut sent = false;
        let debounce = Duration::from_millis(ui.setting_u64("lsp.completionDebounceMs", 250));
        if self
            .pending
            .as_ref()
            .is_some_and(|(at, _)| at.elapsed() >= debounce)
            && ui.overlay.is_none()
        {
            let (_, kind) = self.pending.take().unwrap();
            let available = active_document(ui)
                .and_then(|document| server_for(ui.settings, &document.language))
                .is_some_and(|server| command_available(&server.command));
            if available {
                self.request(kind, ui);
                sent = true;
            }
        }
        self.tasks.retain(|task| !task.is_finished());
        sent
    }

    pub fn handle_event(&mut self, event: LspEvent, ui: &mut Ui<'_>) {
        match event {
            LspEvent::ServerEdit {
                edit,
                versions,
                reply,
            } => {
                if reply.is_closed() {
                    return;
                }
                let applied = self.apply_server_edit(&edit, &versions, ui);
                let _ = reply.send(applied);
            }
            LspEvent::Status {
                language_id,
                message,
            } => {
                if let Some(server) = server_for(ui.settings, &language_id) {
                    self.status.insert(status_key(&server), message.clone());
                }
                ui.status(format!("LSP {language_id}: {message}"));
            }
            LspEvent::Diagnostics {
                uri,
                diagnostics,
                version,
            } => self.set_diagnostics(uri, diagnostics, version, ui),
            LspEvent::Completion { generation, items } => {
                if self.current(generation, ui) && !items.is_empty() {
                    *ui.overlay = Some(modal::lsp_completion(items));
                    anchor_overlay(ui);
                }
            }
            LspEvent::Hover { generation, text } => {
                if self.current(generation, ui)
                    && ui.setting_bool("lsp.hover.enabled", true)
                    && !text.is_empty()
                {
                    *ui.overlay = Some(modal::info("Hover", &text));
                    anchor_overlay(ui);
                }
            }
            LspEvent::Definition {
                generation,
                locations,
            } => {
                if self.current(generation, ui)
                    && let Some(location) = locations.first()
                    && let Some(path) = uri_to_path(&location.uri)
                {
                    ui.effects.push(Effect::OpenFile {
                        path,
                        line: Some(location.range.start.line as usize + 1),
                    });
                }
            }
            LspEvent::Symbols { generation, items } => {
                if self.current(generation, ui) {
                    let items = items
                        .into_iter()
                        .filter_map(|symbol| {
                            let uri = symbol.uri?;
                            Some(ModalItem::new(
                                format!("{}{}", "  ".repeat(symbol.depth), symbol.name),
                                symbol.detail,
                                format!("{}\t{}", uri, symbol.line + 1),
                            ))
                        })
                        .collect();
                    *ui.overlay = Some(modal::search_results(items));
                }
            }
            LspEvent::References {
                generation,
                locations,
            } => {
                if self.current(generation, ui) {
                    let items = locations
                        .into_iter()
                        .filter_map(|location| {
                            let path = uri_to_path(&location.uri)?;
                            Some(ModalItem::new(
                                format!("{}:{}", path.display(), location.range.start.line + 1),
                                "reference".to_owned(),
                                format!("{}\t{}", location.uri, location.range.start.line + 1),
                            ))
                        })
                        .collect();
                    *ui.overlay = Some(modal::search_results(items));
                }
            }
            LspEvent::Signature { generation, text } => {
                if self.current(generation, ui)
                    && ui.setting_bool("lsp.signatureHelp.enabled", true)
                    && !text.is_empty()
                {
                    match ui
                        .settings
                        .get("lsp.signatureHelp.display")
                        .and_then(serde_json::Value::as_str)
                        .unwrap_or("popup")
                    {
                        "statusBar" => ui.status(text),
                        "inline" => {
                            if let Some(editor) = ui.workbench.active_editor_mut() {
                                editor.inline_signature = Some(text);
                            }
                        }
                        _ => {
                            *ui.overlay = Some(modal::info("Signature Help", &text));
                            anchor_overlay(ui);
                        }
                    }
                }
            }
            LspEvent::Edits { generation, files } => {
                if self.current(generation, ui) {
                    apply_edits(files, ui);
                }
            }
            LspEvent::Outline {
                uri,
                items,
                version,
            } => {
                if uri_to_path(&uri)
                    .and_then(|p| ui.documents.get(&p))
                    .is_none_or(|d| saturating_i64(d.version()) == version)
                {
                    self.outline.insert(uri, items);
                }
            }
            LspEvent::CodeActions {
                generation,
                mut actions,
            } => {
                if self.current(generation, ui) {
                    actions.sort_by(|left, right| {
                        right
                            .is_preferred
                            .cmp(&left.is_preferred)
                            .then_with(|| left.title.cmp(&right.title))
                    });
                    if actions.is_empty() {
                        ui.status("LSP: no code actions at the cursor");
                    } else {
                        let items = actions
                            .iter()
                            .enumerate()
                            .map(|(index, action)| {
                                ModalItem::new(
                                    action.title.clone(),
                                    action.kind.clone().unwrap_or_else(|| "action".to_owned()),
                                    index.to_string(),
                                )
                            })
                            .collect();
                        self.code_actions = actions;
                        *ui.overlay = Some(modal::code_actions(items));
                    }
                }
            }
            LspEvent::CommandResult {
                generation,
                message,
            } => {
                if self.current(generation, ui) {
                    ui.status(message);
                }
            }
            LspEvent::Error(message) => ui.status(format!("LSP: {message}")),
        }
    }

    /// Handles the language-server parts of a modal submission and hands back anything else.
    pub fn submit(&mut self, submitted: Submitted, ui: &mut Ui<'_>) -> Option<Submitted> {
        match submitted {
            Submitted::CodeAction(index) => self.run_code_action(index, ui),
            Submitted::LspCompletion(item) => self.apply_completion(*item, ui),
            Submitted::Prompt(PromptAction::Rename, input) => {
                let new_name = input.trim().to_owned();
                if !new_name.is_empty() {
                    self.request(LspRequestKind::Rename(new_name), ui);
                }
            }
            Submitted::Prompt(PromptAction::WorkspaceSymbol, query) => {
                if !query.trim().is_empty() {
                    self.request(LspRequestKind::WorkspaceSymbols(query), ui);
                }
            }
            other => return Some(other),
        }
        None
    }

    fn apply_server_edit(
        &mut self,
        edit: &serde_json::Value,
        versions: &HashMap<String, i64>,
        ui: &mut Ui<'_>,
    ) -> bool {
        let mut valid = true;
        let targets = tide_lsp::checked_workspace_edit(edit)
            .ok()
            .map(|files| {
                files
                    .into_iter()
                    .map(|(uri, _)| uri)
                    .collect::<HashSet<_>>()
            })
            .unwrap_or_default();
        for (uri, version) in versions {
            if targets.contains(uri)
                && let Some(path) = uri_to_path(uri)
                && let Some(handle) = ui.documents.get(&path)
                && saturating_i64(handle.version()) != *version
            {
                valid = false;
            }
        }
        if let Some(changes) = edit
            .get("documentChanges")
            .and_then(serde_json::Value::as_array)
        {
            for change in changes {
                if let Some(doc) = change.get("textDocument")
                    && let Some(version) = doc.get("version").and_then(serde_json::Value::as_i64)
                {
                    let current = doc
                        .get("uri")
                        .and_then(serde_json::Value::as_str)
                        .and_then(uri_to_path)
                        .and_then(|p| ui.documents.get(&p))
                        .map(|d| saturating_i64(d.version()));
                    valid &= current == Some(version);
                }
            }
        }
        if !valid {
            ui.status("LSP edit rejected: document changed since server synchronization");
            return false;
        }
        match tide_lsp::checked_workspace_edit(edit) {
            Ok(files) => apply_edits(
                files
                    .into_iter()
                    .map(|(uri, edits)| LspFileEdits { uri, edits })
                    .collect(),
                ui,
            ),
            Err(error) => {
                ui.status(format!("LSP edit rejected: {error:#}"));
                false
            }
        }
    }

    fn set_diagnostics(
        &mut self,
        uri: String,
        diagnostics: Vec<Diagnostic>,
        version: Option<i64>,
        ui: &mut Ui<'_>,
    ) {
        if version.is_some_and(|version| {
            uri_to_path(&uri)
                .and_then(|p| ui.documents.get(&p))
                .is_some_and(|d| saturating_i64(d.version()) != version)
        }) {
            return;
        }
        let count = diagnostics.len();
        let path = uri_to_path(&uri);
        let gutter = ui
            .settings
            .get("lsp.diagnostics.showInGutter")
            .and_then(serde_json::Value::as_bool)
            != Some(false);
        let lines = diagnostics
            .iter()
            .map(|diagnostic| {
                (
                    diagnostic.range.start.line as usize,
                    diagnostic.severity.unwrap_or(3),
                )
            })
            .collect::<Vec<_>>();
        if let Some(path) = path {
            for tab in ui.workbench.tabs_mut() {
                if let Some(editor) = tab.editor_mut()
                    && editor.path() == Some(path.as_path())
                {
                    editor.set_diagnostics(if gutter { lines.clone() } else { Vec::new() });
                    let ranges = diagnostics
                        .iter()
                        .map(|diagnostic| {
                            let convert = |position: LspPosition| {
                                Position::new(
                                    position.line as usize,
                                    utf16_column(
                                        &editor
                                            .document
                                            .line(position.line as usize)
                                            .unwrap_or_default(),
                                        position.character,
                                    ),
                                )
                            };
                            (
                                tide_core::Range::new(
                                    convert(diagnostic.range.start),
                                    convert(diagnostic.range.end),
                                ),
                                diagnostic.severity.unwrap_or(3),
                            )
                        })
                        .collect();
                    editor.set_diagnostic_ranges(ranges);
                }
            }
        }
        self.diagnostics.insert(uri, diagnostics);
        super::git::sync_diff_diagnostics(ui, &self.diagnostics);
        if count > 0 {
            ui.status(format!("{count} diagnostic(s)"));
        }
    }

    fn run_code_action(&mut self, index: usize, ui: &mut Ui<'_>) {
        let Some(action) = self.code_actions.get(index).cloned() else {
            return;
        };
        if !self.current(self.generation, ui) {
            ui.status("LSP: action is stale; request it again");
            return;
        }
        if let Some(edit) = action.edit.clone() {
            let files = match tide_lsp::checked_workspace_edit(&edit) {
                Ok(files) => files,
                Err(error) => {
                    ui.status(format!("LSP action rejected: {error:#}"));
                    return;
                }
            }
            .into_iter()
            .map(|(uri, edits)| LspFileEdits { uri, edits })
            .collect();
            if !apply_edits(files, ui) {
                return;
            }
        }
        if let Some(command) = action.command {
            self.request(LspRequestKind::ExecuteCommand(command), ui);
        } else if action.edit.is_none() {
            ui.status("LSP: code action had no edit or command");
        }
    }

    /// Sends `kind` for the active document; its reply arrives as an [`LspEvent`].
    pub fn request(&mut self, kind: LspRequestKind, ui: &mut Ui<'_>) {
        let Some(document) = active_document(ui) else {
            ui.status("No language server for this document");
            return;
        };
        let Some(server) = server_for(ui.settings, &document.language) else {
            ui.status(format!(
                "No language server configured for {}",
                document.language
            ));
            return;
        };
        if !command_available(&server.command) {
            ui.status(format!("Language server not installed: {}", server.command));
            return;
        }
        self.generation = self.generation.wrapping_add(1);
        self.pending = None;
        let editor = ui.workbench.active_editor();
        self.origin = Origin {
            revision: editor.map_or(0, |editor| editor.revision()),
            cursor: editor.map_or(Position::new(0, 0), |e| e.document.cursor().position),
            document: editor.map(|editor| editor.document.clone()),
            pane: ui.workbench.focused(),
        };
        let input = RequestInput {
            generation: self.generation,
            language: document.language.clone(),
            uri: document.uri.clone(),
            position: document.position,
            selection: document.selection,
            diagnostics: self
                .diagnostics
                .get(&document.uri)
                .cloned()
                .unwrap_or_default(),
            tab_size: super::tab_size(ui.settings),
            insert_spaces: ui.setting_bool("editor.insertSpaces", true),
        };
        let lease = self.leases.entry(document.uri.clone()).or_default().clone();
        let clients = self.clients.clone();
        let sender = self.events.clone();
        let root = ui.workspace.to_path_buf();
        ui.status(format!("LSP {} request...", document.language));
        if let Some(task) = self.task.take() {
            task.abort();
        }
        self.task = Some(self.jobs.spawn(async move {
            let client = match client(&clients, server, root, sender.clone()).await {
                Ok(client) => client,
                Err(error) => {
                    let message = format!("{}: {error:#}", document.language);
                    let _ = sender.send(lsp_error(message)).await;
                    return;
                }
            };
            if let Err(error) = document.synchronize(&client, &lease).await {
                if !matches!(error, LspError::DocumentClosed) && lease.is_valid() {
                    let message = format!("document synchronization: {error:#}");
                    let _ = sender.send(lsp_error(message)).await;
                }
                return;
            }
            let reply = kind.run(&client, input).await;
            let _ = sender.send(AppEvent::Lsp(reply)).await;
        }));
    }

    /// Opens `path` with its language server, starting the server if needed.
    pub fn open_document(
        &mut self,
        path: &Path,
        text: String,
        version: i64,
        settings: &Settings,
        workspace: &Path,
    ) {
        if !enabled(settings) {
            return;
        }
        let Some(language) = language_id_for_path(path) else {
            return;
        };
        let Some(server) = server_for(settings, language) else {
            return;
        };
        if !command_available(&server.command) {
            self.status
                .insert(status_key(&server), "missing".to_owned());
            return;
        }
        let Some(uri) = path_to_uri(path) else {
            return;
        };
        let document = ActiveDocument {
            language: language.to_owned(),
            uri,
            text,
            version,
            position: LspPosition {
                line: 0,
                character: 0,
            },
            selection: LspRange {
                start: LspPosition {
                    line: 0,
                    character: 0,
                },
                end: LspPosition {
                    line: 0,
                    character: 0,
                },
            },
        };
        let lease = self.leases.entry(document.uri.clone()).or_default().clone();
        let clients = self.clients.clone();
        let sender = self.events.clone();
        let root = workspace.to_path_buf();
        let task = self.jobs.spawn(async move {
            let client = match client(&clients, server, root, sender.clone()).await {
                Ok(client) => client,
                Err(error) => {
                    let message = format!("{}: {error:#}", document.language);
                    let _ = sender.send(lsp_error(message)).await;
                    return;
                }
            };
            let _ = document.synchronize(&client, &lease).await;
        });
        self.tasks.push(task);
    }

    /// Tells the document's running server, if any, that it was saved.
    pub fn document_saved(
        &mut self,
        path: &Path,
        text: String,
        version: i64,
        settings: &Settings,
        workspace: &Path,
    ) {
        let (Some(language), Some(uri)) = (language_id_for_path(path), path_to_uri(path)) else {
            return;
        };
        let (Some(lease), Some(server)) = (
            self.leases.get(&uri).cloned(),
            server_for(settings, language),
        ) else {
            return;
        };
        let clients = self.clients.clone();
        let root = workspace.to_path_buf();
        let language = language.to_owned();
        self.tasks.push(self.jobs.spawn(async move {
            if let Some(client) = clients.running(&server, &root).await
                && client
                    .synchronize_document(&lease, &uri, &language, version, &text)
                    .await
                    .is_ok()
            {
                let _ = client.did_save(&uri, &text).await;
            }
        }));
    }

    pub fn close_document(&mut self, path: &Path, settings: &Settings, workspace: &Path) {
        let (Some(language), Some(uri)) = (language_id_for_path(path), path_to_uri(path)) else {
            return;
        };
        let Some(lease) = self.leases.remove(&uri) else {
            return;
        };
        lease.invalidate();
        let Some(server) = server_for(settings, language) else {
            return;
        };
        let clients = self.clients.clone();
        let root = workspace.to_path_buf();
        self.tasks.push(self.jobs.spawn(async move {
            if let Some(client) = clients.running(&server, &root).await {
                let _ = client.close_document(&lease, &uri).await;
            }
        }));
    }

    pub fn close_all_documents(&mut self, settings: &Settings, workspace: &Path) {
        let paths: Vec<_> = self
            .leases
            .keys()
            .filter_map(|uri| uri_to_path(uri))
            .collect();
        for path in paths {
            self.close_document(&path, settings, workspace);
        }
        self.diagnostics.clear();
        self.outline.clear();
    }

    /// Asks the server for the active document's outline.
    pub fn refresh_outline(&mut self, ui: &mut Ui<'_>) {
        let Some(document) = active_document(ui) else {
            return;
        };
        let Some(server) = server_for(ui.settings, &document.language) else {
            return;
        };
        if !command_available(&server.command) {
            return;
        }
        let lease = self.leases.entry(document.uri.clone()).or_default().clone();
        let clients = self.clients.clone();
        let sender = self.events.clone();
        let root = ui.workspace.to_path_buf();
        let task = self.jobs.spawn(async move {
            let Ok(client) = client(&clients, server, root, sender.clone()).await else {
                return;
            };
            if document.synchronize(&client, &lease).await.is_err() {
                return;
            }
            if let Ok(value) = client.document_symbols(&document.uri).await {
                let items = parse_document_symbols(&value, &document.uri);
                let _ = sender
                    .send(AppEvent::Lsp(LspEvent::Outline {
                        uri: document.uri,
                        items,
                        version: document.version,
                    }))
                    .await;
            }
        });
        self.tasks.push(task);
    }

    pub fn goto_adjacent_diagnostic(&mut self, forward: bool, ui: &mut Ui<'_>) {
        let Some(editor) = ui.workbench.active_editor() else {
            return;
        };
        let Some(uri) = editor.path().and_then(path_to_uri) else {
            return;
        };
        let cursor = editor.document.cursor().position;
        let Some(diagnostics) = self.diagnostics.get(&uri) else {
            ui.status("No diagnostics for this file");
            return;
        };
        let mut ordered = diagnostics
            .iter()
            .map(|diagnostic| {
                (
                    diagnostic.range.start.line as usize,
                    diagnostic.range.start.character as usize,
                )
            })
            .collect::<Vec<_>>();
        ordered.sort_unstable();
        let target = if forward {
            ordered
                .into_iter()
                .find(|(line, column)| (*line, *column) > (cursor.line, cursor.column))
        } else {
            ordered
                .into_iter()
                .rev()
                .find(|(line, column)| (*line, *column) < (cursor.line, cursor.column))
        };
        if let Some((line, _)) = target
            && let Some(editor) = ui.workbench.active_editor_mut()
        {
            editor.goto_line(line + 1);
            ui.status(format!("Diagnostic at line {}", line + 1));
        }
    }

    pub fn show_diagnostics(&mut self, ui: &mut Ui<'_>) {
        let Some(editor) = ui.workbench.active_editor() else {
            ui.status("No active editor");
            return;
        };
        let Some(path) = editor.path() else {
            ui.status("No diagnostics for an untitled buffer");
            return;
        };
        let Some(uri) = path_to_uri(path) else {
            return;
        };
        let diagnostics = self.diagnostics.get(&uri).cloned().unwrap_or_default();
        if diagnostics.is_empty() {
            ui.status("No diagnostics for this file");
            return;
        }
        let text = diagnostics
            .iter()
            .map(|diagnostic| {
                format!(
                    "{}:{}  {}",
                    diagnostic.range.start.line + 1,
                    diagnostic.range.start.character + 1,
                    diagnostic.message
                )
            })
            .collect::<Vec<_>>()
            .join("\n");
        *ui.overlay = Some(modal::info("Diagnostics", &text));
    }

    pub fn apply_completion(&mut self, item: CompletionItem, ui: &mut Ui<'_>) {
        if !self.current(self.generation, ui) {
            ui.status("Completion expired; request it again");
            return;
        }
        let Some(editor) = ui.workbench.active_editor_mut() else {
            return;
        };
        let content = editor.document.text();
        let cursor = editor.document.cursor().position;
        let mut edits = item.additional_edits;
        let primary = if let Some(edit) = item.text_edit {
            edit
        } else {
            // LSP lines are u32; a document that long cannot take a completion.
            let Ok(lsp_line) = u32::try_from(cursor.line) else {
                return;
            };
            let line = editor.document.line(cursor.line).unwrap_or_default();
            let start = line
                .chars()
                .take(cursor.column)
                .collect::<Vec<_>>()
                .iter()
                .rposition(|c| !c.is_alphanumeric() && *c != '_')
                .map_or(0, |i| i + 1);
            let character = |column: usize| line.chars().take(column).map(utf16_units).sum();
            TextEdit {
                range: LspRange {
                    start: LspPosition {
                        line: lsp_line,
                        character: character(start),
                    },
                    end: LspPosition {
                        line: lsp_line,
                        character: character(cursor.column),
                    },
                },
                new_text: item.insert_text,
            }
        };

        edits.push(primary);
        let Some(updated) = apply_text_edits_to_content(&content, &edits) else {
            ui.status("Completion contains invalid or overlapping edits");
            return;
        };
        // Apply one undo group, retaining offsets of independently split views.
        let ranges = edits
            .iter()
            .map(|edit| {
                let convert = |p: LspPosition| {
                    Position::new(
                        p.line as usize,
                        utf16_column(
                            &editor.document.line(p.line as usize).unwrap_or_default(),
                            p.character,
                        ),
                    )
                };
                (
                    tide_core::Range::new(convert(edit.range.start), convert(edit.range.end)),
                    edit.new_text.clone(),
                )
            })
            .collect::<Vec<_>>();
        let primary_index = ranges.len() - 1;
        let Ok(cursors) = editor.document.apply_edits(&ranges, primary_index) else {
            ui.status("Completion contains invalid or overlapping edits");
            return;
        };
        if let Some(cursor) = cursors.get(primary_index) {
            editor.document.set_cursor(cursor.position);
        }
        debug_assert_eq!(editor.document.text(), updated);
        ui.mark_session_dirty();
    }
}

impl LspRequestKind {
    /// The request a typed key triggers once the debounce passes.
    pub fn typed(code: KeyCode, settings: &Settings) -> Option<Self> {
        let enabled = |key: &str| {
            settings
                .get(key)
                .and_then(serde_json::Value::as_bool)
                .unwrap_or(true)
        };
        let triggers = settings
            .get("lsp.triggerCharacters")
            .and_then(serde_json::Value::as_str)
            .unwrap_or(".:/<@(");
        match code {
            KeyCode::Char('(' | ',') if enabled("lsp.signatureHelp.enabled") => {
                Some(Self::SignatureHelp)
            }
            KeyCode::Char(c) if c.is_alphanumeric() || c == '_' || triggers.contains(c) => {
                Some(Self::Completion)
            }
            _ => None,
        }
    }

    /// Runs the request and turns its reply, or its failure, into an event.
    async fn run(self, client: &LspClient, input: RequestInput) -> LspEvent {
        let (generation, uri, position) = (input.generation, input.uri.as_str(), input.position);
        match self {
            Self::Completion => reply(
                "completion",
                client.completion(uri, position).await,
                |items| LspEvent::Completion { generation, items },
            ),
            Self::Hover => reply("hover", client.hover(uri, position).await, |text| {
                LspEvent::Hover { generation, text }
            }),
            Self::Definition => reply(
                "definition",
                client.definition(uri, position).await,
                |locations| LspEvent::Definition {
                    generation,
                    locations,
                },
            ),
            Self::References => reply(
                "references",
                client.references(uri, position).await,
                |locations| LspEvent::References {
                    generation,
                    locations,
                },
            ),
            Self::SignatureHelp => reply(
                "signature",
                client.signature_help(uri, position).await,
                |text| LspEvent::Signature { generation, text },
            ),
            Self::DocumentSymbols => {
                reply("symbols", client.document_symbols(uri).await, |value| {
                    let items = parse_document_symbols(&value, uri);
                    LspEvent::Symbols { generation, items }
                })
            }
            Self::WorkspaceSymbols(query) => reply(
                "workspace symbols",
                client.workspace_symbols(&query).await,
                |value| {
                    let items = parse_workspace_symbols(&value);
                    LspEvent::Symbols { generation, items }
                },
            ),
            Self::CodeActions => {
                let diagnostics = serde_json::to_value(&input.diagnostics)
                    .unwrap_or_else(|_| serde_json::json!([]));
                let point = LspRange {
                    start: position,
                    end: position,
                };
                reply(
                    "code actions",
                    client.code_actions(uri, point, diagnostics).await,
                    |actions| LspEvent::CodeActions {
                        generation,
                        actions,
                    },
                )
            }
            Self::ExecuteCommand(command) => {
                let name = command
                    .get("command")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or_default()
                    .to_owned();
                let arguments = command
                    .get("arguments")
                    .cloned()
                    .unwrap_or_else(|| serde_json::json!([]));
                let result = client.execute_command(&name, arguments).await;
                reply(&format!("execute {name}"), result, |_| {
                    LspEvent::CommandResult {
                        generation,
                        message: format!("LSP: executed {name}"),
                    }
                })
            }
            Self::Format => {
                let result = client
                    .formatting_with_options(uri, input.tab_size, input.insert_spaces)
                    .await;
                reply(
                    "format",
                    result,
                    |value| match tide_lsp::checked_text_edits(&value) {
                        Ok(edits) => LspEvent::Edits {
                            generation,
                            files: vec![LspFileEdits {
                                uri: uri.to_owned(),
                                edits,
                            }],
                        },
                        Err(error) => LspEvent::Error(format!("Formatting rejected: {error:#}")),
                    },
                )
            }
            Self::FormatSelection => {
                if !client.supports("documentRangeFormattingProvider") {
                    return LspEvent::Error(format!(
                        "the {} language server cannot format a selection; use Format Document",
                        input.language
                    ));
                }
                let result = client
                    .range_formatting_with_options(
                        uri,
                        input.selection,
                        input.tab_size,
                        input.insert_spaces,
                    )
                    .await;
                reply(
                    "format selection",
                    result,
                    |value| match tide_lsp::checked_text_edits(&value) {
                        Ok(edits) => LspEvent::Edits {
                            generation,
                            files: vec![LspFileEdits {
                                uri: uri.to_owned(),
                                edits,
                            }],
                        },
                        Err(error) => LspEvent::Error(format!("Formatting rejected: {error:#}")),
                    },
                )
            }
            Self::Rename(new_name) => {
                let result = client.rename(uri, position, &new_name).await;
                reply(
                    "rename",
                    result,
                    |value| match tide_lsp::checked_workspace_edit(&value) {
                        Ok(files) => LspEvent::Edits {
                            generation,
                            files: files
                                .into_iter()
                                .map(|(uri, edits)| LspFileEdits { uri, edits })
                                .collect(),
                        },
                        Err(error) => LspEvent::Error(format!("Rename rejected: {error:#}")),
                    },
                )
            }
        }
    }
}

/// The event for a reply, or an error labelled with the request that failed.
fn reply<T>(label: &str, result: Result<T>, event: impl FnOnce(T) -> LspEvent) -> LspEvent {
    match result {
        Ok(value) => event(value),
        Err(error) => LspEvent::Error(format!("{label}: {error:#}")),
    }
}

impl ActiveDocument {
    async fn synchronize(&self, client: &LspClient, lease: &DocumentLease) -> Result<()> {
        client
            .synchronize_document(lease, &self.uri, &self.language, self.version, &self.text)
            .await
    }
}

fn lsp_error(message: String) -> AppEvent {
    AppEvent::Lsp(LspEvent::Error(message))
}

fn enabled(settings: &Settings) -> bool {
    settings
        .get("lsp.enabled")
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(true)
}

pub(crate) fn active_document(ui: &Ui<'_>) -> Option<ActiveDocument> {
    if !enabled(ui.settings) {
        return None;
    }
    let editor = ui.workbench.active_editor()?;
    let path = editor.path()?;
    let cursor = editor.document.cursor();
    let lsp_position = |position: Position| {
        Some(LspPosition {
            // LSP lines are u32; there is no request position past that.
            line: u32::try_from(position.line).ok()?,
            character: editor
                .document
                .line(position.line)
                .unwrap_or_default()
                .chars()
                .take(position.column)
                .map(utf16_units)
                .sum(),
        })
    };
    let (start, end) = cursor
        .range()
        .filter(|range| !range.is_empty())
        .map_or_else(
            || {
                let line = cursor.position.line;
                (
                    Position::new(line, 0),
                    Position::new(line, editor.document.line_len_chars(line)),
                )
            },
            |range| (range.start, range.end),
        );
    Some(ActiveDocument {
        language: language_id_for_path(path)?.to_owned(),
        uri: path_to_uri(path)?,
        text: editor.document.text(),
        version: saturating_i64(editor.revision()),
        position: lsp_position(cursor.position)?,
        selection: LspRange {
            start: lsp_position(start)?,
            end: lsp_position(end)?,
        },
    })
}

/// The server for `language`: `lsp.servers.<language>` in settings
/// (`{"command": …, "args": […]}`, or `null` to disable it) overrides
/// the built-in default.
pub(crate) fn server_for(settings: &Settings, language: &str) -> Option<ServerConfig> {
    #[derive(serde::Deserialize)]
    struct Override {
        command: String,
        #[serde(default)]
        args: Vec<String>,
    }
    match settings
        .get("lsp.servers")
        .and_then(|servers| servers.get(language))
    {
        Some(serde_json::Value::Null) => None,
        Some(value) => match <Override as serde::Deserialize>::deserialize(value) {
            Ok(server) => Some(ServerConfig::new(language, server.command, server.args)),
            Err(error) => {
                tracing::warn!(%error, language, "invalid lsp.servers entry");
                server_for_language(language)
            }
        },
        None => server_for_language(language),
    }
}

fn status_key(server: &ServerConfig) -> (String, Vec<String>) {
    (server.command.clone(), server.args.clone())
}

fn anchor_overlay(ui: &mut Ui<'_>) {
    let point = ui.workbench.active_editor().and_then(|e| e.cursor_screen());
    if let Some(overlay) = ui.overlay.as_mut() {
        overlay.base_mut().anchor = point;
    }
}

async fn client(
    clients: &LspRegistry,
    server: ServerConfig,
    root: PathBuf,
    sender: Sender<AppEvent>,
) -> Result<Arc<LspClient>> {
    clients
        .get_or_start(&server, &root, || forward_server_events(sender))
        .await
}

fn forward_server_events(app_tx: Sender<AppEvent>) -> Sender<ServerEvent> {
    let (lsp_tx, mut lsp_rx) = tokio::sync::mpsc::channel(64);
    tokio::spawn(async move {
        while let Some(event) = lsp_rx.recv().await {
            let event = match event {
                ServerEvent::ApplyEdit {
                    edit,
                    versions,
                    reply,
                } => LspEvent::ServerEdit {
                    edit,
                    versions,
                    reply,
                },
                ServerEvent::Diagnostics {
                    uri,
                    diagnostics,
                    version,
                } => LspEvent::Diagnostics {
                    uri,
                    diagnostics,
                    version,
                },
                ServerEvent::Status {
                    language_id,
                    message,
                } => LspEvent::Status {
                    language_id,
                    message,
                },
            };
            if app_tx.send(AppEvent::Lsp(event)).await.is_err() {
                break;
            }
        }
    });
    lsp_tx
}

/// Applies server edits across files: closed files are rewritten first and restored if
/// any write fails, then open buffers change in one undo step each.
pub(crate) fn apply_edits(files: Vec<LspFileEdits>, ui: &mut Ui<'_>) -> bool {
    if files.is_empty() {
        return true;
    }
    let Ok(workspace) = Workspace::new(ui.workspace) else {
        ui.status("LSP: workspace is unavailable");
        return false;
    };
    let mut open_writes = Vec::new();
    let mut seen = HashSet::new();
    let mut closed_writes: Vec<(PathBuf, String, String)> = Vec::new();
    for file in &files {
        let Some(path) = uri_to_path(&file.uri) else {
            ui.status(format!("LSP: unsupported URI {}", file.uri));
            return false;
        };
        let Ok(path) = workspace.resolve(&path.display().to_string()) else {
            ui.status(format!("LSP: edit outside workspace: {}", path.display()));
            return false;
        };
        if !seen.insert(path.clone()) {
            ui.status("LSP: duplicate file edit groups are unsupported");
            return false;
        }
        if let Some(handle) = ui.documents.get(&path) {
            if apply_text_edits_to_content(&handle.text(), &file.edits).is_none() {
                ui.status("LSP: invalid open-buffer edit");
                return false;
            }
            if externally_changed(ui.workbench, &path) {
                ui.status("LSP: target changed on disk; reload before applying edits");
                return false;
            }
            let ranges = file
                .edits
                .iter()
                .map(|edit| {
                    let convert = |position: LspPosition| {
                        Position::new(
                            position.line as usize,
                            utf16_column(
                                &handle.line(position.line as usize).unwrap_or_default(),
                                position.character,
                            ),
                        )
                    };
                    (
                        tide_core::Range::new(convert(edit.range.start), convert(edit.range.end)),
                        edit.new_text.clone(),
                    )
                })
                .collect::<Vec<_>>();
            open_writes.push((handle, ranges));
            continue;
        }
        if !path.is_file() {
            ui.status(format!(
                "LSP: refusing to create missing file {}",
                path.display()
            ));
            return false;
        }
        let Ok(original) = std::fs::read_to_string(&path) else {
            ui.status(format!("LSP: cannot read {}", path.display()));
            return false;
        };
        let Some(updated) = apply_text_edits_to_content(&original, &file.edits) else {
            ui.status(format!("LSP: invalid edit for {}", path.display()));
            return false;
        };
        closed_writes.push((path, original, updated));
    }

    // Stage closed files before mutating open buffers. If one write fails,
    // restore the files already written and abort the whole operation.
    let write = |path: &Path, content: &str| crate::editor::write_atomic(path, content.as_bytes());
    let mut written: Vec<(PathBuf, String)> = Vec::new();
    for (path, original, updated) in &closed_writes {
        if std::fs::read_to_string(path).ok().as_deref() != Some(original.as_str())
            || write(path, updated).is_err()
        {
            for (done, before) in written.iter().rev() {
                let _ = write(done, before);
            }
            ui.status(format!("LSP: failed to write {}", path.display()));
            return false;
        }
        written.push((path.clone(), original.clone()));
    }

    let mut applied = closed_writes.len();
    for (handle, edits) in open_writes {
        if handle.apply_edits_preserving_cursor(&edits).is_ok() {
            applied += 1;
        }
    }
    if applied > 0 {
        ui.mark_session_dirty();
        ui.status(format!("Applied LSP edits to {applied} file(s)"));
    } else {
        ui.status("LSP: no edits to apply");
    }
    applied > 0
}

fn externally_changed(workbench: &Workbench, path: &Path) -> bool {
    workbench
        .pane_ids()
        .into_iter()
        .filter_map(|id| workbench.pane(id))
        .flat_map(|pane| &pane.tabs)
        .any(|tab| match tab {
            Tab::Editor(editor) if editor.path() == Some(path) => editor.is_externally_changed(),
            _ => false,
        })
}

fn apply_text_edits_to_content(content: &str, edits: &[TextEdit]) -> Option<String> {
    let mut ranges = edits
        .iter()
        .map(|edit| (edit.range.start, edit.range.end))
        .collect::<Vec<_>>();
    ranges.sort_by_key(|(start, _)| (start.line, start.character));
    for (index, (start, end)) in ranges.iter().enumerate() {
        if (start.line, start.character) > (end.line, end.character) {
            return None;
        }
        for position in [start, end] {
            let line = content
                .split('\n')
                .nth(position.line as usize)?
                .trim_end_matches('\r');
            let mut boundaries = vec![0];
            let mut offset = 0;
            for ch in line.chars() {
                offset += utf16_units(ch);
                boundaries.push(offset);
            }
            if !boundaries.contains(&position.character) {
                return None;
            }
        }
        if index > 0
            && (ranges[index - 1].1.line, ranges[index - 1].1.character)
                > (start.line, start.character)
        {
            return None;
        }
    }
    let mut document = Document::new(content);
    let mut ordered = edits.to_vec();
    ordered.sort_by(|left, right| {
        (right.range.start.line, right.range.start.character)
            .cmp(&(left.range.start.line, left.range.start.character))
    });
    for edit in ordered {
        let start = Position::new(
            edit.range.start.line as usize,
            utf16_column(
                &document.line(edit.range.start.line as usize)?,
                edit.range.start.character,
            ),
        );
        let end = Position::new(
            edit.range.end.line as usize,
            utf16_column(
                &document.line(edit.range.end.line as usize)?,
                edit.range.end.character,
            ),
        );
        document.replace(tide_core::Range::new(start, end), &edit.new_text);
    }
    Some(document.text())
}

pub(crate) fn utf16_column(line: &str, utf16_offset: u32) -> usize {
    let mut units = 0_u32;
    for (index, character) in line.chars().enumerate() {
        if units >= utf16_offset {
            return index;
        }
        units += utf16_units(character);
    }
    line.chars().count()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn utf16_conversion_handles_wide_characters() {
        assert_eq!(utf16_column("a😀b", 0), 0);
        assert_eq!(utf16_column("a😀b", 1), 1);
        assert_eq!(utf16_column("a😀b", 3), 2);
        assert_eq!(utf16_column("a😀b", 4), 3);
    }
}
