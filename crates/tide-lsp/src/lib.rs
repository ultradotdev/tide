//! Minimal, production-shaped LSP client for Tide.
//!
//! The client speaks JSON-RPC over stdio, keeps pending request IDs, routes
//! diagnostics notifications, and exposes typed helpers for the editor.

use std::collections::{HashMap, VecDeque};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::atomic::{AtomicBool, AtomicI64, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStderr, Command};
use tokio::sync::{Mutex, Notify, mpsc, oneshot};
use tokio::task::JoinHandle;

/// Why a language-server operation failed.
#[derive(Debug, thiserror::Error)]
pub enum LspError {
    #[error("start {command}: {error}")]
    Spawn {
        command: String,
        error: std::io::Error,
    },
    #[error("LSP {0}")]
    Pipe(&'static str),
    #[error("workspace URI")]
    WorkspaceUri,
    #[error("LSP writer closed")]
    WriterClosed,
    #[error("{method}: language server exited")]
    Exited { method: String },
    #[error("{method}: request channel closed")]
    Cancelled { method: String },
    #[error("{method}: timeout")]
    Timeout { method: String },
    /// The server answered with a JSON-RPC error.
    #[error("{method}: {message}")]
    Server { method: String, message: String },
    #[error("server selected unsupported position encoding {0}")]
    UnsupportedEncoding(String),
    #[error("Refusing an out-of-order document revision")]
    OutOfOrder,
    #[error("Document was closed")]
    DocumentClosed,
    #[error("{command} stopped unexpectedly; retrying in {retry_ms}ms")]
    Backoff { command: String, retry_ms: u128 },
    /// Initialization failed; `stderr` holds the server's last lines.
    #[error("{command} stderr:\n{stderr}: {error}")]
    Initialize {
        command: String,
        stderr: String,
        error: Box<LspError>,
    },
    #[error("language server start task failed: {0}")]
    Task(String),
    #[error("{0}")]
    MalformedEdit(&'static str),
}

impl LspError {
    /// The underlying failure, looking through initialization context.
    #[must_use]
    pub fn cause(&self) -> &Self {
        match self {
            Self::Initialize { error, .. } => error.cause(),
            error => error,
        }
    }
}

pub type Result<T, E = LspError> = std::result::Result<T, E>;

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ServerConfig {
    pub language_id: String,
    pub command: String,
    #[serde(default)]
    pub args: Vec<String>,
}

impl ServerConfig {
    pub fn new<I, S>(language_id: impl Into<String>, command: impl Into<String>, args: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        Self {
            language_id: language_id.into(),
            command: command.into(),
            args: args.into_iter().map(Into::into).collect(),
        }
    }
}

const DEFAULT_SERVERS: &[(&str, &str, &[&str])] = &[
    ("typescript", "typescript-language-server", &["--stdio"]),
    ("javascript", "typescript-language-server", &["--stdio"]),
    (
        "typescriptreact",
        "typescript-language-server",
        &["--stdio"],
    ),
    (
        "javascriptreact",
        "typescript-language-server",
        &["--stdio"],
    ),
    ("rust", "rust-analyzer", &[]),
    ("python", "pylsp", &[]),
    ("go", "gopls", &[]),
    ("ruby", "solargraph", &["stdio"]),
    ("c", "clangd", &[]),
    ("cpp", "clangd", &[]),
    ("json", "vscode-json-language-server", &["--stdio"]),
    ("html", "vscode-html-language-server", &["--stdio"]),
    ("css", "vscode-css-language-server", &["--stdio"]),
    ("sql", "postgres-language-server", &["lsp-proxy"]),
];

#[must_use]
pub fn default_servers() -> Vec<ServerConfig> {
    DEFAULT_SERVERS
        .iter()
        .map(|(language, command, args)| {
            ServerConfig::new(*language, *command, args.iter().copied())
        })
        .collect()
}

#[must_use]
pub fn language_id_for_path(path: &Path) -> Option<&'static str> {
    let extension = path.extension()?.to_str()?.to_ascii_lowercase();
    Some(match extension.as_str() {
        "ts" => "typescript",
        "tsx" => "typescriptreact",
        "js" | "mjs" | "cjs" => "javascript",
        "jsx" => "javascriptreact",
        "rs" => "rust",
        "py" => "python",
        "go" => "go",
        "rb" => "ruby",
        "c" | "h" => "c",
        "cpp" | "cc" | "cxx" | "hpp" | "hxx" => "cpp",
        "json" => "json",
        "html" | "htm" => "html",
        "css" | "scss" | "less" => "css",
        "sql" | "pgsql" | "psql" => "sql",
        _ => return None,
    })
}

#[must_use]
pub fn server_for_language(language_id: &str) -> Option<ServerConfig> {
    DEFAULT_SERVERS
        .iter()
        .find(|(language, _, _)| *language == language_id)
        .map(|(language, command, args)| {
            ServerConfig::new(*language, *command, args.iter().copied())
        })
}

#[must_use]
pub fn command_available(command: &str) -> bool {
    let Some(path) = std::env::var_os("PATH") else {
        return false;
    };
    std::env::split_paths(&path).any(|directory| {
        let candidate = directory.join(command);
        candidate.is_file()
    })
}

#[must_use]
pub fn path_to_uri(path: &Path) -> Option<String> {
    url::Url::from_file_path(path)
        .ok()
        .map(|url| url.to_string())
}

#[must_use]
pub fn uri_to_path(uri: &str) -> Option<PathBuf> {
    url::Url::parse(uri).ok()?.to_file_path().ok()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Position {
    pub line: u32,
    pub character: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Range {
    pub start: Position,
    pub end: Position,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Diagnostic {
    pub range: Range,
    #[serde(default)]
    pub severity: Option<u8>,
    pub message: String,
    #[serde(default)]
    pub source: Option<String>,
    #[serde(default)]
    pub code: Option<Value>,
}

#[derive(Debug, Clone, Deserialize)]
struct DiagnosticsParams {
    #[serde(default)]
    version: Option<i64>,
    uri: String,
    #[serde(default)]
    diagnostics: Vec<Diagnostic>,
}

#[derive(Debug)]
pub enum LspEvent {
    ApplyEdit {
        edit: Value,
        versions: HashMap<String, i64>,
        reply: oneshot::Sender<bool>,
    },
    Diagnostics {
        version: Option<i64>,
        uri: String,
        diagnostics: Vec<Diagnostic>,
    },
    Status {
        language_id: String,
        message: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompletionItem {
    pub text_edit: Option<TextEdit>,
    pub additional_edits: Vec<TextEdit>,
    pub label: String,
    pub detail: Option<String>,
    pub insert_text: String,
    pub kind: Option<u64>,
}

#[derive(Debug, Clone)]
pub struct Location {
    pub uri: String,
    pub range: Range,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TextEdit {
    pub range: Range,
    pub new_text: String,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CodeActionItem {
    pub title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub edit: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub command: Option<Value>,
    #[serde(default)]
    pub is_preferred: bool,
}

#[derive(Debug, Clone)]
pub struct SymbolItem {
    pub name: String,
    pub detail: String,
    pub uri: Option<String>,
    pub line: u32,
    pub depth: usize,
}

/// Identifies one editor lifetime. Invalidating it synchronously prevents late
/// async work from reopening a closed document or closing its replacement.
#[derive(Clone, Debug)]
pub struct DocumentLease {
    id: i64,
    valid: Arc<AtomicBool>,
}

impl Default for DocumentLease {
    fn default() -> Self {
        static NEXT: AtomicI64 = AtomicI64::new(1);
        Self {
            id: NEXT.fetch_add(1, Ordering::Relaxed),
            valid: Arc::new(AtomicBool::new(true)),
        }
    }
}

impl DocumentLease {
    pub fn invalidate(&self) {
        self.valid.store(false, Ordering::Release);
    }
    pub fn is_valid(&self) -> bool {
        self.valid.load(Ordering::Acquire)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SyncKind {
    None,
    Full,
    Incremental,
}

impl SyncKind {
    fn from_lsp(kind: Option<u64>) -> Self {
        match kind {
            Some(0) => Self::None,
            Some(2) => Self::Incremental,
            _ => Self::Full,
        }
    }
}

/// The server's `textDocumentSync` capability.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TextDocumentSync {
    pub change: SyncKind,
    pub save: bool,
    pub save_include_text: bool,
}

impl TextDocumentSync {
    /// Servers that omit the capability keep receiving full-document sync.
    pub const FULL: Self = Self {
        change: SyncKind::Full,
        save: false,
        save_include_text: false,
    };

    #[must_use]
    pub fn from_capabilities(capabilities: &Value) -> Self {
        match capabilities.get("textDocumentSync") {
            None | Some(Value::Null) => Self::FULL,
            Some(Value::Number(kind)) => Self {
                change: SyncKind::from_lsp(kind.as_u64()),
                save: true,
                save_include_text: false,
            },
            Some(options) => {
                let save = options.get("save");
                Self {
                    change: options
                        .get("change")
                        .map_or(SyncKind::None, |kind| SyncKind::from_lsp(kind.as_u64())),
                    save: save.is_some_and(|save| save.as_bool().unwrap_or(save.is_object())),
                    save_include_text: save
                        .and_then(|save| save.get("includeText"))
                        .and_then(Value::as_bool)
                        .unwrap_or(false),
                }
            }
        }
    }
}

#[derive(Clone)]
struct OpenDocument {
    version: i64,
    owner: i64,
    text: Option<Arc<str>>,
}

type PendingMap = Arc<std::sync::Mutex<HashMap<i64, oneshot::Sender<Result<Value, Reply>>>>>;

/// Why a pending request has no result.
enum Reply {
    Error(String),
    Exited,
}
type OpenMap = Arc<std::sync::Mutex<HashMap<String, OpenDocument>>>;
type StderrTail = Arc<std::sync::Mutex<VecDeque<String>>>;

const MAX_QUEUED_REPLY_BYTES: usize = 8 * 1024 * 1024;
const STDERR_TAIL_LINES: usize = 200;
const STDERR_LINE_BYTES: u64 = 4096;

/// Replies to server requests bypass the bounded writer queue so the reader
/// never waits on a server that is not reading its input.
#[derive(Clone)]
struct Replies {
    tx: mpsc::UnboundedSender<Vec<u8>>,
    queued: Arc<AtomicUsize>,
}

impl Replies {
    fn send(&self, value: Value) {
        let bytes = encode_message(value);
        let length = bytes.len();
        if self.queued.fetch_add(length, Ordering::AcqRel) + length > MAX_QUEUED_REPLY_BYTES {
            self.queued.fetch_sub(length, Ordering::AcqRel);
            tracing::warn!("dropping a reply to a language server that is not reading its input");
            return;
        }
        if self.tx.send(bytes).is_err() {
            self.queued.fetch_sub(length, Ordering::AcqRel);
        }
    }
}

/// Latest diagnostics per URI, delivered without blocking the reader.
#[derive(Default)]
struct DiagnosticsQueue {
    pending: std::sync::Mutex<HashMap<String, (u64, DiagnosticsParams)>>,
    next: AtomicU64,
    ready: Notify,
}

impl DiagnosticsQueue {
    fn push(&self, params: DiagnosticsParams) {
        let order = self.next.fetch_add(1, Ordering::Relaxed);
        self.pending
            .lock()
            .unwrap()
            .insert(params.uri.clone(), (order, params));
        self.ready.notify_one();
    }

    fn take(&self) -> Vec<DiagnosticsParams> {
        let mut batch: Vec<_> = self
            .pending
            .lock()
            .unwrap()
            .drain()
            .map(|(_, entry)| entry)
            .collect();
        batch.sort_by_key(|(order, _)| *order);
        batch.into_iter().map(|(_, params)| params).collect()
    }
}

struct ServerRequests {
    replies: Replies,
    events: mpsc::Sender<LspEvent>,
    opened: OpenMap,
    root_uri: String,
    slots: Arc<tokio::sync::Semaphore>,
}

impl ServerRequests {
    fn answer(&self, id: Value, method: &str, params: &Value) {
        let result = match method {
            "workspace/applyEdit" => return self.apply_edit(id, params),
            "workspace/configuration" => json!(
                params
                    .get("items")
                    .and_then(Value::as_array)
                    .map(|items| vec![Value::Null; items.len().min(1000)])
                    .unwrap_or_default()
            ),
            "workspace/workspaceFolders" => json!([{"uri":self.root_uri,"name":"workspace"}]),
            "window/workDoneProgress/create" | "window/showMessageRequest" => Value::Null,
            _ => {
                return self.replies.send(json!({"jsonrpc":"2.0","id":id,"error":{"code":-32601,"message":"Unsupported server request"}}));
            }
        };
        self.replies
            .send(json!({"jsonrpc":"2.0","id":id,"result":result}));
    }

    fn apply_edit(&self, id: Value, params: &Value) {
        let Ok(permit) = self.slots.clone().try_acquire_owned() else {
            return self.replies.send(json!({"jsonrpc":"2.0","id":id,"result":{"applied":false,"failureReason":"Too many server requests"}}));
        };
        let (reply, rx) = oneshot::channel();
        let versions = self
            .opened
            .lock()
            .unwrap()
            .iter()
            .map(|(uri, document)| (uri.clone(), document.version))
            .collect();
        let event = LspEvent::ApplyEdit {
            edit: params.get("edit").cloned().unwrap_or(Value::Null),
            versions,
            reply,
        };
        if self.events.try_send(event).is_err() {
            return self.replies.send(json!({"jsonrpc":"2.0","id":id,"result":{"applied":false,"failureReason":"Client is busy"}}));
        }
        let replies = self.replies.clone();
        tokio::spawn(async move {
            let _permit = permit;
            let applied = matches!(
                tokio::time::timeout(Duration::from_secs(10), rx).await,
                Ok(Ok(true))
            );
            let result = if applied {
                json!({"applied":true})
            } else {
                json!({"applied":false,"failureReason":"Edit was rejected, stale, unsupported, cancelled or timed out"})
            };
            replies.send(json!({"jsonrpc":"2.0","id":id,"result":result}));
        });
    }
}

pub struct LspClient {
    process: Mutex<Option<Child>>,
    writer: mpsc::Sender<Vec<u8>>,
    pending: PendingMap,
    next_id: AtomicI64,
    opened: OpenMap,
    /// Serializes document notifications; the reader never takes it.
    sync_gate: Mutex<()>,
    sync: OnceLock<TextDocumentSync>,
    reader_alive: Arc<AtomicBool>,
    stderr: StderrTail,
    event_tx: mpsc::Sender<LspEvent>,
    language_id: String,
    command: String,
    root_uri: String,
    tasks: std::sync::Mutex<Vec<JoinHandle<()>>>,
}

impl LspClient {
    pub async fn start(
        config: ServerConfig,
        root: &Path,
        event_tx: mpsc::Sender<LspEvent>,
    ) -> Result<Self> {
        let mut command = Command::new(&config.command);
        tide_core::process::scrub_tide_secrets(command.as_std_mut());
        #[cfg(unix)]
        command.process_group(0);
        let mut child = command
            .args(&config.args)
            .current_dir(root)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true)
            .spawn()
            .map_err(|error| LspError::Spawn {
                command: config.command.clone(),
                error,
            })?;
        let stdin = child.stdin.take().ok_or(LspError::Pipe("stdin"))?;
        let stdout = child.stdout.take().ok_or(LspError::Pipe("stdout"))?;
        let stderr = child.stderr.take().ok_or(LspError::Pipe("stderr"))?;
        let root_uri = path_to_uri(root).ok_or(LspError::WorkspaceUri)?;
        let stderr_tail = StderrTail::default();
        let stderr_task = forward_stderr(stderr, config.command.clone(), stderr_tail.clone());

        let (writer_tx, mut writer_rx) = mpsc::channel::<Vec<u8>>(64);
        let (reply_tx, mut reply_rx) = mpsc::unbounded_channel::<Vec<u8>>();
        let replies = Replies {
            tx: reply_tx,
            queued: Arc::default(),
        };
        let queued = replies.queued.clone();
        let writer_task = tokio::spawn(async move {
            let mut stdin = stdin;
            loop {
                let bytes = tokio::select! {
                    biased;
                    Some(bytes) = reply_rx.recv() => {
                        queued.fetch_sub(bytes.len(), Ordering::AcqRel);
                        bytes
                    }
                    Some(bytes) = writer_rx.recv() => bytes,
                    else => break,
                };
                if stdin.write_all(&bytes).await.is_err() {
                    break;
                }
                let _ = stdin.flush().await;
            }
        });

        let diagnostics = Arc::new(DiagnosticsQueue::default());
        let diagnostics_task = {
            let diagnostics = diagnostics.clone();
            let events = event_tx.clone();
            tokio::spawn(async move {
                loop {
                    diagnostics.ready.notified().await;
                    for params in diagnostics.take() {
                        let event = LspEvent::Diagnostics {
                            uri: params.uri,
                            diagnostics: params.diagnostics,
                            version: params.version,
                        };
                        if events.send(event).await.is_err() {
                            return;
                        }
                    }
                }
            })
        };

        let pending = PendingMap::default();
        let opened = OpenMap::default();
        let reader_alive = Arc::new(AtomicBool::new(true));
        let requests = ServerRequests {
            replies,
            events: event_tx.clone(),
            opened: opened.clone(),
            root_uri: root_uri.clone(),
            slots: Arc::new(tokio::sync::Semaphore::new(8)),
        };
        let reader_task = {
            let pending = pending.clone();
            let reader_alive = reader_alive.clone();
            tokio::spawn(async move {
                let mut reader = BufReader::new(stdout);
                while let Some(body) = read_message(&mut reader).await {
                    let Ok(value) = serde_json::from_slice::<Value>(&body) else {
                        continue;
                    };
                    // Requests may reuse a client's numeric ID or use a string ID.
                    // A method takes precedence over the response correlation table.
                    if let (Some(id), Some(method)) =
                        (value.get("id"), value.get("method").and_then(Value::as_str))
                    {
                        let params = value.get("params").unwrap_or(&Value::Null);
                        requests.answer(id.clone(), method, params);
                        continue;
                    }
                    if let Some(id) = value.get("id").and_then(Value::as_i64) {
                        if let Some(sender) = pending.lock().unwrap().remove(&id) {
                            if let Some(error) = value.get("error") {
                                let _ = sender.send(Err(Reply::Error(error.to_string())));
                            } else {
                                let _ = sender
                                    .send(Ok(value.get("result").cloned().unwrap_or(Value::Null)));
                            }
                        }
                        continue;
                    }
                    if value.get("method").and_then(Value::as_str)
                        == Some("textDocument/publishDiagnostics")
                        && let Some(params) = value.get("params")
                        && let Ok(params) =
                            serde_json::from_value::<DiagnosticsParams>(params.clone())
                    {
                        diagnostics.push(params);
                    }
                }
                reader_alive.store(false, Ordering::SeqCst);
                for (_, sender) in pending.lock().unwrap().drain() {
                    let _ = sender.send(Err(Reply::Exited));
                }
            })
        };

        let client = Self {
            process: Mutex::new(Some(child)),
            writer: writer_tx,
            pending,
            next_id: AtomicI64::new(1),
            opened,
            sync_gate: Mutex::new(()),
            sync: OnceLock::new(),
            reader_alive,
            stderr: stderr_tail,
            event_tx,
            language_id: config.language_id,
            command: config.command,
            root_uri,
            tasks: std::sync::Mutex::new(vec![
                reader_task,
                writer_task,
                diagnostics_task,
                stderr_task,
            ]),
        };
        let _ = client
            .event_tx
            .send(LspEvent::Status {
                language_id: client.language_id.clone(),
                message: "starting".to_owned(),
            })
            .await;
        if let Err(error) = client.initialize().await {
            let tail = client.stderr_tail();
            let tail = tail[tail.len().saturating_sub(5)..].join("\n");
            return Err(if tail.is_empty() {
                error
            } else {
                LspError::Initialize {
                    command: client.command.clone(),
                    stderr: tail,
                    error: Box::new(error),
                }
            });
        }
        let _ = client
            .event_tx
            .send(LspEvent::Status {
                language_id: client.language_id.clone(),
                message: "ready".to_owned(),
            })
            .await;
        Ok(client)
    }

    async fn initialize(&self) -> Result<()> {
        let mut params = json!({
            "processId": std::process::id(),
            "rootUri": self.root_uri,
            "capabilities": {
                "textDocument": {
                    "synchronization": { "didSave": true, "dynamicRegistration": false },
                    "completion": { "completionItem": { "snippetSupport": false } },
                    "hover": { "contentFormat": ["plaintext", "markdown"] },
                    "definition": {},
                    "documentSymbol": {},
                    "references": {},
                    "rename": {},
                    "publishDiagnostics": { "relatedInformation": true }
                },
                "workspace": { "workspaceFolders": true, "symbol": {}, "applyEdit":true, "workspaceEdit":{"documentChanges":true,"failureHandling":"transactional"} },
                "general": { "positionEncodings": ["utf-16"] }
            },
            "workspaceFolders": [{
                "uri": self.root_uri,
                "name": "workspace"
            }]
        });
        if self.language_id.starts_with("typescript")
            || self.language_id.starts_with("javascript")
            || self.command == "typescript-language-server"
        {
            let mut options = json!({ "disableAutomaticTypingAcquisition": true });
            let lookup = tokio::task::spawn_blocking(typescript_tsserver_path);
            if let Ok(Ok(Some(path))) = tokio::time::timeout(Duration::from_secs(5), lookup).await {
                options["tsserver"] = json!({ "path": path });
            }
            params["initializationOptions"] = options;
        }
        let result = self.request("initialize", params).await?;
        let encoding = result
            .pointer("/capabilities/positionEncoding")
            .and_then(Value::as_str)
            .unwrap_or("utf-16");
        if encoding != "utf-16" {
            return Err(LspError::UnsupportedEncoding(encoding.to_owned()));
        }
        let _ = self.sync.set(TextDocumentSync::from_capabilities(
            result.get("capabilities").unwrap_or(&Value::Null),
        ));
        self.notify("initialized", json!({})).await?;
        Ok(())
    }

    async fn send(&self, value: Value) -> Result<()> {
        self.writer
            .send(encode_message(value))
            .await
            .map_err(|_| LspError::WriterClosed)
    }

    async fn request(&self, method: &str, params: Value) -> Result<Value> {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let (tx, rx) = oneshot::channel();
        self.pending.lock().unwrap().insert(id, tx);
        let _guard = PendingRequest {
            pending: self.pending.clone(),
            writer: self.writer.clone(),
            id,
        };
        let method = method.to_owned();
        if !self.reader_alive.load(Ordering::SeqCst) {
            return Err(LspError::Exited { method });
        }
        self.send(json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params }))
            .await?;
        match tokio::time::timeout(Duration::from_secs(30), rx).await {
            Ok(Ok(Ok(value))) => Ok(value),
            Ok(Ok(Err(Reply::Error(message)))) => Err(LspError::Server { method, message }),
            Ok(Ok(Err(Reply::Exited))) => Err(LspError::Exited { method }),
            Ok(Err(_)) => Err(LspError::Cancelled { method }),
            Err(_) => Err(LspError::Timeout { method }),
        }
    }

    async fn notify(&self, method: &str, params: Value) -> Result<()> {
        self.send(json!({ "jsonrpc": "2.0", "method": method, "params": params }))
            .await
    }

    /// The document synchronization the server negotiated during initialize.
    #[must_use]
    pub fn text_document_sync(&self) -> TextDocumentSync {
        self.sync.get().copied().unwrap_or(TextDocumentSync::FULL)
    }

    /// The most recent lines the server wrote to stderr.
    #[must_use]
    pub fn stderr_tail(&self) -> Vec<String> {
        self.stderr.lock().unwrap().iter().cloned().collect()
    }

    fn document(&self, uri: &str) -> Option<OpenDocument> {
        self.opened.lock().unwrap().get(uri).cloned()
    }

    fn remember(&self, uri: &str, version: i64, owner: i64, text: &str) {
        let text = (self.text_document_sync().change == SyncKind::Incremental).then(|| text.into());
        self.opened.lock().unwrap().insert(
            uri.into(),
            OpenDocument {
                version,
                owner,
                text,
            },
        );
    }

    async fn send_open(
        &self,
        uri: &str,
        language_id: &str,
        version: i64,
        text: &str,
    ) -> Result<()> {
        self.notify("textDocument/didOpen", json!({"textDocument":{"uri":uri,"languageId":language_id,"version":version,"text":text}})).await
    }

    async fn send_change(
        &self,
        uri: &str,
        previous: &OpenDocument,
        version: i64,
        text: &str,
    ) -> Result<()> {
        let change = match (self.text_document_sync().change, previous.text.as_deref()) {
            (SyncKind::None, _) => return Ok(()),
            (SyncKind::Incremental, Some(old)) => incremental_change(old, text),
            _ => json!({ "text": text }),
        };
        self.notify(
            "textDocument/didChange",
            json!({"textDocument":{"uri":uri,"version":version},"contentChanges":[change]}),
        )
        .await
    }

    async fn send_close(&self, uri: &str) -> Result<()> {
        self.notify("textDocument/didClose", json!({"textDocument":{"uri":uri}}))
            .await?;
        self.opened.lock().unwrap().remove(uri);
        Ok(())
    }

    pub async fn did_open(
        &self,
        uri: &str,
        language_id: &str,
        version: i64,
        text: &str,
    ) -> Result<()> {
        let _gate = self.sync_gate.lock().await;
        if self.document(uri).is_some() {
            return Ok(());
        }
        self.send_open(uri, language_id, version, text).await?;
        self.remember(uri, version, 0, text);
        Ok(())
    }

    /// Synchronize even when a request wins the race with the app's open task.
    pub async fn did_change(&self, uri: &str, version: i64, text: &str) -> Result<()> {
        let _gate = self.sync_gate.lock().await;
        match self.document(uri) {
            Some(current) if current.version > version => return Err(LspError::OutOfOrder),
            Some(current) if current.version == version => return Ok(()),
            Some(current) => self.send_change(uri, &current, version, text).await?,
            None => {
                self.send_open(uri, &self.language_id, version, text)
                    .await?;
            }
        }
        self.remember(uri, version, 0, text);
        Ok(())
    }

    /// Synchronize a live editor, even after the same URI reopens at revision 1.
    pub async fn synchronize_document(
        &self,
        lease: &DocumentLease,
        uri: &str,
        language_id: &str,
        version: i64,
        text: &str,
    ) -> Result<()> {
        let _gate = self.sync_gate.lock().await;
        if !lease.is_valid() {
            return Err(LspError::DocumentClosed);
        }
        match self.document(uri) {
            Some(current) if current.owner == lease.id => {
                if current.version > version {
                    return Err(LspError::OutOfOrder);
                }
                if current.version == version {
                    return Ok(());
                }
                self.send_change(uri, &current, version, text).await?;
            }
            Some(_) => {
                self.send_close(uri).await?;
                self.send_open(uri, language_id, version, text).await?;
            }
            None => self.send_open(uri, language_id, version, text).await?,
        }
        self.remember(uri, version, lease.id, text);
        Ok(())
    }

    pub async fn close_document(&self, lease: &DocumentLease, uri: &str) -> Result<()> {
        let _gate = self.sync_gate.lock().await;
        if self
            .document(uri)
            .is_some_and(|current| current.owner == lease.id)
        {
            self.send_close(uri).await?;
        }
        Ok(())
    }

    /// Notify the server that `uri` was written to disk with `text`. This is a
    /// no-op unless the server asked for save notifications.
    pub async fn did_save(&self, uri: &str, text: &str) -> Result<()> {
        let sync = self.text_document_sync();
        if !sync.save {
            return Ok(());
        }
        let mut params = json!({ "textDocument": { "uri": uri } });
        if sync.save_include_text {
            params["text"] = json!(text);
        }
        let _gate = self.sync_gate.lock().await;
        self.notify("textDocument/didSave", params).await
    }

    pub async fn did_close(&self, uri: &str) -> Result<()> {
        let _gate = self.sync_gate.lock().await;
        self.send_close(uri).await
    }

    pub async fn is_alive(&self) -> bool {
        if !self.reader_alive.load(Ordering::SeqCst) || self.writer.is_closed() {
            return false;
        }
        let guard = self.process.lock().await;
        let Some(pid) = guard
            .as_ref()
            .and_then(|child| child.id())
            .and_then(tide_core::process::raw_pid)
            .and_then(rustix::process::Pid::from_raw)
        else {
            return false;
        };
        matches!(
            rustix::process::waitid(
                rustix::process::WaitId::Pid(pid),
                rustix::process::WaitIdOptions::EXITED
                    | rustix::process::WaitIdOptions::NOHANG
                    | rustix::process::WaitIdOptions::NOWAIT
            ),
            Ok(None)
        )
    }
    pub async fn completion(&self, uri: &str, position: Position) -> Result<Vec<CompletionItem>> {
        let value = self
            .request(
                "textDocument/completion",
                json!({ "textDocument": { "uri": uri }, "position": position }),
            )
            .await?;
        Ok(parse_completion_items(&value))
    }

    pub async fn hover(&self, uri: &str, position: Position) -> Result<String> {
        let value = self
            .request(
                "textDocument/hover",
                json!({ "textDocument": { "uri": uri }, "position": position }),
            )
            .await?;
        Ok(parse_hover(&value))
    }

    pub async fn definition(&self, uri: &str, position: Position) -> Result<Vec<Location>> {
        let value = self
            .request(
                "textDocument/definition",
                json!({ "textDocument": { "uri": uri }, "position": position }),
            )
            .await?;
        Ok(parse_locations(&value))
    }

    pub async fn document_symbols(&self, uri: &str) -> Result<Value> {
        self.request(
            "textDocument/documentSymbol",
            json!({ "textDocument": { "uri": uri } }),
        )
        .await
    }

    pub async fn workspace_symbols(&self, query: &str) -> Result<Value> {
        self.request("workspace/symbol", json!({ "query": query }))
            .await
    }

    pub async fn references(&self, uri: &str, position: Position) -> Result<Vec<Location>> {
        let value = self
            .request(
                "textDocument/references",
                json!({
                    "textDocument": { "uri": uri },
                    "position": position,
                    "context": { "includeDeclaration": true }
                }),
            )
            .await?;
        Ok(parse_locations(&value))
    }

    pub async fn rename(&self, uri: &str, position: Position, new_name: &str) -> Result<Value> {
        self.request(
            "textDocument/rename",
            json!({
                "textDocument": { "uri": uri },
                "position": position,
                "newName": new_name
            }),
        )
        .await
    }

    pub async fn code_actions(
        &self,
        uri: &str,
        range: Range,
        diagnostics: Value,
    ) -> Result<Vec<CodeActionItem>> {
        let value = self
            .request(
                "textDocument/codeAction",
                json!({
                    "textDocument": { "uri": uri },
                    "range": range,
                    "context": { "diagnostics": diagnostics }
                }),
            )
            .await?;
        Ok(parse_code_actions(&value))
    }

    pub async fn execute_command(&self, command: &str, arguments: Value) -> Result<Value> {
        self.request(
            "workspace/executeCommand",
            json!({ "command": command, "arguments": arguments }),
        )
        .await
    }

    pub async fn formatting(&self, uri: &str) -> Result<Value> {
        self.formatting_with_options(uri, 2, true).await
    }
    pub async fn formatting_with_options(
        &self,
        uri: &str,
        tab_size: usize,
        insert_spaces: bool,
    ) -> Result<Value> {
        self.request(
            "textDocument/formatting",
            json!({
                "textDocument": { "uri": uri },
                "options": { "tabSize": tab_size.clamp(1,16), "insertSpaces": insert_spaces }
            }),
        )
        .await
    }

    pub async fn signature_help(&self, uri: &str, position: Position) -> Result<String> {
        let value = self
            .request(
                "textDocument/signatureHelp",
                json!({ "textDocument": { "uri": uri }, "position": position }),
            )
            .await?;
        Ok(parse_signature_help(&value))
    }

    pub async fn shutdown(&self) {
        let _ = tokio::time::timeout(
            Duration::from_millis(300),
            self.request("shutdown", Value::Null),
        )
        .await;
        let _ = tokio::time::timeout(Duration::from_millis(100), self.notify("exit", Value::Null))
            .await;
        if let Some(mut child) = self.process.lock().await.take() {
            stop_server_group(&mut child);
            let _ = child.kill().await;
        }
        self.abort_tasks();
    }

    fn abort_tasks(&self) {
        for task in self.tasks.lock().unwrap().drain(..) {
            task.abort();
        }
    }
}

fn forward_stderr(stderr: ChildStderr, command: String, tail: StderrTail) -> JoinHandle<()> {
    tokio::spawn(async move {
        let mut reader = BufReader::new(stderr);
        let mut line = Vec::new();
        loop {
            line.clear();
            match (&mut reader)
                .take(STDERR_LINE_BYTES)
                .read_until(b'\n', &mut line)
                .await
            {
                Ok(0) | Err(_) => break,
                Ok(_) => {}
            }
            let text = String::from_utf8_lossy(&line).trim_end().to_owned();
            if text.is_empty() {
                continue;
            }
            tracing::debug!(target: "tide_lsp::server", server = %command, "{text}");
            let mut tail = tail.lock().unwrap();
            if tail.len() == STDERR_TAIL_LINES {
                tail.pop_front();
            }
            tail.push_back(text);
        }
    })
}

/// One `contentChanges` entry that turns `old` into `new`.
fn incremental_change(old: &str, new: &str) -> Value {
    let (old_bytes, new_bytes) = (old.as_bytes(), new.as_bytes());
    let splits_crlf = |offset: usize| {
        offset > 0 && old_bytes[offset - 1] == b'\r' && old_bytes.get(offset) == Some(&b'\n')
    };
    let mut start = old_bytes
        .iter()
        .zip(new_bytes)
        .take_while(|(old, new)| old == new)
        .count();
    while !old.is_char_boundary(start) {
        start -= 1;
    }
    if splits_crlf(start) {
        start -= 1;
    }
    let mut suffix = old_bytes
        .iter()
        .rev()
        .zip(new_bytes.iter().rev())
        .take(old.len().min(new.len()) - start)
        .take_while(|(old, new)| old == new)
        .count();
    while !old.is_char_boundary(old.len() - suffix) {
        suffix -= 1;
    }
    if splits_crlf(old.len() - suffix) {
        suffix -= 1;
    }
    let end = old.len() - suffix;
    let start_position = advance_position(
        Position {
            line: 0,
            character: 0,
        },
        &old[..start],
    );
    let end_position = advance_position(start_position, &old[start..end]);
    json!({
        "range": { "start": start_position, "end": end_position },
        "text": &new[start..new.len() - suffix],
    })
}

/// LSP positions count `\n`, `\r\n` and `\r` as line breaks and UTF-16 columns.
fn advance_position(mut position: Position, text: &str) -> Position {
    let mut chars = text.chars().peekable();
    while let Some(char) = chars.next() {
        match char {
            '\r' | '\n' => {
                if char == '\r' && chars.peek() == Some(&'\n') {
                    chars.next();
                }
                position.line += 1;
                position.character = 0;
            }
            _ => position.character += tide_core::num::utf16_units(char),
        }
    }
    position
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct ServerKey {
    command: String,
    args: Vec<String>,
    root: PathBuf,
}

impl ServerKey {
    fn new(config: &ServerConfig, root: &Path) -> Self {
        Self {
            command: config.command.clone(),
            args: config.args.clone(),
            root: root.to_path_buf(),
        }
    }
}

#[derive(Default)]
struct ServerSlot {
    client: Option<Arc<LspClient>>,
    started: Option<Instant>,
    failures: u32,
    retry_at: Option<Instant>,
}

const RESTART_BACKOFF: Duration = Duration::from_millis(250);
const MAX_RESTART_BACKOFF: Duration = Duration::from_secs(30);
const HEALTHY_UPTIME: Duration = Duration::from_secs(60);

impl ServerSlot {
    fn record_failure(&mut self) {
        if self
            .started
            .take()
            .is_some_and(|started| started.elapsed() >= HEALTHY_UPTIME)
        {
            self.failures = 0;
        }
        self.failures = self.failures.saturating_add(1);
        let delay = RESTART_BACKOFF
            .saturating_mul(1 << (self.failures - 1).min(8))
            .min(MAX_RESTART_BACKOFF);
        self.retry_at = Some(Instant::now() + delay);
    }
}

/// Running language servers, shared by command line and workspace root so
/// that, for example, `.ts` and `.tsx` files use one TypeScript server.
///
/// Servers start under a per-server lock: a slow start never blocks other
/// servers, and a server that keeps exiting is restarted with backoff.
#[derive(Default)]
pub struct LspRegistry {
    slots: std::sync::Mutex<HashMap<ServerKey, Arc<Mutex<ServerSlot>>>>,
}

impl LspRegistry {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    fn slot(&self, config: &ServerConfig, root: &Path) -> Arc<Mutex<ServerSlot>> {
        self.slots
            .lock()
            .unwrap()
            .entry(ServerKey::new(config, root))
            .or_default()
            .clone()
    }

    /// Return the live client for `config`, starting it if needed. `events`
    /// is called only when a new server process starts. The start runs as its
    /// own task, so cancelling the caller does not abandon it.
    pub async fn get_or_start(
        &self,
        config: &ServerConfig,
        root: &Path,
        events: impl FnOnce() -> mpsc::Sender<LspEvent>,
    ) -> Result<Arc<LspClient>> {
        let mut slot = self.slot(config, root).lock_owned().await;
        if let Some(client) = slot.client.clone() {
            if client.is_alive().await {
                return Ok(client);
            }
            slot.client = None;
            slot.record_failure();
            client.shutdown().await;
        }
        if let Some(retry_at) = slot.retry_at
            && retry_at > Instant::now()
        {
            return Err(LspError::Backoff {
                command: config.command.clone(),
                retry_ms: retry_at
                    .saturating_duration_since(Instant::now())
                    .as_millis(),
            });
        }
        let (config, root, events) = (config.clone(), root.to_path_buf(), events());
        tokio::spawn(async move {
            let result = LspClient::start(config, &root, events).await.map(Arc::new);
            match &result {
                Ok(client) => {
                    slot.client = Some(client.clone());
                    slot.started = Some(Instant::now());
                }
                Err(_) => slot.record_failure(),
            }
            result
        })
        .await
        .map_err(|error| LspError::Task(error.to_string()))?
    }

    /// The running client for `config`, without starting one.
    pub async fn running(&self, config: &ServerConfig, root: &Path) -> Option<Arc<LspClient>> {
        let slot = self
            .slots
            .lock()
            .unwrap()
            .get(&ServerKey::new(config, root))
            .cloned()?;
        slot.lock().await.client.clone()
    }

    pub async fn shutdown_all(&self) {
        let slots: Vec<_> = self
            .slots
            .lock()
            .unwrap()
            .drain()
            .map(|(_, slot)| slot)
            .collect();
        for slot in slots {
            let client = slot.lock().await.client.take();
            if let Some(client) = client {
                client.shutdown().await;
            }
        }
    }
}

/// Locate a TypeScript 5.x tsserver. TypeScript 7 removed `tsserver.js`, so
/// typescript-language-server cannot start against it. This may run `npm`;
/// call it from a blocking context.
#[must_use]
pub fn typescript_tsserver_path() -> Option<PathBuf> {
    if let Some(path) = std::env::var_os("TIDE_TSSERVER_PATH")
        && Path::new(&path).is_file()
    {
        return Some(PathBuf::from(path));
    }
    let output = tide_core::process::scrub_tide_secrets(&mut std::process::Command::new("npm"))
        .args(["root", "-g"])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let root = String::from_utf8_lossy(&output.stdout).trim().to_owned();
    let candidate = PathBuf::from(root).join("typescript/lib/tsserver.js");
    candidate.is_file().then_some(candidate)
}

async fn read_message<R: AsyncBufReadExt + Unpin>(reader: &mut R) -> Option<Vec<u8>> {
    let mut content_length = None;
    let mut header_bytes = 0;
    loop {
        let mut line = String::new();
        let read = (&mut *reader).take(8193).read_line(&mut line).await.ok()?;
        header_bytes += read;
        if read > 8192 || header_bytes > 65536 || !line.ends_with('\n') {
            return None;
        }
        if read == 0 {
            return None;
        }
        let trimmed = line.trim_end_matches(['\r', '\n']);
        if trimmed.is_empty() {
            break;
        }
        if let Some((name, value)) = trimmed.split_once(':')
            && name.eq_ignore_ascii_case("Content-Length")
        {
            if content_length.is_some() {
                return None;
            }
            content_length = value.trim().parse::<usize>().ok();
        }
    }
    let length = content_length?;
    if length > 16 * 1024 * 1024 {
        return None;
    }
    let mut body = vec![0_u8; length];
    reader.read_exact(&mut body).await.ok()?;
    Some(body)
}

#[must_use]
pub fn parse_completion_items(value: &Value) -> Vec<CompletionItem> {
    let items = value
        .get("items")
        .and_then(Value::as_array)
        .or_else(|| value.as_array())
        .cloned()
        .unwrap_or_default();
    items
        .into_iter()
        .filter_map(|item| {
            let label = item.get("label")?.as_str()?.to_owned();
            let insert_text = item
                .get("insertText")
                .and_then(Value::as_str)
                .unwrap_or(&label)
                .to_owned();
            // Snippets are not advertised; never insert unresolved placeholders.
            if item.get("insertTextFormat").and_then(Value::as_u64) == Some(2) {
                return None;
            }
            let text_edit = if let Some(edit) = item.get("textEdit") {
                let range = edit.get("range").or_else(|| edit.get("replace"))?;
                Some(TextEdit {
                    range: serde_json::from_value(range.clone()).ok()?,
                    new_text: edit.get("newText")?.as_str()?.to_owned(),
                })
            } else {
                None
            };
            let additional_edits = item
                .get("additionalTextEdits")
                .map_or(Vec::new(), parse_text_edits);
            if item
                .get("additionalTextEdits")
                .and_then(Value::as_array)
                .is_some_and(|edits| edits.len() != additional_edits.len())
            {
                return None;
            }
            Some(CompletionItem {
                text_edit,
                additional_edits,
                detail: item
                    .get("detail")
                    .and_then(Value::as_str)
                    .map(ToOwned::to_owned),
                insert_text,
                kind: item.get("kind").and_then(Value::as_u64),
                label,
            })
        })
        .collect()
}

#[must_use]
pub fn parse_hover(value: &Value) -> String {
    let contents = value.get("contents").unwrap_or(&Value::Null);
    match contents {
        Value::String(text) => text.clone(),
        Value::Object(object) => object
            .get("value")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned(),
        Value::Array(items) => items
            .iter()
            .filter_map(|item| {
                item.as_str().map(ToOwned::to_owned).or_else(|| {
                    item.get("value")
                        .and_then(Value::as_str)
                        .map(ToOwned::to_owned)
                })
            })
            .collect::<Vec<_>>()
            .join("\n"),
        _ => String::new(),
    }
}

#[must_use]
pub fn parse_locations(value: &Value) -> Vec<Location> {
    let values = if let Some(array) = value.as_array() {
        array.clone()
    } else if value.is_object() {
        vec![value.clone()]
    } else {
        Vec::new()
    };
    values
        .into_iter()
        .filter_map(|location| {
            let uri = location.get("uri")?.as_str()?.to_owned();
            let range = serde_json::from_value::<Range>(location.get("range")?.clone()).ok()?;
            Some(Location { uri, range })
        })
        .collect()
}

#[must_use]
pub fn parse_document_symbols(value: &Value, uri: &str) -> Vec<SymbolItem> {
    let mut items = Vec::new();
    fn visit(value: &Value, uri: &str, depth: usize, items: &mut Vec<SymbolItem>) {
        let Some(array) = value.as_array() else {
            return;
        };
        for symbol in array {
            let Some(name) = symbol.get("name").and_then(Value::as_str) else {
                continue;
            };
            let line = symbol
                .get("selectionRange")
                .or_else(|| symbol.get("range"))
                .and_then(|range| range.get("start"))
                .and_then(|start| start.get("line"))
                .and_then(Value::as_u64)
                // An out-of-range line is malformed; treat it like a missing one.
                .and_then(|line| u32::try_from(line).ok())
                .unwrap_or(0);
            items.push(SymbolItem {
                name: name.to_owned(),
                detail: symbol
                    .get("detail")
                    .and_then(Value::as_str)
                    .map_or_else(|| "symbol".to_owned(), ToOwned::to_owned),
                uri: Some(uri.to_owned()),
                line,
                depth,
            });
            if let Some(children) = symbol.get("children") {
                visit(children, uri, depth + 1, items);
            }
        }
    }
    visit(value, uri, 0, &mut items);
    items
}

#[must_use]
pub fn parse_workspace_symbols(value: &Value) -> Vec<SymbolItem> {
    let Some(array) = value.as_array() else {
        return Vec::new();
    };
    array
        .iter()
        .filter_map(|symbol| {
            let name = symbol.get("name")?.as_str()?.to_owned();
            let location = symbol.get("location")?;
            let uri = location.get("uri")?.as_str()?.to_owned();
            let line = location
                .get("range")
                .and_then(|range| range.get("start"))
                .and_then(|start| start.get("line"))
                .and_then(Value::as_u64)
                // An out-of-range line is malformed; treat it like a missing one.
                .and_then(|line| u32::try_from(line).ok())
                .unwrap_or(0);
            Some(SymbolItem {
                detail: symbol
                    .get("containerName")
                    .and_then(Value::as_str)
                    .map_or_else(|| "workspace".to_owned(), ToOwned::to_owned),
                name,
                uri: Some(uri),
                line,
                depth: 0,
            })
        })
        .collect()
}

#[must_use]
pub fn parse_code_actions(value: &Value) -> Vec<CodeActionItem> {
    value.as_array().map_or_else(Vec::new, |items| {
        items
            .iter()
            .filter_map(|item| {
                let title = item.get("title")?.as_str()?.to_owned();
                Some(CodeActionItem {
                    title,
                    kind: item.get("kind").and_then(Value::as_str).map(str::to_owned),
                    edit: item.get("edit").cloned(),
                    command: item.get("command").cloned(),
                    is_preferred: item
                        .get("isPreferred")
                        .and_then(Value::as_bool)
                        .unwrap_or(false),
                })
            })
            .collect()
    })
}

/// Parse a WorkspaceEdit into `(uri, [TextEdit])` groups.
#[must_use]
pub fn parse_workspace_edit(value: &Value) -> Vec<(String, Vec<TextEdit>)> {
    // A resource operation cannot be silently omitted from an atomic action.
    if value
        .get("documentChanges")
        .and_then(Value::as_array)
        .is_some_and(|changes| changes.iter().any(|change| change.get("kind").is_some()))
    {
        return Vec::new();
    }
    let mut files: Vec<(String, Vec<TextEdit>)> = Vec::new();
    if let Some(changes) = value.get("changes").and_then(Value::as_object) {
        for (uri, edits) in changes {
            let edits = parse_text_edits(edits);
            if !edits.is_empty() {
                files.push((uri.clone(), edits));
            }
        }
    }
    if let Some(document_changes) = value.get("documentChanges").and_then(Value::as_array) {
        for change in document_changes {
            let Some(uri) = change
                .get("textDocument")
                .and_then(|document| document.get("uri"))
                .and_then(Value::as_str)
            else {
                continue;
            };
            let edits = change.get("edits").map_or_else(Vec::new, parse_text_edits);
            if !edits.is_empty() {
                files.push((uri.to_owned(), edits));
            }
        }
    }
    files
}

#[must_use]
pub fn parse_text_edits(value: &Value) -> Vec<TextEdit> {
    value.as_array().map_or_else(Vec::new, |items| {
        items
            .iter()
            .filter_map(|edit| {
                let range = serde_json::from_value::<Range>(edit.get("range")?.clone()).ok()?;
                let new_text = edit.get("newText")?.as_str()?.to_owned();
                Some(TextEdit { range, new_text })
            })
            .collect()
    })
}

#[must_use]
pub fn parse_signature_help(value: &Value) -> String {
    let Some(signatures) = value.get("signatures").and_then(Value::as_array) else {
        return String::new();
    };
    signatures
        .iter()
        .filter_map(|signature| {
            let label = signature.get("label")?.as_str()?;
            let docs = signature
                .get("documentation")
                .and_then(|docs| {
                    docs.get("value")
                        .and_then(Value::as_str)
                        .or_else(|| docs.as_str())
                })
                .unwrap_or("");
            Some(if docs.is_empty() {
                label.to_owned()
            } else {
                format!("{label}\n{docs}")
            })
        })
        .collect::<Vec<_>>()
        .join("\n\n")
}

fn stop_server_group(child: &mut Child) {
    if let Some(pid) = child
        .id()
        .filter(|id| *id > 1)
        .and_then(tide_core::process::raw_pid)
        .and_then(rustix::process::Pid::from_raw)
        && rustix::process::waitid(
            rustix::process::WaitId::Pid(pid),
            rustix::process::WaitIdOptions::EXITED
                | rustix::process::WaitIdOptions::NOHANG
                | rustix::process::WaitIdOptions::NOWAIT,
        )
        .is_ok()
    {
        let _ = nix::sys::signal::kill(
            nix::unistd::Pid::from_raw(-pid.as_raw_nonzero().get()),
            nix::sys::signal::Signal::SIGKILL,
        );
    }
    let _ = child.start_kill();
}
impl Drop for LspClient {
    fn drop(&mut self) {
        if let Some(mut child) = self.process.get_mut().take() {
            stop_server_group(&mut child);
        }
        self.abort_tasks();
    }
}

fn encode_message(value: Value) -> Vec<u8> {
    let text = value.to_string();
    format!("Content-Length: {}\r\n\r\n{text}", text.len()).into_bytes()
}
struct PendingRequest {
    pending: PendingMap,
    writer: mpsc::Sender<Vec<u8>>,
    id: i64,
}
impl Drop for PendingRequest {
    fn drop(&mut self) {
        if self.pending.lock().unwrap().remove(&self.id).is_some() {
            let _ = self.writer.try_send(encode_message(
                json!({"jsonrpc":"2.0","method":"$/cancelRequest","params":{"id":self.id}}),
            ));
        }
    }
}
pub fn checked_text_edits(value: &Value) -> Result<Vec<TextEdit>> {
    let array = value
        .as_array()
        .ok_or(LspError::MalformedEdit("Expected a text edit array"))?;
    let edits = parse_text_edits(value);
    ensure_edit(edits.len() == array.len(), "Malformed text edit")?;
    Ok(edits)
}
/// Reject malformed edits and unsupported resource operations atomically.
pub fn checked_workspace_edit(value: &Value) -> Result<Vec<(String, Vec<TextEdit>)>> {
    ensure_edit(value.is_object(), "Missing workspace edit")?;
    ensure_edit(
        value.get("changes").is_none() || value.get("documentChanges").is_none(),
        "Ambiguous workspace edit",
    )?;
    let mut files = Vec::new();
    let mut add = |uri: &str, edits: &Value| -> Result<()> {
        let array = edits
            .as_array()
            .ok_or(LspError::MalformedEdit("Missing edit list"))?;
        let parsed = parse_text_edits(edits);
        ensure_edit(parsed.len() == array.len(), "Malformed text edit")?;
        files.push((uri.to_owned(), parsed));
        Ok(())
    };
    if let Some(changes) = value.get("changes") {
        for (uri, edits) in changes
            .as_object()
            .ok_or(LspError::MalformedEdit("Invalid changes"))?
        {
            add(uri, edits)?;
        }
    }
    if let Some(changes) = value.get("documentChanges") {
        for change in changes
            .as_array()
            .ok_or(LspError::MalformedEdit("Invalid documentChanges"))?
        {
            ensure_edit(
                change.get("kind").is_none(),
                "Resource operations are unsupported",
            )?;
            let uri = change
                .get("textDocument")
                .and_then(|d| d.get("uri"))
                .and_then(Value::as_str)
                .ok_or(LspError::MalformedEdit("Missing edit URI"))?;
            let edits = change
                .get("edits")
                .ok_or(LspError::MalformedEdit("Missing edits"))?;
            add(uri, edits)?;
        }
    }
    Ok(files)
}

fn ensure_edit(condition: bool, reason: &'static str) -> Result<()> {
    if condition {
        Ok(())
    } else {
        Err(LspError::MalformedEdit(reason))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_extensions_to_languages() {
        assert_eq!(language_id_for_path(Path::new("src/main.rs")), Some("rust"));
        assert_eq!(
            language_id_for_path(Path::new("app.tsx")),
            Some("typescriptreact")
        );
        assert_eq!(language_id_for_path(Path::new("README.md")), None);
    }

    #[test]
    fn parses_completion_shapes() {
        let value =
            json!({ "items": [{ "label": "println!", "insertText": "println!()", "kind": 3 }] });
        let items = parse_completion_items(&value);
        assert_eq!(items[0].label, "println!");
        assert_eq!(items[0].insert_text, "println!()");
        let value = json!([{ "label": "x" }]);
        assert_eq!(parse_completion_items(&value)[0].label, "x");
    }

    #[test]
    fn parses_hover_and_locations() {
        assert_eq!(
            parse_hover(&json!({ "contents": { "value": "fn main()" } })),
            "fn main()"
        );
        let locations = parse_locations(&json!({
            "uri": "file:///tmp/a.rs",
            "range": { "start": { "line": 1, "character": 2 }, "end": { "line": 1, "character": 5 } }
        }));
        assert_eq!(locations.len(), 1);
        assert_eq!(locations[0].range.start.line, 1);
    }

    #[test]
    fn parses_symbols_and_signatures() {
        let symbols = parse_document_symbols(
            &json!([{ "name": "main", "selectionRange": { "start": { "line": 4, "character": 0 } }, "children": [{ "name": "inner" }] }]),
            "file:///tmp/a.rs",
        );
        assert_eq!(symbols.len(), 2);
        assert_eq!(symbols[0].line, 4);
        assert_eq!(symbols[1].depth, 1);
        let workspace = parse_workspace_symbols(
            &json!([{ "name": "Thing", "location": { "uri": "file:///tmp/a.rs", "range": { "start": { "line": 2, "character": 0 } } } }]),
        );
        assert_eq!(workspace[0].line, 2);
        assert!(
            parse_signature_help(&json!({ "signatures": [{ "label": "fn(a: i32)" }] }))
                .contains("fn")
        );
    }

    #[test]
    fn parses_code_actions_and_prefers_ordering_input() {
        let actions = parse_code_actions(&json!([
            {"title": "Remove import", "kind": "quickfix", "edit": {"changes": {}}},
            {"title": "Refactor", "kind": "refactor", "isPreferred": true},
            {"title": "Disabled"},
        ]));
        assert_eq!(actions.len(), 3);
        assert_eq!(actions[0].title, "Remove import");
        assert_eq!(actions[0].kind.as_deref(), Some("quickfix"));
        assert!(actions[1].edit.is_none());
        assert!(actions[1].is_preferred);
    }

    #[test]
    fn parses_workspace_edits() {
        let edits = parse_workspace_edit(&json!({
            "changes": {
                "file:///tmp/a.rs": [{
                    "range": { "start": { "line": 1, "character": 0 }, "end": { "line": 1, "character": 3 } },
                    "newText": "new"
                }]
            }
        }));
        assert_eq!(edits.len(), 1);
        assert_eq!(edits[0].1[0].new_text, "new");
    }

    #[test]
    fn finds_a_typescript_five_tsserver_when_installed() {
        if let Some(path) = typescript_tsserver_path() {
            assert!(path.ends_with("typescript/lib/tsserver.js"));
        }
    }

    #[test]
    fn converts_file_uris() {
        let path = Path::new("/tmp/a b.rs");
        let uri = path_to_uri(path).unwrap();
        assert!(uri.starts_with("file://"));
        assert_eq!(uri_to_path(&uri), Some(path.to_path_buf()));
    }

    #[test]
    fn typescript_languages_share_one_server_command() {
        let typescript = server_for_language("typescript").unwrap();
        for language in ["javascript", "typescriptreact", "javascriptreact"] {
            let server = server_for_language(language).unwrap();
            assert_eq!(server.language_id, language);
            assert_eq!(
                (&server.command, &server.args),
                (&typescript.command, &typescript.args)
            );
        }
        assert_eq!(default_servers().len(), DEFAULT_SERVERS.len());
    }

    #[test]
    fn parses_text_document_sync_capabilities() {
        let parse = |value: Value| {
            TextDocumentSync::from_capabilities(&json!({ "textDocumentSync": value }))
        };
        assert_eq!(
            TextDocumentSync::from_capabilities(&json!({})),
            TextDocumentSync::FULL
        );
        assert_eq!(parse(json!(2)).change, SyncKind::Incremental);
        assert!(parse(json!(1)).save);
        assert_eq!(parse(json!({"openClose": true})).change, SyncKind::None);
        let options = parse(json!({"change": 2, "save": {"includeText": true}}));
        assert!(options.save && options.save_include_text);
        let options = parse(json!({"change": 1, "save": true}));
        assert!(options.save && !options.save_include_text);
        assert!(!parse(json!({"change": 1, "save": false})).save);
    }

    #[test]
    fn incremental_changes_never_split_a_crlf_or_a_utf16_pair() {
        let change = incremental_change("a\r\nb", "a\nb");
        assert_eq!(change["range"]["start"], json!({"line":0,"character":1}));
        assert_eq!(change["range"]["end"], json!({"line":1,"character":0}));
        assert_eq!(change["text"], "\n");
        let change = incremental_change("x😀y", "x😃y");
        assert_eq!(change["range"]["start"], json!({"line":0,"character":1}));
        assert_eq!(change["range"]["end"], json!({"line":0,"character":3}));
        assert_eq!(change["text"], "😃");
        let change = incremental_change("same", "same");
        assert_eq!(change["range"]["start"], change["range"]["end"]);
        assert_eq!(change["text"], "");
    }
}
