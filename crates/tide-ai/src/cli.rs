//! AI CLI adapters: executable detection, launch arguments, and native session
//! discovery. The tools themselves run unchanged inside Tide PTY tabs.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum CliProvider {
    ClaudeCode,
    Codex,
    Gemini,
    Antigravity,
    OpenCode,
}

pub const CLI_PROVIDERS: [CliProvider; 5] = [
    CliProvider::ClaudeCode,
    CliProvider::Codex,
    CliProvider::Gemini,
    CliProvider::Antigravity,
    CliProvider::OpenCode,
];

impl CliProvider {
    pub fn id(self) -> &'static str {
        match self {
            Self::ClaudeCode => "claude-code",
            Self::Codex => "codex",
            Self::Gemini => "gemini",
            Self::Antigravity => "antigravity",
            Self::OpenCode => "opencode",
        }
    }

    pub fn display_name(self) -> &'static str {
        match self {
            Self::ClaudeCode => "Claude Code",
            Self::Codex => "Codex",
            Self::Gemini => "Gemini CLI",
            Self::Antigravity => "Antigravity",
            Self::OpenCode => "OpenCode",
        }
    }

    pub fn executable(self) -> &'static str {
        match self {
            Self::ClaudeCode => "claude",
            Self::Codex => "codex",
            Self::Gemini => "gemini",
            Self::Antigravity => "agy",
            Self::OpenCode => "opencode",
        }
    }

    pub fn env_override(self) -> &'static str {
        match self {
            Self::ClaudeCode => "TIDE_CLAUDE_PATH",
            Self::Codex => "TIDE_CODEX_PATH",
            Self::Gemini => "TIDE_GEMINI_PATH",
            Self::Antigravity => "TIDE_ANTIGRAVITY_PATH",
            Self::OpenCode => "TIDE_OPENCODE_PATH",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        CLI_PROVIDERS
            .into_iter()
            .find(|provider| provider.id() == value)
    }

    /// Native metadata root used for session discovery.
    pub fn metadata_root(self, home: &Path) -> Option<PathBuf> {
        match self {
            Self::ClaudeCode => Some(home.join(".claude").join("projects")),
            Self::Codex => Some(home.join(".codex").join("sessions")),
            Self::Gemini => Some(home.join(".gemini").join("tmp")),
            Self::Antigravity => Some(home.join(".gemini").join("antigravity")),
            Self::OpenCode => Some(home.join(".local").join("share").join("opencode")),
        }
    }

    /// Arguments for a fresh or resumed PTY session. An ID that could be read
    /// as an option is never passed; such a request starts a fresh session.
    pub fn launch_args(self, cwd: &Path, session_id: Option<&str>) -> Vec<String> {
        let cwd_arg = cwd.display().to_string();
        match (self, session_id.filter(|id| safe_session_id(id))) {
            (Self::ClaudeCode, None) => self.prepare_new(cwd).0,
            (Self::ClaudeCode, Some(id)) => vec!["--resume".to_owned(), id.to_owned()],
            (Self::Codex, None) => vec!["-C".to_owned(), cwd_arg],
            (Self::Codex, Some(id)) => vec![
                "resume".to_owned(),
                "-C".to_owned(),
                cwd_arg,
                "--".to_owned(),
                id.to_owned(),
            ],
            (Self::Gemini, None) => self.prepare_new(cwd).0,
            (Self::Gemini, Some(id)) => vec!["--resume".to_owned(), id.to_owned()],
            (Self::Antigravity, None) => Vec::new(),
            (Self::Antigravity, Some(id)) => vec!["--conversation".to_owned(), id.to_owned()],
            (Self::OpenCode, None) => Vec::new(),
            (Self::OpenCode, Some(id)) => vec!["--session".to_owned(), id.to_owned()],
        }
    }
}

impl CliProvider {
    /// Prepare a new session. Providers that accept a caller-supplied ID return
    /// it so the tab can be deduped and restored; others return `None`.
    pub fn prepare_new(self, cwd: &Path) -> (Vec<String>, Option<String>) {
        match self {
            Self::ClaudeCode => {
                let id = new_uuid();
                (vec!["--session-id".to_owned(), id.clone()], Some(id))
            }
            Self::Gemini => {
                let id = new_uuid();
                (vec!["--session-id".to_owned(), id.clone()], Some(id))
            }
            Self::Codex => (vec!["-C".to_owned(), cwd.display().to_string()], None),
            Self::Antigravity => (Vec::new(), None),
            Self::OpenCode => (Vec::new(), None),
        }
    }

    pub fn supports_rename(self) -> bool {
        matches!(self, Self::Codex | Self::OpenCode)
    }

    pub fn supports_delete(self) -> bool {
        matches!(self, Self::Codex | Self::Gemini | Self::OpenCode)
    }

    pub fn supports_archive(self) -> bool {
        matches!(self, Self::Codex)
    }

    pub fn delete_args(self, session_id: &str) -> Option<Vec<String>> {
        safe_session_id(session_id)
            .then(|| match self {
                Self::Codex => vec!["delete".to_owned(), "--".to_owned(), session_id.to_owned()],
                Self::Gemini => vec!["--delete-session".to_owned(), session_id.to_owned()],
                Self::OpenCode => vec![
                    "session".to_owned(),
                    "delete".to_owned(),
                    session_id.to_owned(),
                ],
                _ => Vec::new(),
            })
            .filter(|args| !args.is_empty())
    }

    pub fn archive_args(self, session_id: &str) -> Option<Vec<String>> {
        (self == Self::Codex && safe_session_id(session_id))
            .then(|| vec!["archive".to_owned(), "--".to_owned(), session_id.to_owned()])
    }
}

/// Execute a provider session mutation with the same bounded process handling as
/// discovery.
pub fn run_session_action(
    provider: CliProvider,
    workspace: &Path,
    args: &[String],
) -> Result<(), String> {
    let executable = provider_executable(provider)
        .ok_or_else(|| format!("{} is not installed", provider.display_name()))?;
    if provider == CliProvider::Codex && args.first().is_some_and(|arg| arg == "archive") {
        return crate::native_cli::archive_codex(
            &executable,
            workspace,
            args.last().ok_or("Missing session identity")?,
        );
    }
    // OpenCode mutations go through its HTTP API, which only deletes. Match the whole
    // `delete_args` shape so no other action can reach it.
    if provider == CliProvider::OpenCode {
        return match args {
            [command, action, id] if command == "session" && action == "delete" => {
                crate::native_cli::delete_opencode(&executable, workspace, id)
            }
            _ => Err("Unsupported OpenCode session action".to_owned()),
        };
    }
    let args = args.iter().map(String::as_str).collect::<Vec<_>>();
    run_listing(&executable, &args, workspace)
        .and_then(Listing::output)
        .map(|_| ())
        .map_err(|error| match error {
            ListingFailure::Unavailable => "provider is unavailable".to_owned(),
            ListingFailure::Timeout => "provider command timed out".to_owned(),
            ListingFailure::Failed(message) | ListingFailure::NeedsSignIn(message) => message,
        })
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CliStatus {
    pub provider: CliProvider,
    pub installed: bool,
    pub executable: Option<PathBuf>,
}

pub fn detect(provider: CliProvider) -> CliStatus {
    let executable = std::env::var_os(provider.env_override())
        .map(PathBuf::from)
        .filter(|path| path.is_file())
        .or_else(|| find_in_path(provider.executable()));
    CliStatus {
        provider,
        installed: executable.is_some(),
        executable,
    }
}

pub fn detect_all() -> Vec<CliStatus> {
    CLI_PROVIDERS.into_iter().map(detect).collect()
}

fn find_in_path(command: &str) -> Option<PathBuf> {
    let paths = std::env::var_os("PATH")?;
    let extensions: &[&str] = if cfg!(windows) {
        &["", ".exe", ".cmd"]
    } else {
        &[""]
    };
    std::env::split_paths(&paths).find_map(|dir| {
        extensions.iter().find_map(|extension| {
            let candidate = dir.join(format!("{command}{extension}"));
            candidate.is_file().then_some(candidate)
        })
    })
}

pub(crate) fn new_uuid() -> String {
    use rand::RngCore as _;
    let mut bytes = [0_u8; 16];
    rand::thread_rng().fill_bytes(&mut bytes);
    bytes[6] = (bytes[6] & 0x0f) | 0x40;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    format!(
        "{:02x}{:02x}{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
        bytes[0],
        bytes[1],
        bytes[2],
        bytes[3],
        bytes[4],
        bytes[5],
        bytes[6],
        bytes[7],
        bytes[8],
        bytes[9],
        bytes[10],
        bytes[11],
        bytes[12],
        bytes[13],
        bytes[14],
        bytes[15]
    )
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CliSession {
    pub provider_id: String,
    pub session_id: String,
    pub title: String,
    pub cwd: Option<String>,
    pub model: Option<String>,
    pub updated_at: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DiscoveryState {
    Ok,
    Partial,
    Unavailable,
    Error,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct DiscoveryReport {
    pub sessions: Vec<CliSession>,
    pub state: Option<DiscoveryState>,
    pub errors: Vec<String>,
    #[serde(default)]
    pub providers: Vec<ProviderDiscovery>,
    #[serde(default)]
    pub statuses: Vec<CliStatus>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderDiscovery {
    pub provider: CliProvider,
    pub state: DiscoveryState,
    pub sessions: Vec<CliSession>,
    pub error: Option<String>,
    /// A setup step the CLI is waiting for, such as signing in. Not an error: the CLI
    /// is installed and Tide can start it, which is how the user signs in.
    #[serde(default)]
    pub notice: Option<String>,
}

/// Fast, file-backed session discovery suitable for startup.
pub fn discover_sessions(provider: CliProvider, home: &Path, _workspace: &Path) -> Vec<CliSession> {
    match provider {
        CliProvider::ClaudeCode => discover_claude_sessions(&home.join(".claude").join("projects")),
        CliProvider::Codex => discover_codex_sessions(home),
        CliProvider::Gemini | CliProvider::OpenCode | CliProvider::Antigravity => Vec::new(),
    }
}

/// Full discovery, including CLI-backed listing. Call off the UI thread.
pub fn discover_sessions_with_cli(
    provider: CliProvider,
    home: &Path,
    workspace: &Path,
) -> Vec<CliSession> {
    match provider {
        CliProvider::Gemini => discover_gemini_sessions(workspace),
        CliProvider::OpenCode => discover_opencode_sessions(workspace),
        _ => discover_sessions(provider, home, workspace),
    }
}

/// Full discovery with provider status and bounded failure reporting.
pub fn discover_all_with_status(home: &Path, workspace: &Path) -> DiscoveryReport {
    discover_all_with_updates(home, workspace, |_| {})
}

pub fn discover_all_with_updates(
    home: &Path,
    workspace: &Path,
    update: impl Fn(ProviderDiscovery) + Sync,
) -> DiscoveryReport {
    let providers = std::thread::scope(|scope| {
        let cancellation = crate::native_cli::current_cancellation();
        let handles = CLI_PROVIDERS
            .into_iter()
            .map(|provider| {
                let update = &update;
                let cancellation = cancellation.clone();
                (
                    provider,
                    scope.spawn(move || {
                        let _scope = cancellation.map(crate::native_cli::CancellationScope::enter);
                        let result = discover_provider(provider, home, workspace);
                        update(result.clone());
                        result
                    }),
                )
            })
            .collect::<Vec<_>>();
        handles
            .into_iter()
            .map(|(provider, handle)| {
                handle.join().unwrap_or_else(|_| ProviderDiscovery {
                    provider,
                    state: DiscoveryState::Error,
                    sessions: Vec::new(),
                    error: Some("Provider discovery worker failed".into()),
                    notice: None,
                })
            })
            .collect::<Vec<_>>()
    });
    let mut sessions = providers
        .iter()
        .flat_map(|provider| provider.sessions.clone())
        .collect::<Vec<_>>();
    normalize_sessions(&mut sessions);
    let errors = providers
        .iter()
        .filter_map(|provider| provider.error.clone())
        .collect::<Vec<_>>();
    let state = if !errors.is_empty() {
        if sessions.is_empty() {
            DiscoveryState::Error
        } else {
            DiscoveryState::Partial
        }
    } else if providers
        .iter()
        .all(|provider| provider.state == DiscoveryState::Unavailable)
    {
        DiscoveryState::Unavailable
    } else {
        DiscoveryState::Ok
    };
    DiscoveryReport {
        sessions,
        state: Some(state),
        errors,
        providers,
        statuses: detect_all(),
    }
}

fn discover_provider(provider: CliProvider, home: &Path, workspace: &Path) -> ProviderDiscovery {
    let result = match provider {
        CliProvider::ClaudeCode => Ok(discover_claude_sessions(&home.join(".claude/projects"))),
        CliProvider::Codex => {
            if let Some(executable) = provider_executable(provider) {
                match crate::native_cli::CodexClient::start(&executable, workspace)
                    .and_then(|mut client| client.sessions(workspace))
                {
                    Ok(sessions) => Ok(sessions),
                    Err(error) => {
                        let mut sessions = discover_codex_sessions(home);
                        canonicalize_session_cwds(&mut sessions);
                        return ProviderDiscovery {
                            provider,
                            state: if sessions.is_empty() {
                                DiscoveryState::Error
                            } else {
                                DiscoveryState::Partial
                            },
                            sessions,
                            error: Some(format!("Codex native catalog: {error}")),
                            notice: None,
                        };
                    }
                }
            } else {
                Ok(discover_codex_sessions(home))
            }
        }
        CliProvider::Gemini => try_discover_gemini(workspace),
        CliProvider::OpenCode => try_discover_opencode(workspace),
        CliProvider::Antigravity => Ok(discover_antigravity_sessions(home, workspace)),
    };
    match result {
        Ok(mut sessions) => {
            normalize_sessions(&mut sessions);
            canonicalize_session_cwds(&mut sessions);
            ProviderDiscovery {
                provider,
                state: DiscoveryState::Ok,
                sessions,
                error: None,
                notice: None,
            }
        }
        Err(ListingFailure::Unavailable) => ProviderDiscovery {
            provider,
            state: DiscoveryState::Unavailable,
            sessions: Vec::new(),
            error: None,
            notice: None,
        },
        Err(ListingFailure::NeedsSignIn(detail)) => ProviderDiscovery {
            provider,
            state: DiscoveryState::Ok,
            sessions: Vec::new(),
            error: None,
            notice: Some(detail),
        },
        Err(error) => ProviderDiscovery {
            provider,
            state: DiscoveryState::Error,
            sessions: Vec::new(),
            error: Some(format!(
                "{} session discovery: {}",
                provider.display_name(),
                match error {
                    ListingFailure::Timeout => "timed out".to_owned(),
                    ListingFailure::Failed(message) => message,
                    ListingFailure::Unavailable | ListingFailure::NeedsSignIn(_) => unreachable!(),
                }
            )),
            notice: None,
        },
    }
}

pub fn normalize_session_path(path: &Path) -> PathBuf {
    let mut result = PathBuf::new();
    for component in path.components() {
        match component {
            std::path::Component::ParentDir => {
                result.pop();
            }
            std::path::Component::CurDir => {}
            other => result.push(other.as_os_str()),
        }
    }
    result
}

fn canonicalize_session_cwds(sessions: &mut [CliSession]) {
    let mut paths = std::collections::HashMap::new();
    for session in sessions {
        if let Some(cwd) = &session.cwd {
            if !Path::new(cwd).is_absolute() {
                continue;
            }
            let normalized = paths.entry(cwd.clone()).or_insert_with(|| {
                std::fs::canonicalize(cwd)
                    .unwrap_or_else(|_| normalize_session_path(Path::new(cwd)))
                    .display()
                    .to_string()
            });
            session.cwd = Some(normalized.clone());
        }
    }
}

fn provider_executable(provider: CliProvider) -> Option<PathBuf> {
    std::env::var_os(provider.env_override())
        .map(PathBuf::from)
        .filter(|path| path.is_file())
        .or_else(|| find_in_path(provider.executable()))
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ListingFailure {
    Unavailable,
    Timeout,
    Failed(String),
    /// The CLI is installed but not signed in or configured; the CLI's own words.
    NeedsSignIn(String),
}

/// What a listing command printed, and whether it succeeded.
#[derive(Debug, Clone, Default)]
pub struct Listing {
    pub stdout: String,
    pub stderr: String,
    /// The exit status when it was not success.
    pub failure: Option<String>,
}

impl Listing {
    /// The output of a successful listing. A failed one is a sign-in problem when the
    /// CLI says so, and otherwise an error.
    pub fn output(self) -> Result<String, ListingFailure> {
        let Some(status) = &self.failure else {
            return Ok(self.stdout);
        };
        Err(self.sign_in_problem().map_or_else(
            || ListingFailure::Failed(format!("listing exited with {status}")),
            ListingFailure::NeedsSignIn,
        ))
    }

    /// The line where the CLI says it needs signing in, an API key or other setup.
    #[must_use]
    pub fn sign_in_problem(&self) -> Option<String> {
        const SIGNS: &[&str] = &[
            "auth",
            "sign in",
            "signin",
            "log in",
            "login",
            "api key",
            "api_key",
            "credential",
            "not logged",
            "unauthorized",
            "no providers",
            "ineligible",
        ];
        self.stderr
            .lines()
            .chain(self.stdout.lines())
            .map(str::trim)
            .find(|line| {
                let line = line.to_ascii_lowercase();
                SIGNS.iter().any(|sign| line.contains(sign))
            })
            .map(|line| {
                let mut line = line.to_owned();
                if line.len() > 300 {
                    let end = (0..=300)
                        .rev()
                        .find(|index| line.is_char_boundary(*index))
                        .unwrap_or(0);
                    line.truncate(end);
                    line.push('…');
                }
                line
            })
    }
}

/// Reads at most `limit` bytes from `reader` on a thread.
fn read_on_thread(
    reader: impl std::io::Read + Send + 'static,
    limit: u64,
) -> std::sync::mpsc::Receiver<std::io::Result<Vec<u8>>> {
    use std::io::Read as _;
    let (sender, receiver) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let mut bytes = Vec::new();
        let result = reader.take(limit).read_to_end(&mut bytes).map(|_| bytes);
        let _ = sender.send(result);
    });
    receiver
}

fn run_listing(executable: &Path, args: &[&str], cwd: &Path) -> Result<Listing, ListingFailure> {
    use wait_timeout::ChildExt as _;
    let mut command = std::process::Command::new(executable);
    tide_core::process::scrub_tide_secrets(&mut command)
        .args(args)
        .current_dir(cwd)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .stdin(std::process::Stdio::null());
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt as _;
        command.process_group(0);
    }
    let mut child = command
        .spawn()
        .map_err(|error| ListingFailure::Failed(error.to_string()))?;
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| ListingFailure::Failed("missing stdout".into()))?;
    let receiver = read_on_thread(stdout, 2 * 1024 * 1024 + 1);
    let errors = child
        .stderr
        .take()
        .map(|stderr| read_on_thread(stderr, 64 * 1024));
    let status = match child.wait_timeout(std::time::Duration::from_secs(3)) {
        Ok(Some(status)) => status,
        Ok(None) => {
            kill_process_tree(&mut child);
            return Err(ListingFailure::Timeout);
        }
        Err(error) => {
            kill_process_tree(&mut child);
            return Err(ListingFailure::Failed(error.to_string()));
        }
    };
    let failure = (!status.success()).then(|| status.to_string());
    let output = match receiver
        .recv_timeout(deadline.saturating_duration_since(std::time::Instant::now()))
    {
        Ok(Ok(bytes)) if bytes.len() <= 2 * 1024 * 1024 => bytes,
        Ok(Ok(_)) => {
            kill_process_tree(&mut child);
            return Err(ListingFailure::Failed(
                "provider output exceeds 2 MiB".into(),
            ));
        }
        Ok(Err(error)) => return Err(ListingFailure::Failed(error.to_string())),
        Err(_) => {
            kill_process_tree(&mut child);
            return Err(ListingFailure::Timeout);
        }
    };
    let stderr = errors
        .and_then(|errors| {
            errors
                .recv_timeout(deadline.saturating_duration_since(std::time::Instant::now()))
                .ok()
        })
        .and_then(Result::ok)
        .unwrap_or_default();
    Ok(Listing {
        stdout: String::from_utf8_lossy(&output).into_owned(),
        stderr: String::from_utf8_lossy(&stderr).into_owned(),
        failure,
    })
}

fn kill_process_tree(child: &mut std::process::Child) {
    #[cfg(unix)]
    {
        use nix::sys::signal::{Signal, kill};
        use nix::unistd::Pid;
        if let Some(group) = tide_core::process::process_group_target(child.id()) {
            let _ = kill(Pid::from_raw(group), Signal::SIGKILL);
        }
    }
    let _ = child.kill();
    let _ = child.wait();
}

pub fn discover_gemini_sessions(workspace: &Path) -> Vec<CliSession> {
    try_discover_gemini(workspace).unwrap_or_default()
}

fn try_discover_gemini(workspace: &Path) -> Result<Vec<CliSession>, ListingFailure> {
    let executable = provider_executable(CliProvider::Gemini).ok_or(ListingFailure::Unavailable)?;
    gemini_sessions(
        run_listing(&executable, &["--list-sessions"], workspace)?,
        workspace,
    )
}

/// Gemini CLI prints "No previous sessions found for this project." for a project
/// without sessions, then may fail to sign in; that is still an empty list.
pub fn gemini_sessions(
    listing: Listing,
    workspace: &Path,
) -> Result<Vec<CliSession>, ListingFailure> {
    if listing.stdout.contains("No previous sessions found") {
        return Ok(Vec::new());
    }
    Ok(parse_gemini_sessions(&listing.output()?, workspace))
}

pub fn discover_opencode_sessions(workspace: &Path) -> Vec<CliSession> {
    try_discover_opencode(workspace).unwrap_or_default()
}

fn try_discover_opencode(workspace: &Path) -> Result<Vec<CliSession>, ListingFailure> {
    let executable =
        provider_executable(CliProvider::OpenCode).ok_or(ListingFailure::Unavailable)?;
    let listing = run_listing(
        &executable,
        &["session", "list", "--format", "json"],
        workspace,
    )?;
    opencode_sessions(listing, workspace)
}

/// OpenCode prints nothing at all for a project without sessions.
pub fn opencode_sessions(
    listing: Listing,
    workspace: &Path,
) -> Result<Vec<CliSession>, ListingFailure> {
    let output = listing.output()?;
    if output.trim().is_empty() {
        return Ok(Vec::new());
    }
    let value = serde_json::from_str::<Value>(&output)
        .map_err(|_| ListingFailure::Failed("malformed session JSON".into()))?;
    if !value.is_array() && !value.get("sessions").is_some_and(Value::is_array) {
        return Err(ListingFailure::Failed("unsupported session list".into()));
    }
    Ok(parse_opencode_sessions(&output, workspace))
}

pub fn parse_gemini_sessions(output: &str, cwd: &Path) -> Vec<CliSession> {
    let mut sessions = Vec::new();
    for line in output.lines() {
        let trimmed = line.trim();
        let Some((number, rest)) = trimmed.split_once(". ") else {
            continue;
        };
        if number.is_empty() || !number.chars().all(|ch| ch.is_ascii_digit()) {
            continue;
        }
        let Some(open) = rest.rfind('[') else {
            continue;
        };
        let Some(id) = rest[open + 1..].strip_suffix(']') else {
            continue;
        };
        let id = id.trim();
        if id.len() < 8 || !id.chars().all(|ch| ch.is_ascii_hexdigit() || ch == '-') {
            continue;
        }
        let mut title = rest[..open].trim();
        if title.ends_with(')')
            && let Some(index) = title.rfind(" (")
        {
            title = title[..index].trim();
        }
        sessions.push(CliSession {
            provider_id: CliProvider::Gemini.id().to_owned(),
            session_id: id.to_owned(),
            title: title.to_owned(),
            cwd: Some(cwd.display().to_string()),
            model: None,
            updated_at: None,
        });
    }
    sessions
}

pub fn parse_opencode_sessions(output: &str, fallback_cwd: &Path) -> Vec<CliSession> {
    let Ok(value) = serde_json::from_str::<Value>(output) else {
        return Vec::new();
    };
    let records = value
        .as_array()
        .cloned()
        .or_else(|| value.get("sessions").and_then(Value::as_array).cloned())
        .unwrap_or_default();
    let mut sessions = records
        .iter()
        .filter_map(|record| {
            let id = string_field(record, "id").filter(|id| safe_session_id(id))?;
            let title = string_field(record, "title")
                .unwrap_or_else(|| format!("OpenCode session {}", &id[..id.len().min(8)]));
            let cwd = string_field(record, "directory")
                .or_else(|| string_field(record, "cwd"))
                .unwrap_or_else(|| fallback_cwd.display().to_string());
            let updated = record
                .get("time")
                .and_then(|time| time.get("updated"))
                .and_then(Value::as_i64)
                .or_else(|| record.get("updated").and_then(Value::as_i64));
            let timestamp = updated.and_then(epoch_millis_to_rfc3339);
            Some(CliSession {
                provider_id: CliProvider::OpenCode.id().to_owned(),
                session_id: id,
                title,
                cwd: Some(cwd),
                model: string_field(record, "modelID").or_else(|| string_field(record, "model")),
                updated_at: timestamp,
            })
        })
        .collect::<Vec<_>>();
    sessions.sort_by(|left, right| right.updated_at.cmp(&left.updated_at));
    sessions
}

/// Antigravity stores conversation summaries in a small SQLite database.
pub fn discover_antigravity_sessions(home: &Path, workspace: &Path) -> Vec<CliSession> {
    let root = home.join(".gemini").join("antigravity-cli");
    let database = root.join("conversation_summaries.db");
    if !database.is_file() {
        return Vec::new();
    }
    let Ok(connection) = rusqlite::Connection::open_with_flags(
        &database,
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
    ) else {
        return Vec::new();
    };
    let version = connection
        .query_row("PRAGMA user_version", [], |row| row.get::<_, i64>(0))
        .unwrap_or(0);
    if version != 1 {
        return Vec::new();
    }
    let Ok(mut statement) = connection.prepare(
        "SELECT conversation_id, title, last_modified_time, workspace_uris \
         FROM conversation_summaries ORDER BY last_modified_time DESC LIMIT 200",
    ) else {
        return Vec::new();
    };
    let Ok(rows) = statement.query_map([], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1).unwrap_or_default(),
            row.get::<_, String>(2).unwrap_or_default(),
            row.get::<_, String>(3).unwrap_or_default(),
        ))
    }) else {
        return Vec::new();
    };
    let workspace = std::fs::canonicalize(workspace).unwrap_or_else(|_| workspace.to_path_buf());
    let mut sessions = Vec::new();
    for row in rows.flatten() {
        let (session_id, title, modified, workspace_uris) = row;
        let session_id = session_id.trim();
        if session_id.len() != 36
            || !session_id
                .chars()
                .all(|ch| ch.is_ascii_hexdigit() || ch == '-')
        {
            continue;
        }
        let Ok(uris) = serde_json::from_str::<Value>(&workspace_uris) else {
            continue;
        };
        let Some(uris) = uris.as_array() else {
            continue;
        };
        let mut matched = uris
            .iter()
            .filter_map(Value::as_str)
            .map(|uri| uri.strip_prefix("file://").unwrap_or(uri).to_owned())
            .map(|path| std::fs::canonicalize(&path).unwrap_or_else(|_| PathBuf::from(path)))
            .filter(|path| path.starts_with(&workspace))
            .collect::<Vec<_>>();
        matched.sort_by_key(|path| std::cmp::Reverse(path.components().count()));
        let Some(cwd) = matched.first() else {
            continue;
        };
        sessions.push(CliSession {
            provider_id: CliProvider::Antigravity.id().to_owned(),
            session_id: session_id.to_owned(),
            title: if title.trim().is_empty() {
                format!("Antigravity conversation {}", &session_id[..8])
            } else {
                title
            },
            cwd: Some(cwd.display().to_string()),
            model: None,
            updated_at: parse_antigravity_timestamp(&modified),
        });
    }
    sessions
}

fn parse_antigravity_timestamp(value: &str) -> Option<String> {
    let value = value.trim();
    if let Ok(number) = value.parse::<i64>() {
        let millis = if number < 10_000_000_000 {
            number.saturating_mul(1000)
        } else {
            number
        };
        return epoch_millis_to_rfc3339(millis);
    }
    chrono::DateTime::parse_from_rfc3339(value)
        .ok()
        .map(|time| time.to_rfc3339())
}

/// Codex stores paginated thread history in `~/.codex/thread_history_*.sqlite`.
pub fn discover_codex_sessions(home: &Path) -> Vec<CliSession> {
    let root = home.join(".codex");
    let database = std::fs::read_dir(&root).ok().and_then(|entries| {
        entries
            .flatten()
            .map(|entry| entry.path())
            .filter(|path| {
                path.file_name()
                    .and_then(|name| name.to_str())
                    .is_some_and(|name| {
                        name.starts_with("thread_history_") && name.ends_with(".sqlite")
                    })
            })
            .max()
    });
    let mut sessions = read_codex_jsonl(&root.join("sessions"));
    sessions.extend(read_codex_jsonl(&root.join("archived_sessions")));
    if let Some(database) = database {
        let titles = read_codex_sqlite(&database, home);
        for session in &mut sessions {
            if let Some(metadata) = titles
                .iter()
                .find(|row| row.session_id == session.session_id)
            {
                session.title = metadata.title.clone();
                session.updated_at = metadata
                    .updated_at
                    .clone()
                    .or_else(|| session.updated_at.clone());
            }
        }
    }
    normalize_sessions(&mut sessions);
    sessions
}

fn read_codex_sqlite(path: &Path, _home: &Path) -> Vec<CliSession> {
    let Ok(connection) =
        rusqlite::Connection::open_with_flags(path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
    else {
        return Vec::new();
    };
    let Ok(mut statement) = connection.prepare(
        "SELECT thread_id, item_json, created_at_ms FROM thread_items \
         WHERE item_type = 'userMessage' ORDER BY created_at_ms ASC LIMIT 200",
    ) else {
        return Vec::new();
    };
    let rows = statement.query_map([], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, i64>(2)?,
        ))
    });
    let Ok(rows) = rows else {
        return Vec::new();
    };
    let mut titles: std::collections::BTreeMap<String, (String, i64)> =
        std::collections::BTreeMap::new();
    for row in rows.flatten() {
        let (thread_id, item_json, created_at) = row;
        titles.entry(thread_id).or_insert_with(|| {
            let title = codex_title(&item_json).unwrap_or_else(|| "Codex session".to_owned());
            (title, created_at)
        });
    }
    let mut updated: std::collections::BTreeMap<String, i64> = std::collections::BTreeMap::new();
    if let Ok(mut statement) = connection.prepare(
        "SELECT thread_id, MAX(created_at_ms) FROM thread_items GROUP BY thread_id LIMIT 200",
    ) && let Ok(rows) = statement.query_map([], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
    }) {
        for row in rows.flatten() {
            updated.insert(row.0, row.1);
        }
    }
    titles
        .into_iter()
        .map(|(thread_id, (title, created_at))| CliSession {
            provider_id: CliProvider::Codex.id().to_owned(),
            updated_at: updated
                .get(&thread_id)
                .copied()
                .or(Some(created_at))
                .and_then(epoch_millis_to_rfc3339),
            session_id: thread_id,
            title,
            cwd: None,
            model: None,
        })
        .collect()
}

fn codex_title(item_json: &str) -> Option<String> {
    let value: Value = serde_json::from_str(item_json).ok()?;
    if let Some(text) = value
        .get("content")
        .and_then(Value::as_array)
        .and_then(|content| {
            content
                .iter()
                .find_map(|item| item.get("text").and_then(Value::as_str))
        })
        .map(str::trim)
        .filter(|text| !text.is_empty())
    {
        return Some(truncate_title(text));
    }
    value
        .get("text")
        .and_then(Value::as_str)
        .map(truncate_title)
}

fn truncate_title(text: &str) -> String {
    let first = text.lines().next().unwrap_or_default().trim();
    let mut title = first.chars().take(80).collect::<String>();
    if first.chars().count() > 80 {
        title.push('…');
    }
    title
}

pub(crate) fn epoch_millis_to_rfc3339(millis: i64) -> Option<String> {
    chrono::DateTime::from_timestamp_millis(millis).map(|time| time.to_rfc3339())
}

fn read_codex_jsonl(root: &Path) -> Vec<CliSession> {
    let mut sessions = Vec::new();
    let mut stack = vec![(root.to_path_buf(), 0)];
    let mut scanned = 0;
    while let Some((directory, depth)) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&directory) else {
            continue;
        };
        for entry in entries.flatten().take(4096) {
            if scanned >= 2048 {
                break;
            }
            let Ok(kind) = entry.file_type() else {
                continue;
            };
            let path = entry.path();
            if kind.is_dir() {
                if depth < 6 {
                    stack.push((path, depth + 1));
                }
                continue;
            }
            if path
                .extension()
                .is_none_or(|extension| extension != "jsonl")
            {
                continue;
            }
            if !kind.is_file() {
                continue;
            }
            scanned += 1;
            use std::io::{Read, Seek, SeekFrom};
            let Ok(mut file) = std::fs::File::open(&path) else {
                continue;
            };
            let mut bytes = Vec::new();
            let _ = (&mut file).take(65536).read_to_end(&mut bytes);
            if file.metadata().is_ok_and(|m| m.len() > 65536) {
                let _ = file.seek(SeekFrom::End(-65536));
                bytes.push(b'\n');
                let _ = file.take(65536).read_to_end(&mut bytes);
            }
            let text = String::from_utf8_lossy(&bytes);
            let values = text
                .lines()
                .filter_map(|line| serde_json::from_str::<Value>(line).ok())
                .collect::<Vec<_>>();
            let Some(meta) = values
                .iter()
                .find(|value| value.get("type").and_then(Value::as_str) == Some("session_meta"))
            else {
                continue;
            };
            let payload = meta.get("payload").unwrap_or(meta);
            let Some(id) = string_field(payload, "id").filter(|id| safe_session_id(id)) else {
                continue;
            };
            let title = values
                .iter()
                .find_map(|value| {
                    let payload = value.get("payload")?;
                    (payload.get("type").and_then(Value::as_str) == Some("user_message"))
                        .then(|| payload.get("message").and_then(Value::as_str))
                        .flatten()
                        .map(truncate_title)
                })
                .unwrap_or_else(|| format!("Codex session {}", &id[..id.len().min(8)]));
            sessions.push(CliSession {
                provider_id: CliProvider::Codex.id().to_owned(),
                session_id: id,
                title,
                cwd: string_field(payload, "cwd"),
                model: string_field(payload, "model"),
                updated_at: string_field(meta, "timestamp")
                    .or_else(|| string_field(payload, "timestamp")),
            });
        }
    }
    sessions
}

/// One row per main Claude session; subagent transcripts are not sessions.
pub fn discover_claude_sessions(root: &Path) -> Vec<CliSession> {
    let mut sessions = Vec::new();
    collect_jsonl(root, &mut sessions, 0, &mut 0);
    normalize_sessions(&mut sessions);
    sessions
}

pub fn normalize_sessions(sessions: &mut Vec<CliSession>) {
    sessions.retain(|session| {
        CliProvider::parse(&session.provider_id).is_some() && safe_session_id(&session.session_id)
    });
    for session in sessions.iter_mut() {
        session.title = session
            .title
            .chars()
            .map(|ch| if ch.is_control() { ' ' } else { ch })
            .take(256)
            .collect();
        session.model = session.model.as_ref().map(|model| {
            model
                .chars()
                .filter(|ch| !ch.is_control())
                .take(128)
                .collect()
        });
        if session.cwd.as_ref().is_some_and(|cwd| cwd.len() > 8192) {
            session.cwd = None;
        }
    }
    sessions.sort_by_key(|session| std::cmp::Reverse(session_updated_at(session)));
    let mut seen = std::collections::HashSet::new();
    sessions
        .retain(|session| seen.insert((session.provider_id.clone(), session.session_id.clone())));
}

pub fn session_updated_at(session: &CliSession) -> Option<i64> {
    let value = session.updated_at.as_deref()?;
    chrono::DateTime::parse_from_rfc3339(value)
        .ok()
        .map(|time| time.timestamp_millis())
        .or_else(|| {
            chrono::NaiveDate::parse_from_str(value, "%Y-%m-%d")
                .ok()?
                .and_hms_opt(0, 0, 0)
                .map(|time| time.and_utc().timestamp_millis())
        })
}

fn collect_jsonl(dir: &Path, sessions: &mut Vec<CliSession>, depth: usize, scanned: &mut usize) {
    use std::io::{Read, Seek, SeekFrom};
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    let mut entries = entries.flatten().take(4096).collect::<Vec<_>>();
    entries.sort_by_key(|entry| {
        std::cmp::Reverse(entry.metadata().ok().and_then(|m| m.modified().ok()))
    });
    for entry in entries {
        if *scanned >= 2048 {
            return;
        }
        let Ok(kind) = entry.file_type() else {
            continue;
        };
        let path = entry.path();
        if kind.is_dir() {
            if depth < 1 && entry.file_name() != "subagents" {
                collect_jsonl(&path, sessions, depth + 1, scanned);
            }
            continue;
        }
        if !kind.is_file()
            || path
                .extension()
                .is_none_or(|extension| extension != "jsonl")
            || path
                .file_name()
                .is_some_and(|name| name.to_string_lossy().starts_with("agent-"))
        {
            continue;
        }
        *scanned += 1;
        let Ok(mut file) = std::fs::File::open(&path) else {
            continue;
        };
        // Read bounded head/tail metadata even for large conversations. Never
        // deserialize the entire transcript or follow nested subagent trees.
        let mut bytes = Vec::new();
        let _ = (&mut file).take(65536).read_to_end(&mut bytes);
        if file.metadata().is_ok_and(|metadata| metadata.len() > 65536) {
            let _ = file.seek(SeekFrom::End(-65536));
            bytes.push(b'\n');
            let _ = file.take(65536).read_to_end(&mut bytes);
        }
        let text = String::from_utf8_lossy(&bytes);
        let records = text
            .lines()
            .filter_map(|line| serde_json::from_str::<Value>(line).ok())
            .collect::<Vec<_>>();
        if records
            .iter()
            .any(|record| record.get("isSidechain").and_then(Value::as_bool) == Some(true))
        {
            continue;
        }
        let latest = |key| {
            records
                .iter()
                .rev()
                .find_map(|record| string_field(record, key))
        };
        let session_id = latest("sessionId").or_else(|| {
            path.file_stem()
                .and_then(|stem| stem.to_str())
                .map(str::to_owned)
        });
        let Some(session_id) = session_id.filter(|id| safe_session_id(id)) else {
            continue;
        };
        let title = latest("customTitle")
            .or_else(|| latest("slug"))
            .unwrap_or_else(|| {
                format!("Claude session {}", &session_id[..session_id.len().min(8)])
            });
        sessions.push(CliSession {
            provider_id: CliProvider::ClaudeCode.id().to_owned(),
            session_id,
            title,
            cwd: latest("cwd"),
            model: records
                .iter()
                .rev()
                .find_map(|record| {
                    record
                        .get("message")
                        .and_then(|message| message.get("model"))
                        .and_then(Value::as_str)
                        .map(str::to_owned)
                })
                .or_else(|| latest("model")),
            updated_at: latest("timestamp"),
        });
    }
}

/// Session IDs become CLI arguments, so a leading `-` (an option) is refused.
pub(crate) fn safe_session_id(id: &str) -> bool {
    !id.is_empty()
        && !id.starts_with('-')
        && id.len() <= 200
        && id
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.'))
}

fn string_field(value: &Value, key: &str) -> Option<String> {
    value
        .get(key)
        .and_then(Value::as_str)
        .map(str::to_owned)
        .filter(|value| !value.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn discovery_drains_output_larger_than_a_pipe_without_deadlocking() {
        let dir = tempfile::tempdir().unwrap();
        let result = run_listing(
            Path::new("/bin/sh"),
            &["-c", "/usr/bin/head -c 131072 /dev/zero"],
            dir.path(),
        )
        .unwrap();
        assert_eq!(result.stdout.len(), 131072);
    }

    #[test]
    fn empty_listings_are_no_sessions_and_sign_in_problems_are_not_errors() {
        let workspace = Path::new("/work");
        let listing = |stdout: &str, stderr: &str, failure: Option<&str>| Listing {
            stdout: stdout.to_owned(),
            stderr: stderr.to_owned(),
            failure: failure.map(str::to_owned),
        };
        assert_eq!(
            opencode_sessions(listing("", "", None), workspace),
            Ok(Vec::new())
        );
        assert_eq!(
            opencode_sessions(listing("\n  \n", "", None), workspace),
            Ok(Vec::new())
        );
        assert_eq!(
            opencode_sessions(listing("not json", "", None), workspace),
            Err(ListingFailure::Failed("malformed session JSON".into()))
        );
        assert_eq!(
            gemini_sessions(
                listing(
                    "No previous sessions found for this project.",
                    "Error authenticating: IneligibleTierError: this client is not supported",
                    Some("exit status: 1"),
                ),
                workspace
            ),
            Ok(Vec::new())
        );
        assert_eq!(
            gemini_sessions(
                listing(
                    "",
                    "Please set an Auth method in your settings.json or specify GEMINI_API_KEY",
                    Some("exit status: 1"),
                ),
                workspace
            ),
            Err(ListingFailure::NeedsSignIn(
                "Please set an Auth method in your settings.json or specify GEMINI_API_KEY".into()
            ))
        );
        assert_eq!(
            opencode_sessions(listing("", "segfault", Some("exit status: 139")), workspace),
            Err(ListingFailure::Failed(
                "listing exited with exit status: 139".into()
            ))
        );
    }

    #[test]
    fn a_failed_listing_keeps_what_it_printed() {
        let dir = tempfile::tempdir().unwrap();
        let listing = run_listing(
            Path::new("/bin/sh"),
            &["-c", "echo listed; echo 'Not logged in' >&2; exit 3"],
            dir.path(),
        )
        .unwrap();
        assert_eq!(listing.stdout, "listed\n");
        assert_eq!(listing.stderr, "Not logged in\n");
        assert_eq!(
            listing.output(),
            Err(ListingFailure::NeedsSignIn("Not logged in".into()))
        );
    }

    #[test]
    fn all_five_cli_adapters_are_declared() {
        assert_eq!(CLI_PROVIDERS.len(), 5);
        assert_eq!(
            CliProvider::parse("claude-code"),
            Some(CliProvider::ClaudeCode)
        );
        assert_eq!(CliProvider::Codex.display_name(), "Codex");
        assert_eq!(CliProvider::OpenCode.executable(), "opencode");
    }

    #[test]
    fn launch_arguments_resume_native_sessions() {
        let cwd = Path::new("/tmp/work");
        assert_eq!(
            CliProvider::ClaudeCode.launch_args(cwd, Some("abc")),
            vec!["--resume", "abc"]
        );
        assert_eq!(
            CliProvider::ClaudeCode.launch_args(cwd, None)[0],
            "--session-id"
        );
        assert_eq!(
            CliProvider::Codex.launch_args(cwd, Some("id")),
            vec!["resume", "-C", "/tmp/work", "--", "id"]
        );
        assert_eq!(
            CliProvider::Antigravity.launch_args(cwd, Some("c")),
            vec!["--conversation", "c"]
        );
        assert!(CliProvider::OpenCode.launch_args(cwd, None).is_empty());
    }

    #[test]
    fn session_ids_that_look_like_options_are_never_passed_to_a_cli() {
        assert!(!safe_session_id("-x"));
        assert!(!safe_session_id("--dangerously-skip-permissions"));
        assert!(safe_session_id("ses_abc-123"));
        let cwd = Path::new("/tmp/work");
        let hostile = "--dangerously-skip-permissions";
        for provider in CLI_PROVIDERS {
            let args = provider.launch_args(cwd, Some(hostile));
            assert!(
                !args.iter().any(|arg| arg == hostile),
                "{}: {args:?}",
                provider.id()
            );
            assert_eq!(provider.delete_args(hostile), None);
            assert_eq!(provider.archive_args(hostile), None);
        }
        assert_eq!(
            CliProvider::Codex.launch_args(cwd, Some(hostile)),
            vec!["-C", "/tmp/work"]
        );
        assert_eq!(
            CliProvider::Codex.delete_args("id"),
            Some(vec!["delete".to_owned(), "--".to_owned(), "id".to_owned()])
        );
        assert_eq!(
            CliProvider::Codex.archive_args("id"),
            Some(vec!["archive".to_owned(), "--".to_owned(), "id".to_owned()])
        );
    }

    #[test]
    fn new_sessions_expose_an_identity_when_the_provider_allows_it() {
        let (args, id) = CliProvider::ClaudeCode.prepare_new(Path::new("/tmp/work"));
        assert_eq!(args[0], "--session-id");
        assert_eq!(id.as_deref(), Some(args[1].as_str()));
        let (_, id) = CliProvider::Codex.prepare_new(Path::new("/tmp/work"));
        assert!(id.is_none());
        assert_eq!(
            CliProvider::OpenCode.delete_args("ses_abc"),
            Some(vec![
                "session".to_owned(),
                "delete".to_owned(),
                "ses_abc".to_owned()
            ])
        );
        assert!(CliProvider::ClaudeCode.delete_args("abc").is_none());
        assert!(!CliProvider::ClaudeCode.supports_rename());
    }

    #[test]
    fn detection_finds_executables_in_path() {
        let dir = tempfile::tempdir().unwrap();
        let executable = dir.path().join("codex");
        std::fs::write(&executable, "#!/bin/sh\n").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        temp_override_path(dir.path(), || {
            let status = detect(CliProvider::Codex);
            assert!(status.installed);
            assert_eq!(status.executable.as_deref(), Some(executable.as_path()));
        });
        let status = detect(CliProvider::OpenCode);
        // The host may have OpenCode installed; only assert the probe shape.
        assert_eq!(status.provider, CliProvider::OpenCode);
    }

    #[cfg(unix)]
    #[test]
    fn opencode_runs_only_its_delete_action() {
        use std::os::unix::fs::PermissionsExt as _;
        let dir = tempfile::tempdir().unwrap();
        let marker = dir.path().join("ran");
        let executable = dir.path().join("opencode");
        std::fs::write(
            &executable,
            format!("#!/bin/sh\ntouch '{}'\n", marker.display()),
        )
        .unwrap();
        std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o755)).unwrap();
        temp_env::with_var("TIDE_OPENCODE_PATH", Some(executable.as_os_str()), || {
            for args in [
                vec!["session", "rename", "ses_abc"],
                vec!["archive", "--", "ses_abc"],
                vec!["ses_abc"],
                vec![],
            ] {
                let args = args.into_iter().map(str::to_owned).collect::<Vec<_>>();
                assert_eq!(
                    run_session_action(CliProvider::OpenCode, dir.path(), &args),
                    Err("Unsupported OpenCode session action".to_owned()),
                    "{args:?}"
                );
            }
        });
        assert!(!marker.exists(), "an unsupported action started OpenCode");
    }

    fn temp_override_path(path: &Path, run: impl FnOnce()) {
        temp_env::with_var("PATH", Some(path.as_os_str()), run);
    }

    #[test]
    fn parses_opencode_session_json() {
        let output = r#"[
            {"id":"ses_abc12345","title":"Refactor login","directory":"/work/app","updated":1793162814049,"modelID":"claude-sonnet"},
            {"id":"ses_def67890","time":{"updated":1793162814048}}
        ]"#;
        let sessions = parse_opencode_sessions(output, Path::new("/fallback"));
        assert_eq!(sessions.len(), 2);
        assert_eq!(sessions[0].session_id, "ses_abc12345");
        assert_eq!(sessions[0].title, "Refactor login");
        assert_eq!(sessions[0].cwd.as_deref(), Some("/work/app"));
        assert_eq!(sessions[0].model.as_deref(), Some("claude-sonnet"));
        assert!(sessions[0].updated_at.is_some());
        assert_eq!(sessions[1].title, "OpenCode session ses_def6");
        assert_eq!(sessions[1].cwd.as_deref(), Some("/fallback"));
        assert!(parse_opencode_sessions("not json", Path::new("/")).is_empty());
    }

    #[test]
    fn parses_gemini_session_listing() {
        let output = "  1. Fix login bug (2 days ago) [01234567-89ab-cdef]\n  2. Explain repo [abcdef12]\nnot a session\n";
        let sessions = parse_gemini_sessions(output, Path::new("/work"));
        assert_eq!(sessions.len(), 2);
        assert_eq!(sessions[0].title, "Fix login bug");
        assert_eq!(sessions[0].session_id, "01234567-89ab-cdef");
        assert_eq!(sessions[1].title, "Explain repo");
        assert_eq!(sessions[1].cwd.as_deref(), Some("/work"));
    }

    #[test]
    fn discovers_antigravity_sessions_for_matching_workspaces() {
        let home = tempfile::tempdir().unwrap();
        let workspace = tempfile::tempdir().unwrap();
        let root = home.path().join(".gemini").join("antigravity-cli");
        std::fs::create_dir_all(&root).unwrap();
        let database = rusqlite::Connection::open(root.join("conversation_summaries.db")).unwrap();
        database
            .execute_batch(&format!(
                "PRAGMA user_version = 1;
                 CREATE TABLE conversation_summaries (
                    conversation_id TEXT, title TEXT,
                    last_modified_time TEXT, workspace_uris TEXT
                 );
                 INSERT INTO conversation_summaries VALUES
                   ('01234567-89ab-4def-8123-456789abcdef','Antigravity task','1793162814049','[\"{}\"]'),
                   ('01234567-89ab-4def-8123-456789abcdee','Other task','1793162814049','[\"/somewhere/else\"]');",
                workspace.path().display()
            ))
            .unwrap();
        let sessions = discover_antigravity_sessions(home.path(), workspace.path());
        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].title, "Antigravity task");
        assert!(sessions[0].updated_at.is_some());
    }

    #[test]
    fn discovers_codex_sessions_from_thread_history_sqlite() {
        let home = tempfile::tempdir().unwrap();
        let codex = home.path().join(".codex");
        std::fs::create_dir_all(&codex).unwrap();
        let connection = rusqlite::Connection::open(codex.join("thread_history_1.sqlite")).unwrap();
        connection
            .execute_batch(
                "CREATE TABLE thread_items (
                    thread_id TEXT NOT NULL,
                    turn_id TEXT NOT NULL,
                    item_id TEXT NOT NULL,
                    rollout_ordinal INTEGER NOT NULL,
                    created_at_ms INTEGER NOT NULL,
                    item_json TEXT NOT NULL,
                    item_type TEXT NOT NULL DEFAULT ''
                );
                INSERT INTO thread_items VALUES
                  ('thread-1','turn-1','item-1',1,1000,
                   '{\"content\":[{\"text\":\"Fix the build\"}]}','userMessage'),
                  ('thread-1','turn-1','item-2',2,2000,'{}','assistantMessage');",
            )
            .unwrap();
        // History rows have no workspace identity and must never manufacture one.
        assert!(discover_codex_sessions(home.path()).is_empty());
        let rollouts = codex.join("sessions/2026/10/28");
        std::fs::create_dir_all(&rollouts).unwrap();
        std::fs::write(
            rollouts.join("thread-1.jsonl"),
            r#"{"type":"session_meta","payload":{"id":"thread-1","cwd":"/work/project"}}"#,
        )
        .unwrap();
        let sessions = discover_codex_sessions(home.path());
        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].session_id, "thread-1");
        assert_eq!(sessions[0].title, "Fix the build");
        assert_eq!(sessions[0].cwd.as_deref(), Some("/work/project"));
        assert!(sessions[0].updated_at.is_some());
    }

    #[test]
    fn discovery_reports_timeouts_without_hanging() {
        let home = tempfile::tempdir().unwrap();
        let workspace = tempfile::tempdir().unwrap();
        let bin = tempfile::tempdir().unwrap();
        let script = bin.path().join("opencode");
        std::fs::write(&script, "#!/bin/sh\n/bin/sleep 10\n").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        temp_env::with_var("PATH", Some(bin.path().as_os_str()), || {
            let started = std::time::Instant::now();
            let report = discover_all_with_status(home.path(), workspace.path());
            assert!(started.elapsed() < std::time::Duration::from_secs(6));
            assert_eq!(report.state, Some(DiscoveryState::Error));
            assert!(report.sessions.is_empty());
            assert!(
                report
                    .errors
                    .iter()
                    .any(|error| error.contains("timed out")),
                "{:?}",
                report.errors
            );
        });
    }

    #[test]
    fn claude_discovery_excludes_subagents_and_deduplicates_real_session_ids() {
        let root = tempfile::tempdir().unwrap();
        let project = root.path().join("project");
        let agents = project.join("s-1/subagents");
        std::fs::create_dir_all(&agents).unwrap();
        for index in 0..220 {
            std::fs::write(
                agents.join(format!("agent-{index}.jsonl")),
                r#"{"sessionId":"s-1","slug":"duplicate-agent","isSidechain":true}"#,
            )
            .unwrap();
        }
        let original = r#"{"sessionId":"s-1","slug":"same-title","cwd":"/work/a","timestamp":"2026-01-01T00:00:00Z"}"#;
        std::fs::write(project.join("s-1.jsonl"), original).unwrap();
        std::fs::write(project.join("copy.jsonl"), original).unwrap();
        std::fs::write(project.join("s-2.jsonl"), original.replace("s-1", "s-2")).unwrap();
        std::fs::write(
            project.join("agent-legacy.jsonl"),
            original.replace("s-1", "legacy"),
        )
        .unwrap();
        let sessions = discover_claude_sessions(root.path());
        assert_eq!(sessions.len(), 2);
        assert_eq!(
            sessions
                .iter()
                .filter(|session| session.session_id == "s-1")
                .count(),
            1
        );
        assert!(sessions.iter().all(|session| session.title == "same-title"));
    }

    #[test]
    fn claude_discovery_reads_bounded_metadata_from_large_sessions() {
        let root = tempfile::tempdir().unwrap();
        let file = root.path().join("large.jsonl");
        let mut text = String::from("{\"sessionId\":\"large-session\",\"cwd\":\"/work/a\"}\n");
        text.push_str(&"x".repeat(3 * 1024 * 1024));
        text.push_str("\n{\"type\":\"custom-title\",\"customTitle\":\"Real conversation\",\"timestamp\":\"2026-02-01T00:00:00Z\"}\n");
        std::fs::write(file, text).unwrap();
        let sessions = discover_claude_sessions(root.path());
        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].session_id, "large-session");
        assert_eq!(sessions[0].title, "Real conversation");
    }

    #[test]
    fn metadata_paths_and_recency_are_normalized_without_inventing_workspaces() {
        use std::os::unix::fs::symlink;
        let root = tempfile::tempdir().unwrap();
        let real = root.path().join("real");
        std::fs::create_dir(&real).unwrap();
        let alias = root.path().join("alias");
        symlink(&real, &alias).unwrap();
        let make = |id: &str, cwd: &str, time: &str| CliSession {
            provider_id: "claude-code".into(),
            session_id: id.into(),
            title: id.into(),
            cwd: Some(cwd.into()),
            model: None,
            updated_at: Some(time.into()),
        };
        let mut sessions = vec![
            make("same", alias.to_str().unwrap(), "2026-01-01T12:00:00+02:00"),
            make("same", real.to_str().unwrap(), "2026-01-01T11:00:00Z"),
            make("relative", "relative/path", "2026-01-01"),
        ];
        canonicalize_session_cwds(&mut sessions);
        assert_eq!(
            sessions[0].cwd.as_deref(),
            std::fs::canonicalize(&real).unwrap().to_str()
        );
        assert_eq!(sessions[2].cwd.as_deref(), Some("relative/path"));
        normalize_sessions(&mut sessions);
        assert_eq!(sessions.len(), 2);
        assert_eq!(
            sessions[0].updated_at.as_deref(),
            Some("2026-01-01T11:00:00Z")
        );
    }

    #[test]
    fn discovers_claude_code_sessions_from_jsonl_metadata() {
        let root = tempfile::tempdir().unwrap();
        let project = root.path().join("project-a");
        std::fs::create_dir_all(&project).unwrap();
        let file = project.join("session-1.jsonl");
        std::fs::write(
            &file,
            concat!(
                "{\"sessionId\":\"s-1\",\"cwd\":\"/work/a\",\"slug\":\"first-slug\",\"timestamp\":\"2026-01-01T00:00:00Z\"}\n",
                "{\"sessionId\":\"s-1\",\"cwd\":\"/work/a\",\"model\":\"claude-sonnet\",\"timestamp\":\"2026-01-02T00:00:00Z\"}\n",
            ),
        )
        .unwrap();
        let sessions = discover_claude_sessions(root.path());
        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].session_id, "s-1");
        assert_eq!(sessions[0].title, "first-slug");
        assert_eq!(sessions[0].model.as_deref(), Some("claude-sonnet"));
        assert_eq!(
            sessions[0].updated_at.as_deref(),
            Some("2026-01-02T00:00:00Z")
        );
    }
}
