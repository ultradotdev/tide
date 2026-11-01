//! Bounded access to the host clipboard. Tests use dummy commands only.
//!
//! These calls block for up to a few seconds, so the UI runs them on a
//! blocking thread and receives the result as an event.
use std::io::{Read, Write};
use std::process::{Command, Stdio};
use std::time::Duration;
use wait_timeout::ChildExt;

const LIMIT: usize = 1024 * 1024;

/// Builds the helper command that copies (`true`) or pastes (`false`).
pub type Commands = fn(bool) -> Result<Command, String>;

pub fn command(copy: bool) -> Result<Command, String> {
    let candidates: &[(&str, &[&str])] = if cfg!(target_os = "macos") {
        if copy {
            &[("pbcopy", &[])]
        } else {
            &[("pbpaste", &[])]
        }
    } else if std::env::var_os("WAYLAND_DISPLAY").is_some() {
        if copy {
            &[("wl-copy", &[])]
        } else {
            &[("wl-paste", &["--no-newline"])]
        }
    } else if std::env::var_os("DISPLAY").is_some() {
        if copy {
            &[
                ("xclip", &["-selection", "clipboard"]),
                ("xsel", &["--clipboard", "--input"]),
            ]
        } else {
            &[
                ("xclip", &["-selection", "clipboard", "-o"]),
                ("xsel", &["--clipboard", "--output"]),
            ]
        }
    } else {
        &[]
    };
    for (program, args) in candidates {
        if tide_lsp::command_available(program) {
            let mut command = Command::new(program);
            tide_core::process::scrub_tide_secrets(&mut command).args(*args);
            return Ok(command);
        }
    }
    Err("system clipboard unavailable (use terminal paste or install a clipboard helper)".into())
}

fn run(mut command: Command, input: Option<&str>) -> Result<String, String> {
    if input.is_some_and(|text| text.len() > LIMIT) {
        return Err("clipboard exceeds 1 MiB".into());
    }
    // A regular input file avoids an unbounded blocking write to a child pipe.
    let mut file = tempfile::tempfile().map_err(|e| e.to_string())?;
    if let Some(text) = input {
        use std::io::{Seek, SeekFrom};
        file.write_all(text.as_bytes()).map_err(|e| e.to_string())?;
        file.seek(SeekFrom::Start(0)).map_err(|e| e.to_string())?;
    }
    command
        .stdin(file)
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }
    let mut child = command.spawn().map_err(|e| e.to_string())?;
    let output = child.stdout.take().ok_or("missing clipboard output")?;
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let mut bytes = Vec::new();
        let result = output
            .take((LIMIT + 1) as u64)
            .read_to_end(&mut bytes)
            .map(|_| bytes);
        let _ = tx.send(result);
    });
    let status = child
        .wait_timeout(Duration::from_secs(2))
        .map_err(|e| e.to_string())?;
    if status.is_none() {
        #[cfg(unix)]
        {
            if let Some(group) = tide_core::process::process_group_target(child.id()) {
                let _ = nix::sys::signal::kill(
                    nix::unistd::Pid::from_raw(group),
                    nix::sys::signal::Signal::SIGKILL,
                );
            }
        }
        let _ = child.kill();
        let _ = child.wait();
        return Err("clipboard command timed out".into());
    }
    if !status.unwrap().success() {
        return Err("clipboard command failed".into());
    }
    let bytes = rx
        .recv_timeout(Duration::from_secs(2))
        .map_err(|_| "clipboard output timed out")?
        .map_err(|e| e.to_string())?;
    if bytes.len() > LIMIT {
        return Err("clipboard exceeds 1 MiB".into());
    }
    String::from_utf8(bytes).map_err(|_| "clipboard is not UTF-8 text".into())
}

pub fn write(commands: Commands, text: &str) -> Result<(), String> {
    run(commands(true)?, Some(text)).map(|_| ())
}
pub fn read(commands: Commands) -> Result<String, String> {
    run(commands(false)?, None)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn dummy_clipboard_round_trip_and_bounds() {
        let mut command = Command::new("cat");
        assert_eq!(run(command, Some("a😀b\n")).unwrap(), "a😀b\n");
        command = Command::new("cat");
        assert!(run(command, Some(&"x".repeat(LIMIT + 1))).is_err());
        let mut command = Command::new("sleep");
        command.arg("10");
        assert!(run(command, None).unwrap_err().contains("timed out"));
    }
}
