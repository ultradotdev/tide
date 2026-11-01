//! Validation command execution, bounded and sanitized.

use std::path::PathBuf;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use tokio::io::{AsyncRead, AsyncReadExt};
use tokio::process::Command;

use crate::workspace::{ToolError, Workspace};

pub const MAX_OUTPUT_BYTES: usize = 64 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ValidationCommand {
    pub program: String,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub cwd: Option<String>,
    #[serde(default = "default_timeout_secs")]
    pub timeout_secs: u64,
}

const fn default_timeout_secs() -> u64 {
    60
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ValidationOutput {
    pub success: bool,
    pub exit_code: Option<i32>,
    pub stdout: String,
    pub stderr: String,
    pub duration_ms: u128,
    pub timed_out: bool,
}

/// Run a validation command inside the workspace after an explicit opt-in.
pub async fn run_validation(
    workspace: &Workspace,
    command: &ValidationCommand,
) -> Result<ValidationOutput, ToolError> {
    if command.program.trim().is_empty() {
        return Err(ToolError::Validation("empty program".to_owned()));
    }
    let cwd = match command.cwd.as_deref() {
        Some(path) => workspace.resolve(path)?,
        None => workspace.root().to_path_buf(),
    };
    let mut process = Command::new(&command.program);
    process
        .args(&command.args)
        .current_dir(&cwd)
        .kill_on_drop(true)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .stdin(std::process::Stdio::null());
    tide_core::process::scrub_tide_secrets(process.as_std_mut());
    tide_core::process::scrub_provider_keys(process.as_std_mut());
    #[cfg(unix)]
    process.process_group(0);
    let started = Instant::now();
    let mut child = process
        .spawn()
        .map_err(|error| ToolError::Validation(error.to_string()))?;
    let mut group = ProcessGroup(child.id());
    let stdout = CappedReader::spawn(child.stdout.take());
    let stderr = CappedReader::spawn(child.stderr.take());
    let timeout_secs = command.timeout_secs.clamp(1, 600);
    let waited = tokio::time::timeout(Duration::from_secs(timeout_secs), child.wait()).await;
    group.kill();
    let (status, timed_out) = match waited {
        Ok(Ok(status)) => (Some(status), false),
        Ok(Err(error)) => return Err(ToolError::Validation(error.to_string())),
        Err(_) => {
            let _ = child.wait().await;
            (None, true)
        }
    };
    let stdout = stdout.finish().await;
    let mut stderr = stderr.finish().await;
    if timed_out {
        if !stderr.is_empty() && !stderr.ends_with('\n') {
            stderr.push('\n');
        }
        stderr.push_str(&format!("validation timed out after {timeout_secs}s"));
    }
    Ok(ValidationOutput {
        success: status.is_some_and(|status| status.success()),
        exit_code: status.and_then(|status| status.code()),
        stdout,
        stderr,
        duration_ms: started.elapsed().as_millis(),
        timed_out,
    })
}

/// Kills every process left in the command's group, including grandchildren
/// that `kill_on_drop` would miss.
struct ProcessGroup(Option<u32>);

impl ProcessGroup {
    fn kill(&mut self) {
        #[cfg(unix)]
        if let Some(id) = self.0.take().and_then(|id| i32::try_from(id).ok()) {
            let _ = nix::sys::signal::killpg(
                nix::unistd::Pid::from_raw(id),
                nix::sys::signal::Signal::SIGKILL,
            );
        }
    }
}

impl Drop for ProcessGroup {
    fn drop(&mut self) {
        self.kill();
    }
}

#[derive(Default)]
struct Captured {
    bytes: Vec<u8>,
    truncated: bool,
}

/// Drains a pipe to the end, keeping at most `MAX_OUTPUT_BYTES` so a noisy
/// command can neither exhaust memory nor block on a full pipe.
struct CappedReader {
    captured: Arc<Mutex<Captured>>,
    task: tokio::task::JoinHandle<()>,
}

impl CappedReader {
    fn spawn(pipe: Option<impl AsyncRead + Unpin + Send + 'static>) -> Self {
        let captured = Arc::new(Mutex::new(Captured::default()));
        let sink = captured.clone();
        let task = tokio::spawn(async move {
            let Some(mut pipe) = pipe else {
                return;
            };
            let mut chunk = vec![0; 64 * 1024];
            while let Ok(count @ 1..) = pipe.read(&mut chunk).await {
                let mut sink = sink.lock().unwrap_or_else(PoisonError::into_inner);
                let room = MAX_OUTPUT_BYTES - sink.bytes.len();
                sink.bytes.extend_from_slice(&chunk[..count.min(room)]);
                sink.truncated |= count > room;
            }
        });
        Self { captured, task }
    }

    /// A process that escaped the group may still hold the pipe; keep what
    /// arrived instead of waiting for it.
    async fn finish(self) -> String {
        let abort = self.task.abort_handle();
        if tokio::time::timeout(Duration::from_secs(1), self.task)
            .await
            .is_err()
        {
            abort.abort();
        }
        let captured = self.captured.lock().unwrap_or_else(PoisonError::into_inner);
        let mut text = String::from_utf8_lossy(&captured.bytes).into_owned();
        if captured.truncated {
            text.push_str("\n… output truncated");
        }
        text
    }
}

/// Resolve a command's working directory for callers that only need a probe.
pub fn validation_cwd(workspace: &Workspace, cwd: Option<&str>) -> Result<PathBuf, ToolError> {
    cwd.map_or_else(
        || Ok(workspace.root().to_path_buf()),
        |path| workspace.resolve(path),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn captures_success_and_failure() {
        let dir = tempfile::tempdir().unwrap();
        let workspace = Workspace::new(dir.path()).unwrap();
        let ok = run_validation(
            &workspace,
            &ValidationCommand {
                program: "printf".to_owned(),
                args: vec!["hello".to_owned()],
                cwd: None,
                timeout_secs: 5,
            },
        )
        .await
        .unwrap();
        assert!(ok.success);
        assert_eq!(ok.stdout, "hello");

        let failed = run_validation(
            &workspace,
            &ValidationCommand {
                program: "false".to_owned(),
                args: Vec::new(),
                cwd: None,
                timeout_secs: 5,
            },
        )
        .await
        .unwrap();
        assert!(!failed.success);
        assert_eq!(failed.exit_code, Some(1));
    }

    #[tokio::test]
    async fn times_out_long_commands() {
        let dir = tempfile::tempdir().unwrap();
        let workspace = Workspace::new(dir.path()).unwrap();
        let output = run_validation(
            &workspace,
            &ValidationCommand {
                program: "sleep".to_owned(),
                args: vec!["5".to_owned()],
                cwd: None,
                timeout_secs: 1,
            },
        )
        .await
        .unwrap();
        assert!(output.timed_out);
        assert!(!output.success);
    }

    fn command(program: &str, args: &[&str], timeout_secs: u64) -> ValidationCommand {
        ValidationCommand {
            program: program.to_owned(),
            args: args.iter().map(|arg| (*arg).to_owned()).collect(),
            cwd: None,
            timeout_secs,
        }
    }

    fn is_alive(pid: &str) -> bool {
        std::process::Command::new("kill")
            .args(["-0", pid])
            .stderr(std::process::Stdio::null())
            .status()
            .unwrap()
            .success()
    }

    #[tokio::test]
    async fn timeout_kills_the_whole_process_group() {
        let dir = tempfile::tempdir().unwrap();
        let workspace = Workspace::new(dir.path()).unwrap();
        let output = run_validation(
            &workspace,
            &command("sh", &["-c", "sleep 60 & echo $! > grandchild; wait"], 1),
        )
        .await
        .unwrap();
        assert!(output.timed_out);
        let pid = std::fs::read_to_string(dir.path().join("grandchild")).unwrap();
        let pid = pid.trim();
        let deadline = Instant::now() + Duration::from_secs(5);
        while is_alive(pid) && Instant::now() < deadline {
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        assert!(!is_alive(pid), "grandchild {pid} survived the timeout");
    }

    #[tokio::test]
    async fn output_is_capped_while_reading() {
        let dir = tempfile::tempdir().unwrap();
        let workspace = Workspace::new(dir.path()).unwrap();
        let output = run_validation(
            &workspace,
            &command("sh", &["-c", "head -c 52428800 /dev/zero | tr '\\0' a"], 30),
        )
        .await
        .unwrap();
        assert!(output.success);
        assert!(output.stdout.len() <= MAX_OUTPUT_BYTES + 64);
        assert!(output.stdout.ends_with("output truncated"));

        let output = run_validation(
            &workspace,
            &command(
                "sh",
                &[
                    "-c",
                    "echo oops >&2; head -c 8388608 /dev/zero | tr '\\0' a; sleep 60",
                ],
                2,
            ),
        )
        .await
        .unwrap();
        assert!(output.timed_out);
        assert!(output.stdout.starts_with("aaaa"));
        assert!(output.stdout.len() <= MAX_OUTPUT_BYTES + 64);
        assert!(output.stderr.contains("oops"));
        assert!(output.stderr.contains("timed out"));
    }

    #[cfg(unix)]
    #[test]
    fn commands_do_not_inherit_secrets() {
        temp_env::with_vars(
            [
                ("TIDE_MASTER_PASSWORD", Some("master-secret")),
                ("TIDE_SECRET_DB_PASSWORD", Some("db-secret")),
                ("OPENAI_API_KEY", Some("provider-secret")),
                ("GOOGLE_API_KEY", Some("google-secret")),
                ("TIDE_VALIDATION_VISIBLE", Some("visible")),
            ],
            || {
                let dir = tempfile::tempdir().unwrap();
                let workspace = Workspace::new(dir.path()).unwrap();
                let output = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                    .unwrap()
                    .block_on(run_validation(&workspace, &command("env", &[], 5)))
                    .unwrap();
                assert!(output.success);
                assert!(output.stdout.contains("TIDE_VALIDATION_VISIBLE=visible"));
                for secret in [
                    "master-secret",
                    "db-secret",
                    "provider-secret",
                    "google-secret",
                ] {
                    assert!(!output.stdout.contains(secret), "{secret} leaked");
                }
            },
        );
    }

    #[tokio::test]
    async fn rejects_cwd_outside_the_workspace() {
        let dir = tempfile::tempdir().unwrap();
        let workspace = Workspace::new(dir.path()).unwrap();
        let error = run_validation(
            &workspace,
            &ValidationCommand {
                program: "true".to_owned(),
                args: Vec::new(),
                cwd: Some("../".to_owned()),
                timeout_secs: 5,
            },
        )
        .await
        .unwrap_err();
        assert!(matches!(error, ToolError::OutsideWorkspace(_)));
    }
}
