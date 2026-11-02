use std::time::{Duration, Instant};

use tide_terminal::{PtyConfig, PtySession};

#[test]
fn tui_starts_and_exits_on_quit() {
    let home = tempfile::tempdir().unwrap();
    let workspace = home.path().join("workspace");
    std::fs::create_dir_all(&workspace).unwrap();
    std::fs::write(workspace.join("main.ts"), "const ready = true;\n").unwrap();
    let mut config = PtyConfig::command(
        env!("CARGO_BIN_EXE_tide"),
        ["--no-session".to_owned(), workspace.display().to_string()],
    );
    config.cwd = Some(workspace);
    config.cols = 100;
    config.rows = 30;
    config.env = vec![
        ("TERM".to_owned(), "xterm-256color".to_owned()),
        ("HOME".into(), home.path().display().to_string()),
    ];
    for (key, name) in [
        ("XDG_CONFIG_HOME", "config"),
        ("XDG_DATA_HOME", "data"),
        ("XDG_STATE_HOME", "state"),
        ("XDG_CACHE_HOME", "cache"),
    ] {
        config
            .env
            .push((key.into(), home.path().join(name).display().to_string()));
    }

    let mut session = PtySession::spawn(config).unwrap();
    let ready_deadline = Instant::now() + Duration::from_secs(5);
    let mut output = Vec::new();
    while Instant::now() < ready_deadline {
        output.extend(session.read_for(Duration::from_millis(25)));
        if String::from_utf8_lossy(&output).contains("Welcome") {
            break;
        }
    }
    assert!(
        String::from_utf8_lossy(&output).contains("Welcome"),
        "TUI did not render its ready frame; output: {:?}",
        String::from_utf8_lossy(&output)
    );
    // Ctrl+Q is the default `workbench.quit` binding. Escape intentionally
    // only clears selection/overlays in the real editor.
    session.write(&[0x11]).unwrap();

    let deadline = Instant::now() + Duration::from_secs(3);
    let mut exited = false;
    while Instant::now() < deadline {
        match session.poll_exit().unwrap() {
            Some(_) => {
                exited = true;
                break;
            }
            None => std::thread::sleep(Duration::from_millis(50)),
        }
    }

    output.extend(session.drain());
    let output = String::from_utf8_lossy(&output).to_string();
    if !exited {
        let _ = session.kill();
        panic!("TUI did not exit after Ctrl+Q; output: {output:?}");
    }
    assert!(
        output.contains("Explorer") || output.contains("main.ts"),
        "expected workbench output, got: {output:?}"
    );
}
