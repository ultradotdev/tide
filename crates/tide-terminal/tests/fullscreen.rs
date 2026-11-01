use std::process::Command;
use std::time::{Duration, Instant};

use tide_terminal::{PtyConfig, PtySession, TerminalEmulator};

#[test]
#[ignore = "needs vim on PATH"]
fn runs_vim_in_the_alternate_screen() {
    assert!(
        Command::new("vim").arg("--version").output().is_ok(),
        "vim is not on PATH"
    );

    let mut config = PtyConfig::command(
        "vim",
        ["-Nu".to_owned(), "NONE".to_owned(), "-n".to_owned()],
    );
    config.cols = 80;
    config.rows = 24;
    config.env = vec![("TERM".to_owned(), "xterm-256color".to_owned())];
    let mut session = PtySession::spawn(config).unwrap();
    let mut emulator = TerminalEmulator::new(80, 24, 1_000);

    let mut entered_alternate_screen = false;
    let mut rendered = false;
    let ready_deadline = Instant::now() + Duration::from_secs(5);
    while Instant::now() < ready_deadline {
        let output = session.read_for(Duration::from_millis(100));
        if !output.is_empty() {
            emulator.feed(&output);
        }
        let snapshot = emulator.snapshot();
        if snapshot.modes.alternate_screen {
            entered_alternate_screen = true;
            // Switching screens and drawing content can arrive in separate reads.
            if snapshot.text().contains("VIM") || snapshot.text().contains('~') {
                rendered = true;
                break;
            }
        }
    }

    let _ = session.write(b"\x1b:q!\r");
    let deadline = Instant::now() + Duration::from_secs(3);
    while Instant::now() < deadline {
        if session.poll_exit().unwrap().is_some() {
            break;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    let _ = session.kill();

    assert!(
        rendered,
        "vim screen did not render: {}",
        emulator.snapshot().text()
    );
    assert!(
        entered_alternate_screen,
        "vim did not enter the alternate screen; terminal output was not captured as a full-screen app"
    );
}
