use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use tide_terminal::{PtyConfig, PtySession, TerminalEmulator, TerminalSnapshot};

struct Fixture {
    home: tempfile::TempDir,
    workspace: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let home = tempfile::tempdir().unwrap();
        let workspace = home.path().join("workspace");
        std::fs::create_dir_all(&workspace).unwrap();
        Self { home, workspace }
    }

    fn preferences(&self, contents: &str) {
        let directory = self.home.path().join("CONFIG/tide");
        std::fs::create_dir_all(&directory).unwrap();
        std::fs::write(directory.join("settings.jsonc"), contents).unwrap();
    }
    fn config(&self, target: &Path) -> PtyConfig {
        let mut config = PtyConfig::command(
            env!("CARGO_BIN_EXE_tide"),
            ["--no-session".into(), target.display().to_string()],
        );
        config.cwd = Some(self.workspace.clone());
        config.cols = 120;
        config.rows = 40;
        config.env = vec![
            ("TERM".into(), "xterm-256color".into()),
            // A terminal that bundles Nerd Font glyphs, so the default icon style
            // draws them.
            ("TERM_PROGRAM".into(), "ghostty".into()),
            ("PATH".into(), "/usr/bin:/bin".into()),
            ("HOME".into(), self.home.path().display().to_string()),
            ("TIDE_NO_UPDATE_CHECK".into(), "1".into()),
        ];
        for kind in ["CONFIG", "DATA", "STATE", "CACHE"] {
            config.env.push((
                format!("XDG_{kind}_HOME"),
                self.home.path().join(kind).display().to_string(),
            ));
        }
        config
    }
}

/// Overall budget for a screen or process to reach an expected state. Polling
/// returns as soon as the condition holds, so the budget is generous for loaded
/// shared CI runners without slowing passing tests.
const SETTLE_TIMEOUT: Duration = Duration::from_secs(30);

/// Repeats `step` until it yields a value or [`SETTLE_TIMEOUT`] passes. Each step
/// must block briefly (a PTY read or a short sleep) rather than spin.
fn poll_until<T>(mut step: impl FnMut() -> Option<T>) -> Option<T> {
    let deadline = Instant::now() + SETTLE_TIMEOUT;
    while Instant::now() < deadline {
        if let Some(value) = step() {
            return Some(value);
        }
    }
    None
}

/// Waits for the TUI process to exit and returns its status.
fn wait_exit(session: &mut PtySession, mut feed: impl FnMut(&PtySession)) -> Option<u32> {
    poll_until(|| {
        feed(session);
        let status = session.poll_exit().unwrap();
        if status.is_none() {
            std::thread::sleep(Duration::from_millis(20));
        }
        status
    })
}

#[track_caller]
fn wait_screen(
    session: &PtySession,
    screen: &mut TerminalEmulator,
    predicate: impl Fn(&TerminalSnapshot) -> bool,
) -> TerminalSnapshot {
    let settled = poll_until(|| {
        screen.feed(&session.read_for(Duration::from_millis(25)));
        if !predicate(&screen.snapshot()) {
            return None;
        }
        // A terminal frame can span multiple reads. Verify its settled
        // state before deriving geometry or sending the next interaction.
        screen.feed(&session.read_for(Duration::from_millis(100)));
        let settled = screen.snapshot();
        predicate(&settled).then_some(settled)
    });
    settled.unwrap_or_else(|| {
        panic!(
            "screen did not reach expected state:\n{}",
            screen.snapshot().text()
        )
    })
}

fn position(
    snapshot: &TerminalSnapshot,
    needle: &str,
    only_row: Option<usize>,
    min_column: usize,
) -> (usize, usize) {
    let letters = needle
        .chars()
        .map(|letter| letter.to_string())
        .collect::<Vec<_>>();
    for row in 0..snapshot.rows {
        if only_row.is_some_and(|expected| expected != row) {
            continue;
        }
        for column in min_column..=snapshot.cols.saturating_sub(letters.len()) {
            if letters.iter().enumerate().all(|(offset, letter)| {
                snapshot
                    .cell(column + offset, row)
                    .is_some_and(|cell| &cell.symbol == letter)
            }) {
                return (column, row);
            }
        }
    }
    panic!("{needle} not found on screen:\n{}", snapshot.text());
}

fn click(session: &mut PtySession, point: (usize, usize)) {
    let (column, row) = (point.0 + 1, point.1 + 1);
    session
        .write(format!("\x1b[<0;{column};{row}M\x1b[<0;{column};{row}m").as_bytes())
        .unwrap();
}

fn tab_bar_text(snapshot: &TerminalSnapshot) -> String {
    (36..snapshot.cols)
        .filter_map(|column| snapshot.cell(column, 0))
        .map(|cell| cell.symbol.as_str())
        .collect()
}

/// F1 opens the palette from anywhere, a focused terminal included: Ctrl+T and the other
/// Ctrl+letter keys belong to the program running there.
const F1: &[u8] = b"\x1bOP";

/// Quits from the palette, confirming the dialog Tide shows while terminals or agents are
/// still running a program.
#[track_caller]
fn quit_from_palette(session: &mut PtySession, screen: &mut TerminalEmulator) {
    palette_command(session, screen, "Quit");
    let mut confirmed = false;
    let status = poll_until(|| {
        screen.feed(&session.read_for(Duration::from_millis(25)));
        if !confirmed && screen.snapshot().text().contains("Still running") {
            confirmed = true;
            session.write(b"y").unwrap();
        }
        session.poll_exit().unwrap()
    });
    assert_eq!(status.expect("TUI did not quit"), 0);
}

#[track_caller]
fn quit(session: &mut PtySession) {
    session.write(&[0x11]).unwrap();
    let status = wait_exit(session, |_| {});
    assert_eq!(status.expect("TUI did not quit"), 0);
}

#[test]
fn terminal_mouse_closes_the_tab_at_the_rendered_close_icon() {
    let fixture = Fixture::new();
    let file = fixture.workspace.join("close-me.txt");
    std::fs::write(&file, "CLOSE_ICON_FIXTURE\n").unwrap();
    let mut session = PtySession::spawn(fixture.config(&file)).unwrap();
    let mut screen = TerminalEmulator::new(120, 40, 1000);
    let snapshot = wait_screen(&session, &mut screen, |snapshot| {
        snapshot.text().contains("CLOSE_ICON_FIXTURE")
    });
    click(&mut session, position(&snapshot, "×", Some(0), 36));
    wait_screen(&session, &mut screen, |snapshot| {
        snapshot.text().contains("No open tabs")
    });
    quit(&mut session);
}

#[test]
fn terminal_wrapped_quit_dialog_has_safe_keyboard_default_and_real_buttons() {
    let fixture = Fixture::new();
    let file = fixture.workspace.join("draft.txt");
    std::fs::write(&file, "DIALOG_FIXTURE\n").unwrap();
    let mut session = PtySession::spawn(fixture.config(&file)).unwrap();
    let mut screen = TerminalEmulator::new(120, 40, 1000);
    wait_screen(&session, &mut screen, |s| {
        s.text().contains("DIALOG_FIXTURE")
    });
    session.write(b"x\x11").unwrap();
    let snapshot = wait_screen(&session, &mut screen, |s| {
        s.text().contains("Unsaved Changes")
            && s.text().contains("recovery copy.")
            && s.text().contains("Cancel [Esc]")
            && s.text().contains("Discard [N]")
    });
    let confirm = position(&snapshot, "Discard [N]", None, 0);
    let cancel = position(&snapshot, "Cancel [Esc]", None, 0);
    assert_eq!(confirm.1, cancel.1);
    assert!(confirm.0 > 30 && cancel.0 < 90);
    session.write(b"\r").unwrap();
    wait_screen(&session, &mut screen, |s| {
        !s.text().contains("Unsaved Changes")
    });
    assert!(session.poll_exit().unwrap().is_none());
    session.write(&[0x11]).unwrap();
    let snapshot = wait_screen(&session, &mut screen, |s| {
        s.text().contains("Unsaved Changes")
    });
    click(&mut session, position(&snapshot, "Cancel [Esc]", None, 0));
    wait_screen(&session, &mut screen, |s| {
        !s.text().contains("Unsaved Changes")
    });
    assert!(session.poll_exit().unwrap().is_none());
    session.write(&[0x11]).unwrap();
    let snapshot = wait_screen(&session, &mut screen, |s| {
        s.text().contains("Unsaved Changes")
    });
    click(&mut session, position(&snapshot, "Discard [N]", None, 0));
    let status = wait_exit(&mut session, |_| {});
    assert_eq!(status.expect("confirm button did not quit"), 0);
    assert_eq!(std::fs::read_to_string(file).unwrap(), "DIALOG_FIXTURE\n");
}

#[cfg(unix)]
#[test]
fn terminal_new_codex_uses_native_identity_and_session_click_focuses_the_existing_cli() {
    use std::os::unix::fs::PermissionsExt;
    let fixture = Fixture::new();
    fixture.preferences(r#"{"tui.agentsSidebar.visible":false}"#);
    let executable = fixture.workspace.join("fake-codex");
    let state = fixture.workspace.join("native-state");
    let launches = fixture.workspace.join("launches");
    let script=r#"#!/usr/bin/python3
import sys,json,os,time
if sys.argv[1]=='app-server':
 for line in sys.stdin:
  r=json.loads(line)
  if 'id' not in r:continue
  result={}
  if r['method']=='thread/start':
   with open(__STATE__,'w') as f:f.write('native-fresh')
   result={'thread':{'id':'native-fresh'}}
  if r['method']=='thread/list':result={'data':[{'id':'native-fresh','name':'Native fresh','cwd':__WORKSPACE__,'updatedAt':1700000000}] if os.path.exists(__STATE__) else [],'nextCursor':None}
  print(json.dumps({'id':r['id'],'result':result}),flush=True)
else:
 with open(__LAUNCHES__,'a') as f:f.write(' '.join(sys.argv[1:])+'\n')
 print('CODEX_NATIVE_READY '+sys.argv[-1],flush=True)
 time.sleep(30)
"#.replace("__STATE__",&serde_json::to_string(&state.display().to_string()).unwrap())
        .replace("__WORKSPACE__",&serde_json::to_string(&fixture.workspace.display().to_string()).unwrap())
        .replace("__LAUNCHES__",&serde_json::to_string(&launches.display().to_string()).unwrap());
    std::fs::write(&executable, script).unwrap();
    std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o700)).unwrap();
    let mut config = fixture.config(&fixture.workspace);
    config
        .env
        .push(("TIDE_CODEX_PATH".into(), executable.display().to_string()));
    let mut session = PtySession::spawn(config).unwrap();
    let mut screen = TerminalEmulator::new(120, 40, 1000);
    wait_screen(&session, &mut screen, |s| s.text().contains("Ctrl+P"));
    session.write(F1).unwrap();
    wait_screen(&session, &mut screen, |s| {
        s.text().contains("Command Palette")
    });
    session.write(b"Agents: New Session\r").unwrap();
    wait_screen(&session, &mut screen, |s| s.text().contains("Open AI CLI"));
    session.write(b"Codex\r").unwrap();
    wait_screen(&session, &mut screen, |s| {
        s.text().contains("CODEX_NATIVE_READY native-fresh")
    });
    session.write(F1).unwrap();
    wait_screen(&session, &mut screen, |s| {
        s.text().contains("Command Palette")
    });
    session.write(b"Agents: Toggle Sidebar\r").unwrap();
    let snapshot = wait_screen(&session, &mut screen, |s| {
        s.text().contains("Native fresh") && s.text().contains("Codex (1)")
    });
    click(&mut session, position(&snapshot, "Native fresh", None, 0));
    quit_from_palette(&mut session, &mut screen);
    let text = std::fs::read_to_string(launches).unwrap();
    assert_eq!(text.lines().count(), 1);
    assert!(text.starts_with("resume -C "), "{text}");
    assert!(text.trim_end().ends_with(" -- native-fresh"), "{text}");
}

#[test]
fn terminal_overflow_picker_and_horizontal_wheel_reach_hidden_tabs() {
    let fixture = Fixture::new();
    let files = (0..6)
        .map(|index| {
            fixture
                .workspace
                .join(format!("document-long-{index:02}.txt"))
        })
        .collect::<Vec<_>>();
    for (index, file) in files.iter().enumerate() {
        std::fs::write(file, format!("TAB_CONTENT_{index:02}\n")).unwrap();
    }
    let mut session = PtySession::spawn(fixture.config(&files[0])).unwrap();
    let mut screen = TerminalEmulator::new(120, 40, 1000);
    wait_screen(&session, &mut screen, |snapshot| {
        snapshot.text().contains("TAB_CONTENT_00")
    });
    for (index, file) in files.iter().enumerate().skip(1) {
        session.write(&[0x10]).unwrap(); // Ctrl+P
        wait_screen(&session, &mut screen, |snapshot| {
            snapshot.text().contains("Quick Open")
        });
        session
            .write(format!("{}\r", file.file_name().unwrap().to_string_lossy()).as_bytes())
            .unwrap();
        wait_screen(&session, &mut screen, |snapshot| {
            snapshot.text().contains(&format!("TAB_CONTENT_{index:02}"))
        });
    }
    let snapshot = screen.snapshot();
    click(&mut session, position(&snapshot, "▾", Some(0), 36));
    wait_screen(&session, &mut screen, |snapshot| {
        snapshot.text().contains("Open Tabs")
    });
    session.write(b"document-long-00\r").unwrap();
    let snapshot = wait_screen(&session, &mut screen, |snapshot| {
        snapshot.text().contains("TAB_CONTENT_00") && !snapshot.text().contains("Open Tabs")
    });
    let point = position(&snapshot, "document-long-00", Some(0), 36);
    session
        .write(format!("\x1b[<67;{};{}M", point.0 + 1, point.1 + 1).as_bytes())
        .unwrap();
    let snapshot = wait_screen(&session, &mut screen, |snapshot| {
        let bar = tab_bar_text(snapshot);
        bar.contains("document-long-01")
            && !bar.contains("document-long-00")
            && snapshot.text().contains("TAB_CONTENT_00")
    });
    click(&mut session, position(&snapshot, "×", Some(0), 36));
    let snapshot = wait_screen(&session, &mut screen, |snapshot| {
        tab_bar_text(snapshot).contains("document-long-02")
            && !tab_bar_text(snapshot).contains("document-long-01")
            && snapshot.text().contains("TAB_CONTENT_00")
    });
    click(&mut session, position(&snapshot, "▾", Some(0), 36));
    let snapshot = wait_screen(&session, &mut screen, |snapshot| {
        snapshot.text().contains("Open Tabs")
    });
    let row = snapshot
        .text()
        .lines()
        .enumerate()
        .find(|(row, line)| *row > 5 && line.contains("document-long-03.txt"))
        .unwrap()
        .0;
    click(
        &mut session,
        position(&snapshot, "document-long-03.txt", Some(row), 30),
    );
    wait_screen(&session, &mut screen, |snapshot| {
        snapshot.text().contains("TAB_CONTENT_03") && !snapshot.text().contains("Open Tabs")
    });
    quit(&mut session);
}

#[cfg(unix)]
#[test]
fn terminal_agents_collapse_and_session_click_launch_one_native_cli() {
    use std::os::unix::fs::PermissionsExt;
    let fixture = Fixture::new();
    fixture.preferences(r#"{"tui.agentsSidebar.visible":false}"#);
    let project = fixture.home.path().join(".claude/projects/project");
    let subagents = project.join("one-session/subagents");
    std::fs::create_dir_all(&subagents).unwrap();
    let record = serde_json::json!({"sessionId":"one-session", "customTitle":"ONLY_SESSION", "cwd":fixture.workspace, "timestamp":"2026-10-28T00:00:00Z"}).to_string();
    std::fs::write(project.join("one-session.jsonl"), &record).unwrap();
    for index in 0..20 {
        std::fs::write(subagents.join(format!("agent-{index}.jsonl")), &record).unwrap();
    }
    let executable = fixture.workspace.join("fake-claude");
    let launches = fixture.workspace.join("launches");
    std::fs::write(
        &executable,
        format!(
            "#!/bin/sh\nprintf x >> '{}'\nprintf SESSION_CLICK_OPENED\nexec sleep 30\n",
            launches.display()
        ),
    )
    .unwrap();
    std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o700)).unwrap();
    let mut config = fixture.config(&fixture.workspace);
    config
        .env
        .push(("TIDE_CLAUDE_PATH".into(), executable.display().to_string()));
    let mut session = PtySession::spawn(config).unwrap();
    let mut screen = TerminalEmulator::new(120, 40, 1000);
    wait_screen(&session, &mut screen, |snapshot| {
        snapshot.text().contains("Ctrl+P")
    });
    session.write(F1).unwrap(); // palette
    wait_screen(&session, &mut screen, |snapshot| {
        snapshot.text().contains("Command Palette")
    });
    session.write(b"Agents: Toggle Sidebar\r").unwrap();
    let snapshot = wait_screen(&session, &mut screen, |snapshot| {
        snapshot.text().contains("ONLY_SESSION") && snapshot.text().contains("Claude Code (1)")
    });
    click(
        &mut session,
        position(&snapshot, "Claude Code (1)", None, 0),
    );
    let snapshot = wait_screen(&session, &mut screen, |snapshot| {
        !snapshot.text().contains("ONLY_SESSION") && snapshot.text().contains("Claude Code (1)")
    });
    click(
        &mut session,
        position(&snapshot, "Claude Code (1)", None, 0),
    );
    let snapshot = wait_screen(&session, &mut screen, |snapshot| {
        snapshot.text().contains("ONLY_SESSION")
    });
    click(&mut session, position(&snapshot, "ONLY_SESSION", None, 0));
    click(&mut session, position(&snapshot, "ONLY_SESSION", None, 0));
    let snapshot = wait_screen(&session, &mut screen, |snapshot| {
        snapshot.text().contains("SESSION_CLICK_OPENED")
    });
    // Focusing the terminal responsively hides Agents at 120 columns. Its
    // directional restore arrow stays at the pane edge.
    let arrow = (0..snapshot.cols)
        .rev()
        .find(|x| snapshot.cell(*x, 0).is_some_and(|c| c.symbol == "←"))
        .expect("Agents restore arrow");
    click(&mut session, (arrow, 0));
    let snapshot = wait_screen(&session, &mut screen, |s| s.text().contains("ONLY_SESSION"));
    click(&mut session, position(&snapshot, "ONLY_SESSION", None, 0));
    click(&mut session, position(&snapshot, "ONLY_SESSION", None, 0));
    quit_from_palette(&mut session, &mut screen);
    assert_eq!(std::fs::read_to_string(launches).unwrap(), "x");
}

#[test]
fn terminal_editor_fold_click_targets_visible_line_and_find_buttons_replace_matches() {
    let fixture = Fixture::new();
    let file = fixture.workspace.join("editor.py");
    std::fs::write(
        &file,
        "if ready:\n    hidden-A\n    hidden-B\nafter marker\nfoo FOO food foo\n👩‍💻 end\n",
    )
    .unwrap();
    let mut session = PtySession::spawn(fixture.config(&file)).unwrap();
    let mut screen = TerminalEmulator::new(120, 40, 0);
    let snapshot = wait_screen(&session, &mut screen, |s| s.text().contains("hidden-A"));
    let (_, row) = position(&snapshot, "if ready:", None, 37);
    click(&mut session, position(&snapshot, "▾", Some(row), 37));
    let snapshot = wait_screen(&session, &mut screen, |s| {
        !s.text().contains("hidden-A") && s.text().contains("after marker")
    });
    let (column, row) = position(&snapshot, "after marker", None, 37);
    click(&mut session, (column + 5, row));
    session.write(b"X\x13").unwrap();
    wait_screen(&session, &mut screen, |s| {
        s.text().contains("afterX marker")
    });
    assert!(
        std::fs::read_to_string(&file)
            .unwrap()
            .contains("afterX marker")
    );
    session.write(b"\x06foo\r").unwrap();
    let snapshot = wait_screen(&session, &mut screen, |s| {
        s.text().contains("Find") && s.text().contains("Replace all")
    });
    click(&mut session, position(&snapshot, "Replace all", None, 0));
    wait_screen(&session, &mut screen, |s| s.text().contains("Replace:"));
    session.write(b"bar").unwrap();
    let snapshot = wait_screen(&session, &mut screen, |s| {
        s.text().contains("bar") && s.text().contains("Replace:")
    });
    click(&mut session, position(&snapshot, "Replace all", None, 0));
    session.write(b"\x1b[27u").unwrap();
    wait_screen(&session, &mut screen, |s| {
        !s.text().contains("Find and Replace")
    });
    session.write(&[0x13]).unwrap();
    wait_screen(&session, &mut screen, |s| {
        s.text().contains("bar bar bard bar") && s.text().contains("Saved ")
    });
    assert!(
        std::fs::read_to_string(&file)
            .unwrap()
            .contains("bar bar bard bar")
    );
    quit(&mut session);
}

fn palette_command(session: &mut PtySession, screen: &mut TerminalEmulator, label: &str) {
    session.write(F1).unwrap();
    wait_screen(session, screen, |s| s.text().contains("Command Palette"));
    session.write(format!("{label}\r").as_bytes()).unwrap();
}

#[test]
fn outside_click_dismisses_palette_and_find_but_preserves_dirty_close_decision() {
    let fixture = Fixture::new();
    let file = fixture.workspace.join("outside.txt");
    std::fs::write(&file, "OUTSIDE_CLICK_FIXTURE\n").unwrap();
    let mut session = PtySession::spawn(fixture.config(&file)).unwrap();
    let mut screen = TerminalEmulator::new(120, 40, 1000);
    wait_screen(&session, &mut screen, |s| {
        s.text().contains("OUTSIDE_CLICK_FIXTURE")
    });
    session.write(&[0x0b]).unwrap();
    wait_screen(&session, &mut screen, |s| {
        s.text().contains("Command Palette")
    });
    // This status control would reopen the palette if the click leaked through.
    click(&mut session, (0, 39));
    wait_screen(&session, &mut screen, |s| {
        !s.text().contains("Command Palette")
    });
    session.write(&[0x06]).unwrap();
    wait_screen(&session, &mut screen, |s| s.text().contains("Find:"));
    session.write(b"OUTSIDE").unwrap();
    wait_screen(&session, &mut screen, |s| s.text().contains("1 matches"));
    click(&mut session, (0, 39));
    wait_screen(&session, &mut screen, |s| {
        !s.text().contains("Find:") && !s.text().contains("Command Palette")
    });
    session.write(b"DIRTY_").unwrap();
    let snapshot = wait_screen(&session, &mut screen, |s| s.text().contains("DIRTY_"));
    click(&mut session, position(&snapshot, "×", Some(0), 36));
    wait_screen(&session, &mut screen, |s| {
        s.text().contains("Unsaved Changes")
    });
    click(&mut session, (0, 39));
    wait_screen(&session, &mut screen, |s| {
        s.text().contains("Unsaved Changes")
    });
    session.write(b"\x1b[27u").unwrap();
    wait_screen(&session, &mut screen, |s| {
        !s.text().contains("Unsaved Changes") && s.text().contains("DIRTY_")
    });
    assert_eq!(
        std::fs::read_to_string(&file).unwrap(),
        "OUTSIDE_CLICK_FIXTURE\n"
    );
    session.kill().unwrap();
}

#[test]
fn terminal_pane_chrome_obeys_dirty_close_and_arrows_restore_hidden_sidebar() {
    let fixture = Fixture::new();
    let file = fixture.workspace.join("pane.txt");
    std::fs::write(&file, "PANE_CHROME_FIXTURE\n").unwrap();
    let mut session = PtySession::spawn(fixture.config(&file)).unwrap();
    let mut screen = TerminalEmulator::new(120, 40, 1000);
    wait_screen(&session, &mut screen, |s| {
        s.text().contains("PANE_CHROME_FIXTURE")
    });
    session.write(&[0x1c]).unwrap();
    let snapshot = wait_screen(&session, &mut screen, |s| {
        (0..s.cols)
            .filter(|x| *x > 36 && s.cell(*x, 0).is_some_and(|c| c.symbol == "▾"))
            .count()
            == 2
    });
    let right = (0..snapshot.cols)
        .rev()
        .find(|x| snapshot.cell(*x, 0).is_some_and(|c| c.symbol == "←"))
        .unwrap();
    let close = (
        (0..right)
            .rev()
            .find(|x| snapshot.cell(*x, 0).is_some_and(|c| c.symbol == "×"))
            .unwrap(),
        0,
    );
    session.write(b"!").unwrap();
    click(&mut session, close);
    wait_screen(&session, &mut screen, |s| {
        s.text().contains("Save changes before closing this pane?")
            && s.text().contains("Cancel [Esc]")
    });
    session.write(b"\r").unwrap();
    wait_screen(&session, &mut screen, |s| {
        !s.text().contains("Close Pane")
            && (0..s.cols)
                .filter(|x| *x > 36 && s.cell(*x, 0).is_some_and(|c| c.symbol == "▾"))
                .count()
                == 2
    });
    click(&mut session, close);
    let snapshot = wait_screen(&session, &mut screen, |s| {
        s.text().contains("Close Pane") && s.text().contains("Cancel [Esc]")
    });
    click(&mut session, position(&snapshot, "Discard [N]", None, 0));
    let snapshot = wait_screen(&session, &mut screen, |s| {
        !s.text().contains("Close Pane")
            && (0..s.cols)
                .filter(|x| *x > 36 && s.cell(*x, 0).is_some_and(|c| c.symbol == "▾"))
                .count()
                == 1
    });
    session.write(&[0x13]).unwrap();
    wait_screen(&session, &mut screen, |s| {
        s.text().contains("Saved pane.txt")
    });
    assert!(std::fs::read_to_string(&file).unwrap().starts_with('!'));
    click(&mut session, position(&snapshot, "←", Some(0), 0));
    let snapshot = wait_screen(&session, &mut screen, |s| !s.text().contains("Explorer"));
    click(&mut session, position(&snapshot, "→", Some(0), 0));
    let snapshot = wait_screen(&session, &mut screen, |s| s.text().contains("Explorer"));
    click(&mut session, position(&snapshot, ">_", Some(39), 0));
    wait_screen(&session, &mut screen, |s| {
        s.text().contains("Command Palette")
    });
    session.write(b"\x1b[27u").unwrap();
    wait_screen(&session, &mut screen, |s| {
        !s.text().contains("Command Palette") && !s.text().contains("Tabs Across Panes")
    });
    quit(&mut session);
}

#[test]
fn terminal_dock_overflow_selects_and_closes_the_rendered_terminal() {
    let fixture = Fixture::new();
    let file = fixture.workspace.join("dock.txt");
    std::fs::write(&file, "DOCK_FIXTURE\n").unwrap();
    let mut session = PtySession::spawn(fixture.config(&file)).unwrap();
    let mut screen = TerminalEmulator::new(120, 40, 1000);
    wait_screen(&session, &mut screen, |s| s.text().contains("DOCK_FIXTURE"));
    for index in 1..=6 {
        palette_command(&mut session, &mut screen, "Open Bottom Terminal Tab");
        wait_screen(&session, &mut screen, |s| {
            !s.text().contains("Command Palette") && s.text().contains(&format!("Terminal {index}"))
        });
    }
    let snapshot = wait_screen(&session, &mut screen, |s| s.text().contains("Terminal 6"));
    let (_, row) = position(&snapshot, "Terminal 6", None, 36);
    click(&mut session, position(&snapshot, "▾", Some(row), 36));
    wait_screen(&session, &mut screen, |s| s.text().contains("Open Tabs"));
    session.write(b"Terminal 2\r").unwrap();
    let snapshot = wait_screen(&session, &mut screen, |s| {
        !s.text().contains("Open Tabs") && s.text().contains("Terminal 2")
    });
    let (column, row) = position(&snapshot, "Terminal 2", Some(row), 36);
    click(
        &mut session,
        position(&snapshot, "×", Some(row), column + 10),
    );
    wait_screen(&session, &mut screen, |s| !s.text().contains("Terminal 2"));
    palette_command(&mut session, &mut screen, "Editor: Switch Tab Across Panes");
    wait_screen(&session, &mut screen, |s| {
        s.text().contains("Tabs Across Panes")
    });
    session.write(b"Terminal 2").unwrap();
    wait_screen(&session, &mut screen, |s| {
        s.text().contains("No matching items")
    });
    session.write(b"\x1b[27u").unwrap();
    wait_screen(&session, &mut screen, |s| {
        !s.text().contains("Command Palette") && !s.text().contains("Tabs Across Panes")
    });
    quit_from_palette(&mut session, &mut screen);
}

#[test]
fn git_diff_mouse_edit_stages_custom_text_without_touching_working_file() {
    let fixture = Fixture::new();
    let git = |args: &[&str]| {
        let output = std::process::Command::new("git")
            .arg("-C")
            .arg(&fixture.workspace)
            .args(args)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        output.stdout
    };
    git(&["init", "-q", "-b", "main"]);
    git(&["config", "user.name", "Tide Fixture"]);
    git(&["config", "user.email", "tide@example.test"]);
    let path = fixture.workspace.join("git-file.txt");
    std::fs::write(&path, "BASELINE\n").unwrap();
    git(&["add", "--", "git-file.txt"]);
    git(&["commit", "-q", "-m", "initial"]);
    std::fs::write(&path, "WORKING_VERSION\n").unwrap();
    let mut config = fixture.config(&path);
    config.args.push("--debug".into());
    config.env.push((
        "RUST_LOG".into(),
        "tide_ui::git_jobs=debug,tide_services::git_process=debug".into(),
    ));
    let mut session = PtySession::spawn(config).unwrap();
    let mut screen = TerminalEmulator::new(120, 40, 1000);
    wait_screen(&session, &mut screen, |s| {
        s.text().contains("WORKING_VERSION")
    });
    session.write(F1).unwrap();
    wait_screen(&session, &mut screen, |s| {
        s.text().contains("Command Palette")
    });
    session.write(b"View Diff\r").unwrap();
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        wait_screen(&session, &mut screen, |s| {
            s.text().contains("Edit [e]") && s.text().contains("@@")
        })
    }));
    let snapshot = match result {
        Ok(snapshot) => snapshot,
        Err(error) => {
            use std::io::Read;
            let mut trace = String::new();
            if let Ok(file) =
                std::fs::File::open(fixture.home.path().join("STATE/tide/logs/debug.log"))
            {
                let _ = file.take(64 * 1024).read_to_string(&mut trace);
            }
            eprintln!("Synthetic Git queue diagnostics:\n{trace}");
            std::panic::resume_unwind(error);
        }
    };
    click(&mut session, position(&snapshot, "Edit [e]", None, 0));
    wait_screen(&session, &mut screen, |s| s.text().contains("Edit hunk:"));
    session.write(&[0x01]).unwrap();
    session.write(b"\x1b[200~STAGED_CUSTOM\n\x1b[201~").unwrap();
    session.write(&[0x13]).unwrap();
    wait_screen(&session, &mut screen, |s| {
        s.text().contains("Edited hunk staged")
    });
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "WORKING_VERSION\n");
    assert_eq!(
        String::from_utf8(git(&["show", ":git-file.txt"])).unwrap(),
        "STAGED_CUSTOM\n"
    );
    quit(&mut session);
}

fn mock_rust_server(fixture: &Fixture) -> PathBuf {
    use std::os::unix::fs::PermissionsExt;
    let bin = fixture.home.path().join("lsp-bin");
    std::fs::create_dir(&bin).unwrap();
    let executable = bin.join("rust-analyzer");
    let source = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../tide-lsp/tests/mock_server.py");
    std::fs::write(
        &executable,
        format!("#!/bin/sh\nexec /usr/bin/python3 '{}'\n", source.display()),
    )
    .unwrap();
    std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o700)).unwrap();
    bin
}
#[test]
fn lsp_code_action_synchronizes_its_edit_before_the_dependent_command() {
    let fixture = Fixture::new();
    let bin = mock_rust_server(&fixture);
    let path = fixture.workspace.join("action.rs");
    std::fs::write(&path, "old\n").unwrap();
    let log = fixture.home.path().join("lsp-actions.jsonl");
    let mut config = fixture.config(&path);
    config
        .env
        .push(("PATH".into(), format!("{}:/usr/bin:/bin", bin.display())));
    config
        .env
        .push(("TIDE_MOCK_LSP_LOG".into(), log.display().to_string()));
    let mut session = PtySession::spawn(config).unwrap();
    let mut screen = TerminalEmulator::new(120, 40, 1000);
    wait_screen(&session, &mut screen, |s| s.text().contains("old"));
    session.write(F1).unwrap();
    wait_screen(&session, &mut screen, |s| {
        s.text().contains("Command Palette")
    });
    session.write(b"Show Code Actions\r").unwrap();
    let snapshot = wait_screen(&session, &mut screen, |s| s.text().contains("Fix and Run"));
    click(&mut session, position(&snapshot, "Fix and Run", None, 0));
    wait_screen(&session, &mut screen, |s| {
        s.text().contains("executed fixture.order")
    });
    session.write(&[0x13]).unwrap();
    wait_screen(&session, &mut screen, |s| {
        s.text().contains("Saved action.rs")
    });
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "fixed\n");
    quit(&mut session);
    let line = std::fs::read_to_string(log).unwrap();
    assert!(line.contains("fixed\\n"));
    assert!(line.contains("fixture.order"));
}
#[test]
fn typed_lsp_completion_uses_insert_text_and_accepts_mouse_selection() {
    let fixture = Fixture::new();
    let bin = mock_rust_server(&fixture);
    let path = fixture.workspace.join("completion.rs");
    std::fs::write(&path, "mo").unwrap();
    let mut config = fixture.config(&path);
    config
        .env
        .push(("PATH".into(), format!("{}:/usr/bin:/bin", bin.display())));
    let mut session = PtySession::spawn(config).unwrap();
    let mut screen = TerminalEmulator::new(120, 40, 1000);
    wait_screen(&session, &mut screen, |s| {
        s.text().contains("completion.rs")
    });
    session.write(b"\x1b[Fck").unwrap();
    let snapshot = wait_screen(&session, &mut screen, |s| {
        s.text().contains("mock_fn") && s.text().contains("Completion")
    });
    click(&mut session, position(&snapshot, "mock_fn", None, 0));
    session.write(&[0x13]).unwrap();
    wait_screen(&session, &mut screen, |s| {
        s.text().contains("Saved completion.rs")
    });
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "mock_fn()");
    quit(&mut session);
}
#[test]
fn native_restart_cancels_safely_then_restores_the_dirty_buffer() {
    let fixture = Fixture::new();
    let path = fixture.workspace.join("restart.txt");
    std::fs::write(&path, "original").unwrap();
    let mut session = PtySession::spawn(fixture.config(&path)).unwrap();
    let mut screen = TerminalEmulator::new(120, 40, 1000);
    wait_screen(&session, &mut screen, |s| s.text().contains("original"));
    session.write(b"changed ").unwrap();
    for confirm in [false, true] {
        session.write(F1).unwrap();
        wait_screen(&session, &mut screen, |s| {
            s.text().contains("Command Palette")
        });
        session.write(b"Restart Tide\r").unwrap();
        wait_screen(&session, &mut screen, |s| {
            s.text().contains("Unsaved Changes") && s.text().contains("Cancel [Esc]")
        });
        session.write(if confirm { b"n" } else { b"\x1b" }).unwrap();
        wait_screen(&session, &mut screen, |s| {
            !s.text().contains("Unsaved Changes") && s.text().contains("changed original")
        });
    }
    session.write(&[0x13]).unwrap();
    wait_screen(&session, &mut screen, |s| {
        s.text().contains("Saved restart.txt")
    });
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "changed original");
    quit(&mut session);
}

#[test]
fn terminal_minimap_compresses_source_and_scrolls_without_relocating_the_caret() {
    let fixture = Fixture::new();
    fixture.preferences(r#"{"editor.minimap.enabled":true,"editor.wordWrap":"off","tui.agentsSidebar.visible":false}"#);
    let path = fixture.workspace.join("overview.txt");
    let content = (0..180)
        .map(|n| format!("SOURCE_LINE_{n:03} {}\n", "dense source ".repeat(6)))
        .collect::<String>();
    std::fs::write(&path, &content).unwrap();
    let mut session = PtySession::spawn(fixture.config(&path)).unwrap();
    let mut screen = TerminalEmulator::new(120, 40, 0);
    let snapshot = wait_screen(&session, &mut screen, |s| {
        s.text().contains("SOURCE_LINE_000")
            && s.text().contains(['░', '▒', '▓', '█'])
            && s.text()
                .lines()
                .last()
                .is_some_and(|row| row.contains(">_"))
    });
    let right = snapshot.cols - 1; // The proportional rail is the rightmost cell.
    let left = (36..right)
        .find(|x| {
            snapshot
                .cell(*x, 2)
                .is_some_and(|c| matches!(c.symbol.as_str(), "░" | "▒" | "▓" | "█"))
        })
        .expect("rendered miniature overview");
    assert_eq!(right - left, 10, "minimap must retain its ten-cell width");
    for y in 2..snapshot.rows - 2 {
        for x in left..right {
            let symbol = &snapshot.cell(x, y).unwrap().symbol;
            assert!(
                matches!(symbol.as_str(), " " | "░" | "▒" | "▓" | "█"),
                "uncompressed minimap {x},{y}: {symbol}"
            );
        }
    }
    click(&mut session, ((left + right) / 2, 25));
    wait_screen(&session, &mut screen, |s| {
        !s.text().contains("SOURCE_LINE_000") && s.text().contains("SOURCE_LINE_060")
    });
    session.write(b"\x1b[200~CARET_\x1b[201~\x13").unwrap();
    wait_screen(&session, &mut screen, |s| {
        s.text().contains("Saved overview.txt")
    });
    assert_eq!(
        std::fs::read_to_string(path).unwrap(),
        format!("CARET_{content}")
    );
    quit(&mut session);
}

#[test]
fn explorer_type_icons_and_proportional_scrollbar_are_visible_and_clickable() {
    let fixture = Fixture::new();
    fixture.preferences(r#"{"tui.agentsSidebar.visible":false}"#);
    std::fs::create_dir(fixture.workspace.join("folder")).unwrap();
    for name in ["a.ts", "b.js", "c.rs", "d.json", "e.md"] {
        std::fs::write(fixture.workspace.join(name), "icon fixture").unwrap();
    }
    for n in 0..100 {
        std::fs::write(
            fixture.workspace.join(format!("z_{n:03}.txt")),
            "scroll fixture",
        )
        .unwrap();
    }
    let mut session = PtySession::spawn(fixture.config(&fixture.workspace)).unwrap();
    let mut screen = TerminalEmulator::new(120, 40, 0);
    let snapshot = wait_screen(&session, &mut screen, |s| {
        // Rail bounds come from the completed first frame. The first file
        // and icon can arrive while only the upper half has been painted.
        s.text().contains("a.ts")
            && s.text().contains('\u{f07b}')
            && s.text()
                .lines()
                .last()
                .is_some_and(|row| row.contains(">_"))
    });
    for icon in ['\u{e628}', '\u{e74e}', '\u{e7a8}', '\u{e60b}', '\u{e609}'] {
        assert!(snapshot.text().contains(icon), "missing icon {icon}");
    }
    let start = position(&snapshot, "Explorer", None, 0).1 + 1;
    let end = position(&snapshot, "Enter open", None, 0).1;
    let rows = (start..end)
        .filter(|y| {
            snapshot
                .cell(34, *y)
                .is_some_and(|c| matches!(c.symbol.as_str(), "█" | "│"))
        })
        .collect::<Vec<_>>();
    let first = *rows.first().unwrap();
    let last = *rows.last().unwrap();
    let thumb = rows
        .iter()
        .filter(|y| snapshot.cell(34, **y).unwrap().symbol == "█")
        .count();
    assert!(thumb > 1 && thumb < rows.len());
    click(&mut session, (34, last));
    wait_screen(&session, &mut screen, |s| {
        !s.text().contains("a.ts") && s.text().contains("z_099.txt")
    });
    session
        .write(
            format!(
                "\x1b[<0;35;{}M\x1b[<32;35;{}M\x1b[<0;35;{}m",
                last + 1,
                first + 1,
                first + 1
            )
            .as_bytes(),
        )
        .unwrap();
    wait_screen(&session, &mut screen, |s| s.text().contains("a.ts"));
    quit(&mut session);
}

#[test]
#[ignore = "needs TIDE_TEST_DATABASE_URL (live PostgreSQL)"]
fn live_database_mouse_sort_edit_history_and_connection_form_use_real_targets() {
    let url = std::env::var("TIDE_TEST_DATABASE_URL").expect("TIDE_TEST_DATABASE_URL");
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let (mut connection, password) = tide_db::parse_postgres_url(&url).unwrap();
    connection.id = "ui-live".into();
    connection.name = "UI Fixture".into();
    connection.read_only = false;
    connection.password_secret = Some("db.ui.password".into());
    let backend = runtime
        .block_on(tide_db::PostgresSession::connect(
            &connection,
            password.as_deref(),
        ))
        .unwrap();
    runtime.block_on(backend.execute("DROP TABLE IF EXISTS tide_ui_workflow;CREATE TABLE tide_ui_workflow (id int PRIMARY KEY,label text);INSERT INTO tide_ui_workflow VALUES (10,'ten'),(2,'two')",true)).unwrap();
    let fixture = Fixture::new();
    fixture.preferences(r#"{"tui.agentsSidebar.visible":false,"editor.minimap.enabled":false}"#);
    let directory = fixture.home.path().join("CONFIG/tide");
    std::fs::write(
        directory.join("connections.json"),
        serde_json::to_vec(&serde_json::json!({"connections":[connection]})).unwrap(),
    )
    .unwrap();
    let mut config = fixture.config(&fixture.workspace);
    config.env.push((
        "TIDE_SECRET_DB_UI_PASSWORD".into(),
        password.clone().unwrap_or_default(),
    ));
    let mut session = PtySession::spawn(config).unwrap();
    let mut screen = TerminalEmulator::new(120, 40, 0);
    wait_screen(&session, &mut screen, |s| s.text().contains("Welcome"));
    palette_command(&mut session, &mut screen, "Database: New Query");
    wait_screen(&session, &mut screen, |s| {
        s.text().contains("SQL · UI Fixture")
    });
    session
        .write(b"\x1b[200~SELECT * FROM public.tide_ui_workflow ORDER BY id DESC;\x1b[201~\x12")
        .unwrap();
    let snapshot = wait_screen(&session, &mut screen, |s| {
        s.text().contains("2 rows") && s.text().contains("ten")
    });
    let title_row = position(&snapshot, "Results", None, 36).1;
    let id_header = position(&snapshot, "id", Some(title_row + 2), 36);
    click(&mut session, id_header);
    wait_screen(&session, &mut screen, |s| {
        s.cell(id_header.0, title_row + 3)
            .is_some_and(|c| c.symbol == "2")
    });
    click(&mut session, (id_header.0, title_row + 3));
    click(&mut session, (id_header.0, title_row + 3));
    let snapshot = wait_screen(&session, &mut screen, |s| {
        s.text().contains("← Results") && s.text().contains("label:")
    });
    let field = position(&snapshot, "label:", None, 36);
    click(&mut session, field);
    click(&mut session, field);
    wait_screen(&session, &mut screen, |s| s.text().contains("Edit label"));
    session.write(&[127; 64]).unwrap();
    let changed = "mouse'; DROP TABLE nobody; -- 👩‍💻";
    let value = serde_json::to_string(changed).unwrap();
    session
        .write(format!("\x1b[200~{value}\x1b[201~\x13").as_bytes())
        .unwrap();
    wait_screen(&session, &mut screen, |s| {
        s.text().contains("Database: row saved")
    });
    let result = runtime
        .block_on(backend.execute("SELECT label FROM tide_ui_workflow WHERE id=2", false))
        .unwrap();
    assert_eq!(result.rows[0]["label"], changed);
    let snapshot = screen.snapshot();
    let header = position(&snapshot, "id", Some(title_row + 2), 36);
    click(&mut session, header);
    wait_screen(&session, &mut screen, |s| {
        s.cell(header.0, title_row + 3)
            .is_some_and(|c| c.symbol == "2")
    });
    click(&mut session, (header.0, title_row + 3));
    click(&mut session, (header.0, title_row + 3));
    let snapshot = wait_screen(&session, &mut screen, |s| s.text().contains("← Results"));
    click(&mut session, position(&snapshot, "[Delete]", None, 36));
    wait_screen(&session, &mut screen, |s| {
        s.text().contains("Delete Row") && s.text().contains("id=2")
    });
    session.write(b"\r").unwrap();
    let snapshot = wait_screen(&session, &mut screen, |s| {
        !s.text().contains("Delete Row") && s.text().contains("← Results")
    });
    let count = runtime
        .block_on(backend.execute(
            "SELECT count(*) AS n FROM tide_ui_workflow WHERE id=2",
            false,
        ))
        .unwrap();
    assert_eq!(count.rows[0]["n"], 1);
    click(&mut session, position(&snapshot, "[Delete]", None, 36));
    let snapshot = wait_screen(&session, &mut screen, |s| s.text().contains("Delete Row"));
    click(&mut session, position(&snapshot, "Delete [Y]", None, 0));
    wait_screen(&session, &mut screen, |s| {
        s.text().contains("Database: row deleted")
    });
    let count = runtime
        .block_on(backend.execute(
            "SELECT count(*) AS n FROM tide_ui_workflow WHERE id=2",
            false,
        ))
        .unwrap();
    assert_eq!(count.rows[0]["n"], 0);
    palette_command(&mut session, &mut screen, "Database: Query History");
    let snapshot = wait_screen(&session, &mut screen, |s| {
        s.text().contains("Query History") && s.text().contains("Favorite [F]")
    });
    click(&mut session, position(&snapshot, "Favorite [F]", None, 0));
    let snapshot = wait_screen(&session, &mut screen, |s| s.text().contains('★'));
    click(&mut session, position(&snapshot, "Favorites [O]", None, 0));
    wait_screen(&session, &mut screen, |s| {
        s.text().contains("Query History · favorites")
    });
    session.write(b"\r").unwrap();
    wait_screen(&session, &mut screen, |s| {
        !s.text().contains("Query History") && s.text().contains("Ctrl+R runs the query")
    });
    palette_command(&mut session, &mut screen, "Database: Edit Connection");
    wait_screen(&session, &mut screen, |s| {
        s.text().contains("Edit PostgreSQL Connection")
    });
    session.write(b"\x15Renamed UI Fixture").unwrap();
    let snapshot = screen.snapshot();
    click(&mut session, position(&snapshot, "[Test]", None, 0));
    let snapshot = wait_screen(&session, &mut screen, |s| {
        s.text().contains("Database: Renamed UI Fixture OK")
    });
    click(&mut session, position(&snapshot, "[Save]", None, 0));
    wait_screen(&session, &mut screen, |s| {
        s.text().contains("Database: saved Renamed UI Fixture")
    });
    let stored = std::fs::read_to_string(directory.join("connections.json")).unwrap();
    assert!(stored.contains("Renamed UI Fixture"));
    let stored: serde_json::Value = serde_json::from_str(&stored).unwrap();
    assert!(stored["connections"][0].get("password").is_none());
    assert_eq!(stored["connections"][0]["passwordSecret"], "db.ui.password");
    // A proportional result thumb is interactive independently of the editor.
    session
        .write(b"\x01\x1b[200~SELECT generate_series(1,24) AS scroll_number;\x1b[201~\x12")
        .unwrap();
    let snapshot = wait_screen(&session, &mut screen, |s| {
        s.text().contains("24 rows") && s.text().contains("scroll_number")
    });
    let title = position(&snapshot, "Results", None, 36).1;
    let letters: Vec<_> = "scroll_number".chars().collect();
    let (data_column, header_row) = (title + 1..38)
        .find_map(|row| {
            (36..snapshot.cols.saturating_sub(letters.len()))
                .find(|column| {
                    letters.iter().enumerate().all(|(i, c)| {
                        snapshot
                            .cell(column + i, row)
                            .is_some_and(|cell| cell.symbol == c.to_string())
                    })
                })
                .map(|column| (column, row))
        })
        .expect("rendered result column header");
    let data_row = header_row + 1;
    let bar_column = (36..snapshot.cols)
        .rev()
        .find(|column| {
            snapshot
                .cell(*column, data_row)
                .is_some_and(|c| c.symbol == "█")
        })
        .expect("rendered result scroll thumb");
    let rows = (data_row..38)
        .filter(|y| {
            snapshot
                .cell(bar_column, *y)
                .is_some_and(|c| matches!(c.symbol.as_str(), "█" | "│"))
        })
        .collect::<Vec<_>>();
    let thumb = rows
        .iter()
        .filter(|y| snapshot.cell(bar_column, **y).unwrap().symbol == "█")
        .count();
    assert!(thumb > 1 && thumb < rows.len());
    let expected = (25 - rows.len()).to_string();
    click(&mut session, (bar_column, *rows.last().unwrap()));
    let snapshot = wait_screen(&session, &mut screen, |s| {
        expected.chars().enumerate().all(|(i, c)| {
            s.cell(data_column + i, data_row)
                .is_some_and(|cell| cell.symbol == c.to_string())
        })
    });
    click(&mut session, position(&snapshot, "← SQL", None, 36));
    session
        .write(b"\x01\x1b[200~SELECT pg_sleep(5);\x1b[201~\x12")
        .unwrap();
    let snapshot = wait_screen(&session, &mut screen, |s| s.text().contains("[Cancel]"));
    let started = Instant::now();
    click(&mut session, position(&snapshot, "[Cancel]", None, 36));
    wait_screen(&session, &mut screen, |s| {
        s.text().contains("Database: operation canceled")
    });
    assert!(started.elapsed() < Duration::from_secs(2));
    session.write(&[0x11]).unwrap();
    wait_screen(&session, &mut screen, |s| {
        s.text().contains("Unsaved Changes")
    });
    session.write(b"n").unwrap();
    let status = wait_exit(&mut session, |_| {});
    assert_eq!(status.expect("discard did not quit"), 0);
    runtime
        .block_on(backend.execute("DROP TABLE tide_ui_workflow", true))
        .unwrap();
}

#[test]
fn hostile_document_and_filename_controls_never_reach_the_host_terminal() {
    let fixture = Fixture::new();
    let file = fixture
        .workspace
        .join("hostile-\u{1b}]2;ATTACK_TITLE\u{7}.txt");
    std::fs::write(
        &file,
        concat!(
            "CONTROL_FIXTURE\n",
            "before\u{1b}]52;c;QVRUQUNLX0NMSVBCT0FSRA==\u{7}after\n",
            "before\u{1b}[2JATTACK_CLEAR\u{1b}[Hafter\n",
            "before\u{1b}]8;;https://invalid.example\u{1b}\\ATTACK_LINK\u{1b}]8;;\u{1b}\\after\n",
            "Unicode café 界🙂 stays readable\n",
            "C1 \u{9b}2J visible\n"
        ),
    )
    .unwrap();
    let mut session = PtySession::spawn(fixture.config(&file)).unwrap();
    let mut screen = TerminalEmulator::new(120, 40, 1000);
    let mut raw = Vec::new();
    let rendered = poll_until(|| {
        let bytes = session.read_for(Duration::from_millis(50));
        screen.feed(&bytes);
        raw.extend(bytes);
        let text = screen.snapshot().text();
        (text.contains("CONTROL_FIXTURE") && text.contains("stays readable")).then_some(())
    });
    assert!(rendered.is_some(), "{}", screen.snapshot().text());
    for attack in [
        b"\x1b]52;".as_slice(),
        b"\x1b]2;ATTACK_TITLE",
        b"\x1b]8;;https://invalid.example",
        b"\xc2\x9b2J",
    ] {
        assert!(
            !raw.windows(attack.len()).any(|bytes| bytes == attack),
            "attacker control escaped the renderer"
        );
    }
    let text = screen.snapshot().text();
    assert!(text.contains("before") && text.contains("after") && text.contains("café"));
    assert!(
        !text
            .chars()
            .any(|c| c == '\u{1b}' || ('\u{80}'..='\u{9f}').contains(&c))
    );
    quit(&mut session);
}

#[cfg(unix)]
#[test]
fn source_rebuild_runs_locked_offline_then_restarts_with_recovery() {
    use std::os::unix::fs::PermissionsExt;
    let fixture = Fixture::new();
    let path = fixture.workspace.join("rebuild.txt");
    std::fs::write(&path, "REBUILD_ORIGINAL").unwrap();
    let bin = fixture.home.path().join("bin");
    std::fs::create_dir(&bin).unwrap();
    let cargo = bin.join("cargo");
    // The real offline release build is a separate acceptance gate. This stub
    // isolates maintenance routing from Cargo's parent test-process lock.
    std::fs::write(
        &cargo,
        "#!/bin/sh\nprintf '%s\\n' \"$@\" > \"$TIDE_REBUILD_MARKER\"\nexit 0\n",
    )
    .unwrap();
    std::fs::set_permissions(&cargo, std::fs::Permissions::from_mode(0o700)).unwrap();
    let marker = fixture.home.path().join("build-arguments");
    let mut config = fixture.config(&path);
    config
        .env
        .push(("PATH".into(), format!("{}:/usr/bin:/bin", bin.display())));
    config
        .env
        .push(("TIDE_REBUILD_MARKER".into(), marker.display().to_string()));
    let mut session = PtySession::spawn(config).unwrap();
    let mut screen = TerminalEmulator::new(120, 40, 1000);
    wait_screen(&session, &mut screen, |s| {
        s.text().contains("REBUILD_ORIGINAL")
    });
    session.write(b"CHANGED_").unwrap();
    palette_command(&mut session, &mut screen, "Restart and Rebuild Tide");
    wait_screen(&session, &mut screen, |s| {
        s.text().contains("Unsaved Changes") && s.text().contains("Cancel [Esc]")
    });
    session.write(b"n").unwrap();
    wait_screen(&session, &mut screen, |s| {
        !s.text().contains("Unsaved Changes")
            && s.text().contains("CHANGED_REBUILD_ORIGINAL")
            && marker.exists()
    });
    let arguments = std::fs::read_to_string(marker).unwrap();
    assert_eq!(arguments, "build\n--locked\n--offline\n-p\ntide\n");
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "REBUILD_ORIGINAL");
    session.write(&[0x13]).unwrap();
    wait_screen(&session, &mut screen, |s| {
        s.text().contains("Saved rebuild.txt")
    });
    assert_eq!(
        std::fs::read_to_string(&path).unwrap(),
        "CHANGED_REBUILD_ORIGINAL"
    );
    quit(&mut session);
}

#[test]
fn terminal_save_before_quit_waits_for_draft_save_as_and_persists_the_file() {
    let fixture = Fixture::new();
    let mut session = PtySession::spawn(fixture.config(&fixture.workspace)).unwrap();
    let mut screen = TerminalEmulator::new(120, 40, 1000);
    wait_screen(&session, &mut screen, |s| s.text().contains("Welcome"));
    session.write(&[0x0e]).unwrap();
    wait_screen(&session, &mut screen, |s| {
        s.text().contains("Untitled") || s.text().contains("untitled")
    });
    session.write(b"SAVED_DRAFT_FIXTURE\x11").unwrap();
    let _confirm = wait_screen(&session, &mut screen, |s| {
        s.text().contains("Save [Y]") && s.text().contains("Cancel [Esc]")
    });
    session.write(b"y").unwrap();
    wait_screen(&session, &mut screen, |s| {
        s.text().contains("Save As") && s.text().contains("untitled.txt")
    });
    assert!(session.poll_exit().unwrap().is_none());
    session.write(&[127; 128]).unwrap();
    session.write(b"draft-saved.txt\r").unwrap();
    let status = wait_exit(&mut session, |session| {
        screen.feed(&session.read_for(Duration::from_millis(25)));
    });
    assert_eq!(
        status.unwrap_or_else(|| panic!("save/quit did not finish: {}", screen.snapshot().text())),
        0
    );
    assert_eq!(
        std::fs::read_to_string(fixture.workspace.join("draft-saved.txt")).unwrap(),
        "SAVED_DRAFT_FIXTURE"
    );
}

#[test]
fn welcome_opens_once_and_help_follows_the_palette_and_user_keys() {
    let fixture = Fixture::new();
    let mut session = PtySession::spawn(fixture.config(&fixture.workspace)).unwrap();
    let mut screen = TerminalEmulator::new(120, 40, 1000);
    wait_screen(&session, &mut screen, |s| {
        s.text().contains("Welcome to Tide 0") && !s.text().contains("# Welcome")
    });
    session.write(&[0x0b]).unwrap();
    wait_screen(&session, &mut screen, |s| {
        s.text().contains("Command Palette")
    });
    session.write(b"Help: Git").unwrap();
    wait_screen(&session, &mut screen, |s| s.text().contains("Help: Git"));
    session.write(b"\r").unwrap();
    wait_screen(&session, &mut screen, |s| {
        s.text().contains("Git Workflows")
            && !s.text().contains("# Git")
            && tab_bar_text(s).contains("Help: Git")
    });
    session.write(b"x").unwrap();
    wait_screen(&session, &mut screen, |s| {
        s.text().contains("Help: Git is read-only")
    });
    quit(&mut session);
    assert!(
        fixture
            .home
            .path()
            .join("STATE/tide/welcome-shown")
            .is_file()
    );

    std::fs::write(
        fixture.home.path().join("CONFIG/tide/keybindings.jsonc"),
        r#"[{ "key": "ctrl+alt+w", "command": "help.welcome" }]"#,
    )
    .unwrap();
    let mut session = PtySession::spawn(fixture.config(&fixture.workspace)).unwrap();
    let mut screen = TerminalEmulator::new(120, 40, 1000);
    let start = wait_screen(&session, &mut screen, |s| {
        s.text().contains("Ctrl+Alt+W opens the Welcome guide")
    });
    assert!(!start.text().contains("Welcome to Tide 0"));
    session.write(b"\x1b\x17").unwrap();
    wait_screen(&session, &mut screen, |s| {
        s.text().contains("Welcome to Tide 0") && s.text().contains("Ctrl+Alt+W")
    });
    quit(&mut session);
}
