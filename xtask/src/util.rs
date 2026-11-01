//! Shared helpers: repository paths, hashing, JSON and an injectable command runner.
use anyhow::{Context, Result, bail, ensure};
use serde::de::DeserializeOwned;
use sha2::{Digest, Sha256};
use std::cmp::Ordering;
use std::ffi::OsStr;
use std::io::Read;
use std::path::{Component, Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Duration;

/// Builds an argv vector from any mix of strings and paths.
#[macro_export]
macro_rules! argv {
    ($($item:expr),* $(,)?) => {
        ::std::vec::Vec::<::std::string::String>::from([$($crate::util::arg(&$item)),*])
    };
}

pub fn arg(value: &(impl AsRef<OsStr> + ?Sized)) -> String {
    value.as_ref().to_string_lossy().into_owned()
}

/// The repository that contains this xtask crate.
pub fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("xtask lives inside the repository")
        .to_path_buf()
}

pub fn sha256_hex(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

pub fn sha256_file(path: &Path) -> Result<String> {
    let mut file = std::fs::File::open(path).with_context(|| format!("open {}", path.display()))?;
    let mut hash = Sha256::new();
    let mut buffer = [0_u8; 65536];
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        hash.update(&buffer[..read]);
    }
    Ok(format!("{:x}", hash.finalize()))
}

pub fn read_text(path: &Path) -> Result<String> {
    std::fs::read_to_string(path).with_context(|| format!("read {}", path.display()))
}

pub fn read_json<T: DeserializeOwned>(path: &Path) -> Result<T> {
    serde_json::from_str(&read_text(path)?).with_context(|| format!("parse {}", path.display()))
}

/// Writes `value` the way `JSON.stringify(value, null, 2)` plus a newline did.
pub fn pretty_json(value: &impl serde::Serialize) -> Result<String> {
    Ok(format!("{}\n", serde_json::to_string_pretty(value)?))
}

/// Approximates `String.prototype.localeCompare` for the ASCII file names the
/// release tooling sorts: case-insensitive first, then by code point.
pub fn locale_compare(left: &str, right: &str) -> Ordering {
    left.to_lowercase()
        .cmp(&right.to_lowercase())
        .then_with(|| left.cmp(right))
}

/// Lexically resolves `relative` against `base`, like Node's `path.resolve`.
pub fn resolve_lexically(base: &Path, relative: &Path) -> PathBuf {
    let mut resolved = PathBuf::new();
    let joined = if relative.is_absolute() {
        relative.to_path_buf()
    } else {
        base.join(relative)
    };
    for component in joined.components() {
        match component {
            Component::ParentDir => {
                resolved.pop();
            }
            Component::CurDir => {}
            other => resolved.push(other),
        }
    }
    resolved
}

pub fn is_full_git_sha(value: &str) -> bool {
    value.len() == 40
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RunOptions {
    pub cwd: Option<PathBuf>,
}

impl RunOptions {
    pub fn cwd(path: &Path) -> Self {
        Self {
            cwd: Some(path.to_path_buf()),
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CommandOutput {
    pub stdout: String,
    pub stderr: String,
}

/// Executes argv vectors. Release, signing and notarization code takes a runner
/// so tests can record the exact commands without executing them.
pub trait CommandRunner {
    fn run(&mut self, argv: &[String], options: &RunOptions) -> Result<CommandOutput>;

    fn run_plain(&mut self, argv: &[String]) -> Result<CommandOutput> {
        self.run(argv, &RunOptions::default())
    }
}

/// Runs real processes and fails on a non-zero exit.
#[derive(Debug, Default)]
pub struct SystemRunner;

impl CommandRunner for SystemRunner {
    fn run(&mut self, argv: &[String], options: &RunOptions) -> Result<CommandOutput> {
        let (program, args) = argv
            .split_first()
            .context("Command argv must not be empty")?;
        let mut command = Command::new(program);
        command.args(args).stdin(Stdio::null());
        if let Some(cwd) = &options.cwd {
            command.current_dir(cwd);
        }
        let output = command
            .output()
            .with_context(|| format!("spawn {program}"))?;
        let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
        let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
        if !output.status.success() {
            let name = Path::new(program).file_name().map_or_else(
                || program.clone(),
                |name| name.to_string_lossy().into_owned(),
            );
            let detail = if stderr.trim().is_empty() {
                stdout.trim()
            } else {
                stderr.trim()
            };
            let code = output
                .status
                .code()
                .map_or_else(|| "signal".to_owned(), |code| code.to_string());
            bail!("{name} failed with exit {code}: {detail}");
        }
        Ok(CommandOutput { stdout, stderr })
    }
}

/// Runs a maintainer command with inherited stdio, failing on a non-zero exit.
pub fn run_inherited(command: &mut Command) -> Result<()> {
    let status = command
        .status()
        .with_context(|| format!("spawn {:?}", command.get_program()))?;
    ensure!(status.success(), "{command:?} failed with {status}");
    Ok(())
}

/// Captures stdout of a maintainer command, failing on a non-zero exit.
pub fn capture(command: &mut Command) -> Result<String> {
    let output = command
        .stderr(Stdio::inherit())
        .output()
        .with_context(|| format!("spawn {:?}", command.get_program()))?;
    ensure!(
        output.status.success(),
        "{command:?} failed with {}",
        output.status
    );
    String::from_utf8(output.stdout).context("command output is not UTF-8")
}

#[derive(Debug)]
pub struct TimedOutput {
    pub success: bool,
    pub stdout: String,
    pub stderr: String,
}

/// Runs a command with piped output and a hard timeout, like Python's
/// `subprocess.run(..., capture_output=True, timeout=...)`.
pub fn run_with_timeout(command: &mut Command, timeout: Duration) -> Result<TimedOutput> {
    use wait_timeout::ChildExt;
    let mut child = command
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .with_context(|| format!("spawn {:?}", command.get_program()))?;
    let mut stdout = child.stdout.take().context("stdout pipe")?;
    let mut stderr = child.stderr.take().context("stderr pipe")?;
    let out_thread = std::thread::spawn(move || {
        let mut text = Vec::new();
        let _ = stdout.read_to_end(&mut text);
        text
    });
    let err_thread = std::thread::spawn(move || {
        let mut text = Vec::new();
        let _ = stderr.read_to_end(&mut text);
        text
    });
    let Some(status) = child.wait_timeout(timeout)? else {
        let _ = child.kill();
        let _ = child.wait();
        bail!(
            "{:?} timed out after {}s",
            command.get_program(),
            timeout.as_secs()
        );
    };
    let stdout = out_thread.join().unwrap_or_default();
    let stderr = err_thread.join().unwrap_or_default();
    Ok(TimedOutput {
        success: status.success(),
        stdout: String::from_utf8_lossy(&stdout).into_owned(),
        stderr: String::from_utf8_lossy(&stderr).into_owned(),
    })
}

/// Sets both access and modification times, like Node's `utimes`.
pub fn set_times(path: &Path, epoch_seconds: u64) -> Result<()> {
    let time = std::time::UNIX_EPOCH + Duration::from_secs(epoch_seconds);
    let file = std::fs::File::open(path).with_context(|| format!("open {}", path.display()))?;
    file.set_times(
        std::fs::FileTimes::new()
            .set_accessed(time)
            .set_modified(time),
    )
    .with_context(|| format!("set times on {}", path.display()))
}

pub fn chmod(path: &Path, mode: u32) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode))
        .with_context(|| format!("chmod {}", path.display()))
}

/// Writes a new owner-only file.
pub fn write_private(path: &Path, bytes: &[u8]) -> Result<()> {
    use std::io::Write;
    use std::os::unix::fs::OpenOptionsExt;
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(path)
        .with_context(|| format!("create {}", path.display()))?;
    file.write_all(bytes)?;
    chmod(path, 0o600)
}

/// Paths Git tracks in the work tree at `root`, or `None` when `root` is not the
/// top of a Git work tree (an unpacked archive or a scratch directory).
pub fn tracked_files(root: &Path) -> Result<Option<Vec<String>>> {
    let Ok(toplevel) = Command::new("git")
        .args(["rev-parse", "--show-toplevel"])
        .current_dir(root)
        .stderr(Stdio::null())
        .output()
    else {
        return Ok(None);
    };
    if !toplevel.status.success() {
        return Ok(None);
    }
    let toplevel = PathBuf::from(String::from_utf8(toplevel.stdout)?.trim_end_matches('\n'));
    if std::fs::canonicalize(toplevel)? != std::fs::canonicalize(root)? {
        return Ok(None);
    }
    let listing = capture(
        Command::new("git")
            .args(["ls-files", "-z"])
            .current_dir(root),
    )?;
    Ok(Some(
        listing
            .split('\0')
            .filter(|name| !name.is_empty())
            .map(str::to_owned)
            .collect(),
    ))
}

/// Prints a success line or turns collected failures into one error.
pub fn finish(failures: &[String], success: &str) -> Result<()> {
    if failures.is_empty() {
        println!("{success}");
        return Ok(());
    }
    let mut unique = Vec::new();
    for failure in failures {
        if !unique.contains(failure) {
            unique.push(failure.clone());
        }
    }
    bail!("{}", unique.join("\n"))
}

#[cfg(test)]
pub mod testing {
    use super::{CommandOutput, CommandRunner, RunOptions};
    use anyhow::Result;

    type Responder = Box<dyn FnMut(&[String], &RunOptions) -> Result<CommandOutput>>;

    /// Records every command and answers from a closure instead of executing.
    pub struct RecordingRunner {
        pub calls: Vec<(Vec<String>, RunOptions)>,
        responder: Responder,
    }

    impl RecordingRunner {
        pub fn new(
            responder: impl FnMut(&[String], &RunOptions) -> Result<CommandOutput> + 'static,
        ) -> Self {
            Self {
                calls: Vec::new(),
                responder: Box::new(responder),
            }
        }

        pub fn commands(&self) -> Vec<String> {
            self.calls.iter().map(|(argv, _)| argv.join(" ")).collect()
        }
    }

    impl CommandRunner for RecordingRunner {
        fn run(&mut self, argv: &[String], options: &RunOptions) -> Result<CommandOutput> {
            self.calls.push((argv.to_vec(), options.clone()));
            (self.responder)(argv, options)
        }
    }

    pub fn stdout(text: &str) -> CommandOutput {
        CommandOutput {
            stdout: text.to_owned(),
            stderr: String::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolves_parent_components_without_touching_the_filesystem() {
        assert_eq!(
            resolve_lexically(Path::new("/repo/docs"), Path::new("../README.md")),
            PathBuf::from("/repo/README.md")
        );
        assert_eq!(
            resolve_lexically(Path::new("/repo/docs"), Path::new("../../outside.md")),
            PathBuf::from("/outside.md")
        );
    }

    #[test]
    fn locale_order_ignores_case_before_code_points() {
        let mut names = vec!["tide", "LICENSE", "uninstall.sh", "ATTRIBUTIONS.md"];
        names.sort_by(|left, right| locale_compare(left, right));
        assert_eq!(
            names,
            ["ATTRIBUTIONS.md", "LICENSE", "tide", "uninstall.sh"]
        );
    }

    #[test]
    fn system_runner_reports_the_failing_program_and_its_stderr() {
        let error = SystemRunner
            .run_plain(&argv!["/bin/sh", "-c", "echo broken >&2; exit 3"])
            .unwrap_err()
            .to_string();
        assert_eq!(error, "sh failed with exit 3: broken");
    }
}
