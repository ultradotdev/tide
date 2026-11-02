use std::io::{Read, Write};
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, SyncSender};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, AtomicUsize, Ordering},
};
use std::time::Duration;

use portable_pty::{Child, CommandBuilder, MasterPty, PtySize, native_pty_system};

/// Inputs for a PTY-backed process.
#[derive(Debug, Clone)]
pub struct PtyConfig {
    pub program: String,
    pub args: Vec<String>,
    pub cwd: Option<PathBuf>,
    pub env: Vec<(String, String)>,
    pub cols: u16,
    pub rows: u16,
    /// Called on the reader thread whenever output is queued or the PTY closes,
    /// so an event loop can wake instead of polling.
    pub on_output: Option<OutputSignal>,
}

#[derive(Clone)]
pub struct OutputSignal(pub Arc<dyn Fn() + Send + Sync>);

impl std::fmt::Debug for OutputSignal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("OutputSignal")
    }
}

impl PtyConfig {
    #[must_use]
    pub fn shell(cwd: Option<PathBuf>) -> Self {
        let program = std::env::var("SHELL").unwrap_or_else(|_| "/bin/sh".to_owned());
        let shell_name = std::path::Path::new(&program)
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or_default();
        let args = if matches!(shell_name, "bash" | "zsh" | "sh") {
            vec!["-il".to_owned()]
        } else {
            Vec::new()
        };
        Self {
            program,
            args,
            cwd,
            env: vec![
                ("TERM".to_owned(), "xterm-256color".to_owned()),
                ("COLORTERM".to_owned(), "truecolor".to_owned()),
                ("TERM_PROGRAM".to_owned(), "tide".to_owned()),
            ],
            cols: 80,
            rows: 24,
            on_output: None,
        }
    }

    #[must_use]
    pub fn command(program: impl Into<String>, args: impl IntoIterator<Item = String>) -> Self {
        Self {
            program: program.into(),
            args: args.into_iter().collect(),
            cwd: None,
            env: Vec::new(),
            cols: 80,
            rows: 24,
            on_output: None,
        }
    }
}

/// A spawned PTY plus a background reader that forwards output as byte chunks.
pub struct PtySession {
    master: Box<dyn MasterPty + Send>,
    input: Option<SyncSender<Vec<u8>>>,
    input_state: Arc<InputState>,
    child: Box<dyn Child + Send + Sync>,
    output: Option<Receiver<Vec<u8>>>,
    size: PtySize,
    pub pid: Option<u32>,
    exit_code: Option<u32>,
}

/// Why a terminal operation failed.
#[derive(Debug, thiserror::Error)]
pub enum TerminalError {
    /// The pseudo-terminal itself failed (`operation` names the step).
    #[error("{operation}: {reason}")]
    Pty {
        operation: &'static str,
        reason: String,
    },
    #[error("spawn {program}: {reason}")]
    Spawn { program: String, reason: String },
    #[error("start PTY thread: {0}")]
    Thread(std::io::Error),
    #[error("Terminal input exceeds 4 MiB; paste a smaller chunk")]
    InputTooLarge,
    #[error("Terminal input is closed")]
    InputClosed,
    #[error("Terminal input buffer is full; retry after the child reads input")]
    InputFull,
    #[error("PTY child has no process identity")]
    NoProcess,
    #[error("wait for the PTY child: {0}")]
    Wait(#[from] std::io::Error),
    #[error("PTY process did not exit before the shutdown deadline")]
    ShutdownTimeout,
    #[error("PTY smoke check produced unexpected output: {0:?}")]
    SmokeCheck(String),
}

impl TerminalError {
    fn pty(operation: &'static str) -> impl FnOnce(anyhow::Error) -> Self {
        move |error| Self::Pty {
            operation,
            reason: format!("{error:#}"),
        }
    }
}

#[cfg(unix)]
impl From<rustix::io::Errno> for TerminalError {
    fn from(error: rustix::io::Errno) -> Self {
        Self::Wait(error.into())
    }
}

pub type Result<T, E = TerminalError> = std::result::Result<T, E>;

const MAX_INPUT_WRITE: usize = 4 * 1024 * 1024 + 16;
const MAX_QUEUED_INPUT: usize = 8 * 1024 * 1024;
const HANGUP_GRACE: Duration = Duration::from_millis(500);
#[derive(Default)]
struct InputState {
    queued: AtomicUsize,
    closed: AtomicBool,
    error: Mutex<Option<String>>,
}

impl PtySession {
    pub fn spawn(config: PtyConfig) -> Result<Self> {
        let pty_system = native_pty_system();
        let size = PtySize {
            rows: config.rows.max(1),
            cols: config.cols.max(2),
            pixel_width: 0,
            pixel_height: 0,
        };
        let pair = pty_system
            .openpty(size)
            .map_err(TerminalError::pty("open a pseudo-terminal"))?;

        let mut command = CommandBuilder::new(&config.program);
        for name in tide_core::process::inherited_vars(tide_core::process::is_tide_secret) {
            command.env_remove(name);
        }
        command.args(&config.args);
        if let Some(cwd) = &config.cwd {
            command.cwd(cwd);
        }
        for (key, value) in &config.env {
            command.env(key, value);
        }

        let mut reader = pair
            .master
            .try_clone_reader()
            .map_err(TerminalError::pty("clone PTY reader"))?;
        let mut writer = pair
            .master
            .take_writer()
            .map_err(TerminalError::pty("take PTY writer"))?;
        let child = pair
            .slave
            .spawn_command(command)
            .map_err(|error| TerminalError::Spawn {
                program: config.program.clone(),
                reason: format!("{error:#}"),
            })?;
        drop(pair.slave);

        let pid = child.process_id();
        let (tx, output) = mpsc::sync_channel(64);
        let (input_sender, input_receiver) = mpsc::sync_channel::<Vec<u8>>(64);
        let input_state = Arc::new(InputState::default());
        let session = Self {
            master: pair.master,
            input: Some(input_sender),
            input_state: input_state.clone(),
            child,
            output: Some(output),
            size,
            pid,
            exit_code: None,
        };
        // Both threads are detached. They end when the slave side or their
        // channel closes, and closing the session guarantees both.
        std::thread::Builder::new()
            .name(format!("tide-pty-{}", pid.unwrap_or(0)))
            .spawn(move || {
                forward_output(&mut reader, &tx, config.on_output.as_ref());
            })
            .map_err(TerminalError::Thread)?;
        std::thread::Builder::new()
            .name(format!("tide-pty-input-{}", pid.unwrap_or(0)))
            .spawn(move || {
                while let Ok(bytes) = input_receiver.recv() {
                    let length = bytes.len();
                    let result = writer.write_all(&bytes).and_then(|_| writer.flush());
                    input_state.queued.fetch_sub(length, Ordering::AcqRel);
                    if let Err(error) = result {
                        input_state.closed.store(true, Ordering::Release);
                        *input_state.error.lock().unwrap() =
                            Some(format!("Terminal input closed: {error}"));
                        break;
                    }
                }
            })
            .map_err(TerminalError::Thread)?;
        Ok(session)
    }

    /// Queue one complete input request without blocking the UI on the child.
    pub fn write(&mut self, bytes: &[u8]) -> Result<()> {
        if bytes.is_empty() {
            return Ok(());
        }
        if bytes.len() > MAX_INPUT_WRITE {
            return Err(TerminalError::InputTooLarge);
        }
        if self.input_state.closed.load(Ordering::Acquire) {
            return Err(TerminalError::InputClosed);
        }
        let mut queued = self.input_state.queued.load(Ordering::Acquire);
        loop {
            let next = queued
                .checked_add(bytes.len())
                .filter(|next| *next <= MAX_QUEUED_INPUT)
                .ok_or(TerminalError::InputFull)?;
            match self.input_state.queued.compare_exchange_weak(
                queued,
                next,
                Ordering::AcqRel,
                Ordering::Acquire,
            ) {
                Ok(_) => break,
                Err(current) => queued = current,
            }
        }
        let result = self
            .input
            .as_ref()
            .ok_or(TerminalError::InputClosed)
            .and_then(|input| {
                input.try_send(bytes.to_vec()).map_err(|error| match error {
                    mpsc::TrySendError::Full(_) => TerminalError::InputFull,
                    mpsc::TrySendError::Disconnected(_) => TerminalError::InputClosed,
                })
            });
        if result.is_err() {
            self.input_state
                .queued
                .fetch_sub(bytes.len(), Ordering::AcqRel);
        }
        result
    }

    pub fn take_write_error(&mut self) -> Option<String> {
        self.input_state.error.lock().unwrap().take()
    }

    pub fn resize(&mut self, cols: u16, rows: u16) -> Result<()> {
        self.size = PtySize {
            rows: rows.max(1),
            cols: cols.max(2),
            pixel_width: 0,
            pixel_height: 0,
        };
        self.master
            .resize(self.size)
            .map_err(TerminalError::pty("resize the pseudo-terminal"))
    }

    #[must_use]
    pub fn size(&self) -> (u16, u16) {
        (self.size.cols, self.size.rows)
    }

    /// Drain currently available output without blocking.
    #[must_use]
    pub fn drain(&self) -> Vec<u8> {
        let mut bytes = Vec::new();
        if let Some(output) = &self.output {
            while bytes.len() < 1024 * 1024 {
                let Ok(chunk) = output.try_recv() else {
                    break;
                };
                bytes.extend_from_slice(&chunk);
            }
        }
        bytes
    }

    /// Wait up to `timeout` for output or process exit.
    pub fn read_for(&self, timeout: Duration) -> Vec<u8> {
        let mut bytes = Vec::new();
        let deadline = std::time::Instant::now() + timeout;
        loop {
            let remaining = deadline.saturating_duration_since(std::time::Instant::now());
            if remaining.is_zero() {
                break;
            }
            let Some(output) = &self.output else {
                break;
            };
            match output.recv_timeout(remaining) {
                Ok(chunk) => bytes.extend_from_slice(&chunk),
                Err(_) => break,
            }
        }
        bytes
    }

    pub fn poll_exit(&mut self) -> Result<Option<u32>> {
        if let Some(code) = self.exit_code {
            return Ok(Some(code));
        }
        #[cfg(unix)]
        {
            let Some(pid) = self.owned_pid() else {
                return Err(TerminalError::NoProcess);
            };
            if rustix::process::waitid(
                rustix::process::WaitId::Pid(pid),
                rustix::process::WaitIdOptions::EXITED
                    | rustix::process::WaitIdOptions::NOHANG
                    | rustix::process::WaitIdOptions::NOWAIT,
            )?
            .is_none()
            {
                return Ok(None);
            }
            // The unreaped child reserves its PID (and shell process group).
            // Clean up helpers before releasing that identity to the OS.
            self.stop_owned_groups();
        }
        let result = self.child.try_wait()?.map(|status| status.exit_code());
        self.exit_code = result;
        Ok(result)
    }

    pub fn wait(&mut self) -> Result<u32> {
        if let Some(code) = self.exit_code {
            return Ok(code);
        }
        #[cfg(unix)]
        {
            let pid = self.owned_pid().ok_or(TerminalError::NoProcess)?;
            rustix::process::waitid(
                rustix::process::WaitId::Pid(pid),
                rustix::process::WaitIdOptions::EXITED | rustix::process::WaitIdOptions::NOWAIT,
            )?;
            self.stop_owned_groups();
        }
        let code = self.child.wait()?.exit_code();
        self.exit_code = Some(code);
        Ok(code)
    }

    #[cfg(unix)]
    fn owned_pid(&self) -> Option<rustix::process::Pid> {
        self.pid
            .filter(|pid| *pid > 1)
            .and_then(|pid| i32::try_from(pid).ok())
            .and_then(rustix::process::Pid::from_raw)
    }

    #[cfg(unix)]
    fn stop_owned_groups(&self) {
        use nix::{
            sys::signal::{Signal, kill},
            unistd::{Pid, getsid},
        };
        let Some(pid) = self.owned_pid() else { return };
        let raw = pid.as_raw_nonzero().get();
        if !reserved(pid) {
            return;
        }
        if let Some(group) = self.foreground_group()
            && group != raw
            && getsid(Some(Pid::from_raw(group))).is_ok_and(|sid| sid.as_raw() == raw)
        {
            let _ = kill(Pid::from_raw(-group), Signal::SIGKILL);
        }
        let _ = kill(Pid::from_raw(-raw), Signal::SIGKILL);
        let _ = kill(Pid::from_raw(raw), Signal::SIGKILL);
    }

    #[cfg(unix)]
    fn foreground_group(&self) -> Option<i32> {
        self.master.process_group_leader().filter(|id| *id > 1)
    }

    /// Whether a job other than the session leader, such as a command the shell started,
    /// owns the terminal's foreground.
    #[must_use]
    pub fn has_foreground_job(&self) -> bool {
        #[cfg(unix)]
        {
            let Some(leader) = self.owned_pid() else {
                return false;
            };
            self.exit_code.is_none()
                && self
                    .foreground_group()
                    .is_some_and(|group| group != leader.as_raw_nonzero().get())
        }
        #[cfg(not(unix))]
        false
    }

    /// Kill every process group in the session at once, without the hangup grace that
    /// closing allows, then reap the leader.
    pub fn terminate(&mut self) -> Result<()> {
        if self.poll_exit()?.is_some() {
            return Ok(());
        }
        #[cfg(unix)]
        if let Some(pid) = self.owned_pid()
            && reserved(pid)
        {
            use nix::sys::signal::{Signal, kill};
            use nix::unistd::Pid;
            let raw = pid.as_raw_nonzero().get();
            for group in session_groups(raw, self.foreground_group()) {
                let _ = kill(Pid::from_raw(-group), Signal::SIGKILL);
            }
            let _ = kill(Pid::from_raw(raw), Signal::SIGKILL);
        }
        #[cfg(not(unix))]
        let _ = self.child.kill();
        let deadline = std::time::Instant::now() + Duration::from_millis(250);
        while std::time::Instant::now() < deadline {
            if self.poll_exit()?.is_some() {
                return Ok(());
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        Err(TerminalError::ShutdownTimeout)
    }

    /// Close the terminal: hang up every process group in the session, allow
    /// a grace period for cleanup, kill survivors, then reap the shell.
    pub fn kill(&mut self) -> Result<()> {
        if self.poll_exit()?.is_some() {
            return Ok(());
        }
        #[cfg(unix)]
        if let Some(pid) = self.owned_pid() {
            hang_up(pid, self.foreground_group());
        }
        #[cfg(not(unix))]
        let _ = self.child.kill();
        let deadline = std::time::Instant::now() + Duration::from_millis(250);
        while std::time::Instant::now() < deadline {
            if self.poll_exit()?.is_some() {
                return Ok(());
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        Err(TerminalError::ShutdownTimeout)
    }
}

impl Drop for PtySession {
    fn drop(&mut self) {
        self.output.take();
        // Dropping the PTY writer sends EOF, which would let the shell exit
        // before it sees the hangup, so keep the input open until then.
        let input = self.input.take();
        if self.exit_code.is_some() || matches!(self.poll_exit(), Ok(Some(_))) {
            return;
        }
        #[cfg(unix)]
        if let Some(pid) = self.owned_pid() {
            // Hang up and reap off the caller's thread. Only this closure
            // reaps the child now, so its identity stays reserved until then.
            let foreground = self.foreground_group();
            let close = move |input: Option<SyncSender<Vec<u8>>>| {
                hang_up(pid, foreground);
                drop(input);
                let _ = rustix::process::waitid(
                    rustix::process::WaitId::Pid(pid),
                    rustix::process::WaitIdOptions::EXITED,
                );
            };
            if std::thread::Builder::new()
                .name(format!("tide-pty-close-{}", pid.as_raw_nonzero()))
                .spawn(move || close(input))
                .is_err()
            {
                close(None);
            }
            return;
        }
        let _ = self.child.kill();
    }
}

/// True while `pid` is our unreaped child, so neither its PID nor the session
/// and process group it leads can be reused. Check before signalling.
#[cfg(unix)]
fn reserved(pid: rustix::process::Pid) -> bool {
    rustix::process::waitid(
        rustix::process::WaitId::Pid(pid),
        rustix::process::WaitIdOptions::EXITED
            | rustix::process::WaitIdOptions::NOHANG
            | rustix::process::WaitIdOptions::NOWAIT,
    )
    .is_ok()
}

#[cfg(unix)]
fn hang_up(leader: rustix::process::Pid, foreground: Option<i32>) {
    use nix::sys::signal::{Signal, kill};
    use nix::unistd::Pid;
    let raw = leader.as_raw_nonzero().get();
    let signal_all = |groups: &[i32], signal| {
        for group in groups {
            let _ = kill(Pid::from_raw(-group), signal);
        }
    };
    if !reserved(leader) {
        return;
    }
    let groups = session_groups(raw, foreground);
    signal_all(&groups, Signal::SIGHUP);
    // Stopped jobs only act on the hangup once continued.
    signal_all(&groups, Signal::SIGCONT);
    let deadline = std::time::Instant::now() + HANGUP_GRACE;
    while std::time::Instant::now() < deadline {
        let exited = rustix::process::waitid(
            rustix::process::WaitId::Pid(leader),
            rustix::process::WaitIdOptions::EXITED
                | rustix::process::WaitIdOptions::NOHANG
                | rustix::process::WaitIdOptions::NOWAIT,
        )
        .is_ok_and(|status| status.is_some());
        if exited
            && groups
                .iter()
                .all(|group| *group == raw || kill(Pid::from_raw(-group), None).is_err())
        {
            break;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    if !reserved(leader) {
        return;
    }
    // Enumerate again so groups created during the grace period are covered.
    signal_all(&session_groups(raw, foreground), Signal::SIGKILL);
    let _ = kill(Pid::from_raw(raw), Signal::SIGKILL);
}

/// Process groups with a member in the session `leader` created. `getsid`
/// proves membership, and the session ID cannot be reused while the leader
/// is unreaped.
#[cfg(unix)]
fn session_groups(leader: i32, foreground: Option<i32>) -> Vec<i32> {
    use nix::unistd::{Pid, getpgid, getsid};
    let in_session =
        |pid: i32| getsid(Some(Pid::from_raw(pid))).is_ok_and(|sid| sid.as_raw() == leader);
    let members = process_ids()
        .into_iter()
        .filter(|pid| *pid > 1 && *pid != leader && in_session(*pid))
        .filter_map(|pid| getpgid(Some(Pid::from_raw(pid))).ok().map(Pid::as_raw));
    let mut groups = vec![leader];
    for group in foreground
        .filter(|group| in_session(*group))
        .into_iter()
        .chain(members)
    {
        if group > 1 && !groups.contains(&group) {
            groups.push(group);
        }
    }
    groups
}

#[cfg(target_os = "linux")]
fn process_ids() -> Vec<i32> {
    std::fs::read_dir("/proc")
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|entry| entry.file_name().to_str()?.parse().ok())
        .collect()
}

#[cfg(all(unix, not(target_os = "linux")))]
fn process_ids() -> Vec<i32> {
    std::process::Command::new("/bin/ps")
        .args(["-A", "-o", "pid="])
        .stdin(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .output()
        .map(|output| {
            String::from_utf8_lossy(&output.stdout)
                .split_whitespace()
                .filter_map(|pid| pid.parse().ok())
                .collect()
        })
        .unwrap_or_default()
}

// Signals can interrupt a blocking PTY read without closing the terminal.
// Keep forwarding subsequent frames; EOF and actual I/O errors still end it.
fn forward_output(
    reader: &mut impl Read,
    output: &SyncSender<Vec<u8>>,
    signal: Option<&OutputSignal>,
) {
    let notify = || {
        if let Some(signal) = signal {
            (signal.0)();
        }
    };
    let mut buffer = [0_u8; 16 * 1024];
    loop {
        match reader.read(&mut buffer) {
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
            Ok(0) | Err(_) => break,
            Ok(read) => {
                if output.send(buffer[..read].to_vec()).is_err() {
                    break;
                }
                notify();
            }
        }
    }
    notify();
}

/// Lightweight packaging/CI smoke check for the native PTY path.
pub fn check_terminal() -> Result<String> {
    let mut config = PtyConfig::command(
        "/bin/sh",
        ["-c".to_owned(), "printf 'terminal-ok'".to_owned()],
    );
    config.cols = 20;
    config.rows = 5;
    let mut session = PtySession::spawn(config)?;
    let output = session.read_for(Duration::from_secs(2));
    let _ = session.wait();
    let text = String::from_utf8_lossy(&output).to_string();
    if text.contains("terminal-ok") {
        Ok(text)
    } else {
        Err(TerminalError::SmokeCheck(text))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(unix)]
    #[test]
    fn oversized_input_is_refused_by_kind() {
        let mut session = PtySession::spawn(PtyConfig::command(
            "/bin/sh",
            ["-c".to_owned(), "sleep 5".to_owned()],
        ))
        .unwrap();
        assert!(matches!(
            session.write(&vec![b'x'; MAX_INPUT_WRITE + 1]),
            Err(TerminalError::InputTooLarge)
        ));
        session.write(b"x").unwrap();
        session.kill().unwrap();
    }

    #[test]
    fn pty_output_reader_retries_interrupted_reads_without_losing_later_bytes() {
        struct InterruptedReader(usize);
        impl std::io::Read for InterruptedReader {
            fn read(&mut self, bytes: &mut [u8]) -> std::io::Result<usize> {
                self.0 += 1;
                match self.0 {
                    1 | 3 => Err(std::io::ErrorKind::Interrupted.into()),
                    2 | 4 => {
                        let frame = if self.0 == 2 { b"first" } else { b"later" };
                        bytes[..frame.len()].copy_from_slice(frame);
                        Ok(frame.len())
                    }
                    _ => Ok(0),
                }
            }
        }
        let (sender, receiver) = std::sync::mpsc::sync_channel(4);
        super::forward_output(&mut InterruptedReader(0), &sender, None);
        drop(sender);
        assert_eq!(
            receiver.into_iter().flatten().collect::<Vec<_>>(),
            b"firstlater"
        );
    }

    fn wait_for_fixture(session: &PtySession, token: &str) {
        let deadline = std::time::Instant::now() + Duration::from_secs(2);
        let mut output = Vec::new();
        while !String::from_utf8_lossy(&output).contains(token) {
            assert!(
                std::time::Instant::now() < deadline,
                "fixture did not initialize: {}",
                String::from_utf8_lossy(&output),
            );
            output.extend(session.read_for(Duration::from_millis(25)));
        }
    }

    #[test]
    fn output_signal_fires_for_output_and_close() {
        let (sender, signals) = mpsc::channel();
        let sender = Mutex::new(sender);
        let mut config = PtyConfig::command("/bin/sh", ["-c".into(), "printf ready".into()]);
        config.on_output = Some(OutputSignal(Arc::new(move || {
            let _ = sender.lock().unwrap().send(());
        })));
        let session = PtySession::spawn(config).unwrap();
        signals.recv_timeout(Duration::from_secs(5)).unwrap();
        wait_for_fixture(&session, "ready");
    }

    #[cfg(unix)]
    #[test]
    fn terminals_never_inherit_tide_secrets() {
        temp_env::with_vars(
            [
                ("TIDE_MASTER_PASSWORD", Some("master-leak")),
                ("TIDE_SECRET_TOKEN", Some("token-leak")),
                ("TIDE_PTY_CONTROL", Some("kept")),
            ],
            || {
                let session = PtySession::spawn(PtyConfig::command(
                    "/bin/sh",
                    [
                        "-c".into(),
                        "echo \"[${TIDE_MASTER_PASSWORD:-none}|${TIDE_SECRET_TOKEN:-none}|$TIDE_PTY_CONTROL]\"".into(),
                    ],
                ))
                .unwrap();
                wait_for_fixture(&session, "[none|none|kept]");
            },
        );
    }
    #[test]
    fn completed_parent_helpers_stop_before_reap_for_every_exit_path() {
        for mode in ["poll", "wait", "kill", "drop"] {
            let fixture = tempfile::tempdir().unwrap();
            let heartbeat = fixture.path().join("heartbeat");
            let script = r#"import os,sys,time,signal
signal.signal(signal.SIGHUP, signal.SIG_IGN)
pid=os.fork()
if pid==0:
 for i in range(3000):
  with open(sys.argv[1], 'w') as f:f.write(str(i))
  time.sleep(.01)
 os._exit(0)
while not os.path.exists(sys.argv[1]):time.sleep(.005)
print('READY',flush=True)
os._exit(0)
"#;
            let mut session = PtySession::spawn(PtyConfig::command(
                "/usr/bin/python3",
                ["-c".into(), script.into(), heartbeat.display().to_string()],
            ))
            .unwrap();
            wait_for_fixture(&session, "READY");
            // Observe the completed parent without reaping it. Every exit path
            // below must still reserve its identity while stopping helpers.
            let deadline = std::time::Instant::now() + Duration::from_secs(2);
            while rustix::process::waitid(
                rustix::process::WaitId::Pid(session.owned_pid().unwrap()),
                rustix::process::WaitIdOptions::EXITED
                    | rustix::process::WaitIdOptions::NOHANG
                    | rustix::process::WaitIdOptions::NOWAIT,
            )
            .unwrap()
            .is_none()
            {
                assert!(
                    std::time::Instant::now() < deadline,
                    "fixture parent did not exit"
                );
                std::thread::sleep(Duration::from_millis(5));
            }
            match mode {
                "wait" => assert_eq!(session.wait().unwrap(), 0),
                "kill" => session.kill().unwrap(),
                "poll" => assert_eq!(session.poll_exit().unwrap(), Some(0)),
                "drop" => (),
                _ => unreachable!(),
            }
            if mode != "drop" {
                // Repeated calls must only use the cached exit status.
                session.kill().unwrap();
                assert_eq!(session.poll_exit().unwrap(), Some(0));
                assert_eq!(session.wait().unwrap(), 0);
            }
            drop(session);
            std::thread::sleep(Duration::from_millis(100));
            let stopped = std::fs::read(&heartbeat).unwrap();
            std::thread::sleep(Duration::from_millis(100));
            assert_eq!(
                std::fs::read(&heartbeat).unwrap(),
                stopped,
                "helper survived {mode}"
            );
        }
    }

    #[cfg(unix)]
    #[test]
    fn closing_terminal_stops_a_distinct_foreground_job_group() {
        let fixture = tempfile::tempdir().unwrap();
        let heartbeat = fixture.path().join("heartbeat");
        let script = r#"import os,sys,time,signal
signal.signal(signal.SIGHUP, signal.SIG_IGN)
signal.signal(signal.SIGTTOU, signal.SIG_IGN)
pid=os.fork()
if pid==0:
 os.setpgid(0,0)
 for i in range(3000):
  with open(sys.argv[1], 'w') as f:f.write(str(i))
  time.sleep(.01)
 os._exit(0)
os.setpgid(pid,pid)
os.tcsetpgrp(0,pid)
while not os.path.exists(sys.argv[1]):time.sleep(.005)
print('READY',flush=True)
os.waitpid(pid,0)
"#;
        let mut session = PtySession::spawn(PtyConfig::command(
            "/usr/bin/python3",
            ["-c".into(), script.into(), heartbeat.display().to_string()],
        ))
        .unwrap();
        wait_for_fixture(&session, "READY");
        session.kill().unwrap();
        drop(session);
        std::thread::sleep(Duration::from_millis(100));
        let stopped = std::fs::read(&heartbeat).unwrap();
        std::thread::sleep(Duration::from_millis(100));
        assert_eq!(std::fs::read(&heartbeat).unwrap(), stopped);
    }

    #[cfg(unix)]
    #[test]
    fn foreground_jobs_are_reported_and_terminate_kills_them_with_the_shell() {
        let fixture = tempfile::tempdir().unwrap();
        let mut config = PtyConfig::command(
            "/bin/bash",
            ["--norc", "--noprofile", "--noediting", "-i"].map(String::from),
        );
        config.cwd = Some(fixture.path().to_path_buf());
        let mut session = PtySession::spawn(config).unwrap();
        session.write(b"echo RE''ADY\n").unwrap();
        wait_for_fixture(&session, "READY");
        assert!(!session.has_foreground_job());
        session
            .write(b"/bin/sh -c 'echo $$ > job; trap \"\" HUP TERM; exec sleep 30'\n")
            .unwrap();
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        while !session.has_foreground_job() {
            assert!(std::time::Instant::now() < deadline, "no foreground job");
            std::thread::sleep(Duration::from_millis(10));
        }
        while !fixture.path().join("job").exists() {
            assert!(std::time::Instant::now() < deadline, "job did not start");
            std::thread::sleep(Duration::from_millis(10));
        }
        let job = nix::unistd::Pid::from_raw(
            std::fs::read_to_string(fixture.path().join("job"))
                .unwrap()
                .trim()
                .parse()
                .unwrap(),
        );
        let started = std::time::Instant::now();
        session.terminate().unwrap();
        assert!(started.elapsed() < Duration::from_millis(500));
        assert!(session.poll_exit().unwrap().is_some());
        assert!(!session.has_foreground_job());
        while nix::sys::signal::kill(job, None).is_ok() {
            assert!(std::time::Instant::now() < deadline, "the job survived");
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    #[cfg(unix)]
    #[test]
    fn closing_an_interactive_shell_hangs_up_jobs_then_kills_survivors_and_reaps() {
        use nix::{sys::signal::kill, unistd::getpgid};
        for mode in ["kill", "drop"] {
            let fixture = tempfile::tempdir().unwrap();
            // bash 5 defers a HUP trap while readline waits at the prompt, so the
            // probe trap only runs reliably with line editing off.
            let mut config = PtyConfig::command(
                "/bin/bash",
                ["--norc", "--noprofile", "--noediting", "-i"].map(String::from),
            );
            config.cwd = Some(fixture.path().to_path_buf());
            let mut session = PtySession::spawn(config).unwrap();
            let shell = i32::try_from(session.pid.unwrap()).unwrap();
            session
                .write(
                    b"trap 'echo fired > hup; exit' HUP\n\
                      sleep 30 & echo $! > job\n\
                      /bin/sh -c 'trap \"\" HUP; exec sleep 30' & echo $! > stubborn\n\
                      echo RE''ADY\n",
                )
                .unwrap();
            wait_for_fixture(&session, "READY");
            let pid = |name: &str| {
                let text = std::fs::read_to_string(fixture.path().join(name)).unwrap();
                nix::unistd::Pid::from_raw(text.trim().parse().unwrap())
            };
            let jobs = [pid("job"), pid("stubborn")];
            for job in jobs {
                // Job control gives each background job its own process group.
                assert_ne!(getpgid(Some(job)).unwrap().as_raw(), shell);
            }
            let started = std::time::Instant::now();
            if mode == "kill" {
                session.kill().unwrap();
                assert!(session.poll_exit().unwrap().is_some());
            }
            drop(session);
            assert!(started.elapsed() < Duration::from_secs(2), "{mode} blocked");
            let deadline = std::time::Instant::now() + Duration::from_secs(3);
            let all = [nix::unistd::Pid::from_raw(shell), jobs[0], jobs[1]];
            while all.iter().any(|pid| kill(*pid, None).is_ok()) {
                assert!(
                    std::time::Instant::now() < deadline,
                    "{mode}: a session process survived or was left unreaped"
                );
                std::thread::sleep(Duration::from_millis(10));
            }
            assert_eq!(
                std::fs::read_to_string(fixture.path().join("hup")).unwrap_or_default(),
                "fired\n",
                "{mode}: the shell's HUP trap did not run"
            );
        }
    }

    #[test]
    fn blocked_child_input_is_bounded_nonblocking_and_shutdown_still_completes() {
        let config = PtyConfig::command(
            "/usr/bin/python3",
            [
                "-c",
                "import tty,time;tty.setraw(0);print('READY',flush=True);time.sleep(30)",
            ]
            .map(String::from),
        );
        let mut session = PtySession::spawn(config).unwrap();
        wait_for_fixture(&session, "READY");
        let bytes = vec![b'x'; 4 * 1024 * 1024];
        let start = std::time::Instant::now();
        session.write(&bytes).unwrap();
        session.write(&bytes).unwrap();
        assert!(session.write(b"overflow").is_err());
        assert!(
            start.elapsed() < Duration::from_millis(500),
            "input blocked the caller"
        );
        assert!(session.poll_exit().unwrap().is_none());
        let shutdown = std::time::Instant::now();
        drop(session);
        assert!(shutdown.elapsed() < Duration::from_secs(1));
    }

    #[test]
    fn closing_a_pty_terminates_children_and_does_not_wait_for_their_output() {
        let mut session = PtySession::spawn(PtyConfig::command(
            "/bin/sh",
            [
                "-c".into(),
                "trap '' HUP; sleep 30 & printf ready; wait".into(),
            ],
        ))
        .unwrap();
        wait_for_fixture(&session, "ready");
        let started = std::time::Instant::now();
        session.kill().unwrap();
        assert!(session.poll_exit().unwrap().is_some());
        drop(session);
        assert!(started.elapsed() < Duration::from_secs(2));
    }

    #[test]
    fn spawns_a_command_and_reads_output() {
        let config = PtyConfig::command(
            "/bin/sh",
            ["-c".to_owned(), "printf 'hello from pty'".to_owned()],
        );
        let mut session = PtySession::spawn(config).unwrap();
        let output = session.read_for(Duration::from_secs(2));
        let _ = session.wait();
        let text = String::from_utf8_lossy(&output);
        assert!(text.contains("hello from pty"), "output: {text:?}");
    }

    #[test]
    fn resize_is_propagated_to_the_master() {
        let mut config = PtyConfig::command("/bin/sh", ["-c".to_owned(), "sleep 1".to_owned()]);
        config.cols = 40;
        config.rows = 10;
        let mut session = PtySession::spawn(config).unwrap();
        session.resize(100, 30).unwrap();
        assert_eq!(session.size(), (100, 30));
        let _ = session.kill();
    }

    #[test]
    fn check_terminal_smoke_passes() {
        assert!(check_terminal().is_ok());
    }
}
