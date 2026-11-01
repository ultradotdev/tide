use std::time::{Duration, Instant};

use tide_terminal::{PtyConfig, PtySession, TerminalEmulator, TerminalSnapshot};

fn wait_for(session: &PtySession, screen: &mut TerminalEmulator, marker: &str) -> TerminalSnapshot {
    let deadline = Instant::now() + Duration::from_secs(5);
    while Instant::now() < deadline {
        screen.feed(&session.read_for(Duration::from_millis(25)));
        let snapshot = screen.snapshot();
        if snapshot.text().contains(marker) {
            return snapshot;
        }
    }
    panic!("did not render {marker}: {}", screen.snapshot().text());
}
fn click_text(session: &mut PtySession, snapshot: &TerminalSnapshot, text: &str) {
    let letters = text.chars().map(|ch| ch.to_string()).collect::<Vec<_>>();
    for row in 0..snapshot.rows {
        for column in 0..snapshot.cols.saturating_sub(letters.len()) {
            if letters.iter().enumerate().all(|(offset, ch)| {
                snapshot
                    .cell(column + offset, row)
                    .is_some_and(|cell| &cell.symbol == ch)
            }) {
                let (x, y) = (column + 1, row + 1);
                session
                    .write(format!("\x1b[<0;{x};{y}M\x1b[<0;{x};{y}m").as_bytes())
                    .unwrap();
                return;
            }
        }
    }
    panic!("missing target {text}: {}", snapshot.text());
}

#[test]
fn sgr_mouse_expands_the_explorer_and_opens_a_file() {
    let home = tempfile::tempdir().unwrap();
    let workspace = home.path().join("workspace");
    let folder = workspace.join("src");
    std::fs::create_dir_all(&folder).unwrap();
    std::fs::write(folder.join("inner.txt"), "MOUSE_OPENED_THIS_FILE\n").unwrap();
    let mut config = PtyConfig::command(
        env!("CARGO_BIN_EXE_tide"),
        ["--no-session".into(), workspace.display().to_string()],
    );
    config.cwd = Some(workspace);
    config.cols = 100;
    config.rows = 30;
    config.env = vec![
        ("TERM".into(), "xterm-256color".into()),
        ("HOME".into(), home.path().display().to_string()),
    ];
    for kind in ["CONFIG", "DATA", "STATE", "CACHE"] {
        config.env.push((
            format!("XDG_{kind}_HOME"),
            home.path().join(kind).display().to_string(),
        ));
    }
    let mut session = PtySession::spawn(config).unwrap();
    let mut screen = TerminalEmulator::new(100, 30, 0);
    let snapshot = wait_for(&session, &mut screen, "Ctrl+P");
    click_text(&mut session, &snapshot, "src");
    let snapshot = wait_for(&session, &mut screen, "inner.txt");
    click_text(&mut session, &snapshot, "inner.txt");
    wait_for(&session, &mut screen, "MOUSE_OPENED_THIS_FILE");
    session.write(&[0x11]).unwrap();
    let deadline = Instant::now() + Duration::from_secs(3);
    while Instant::now() < deadline {
        if let Some(status) = session.poll_exit().unwrap() {
            assert_eq!(status, 0);
            return;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    panic!("mouse smoke did not exit after Ctrl+Q");
}
