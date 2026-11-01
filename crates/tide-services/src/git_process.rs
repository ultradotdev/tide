//! Shell-free Git execution with bounded output, input and process lifetime.
use anyhow::{Context, Result, anyhow, bail, ensure};
use nix::{
    poll::{PollFd, PollFlags, poll},
    sys::signal::{Signal, kill},
    unistd::Pid,
};
use std::{
    fs::File,
    io::{Read, Write},
    os::fd::{AsFd, OwnedFd},
    path::Path,
    process::{Child, Command, Stdio},
    sync::atomic::{AtomicBool, Ordering},
    thread,
    time::{Duration, Instant},
};

const MAX_OUTPUT: usize = 32 * 1024 * 1024;
pub struct GitOutput {
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
    pub success: bool,
}
struct OwnedChild(Child, bool);
impl Drop for OwnedChild {
    fn drop(&mut self) {
        if !self.1 {
            stop(&mut self.0);
        }
    }
}
fn stop(child: &mut Child) {
    if let Some(group) = tide_core::process::process_group_target(child.id()) {
        let group = Pid::from_raw(group);
        let _ = kill(group, Signal::SIGTERM);
        // Keep the unreaped child PID reserved until its whole group is stopped.
        thread::sleep(Duration::from_millis(50));
        let _ = kill(group, Signal::SIGKILL);
    }
    let _ = child.kill();
    let _ = child.wait();
}
/// Stop helpers left in the group of a reaped leader. Live members keep the group ID reserved.
fn stop_leftover_group(leader: u32) {
    let Some(group) = tide_core::process::process_group_target(leader) else {
        return;
    };
    let group = Pid::from_raw(group);
    if kill(group, None).is_err() {
        return;
    }
    let _ = kill(group, Signal::SIGTERM);
    let deadline = Instant::now() + Duration::from_millis(50);
    while Instant::now() < deadline {
        if kill(group, None).is_err() {
            return;
        }
        thread::sleep(Duration::from_millis(1));
    }
    let _ = kill(group, Signal::SIGKILL);
}

struct Capture {
    pipe: Option<File>,
    bytes: Vec<u8>,
    limit: usize,
}
impl Capture {
    fn new(pipe: impl Into<OwnedFd>, limit: usize) -> Self {
        Self {
            pipe: Some(File::from(pipe.into())),
            bytes: Vec::new(),
            limit,
        }
    }
}
/// Every pipe of one child, serviced from the calling thread with `poll`.
struct Pipes<'a> {
    output: [Capture; 2],
    stdin: Option<File>,
    input: &'a [u8],
    written: usize,
    input_error: Option<anyhow::Error>,
    buffer: Vec<u8>,
}
impl Pipes<'_> {
    fn open(&self) -> bool {
        self.reading() || self.stdin.is_some()
    }
    fn reading(&self) -> bool {
        self.output.iter().any(|capture| capture.pipe.is_some())
    }
    /// Wait up to `timeout` for pipe activity; returns whether any pipe was ready.
    fn pump(&mut self, timeout: Duration) -> Result<bool> {
        let mut ready = [false; 3];
        {
            let mut targets = Vec::with_capacity(3);
            let mut fds = Vec::with_capacity(3);
            for (index, capture) in self.output.iter().enumerate() {
                if let Some(pipe) = &capture.pipe {
                    targets.push(index);
                    fds.push(PollFd::new(pipe.as_fd(), PollFlags::POLLIN));
                }
            }
            if let Some(stdin) = &self.stdin {
                targets.push(2);
                fds.push(PollFd::new(stdin.as_fd(), PollFlags::POLLOUT));
            }
            let millis = u16::try_from(timeout.as_millis()).unwrap_or(u16::MAX);
            match poll(&mut fds, millis) {
                Ok(_) => {}
                Err(nix::errno::Errno::EINTR) => return Ok(true),
                Err(error) => bail!("Git output polling failed: {error}"),
            }
            for (target, fd) in targets.into_iter().zip(&fds) {
                ready[target] = fd.revents().is_some_and(|events| !events.is_empty());
            }
        }
        for index in [0, 1] {
            if ready[index] {
                self.read(index)?;
            }
        }
        if ready[2] {
            self.write();
        }
        Ok(ready.contains(&true))
    }
    fn read(&mut self, index: usize) -> Result<()> {
        let capture = &mut self.output[index];
        let Some(pipe) = capture.pipe.as_mut() else {
            return Ok(());
        };
        match pipe.read(&mut self.buffer) {
            Ok(0) => capture.pipe = None,
            Ok(count) => {
                ensure!(
                    capture.bytes.len() + count <= capture.limit,
                    "Git output limit exceeded"
                );
                capture.bytes.extend_from_slice(&self.buffer[..count]);
            }
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => {}
            Err(error) => return Err(error.into()),
        }
        Ok(())
    }
    fn write(&mut self) {
        let Some(stdin) = self.stdin.as_mut() else {
            return;
        };
        match stdin.write(&self.input[self.written..]) {
            Ok(0) => self.close_input(Some(anyhow!("Git input closed"))),
            Ok(count) => {
                self.written += count;
                if self.written == self.input.len() {
                    self.close_input(None);
                }
            }
            Err(error)
                if matches!(
                    error.kind(),
                    std::io::ErrorKind::Interrupted | std::io::ErrorKind::WouldBlock
                ) => {}
            Err(error) => self.close_input(Some(error.into())),
        }
    }
    fn close_input(&mut self, error: Option<anyhow::Error>) {
        self.stdin = None;
        self.input_error = self.input_error.take().or(error);
    }
}

pub fn git(root: &Path, args: &[&str], input: Option<&[u8]>) -> Result<GitOutput> {
    static NEVER: AtomicBool = AtomicBool::new(false);
    git_cancellable(root, args, input, &NEVER)
}
pub fn git_cancellable(
    root: &Path,
    args: &[&str],
    input: Option<&[u8]>,
    cancel: &AtomicBool,
) -> Result<GitOutput> {
    run(
        std::ffi::OsStr::new("git"),
        root,
        args,
        input,
        cancel,
        Duration::from_secs(15),
    )
}
fn run(
    program: &std::ffi::OsStr,
    root: &Path,
    args: &[&str],
    input: Option<&[u8]>,
    cancel: &AtomicBool,
    timeout: Duration,
) -> Result<GitOutput> {
    run_mode(program, root, args, input, cancel, timeout, true)
}
/// Run an explicitly requested local maintenance command with the same bounds.
pub fn local_command(
    program: &std::ffi::OsStr,
    root: &Path,
    args: &[&str],
    cancel: &AtomicBool,
    timeout: Duration,
) -> Result<GitOutput> {
    ensure!(
        timeout <= Duration::from_secs(120),
        "Maintenance timeout exceeds two minutes"
    );
    run_mode(program, root, args, None, cancel, timeout, false)
}
#[allow(clippy::too_many_arguments)]
fn run_mode(
    program: &std::ffi::OsStr,
    root: &Path,
    args: &[&str],
    input: Option<&[u8]>,
    cancel: &AtomicBool,
    timeout: Duration,
    git_args: bool,
) -> Result<GitOutput> {
    ensure!(!cancel.load(Ordering::Acquire), "Git operation cancelled");
    let started = Instant::now();
    tracing::debug!(
        operation = args.first().copied().unwrap_or(""),
        "Local command started"
    );
    ensure!(
        input.is_none_or(|input| input.len() <= MAX_OUTPUT),
        "Git input limit exceeded"
    );
    let mut command = Command::new(program);
    tide_core::process::scrub_tide_secrets(&mut command);
    if git_args {
        command
            .args(["--literal-pathspecs", "--no-pager", "-C"])
            .arg(root);
    }
    command
        .current_dir(root)
        .args(args)
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GIT_EDITOR", "true")
        .env("GIT_PAGER", "cat")
        .stdin(if input.is_some() {
            Stdio::piped()
        } else {
            Stdio::null()
        })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }
    let mut child = OwnedChild(command.spawn().context("start Git")?, false);
    let input = input.unwrap_or_default();
    let stdin = match child.0.stdin.take() {
        Some(stdin) if !input.is_empty() => {
            use nix::fcntl::{FcntlArg, OFlag, fcntl};
            let flags = fcntl(&stdin, FcntlArg::F_GETFL)?;
            fcntl(
                &stdin,
                FcntlArg::F_SETFL(OFlag::from_bits_truncate(flags) | OFlag::O_NONBLOCK),
            )?;
            Some(File::from(OwnedFd::from(stdin)))
        }
        _ => None,
    };
    let mut pipes = Pipes {
        output: [
            Capture::new(child.0.stdout.take().context("Git stdout")?, MAX_OUTPUT),
            Capture::new(child.0.stderr.take().context("Git stderr")?, 1024 * 1024),
        ],
        stdin,
        input,
        written: 0,
        input_error: None,
        buffer: vec![0; 64 * 1024],
    };
    use rustix::process::{WaitId, WaitIdOptions, waitid};
    let pid = tide_core::process::raw_pid(child.0.id())
        .and_then(rustix::process::Pid::from_raw)
        .context("Invalid Git PID")?;
    let deadline = Instant::now() + timeout;
    loop {
        let now = Instant::now();
        if cancel.load(Ordering::Acquire) || now >= deadline {
            bail!(if cancel.load(Ordering::Acquire) {
                "Git operation cancelled"
            } else {
                "Git command timed out"
            });
        }
        // Observe exit without reaping: PID reservation keeps group cleanup safe.
        if waitid(
            WaitId::Pid(pid),
            WaitIdOptions::EXITED | WaitIdOptions::NOHANG | WaitIdOptions::NOWAIT,
        )?
        .is_some()
        {
            break;
        }
        let wait = (deadline - now).min(Duration::from_millis(10));
        if pipes.open() {
            pipes.pump(wait)?;
        } else {
            thread::sleep(wait.min(Duration::from_millis(1)));
        }
    }
    let status = child.0.wait()?;
    child.1 = true;
    stop_leftover_group(child.0.id());
    if pipes.stdin.is_some() {
        pipes.close_input(Some(anyhow!("Git closed before input completed")));
    }
    let finished = Instant::now();
    while pipes.reading() {
        ensure!(
            finished.elapsed() <= Duration::from_millis(250),
            "Git output did not finish after process exit"
        );
        if !pipes.pump(Duration::from_millis(50))? {
            break;
        }
    }
    if status.success()
        && let Some(error) = pipes.input_error.take()
    {
        return Err(error);
    }
    tracing::debug!(
        elapsed_ms = started.elapsed().as_millis(),
        success = status.success(),
        "Local command finished"
    );
    let [stdout, stderr] = pipes.output;
    Ok(GitOutput {
        stdout: stdout.bytes,
        stderr: stderr.bytes,
        success: status.success(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;
    #[test]
    fn completed_parent_cannot_leave_a_running_helper() {
        let dir = tempfile::tempdir().unwrap();
        let script = dir.path().join("git-helper");
        std::fs::write(&script,"#!/usr/bin/python3\nimport os,time\npid=os.fork()\nif pid==0:\n time.sleep(30)\nelse:\n print(pid,flush=True)\n os._exit(0)\n").unwrap();
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o700)).unwrap();
        let started = Instant::now();
        let output = run(
            script.as_os_str(),
            dir.path(),
            &[],
            None,
            &AtomicBool::new(false),
            Duration::from_secs(3),
        )
        .unwrap();
        assert!(output.success);
        assert!(started.elapsed() < Duration::from_secs(1));
        let pid = String::from_utf8(output.stdout)
            .unwrap()
            .trim()
            .parse::<i32>()
            .unwrap();
        // A sandbox PID 1 may retain a killed grandchild as a zombie. It must not run.
        #[cfg(target_os = "linux")]
        {
            let deadline = Instant::now() + Duration::from_millis(500);
            loop {
                match std::fs::read_to_string(format!("/proc/{pid}/stat")) {
                    Err(_) => break,
                    Ok(stat) if stat.split_once(") ").unwrap().1.starts_with('Z') => break,
                    Ok(stat) => {
                        assert!(Instant::now() < deadline, "Helper still running: {stat}");
                        thread::sleep(Duration::from_millis(5));
                    }
                }
            }
        }
        #[cfg(not(target_os = "linux"))]
        {
            let status = std::process::Command::new("ps")
                .args(["-o", "stat=", "-p", &pid.to_string()])
                .output()
                .unwrap();
            let state = String::from_utf8_lossy(&status.stdout);
            assert!(state.trim().is_empty() || state.trim().starts_with('Z'));
        }
    }
    #[test]
    fn clean_exit_adds_no_cleanup_delay() {
        let dir = tempfile::tempdir().unwrap();
        let fastest = (0..5)
            .map(|_| {
                let started = Instant::now();
                let output = run(
                    std::ffi::OsStr::new("true"),
                    dir.path(),
                    &[],
                    None,
                    &AtomicBool::new(false),
                    Duration::from_secs(3),
                )
                .unwrap();
                assert!(output.success);
                started.elapsed()
            })
            .min()
            .unwrap();
        assert!(fastest < Duration::from_millis(40), "{fastest:?}");
    }
    #[test]
    fn helper_ignoring_sigterm_is_killed_after_a_clean_exit() {
        let dir = tempfile::tempdir().unwrap();
        let script = dir.path().join("git-stubborn");
        std::fs::write(&script,"#!/usr/bin/python3\nimport os,signal,time\npid=os.fork()\nif pid==0:\n signal.signal(signal.SIGTERM,signal.SIG_IGN)\n os.close(1)\n os.close(2)\n time.sleep(30)\nelse:\n time.sleep(0.1)\n print(pid,flush=True)\n os._exit(0)\n").unwrap();
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o700)).unwrap();
        let output = run(
            script.as_os_str(),
            dir.path(),
            &[],
            None,
            &AtomicBool::new(false),
            Duration::from_secs(3),
        )
        .unwrap();
        assert!(output.success);
        let pid = String::from_utf8(output.stdout)
            .unwrap()
            .trim()
            .parse::<i32>()
            .unwrap();
        let deadline = Instant::now() + Duration::from_millis(500);
        loop {
            let status = std::process::Command::new("ps")
                .args(["-o", "stat=", "-p", &pid.to_string()])
                .output()
                .unwrap();
            let state = String::from_utf8_lossy(&status.stdout).trim().to_owned();
            if state.is_empty() || state.starts_with('Z') {
                break;
            }
            assert!(Instant::now() < deadline, "Helper still running: {state}");
            thread::sleep(Duration::from_millis(5));
        }
    }
    #[test]
    fn git_process_bounds_input_output_lifetime_and_cancellation() {
        let dir = tempfile::tempdir().unwrap();
        let script = dir.path().join("git-fixture");
        std::fs::write(&script,"#!/usr/bin/python3\nimport os,sys,time\nif 'flood' in sys.argv: os.write(1,b'x'*(33*1024*1024))\nelif 'blocked' in sys.argv: time.sleep(30)\nelse: os.write(1,sys.stdin.buffer.read())\n").unwrap();
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o700)).unwrap();
        let cancel = AtomicBool::new(false);
        let echoed = run(
            script.as_os_str(),
            dir.path(),
            &[],
            Some(b"safe input"),
            &cancel,
            Duration::from_secs(2),
        )
        .unwrap();
        assert!(echoed.success);
        assert_eq!(echoed.stdout, b"safe input");
        assert!(
            run(
                script.as_os_str(),
                dir.path(),
                &["flood"],
                None,
                &cancel,
                Duration::from_secs(2)
            )
            .is_err()
        );
        let start = Instant::now();
        assert!(
            run(
                script.as_os_str(),
                dir.path(),
                &["blocked"],
                Some(&vec![b'x'; 4 * 1024 * 1024]),
                &cancel,
                Duration::from_millis(150)
            )
            .is_err()
        );
        assert!(start.elapsed() < Duration::from_secs(1));
        cancel.store(true, Ordering::Release);
        let start = Instant::now();
        assert!(
            run(
                script.as_os_str(),
                dir.path(),
                &["blocked"],
                None,
                &cancel,
                Duration::from_secs(15)
            )
            .is_err()
        );
        assert!(start.elapsed() < Duration::from_secs(1));
    }
}
