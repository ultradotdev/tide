use std::time::{Duration, Instant};

use tide_terminal::{PtyConfig, PtySession};

#[test]
fn edits_and_saves_a_file_through_the_tui() {
    let home = tempfile::tempdir().unwrap();
    let workspace = home.path().join("workspace");
    std::fs::create_dir_all(&workspace).unwrap();
    let file = workspace.join("hello.txt");
    std::fs::write(&file, "hello\n").unwrap();

    let mut config = PtyConfig::command(env!("CARGO_BIN_EXE_tide"), [file.display().to_string()]);
    config.cwd = Some(workspace.clone());
    config.cols = 100;
    config.rows = 30;
    config.env = vec![
        ("TERM".to_owned(), "xterm-256color".to_owned()),
        ("HOME".to_owned(), home.path().display().to_string()),
        (
            "XDG_CONFIG_HOME".to_owned(),
            home.path().join("config").display().to_string(),
        ),
        (
            "XDG_DATA_HOME".to_owned(),
            home.path().join("data").display().to_string(),
        ),
        (
            "XDG_STATE_HOME".to_owned(),
            home.path().join("state").display().to_string(),
        ),
        (
            "XDG_CACHE_HOME".to_owned(),
            home.path().join("cache").display().to_string(),
        ),
    ];

    let mut session = PtySession::spawn(config).unwrap();
    let ready_deadline = Instant::now() + Duration::from_secs(5);
    let mut ready_output = Vec::new();
    while Instant::now() < ready_deadline {
        ready_output.extend(session.read_for(Duration::from_millis(25)));
        if String::from_utf8_lossy(&ready_output).contains("hello.txt") {
            break;
        }
    }
    assert!(
        String::from_utf8_lossy(&ready_output).contains("hello.txt"),
        "editor did not render its ready frame"
    );
    session.write(b"Z").unwrap();
    std::thread::sleep(Duration::from_millis(200));
    session.write(&[0x13]).unwrap(); // Ctrl+S
    std::thread::sleep(Duration::from_millis(300));
    session.write(&[0x11]).unwrap(); // Ctrl+Q

    let deadline = Instant::now() + Duration::from_secs(4);
    let mut exited = false;
    while Instant::now() < deadline {
        if session.poll_exit().unwrap().is_some() {
            exited = true;
            break;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    let output = String::from_utf8_lossy(&session.drain()).to_string();
    if !exited {
        let _ = session.kill();
        panic!("TUI did not exit; output tail: {output:?}");
    }

    assert_eq!(std::fs::read_to_string(&file).unwrap(), "Zhello\n");
}
