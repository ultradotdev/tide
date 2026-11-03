//! Bounded access to the host clipboard. Tests use dummy commands only.
//!
//! These calls block for up to a few seconds, so the UI runs them on a
//! blocking thread and receives the result as an event.
//!
//! Copies can also travel through the terminal Tide runs in, with the OSC 52
//! escape sequence. That is the only clipboard an SSH session can reach.
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

/// When a copy is also sent to the host terminal's clipboard with OSC 52.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Osc52 {
    /// In an SSH session, and whenever the system clipboard cannot be reached.
    Auto,
    /// With every copy, alongside the system clipboard.
    Always,
    /// Never; only the system clipboard is used.
    Off,
}

impl Osc52 {
    /// The `clipboard.osc52` setting; anything unknown is `auto`.
    #[must_use]
    pub fn from_setting(value: Option<&str>) -> Self {
        match value {
            Some("always") => Self::Always,
            Some("off") => Self::Off,
            _ => Self::Auto,
        }
    }
}

/// Facts about the session Tide runs in that decide how OSC 52 is sent.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Host {
    /// Started from an SSH login: the system clipboard is the remote machine's.
    pub remote: bool,
    /// Running inside tmux, which needs `set-clipboard on` or passthrough for OSC 52.
    pub tmux: bool,
}

impl Host {
    #[must_use]
    pub fn detect() -> Self {
        let set = |name| std::env::var_os(name).is_some_and(|value| !value.is_empty());
        Self {
            remote: set("SSH_TTY") || set("SSH_CONNECTION") || set("SSH_CLIENT"),
            tmux: set("TMUX"),
        }
    }
}

/// The bytes that put `text` on the host terminal's clipboard: `ESC ] 52 ; c ; <base64> BEL`.
/// Inside tmux the sequence is sent twice: plain, which tmux takes with `set-clipboard on`,
/// and wrapped for passthrough, which reaches the outer terminal with `allow-passthrough on`.
pub fn osc52(text: &str, host: Host) -> Result<Vec<u8>, String> {
    use base64::Engine as _;
    if text.len() > LIMIT {
        return Err("clipboard exceeds 1 MiB".into());
    }
    let encoded = base64::engine::general_purpose::STANDARD.encode(text);
    let plain = format!("\x1b]52;c;{encoded}\x07");
    let mut bytes = plain.clone().into_bytes();
    if host.tmux {
        bytes.extend_from_slice(b"\x1bPtmux;");
        bytes.extend_from_slice(plain.replace('\x1b', "\x1b\x1b").as_bytes());
        bytes.extend_from_slice(b"\x1b\\");
    }
    Ok(bytes)
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
    fn osc52_encodes_utf8_as_base64_and_wraps_for_tmux() {
        let local = Host::default();
        assert_eq!(osc52("hi", local).unwrap(), b"\x1b]52;c;aGk=\x07");
        assert_eq!(
            osc52("a😀\n", local).unwrap(),
            b"\x1b]52;c;YfCfmIAK\x07",
            "UTF-8 bytes are encoded, newline included"
        );
        assert_eq!(osc52("", local).unwrap(), b"\x1b]52;c;\x07");
        let tmux = Host {
            remote: true,
            tmux: true,
        };
        assert_eq!(
            osc52("hi", tmux).unwrap(),
            b"\x1b]52;c;aGk=\x07\x1bPtmux;\x1b\x1b]52;c;aGk=\x07\x1b\\"
        );
        assert!(osc52(&"x".repeat(LIMIT + 1), local).is_err());
        assert_eq!(Osc52::from_setting(Some("always")), Osc52::Always);
        assert_eq!(Osc52::from_setting(Some("off")), Osc52::Off);
        assert_eq!(Osc52::from_setting(Some("auto")), Osc52::Auto);
        assert_eq!(Osc52::from_setting(None), Osc52::Auto);
    }
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
