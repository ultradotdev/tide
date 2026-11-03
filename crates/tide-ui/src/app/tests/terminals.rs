use super::*;

fn spawn(app: &App, program: &str, args: &[&str]) -> TerminalTab {
    let mut spec = SpawnSpec::new(program, args.iter().map(|arg| (*arg).to_owned()).collect());
    spec.cols = 40;
    spec.rows = 8;
    TerminalTab::spawn(1, &app.workspace, spec).unwrap()
}

/// An interactive shell without startup files, so tests do not depend on the host's rc files.
fn shell(app: &App) -> TerminalTab {
    spawn(
        app,
        "/bin/bash",
        &["--norc", "--noprofile", "--noediting", "-i"],
    )
}

fn wait_until(terminal: &mut TerminalTab, done: impl Fn(&TerminalTab) -> bool) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while !done(terminal) {
        assert!(
            Instant::now() < deadline,
            "terminal did not reach the state"
        );
        terminal.poll();
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn press(app: &mut App, ch: char) {
    app.handle_modal_key(KeyEvent::new(KeyCode::Char(ch), KeyModifiers::NONE));
}

fn screen(app: &App) -> String {
    app.dock.active().unwrap().snapshot().text()
}

#[test]
fn clear_and_scroll_reach_the_bottom_terminal_from_any_region() {
    let (_home, mut app) = test_app();
    app.execute_command(Command::TerminalClear);
    assert!(app.status.contains("no terminal"), "{}", app.status);
    let mut terminal = spawn(
        &app,
        "/bin/sh",
        &[
            "-c",
            "i=1; while [ $i -le 40 ]; do echo row$i; i=$((i+1)); done; exec sleep 30",
        ],
    );
    wait_until(&mut terminal, |terminal| {
        terminal.snapshot().text().contains("row40")
    });
    app.dock.tabs.push(terminal);
    app.dock.visible = true;
    app.focus = Focus::Main;
    app.execute_command(Command::TerminalScrollToTop);
    assert!(screen(&app).contains("row1\n"), "{}", screen(&app));
    app.execute_command(Command::TerminalScrollToBottom);
    assert!(screen(&app).contains("row40"));
    app.execute_command(Command::TerminalClear);
    // Nothing is typed into the child: the screen is cleared by the emulator itself.
    assert!(!screen(&app).contains("row40"), "{}", screen(&app));
    app.execute_command(Command::TerminalScrollToTop);
    assert!(
        !screen(&app).contains("row1\n"),
        "scrollback survived the clear"
    );
}

#[test]
fn kill_confirms_only_while_a_program_runs_and_marks_the_tab_exited() {
    let (_home, mut app) = test_app();
    let mut idle = shell(&app);
    idle.send_text("echo RE''ADY\n");
    wait_until(&mut idle, |terminal| {
        terminal.snapshot().text().contains("READY")
    });
    app.dock.tabs.push(idle);
    app.dock.visible = true;
    app.focus = Focus::Dock;
    app.execute_command(Command::TerminalKill);
    assert!(app.overlay.is_none(), "an idle shell is killed at once");
    let killed = app.dock.active().unwrap();
    assert!(killed.exited.is_some());
    assert!(killed.title.contains("[exit"), "{}", killed.title);
    app.execute_command(Command::TerminalKill);
    assert!(app.status.contains("already exited"), "{}", app.status);

    let mut busy = shell(&app);
    busy.send_text("sleep 30\n");
    wait_until(&mut busy, TerminalTab::is_busy);
    app.dock.tabs.push(busy);
    app.dock.active = 1;
    app.execute_command(Command::TerminalKill);
    assert!(matches!(
        app.overlay.as_ref().unwrap().kind(),
        ModalKind::Confirm(ConfirmAction::KillTerminal(_))
    ));
    app.handle_modal_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
    assert!(
        app.dock.active().unwrap().exited.is_none(),
        "cancel kept it"
    );
    app.execute_command(Command::TerminalKill);
    press(&mut app, 'y');
    assert!(app.dock.active().unwrap().exited.is_some());
    assert_eq!(
        app.dock.tabs.len(),
        2,
        "killed tabs stay open, marked exited"
    );
}

#[test]
fn close_all_asks_while_busy_and_leaves_agent_sessions_open() {
    let (_home, mut app) = test_app();
    app.execute_command(Command::TerminalCloseAll);
    assert!(app.status.contains("no terminals"), "{}", app.status);
    let idle = shell(&app);
    app.dock.tabs.push(idle);
    app.workbench.insert_tab_focused(Tab::Terminal(spawn(
        &app,
        "/bin/sh",
        &["-c", "exec sleep 30"],
    )));
    let mut agent = spawn(&app, "/bin/sh", &["-c", "exec sleep 30"]);
    agent.ai_provider = Some("claude".into());
    app.workbench.insert_tab_focused(Tab::Terminal(agent));
    // Without a running program nothing needs confirming.
    app.execute_command(Command::TerminalCloseAll);
    assert!(app.overlay.is_none());
    assert!(app.dock.tabs.is_empty());
    let terminals = app
        .workbench
        .tabs()
        .filter_map(|tab| match tab {
            Tab::Terminal(terminal) => Some(terminal.ai_provider.clone()),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(terminals, [Some("claude".to_owned())]);
    assert!(app.status.contains("closed 2"), "{}", app.status);

    let mut busy = shell(&app);
    busy.send_text("sleep 30\n");
    wait_until(&mut busy, TerminalTab::is_busy);
    app.dock.tabs.push(busy);
    app.execute_command(Command::TerminalCloseAll);
    assert!(matches!(
        app.overlay.as_ref().unwrap().kind(),
        ModalKind::Confirm(ConfirmAction::CloseAllTerminals)
    ));
    press(&mut app, 'y');
    assert!(app.dock.tabs.is_empty());
}

#[test]
fn quitting_asks_first_while_a_terminal_runs_a_program_and_ctrl_q_stays_with_the_shell() {
    let (_home, mut app) = test_app();
    let mut busy = shell(&app);
    busy.title = "build".into();
    busy.send_text("sleep 30\n");
    wait_until(&mut busy, TerminalTab::is_busy);
    app.dock.tabs.push(busy);
    app.dock.visible = true;
    app.focus = Focus::Dock;

    // Ctrl+Q in a focused terminal is the program's key, not Quit.
    app.handle_key(KeyEvent::new(KeyCode::Char('q'), KeyModifiers::CONTROL));
    assert!(app.overlay.is_none());
    assert!(!app.should_quit);

    app.execute_command(Command::WorkbenchQuit);
    assert!(matches!(
        app.overlay.as_ref().unwrap().kind(),
        ModalKind::Confirm(ConfirmAction::QuitBusy { restart: false })
    ));
    app.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
    assert!(app.overlay.is_none());
    assert!(!app.should_quit);

    app.execute_command(Command::WorkbenchQuit);
    press(&mut app, 'y');
    assert!(app.should_quit);
}

fn copies_succeed(_copy: bool) -> Result<std::process::Command, String> {
    Ok(std::process::Command::new("cat"))
}

fn no_system_clipboard(_copy: bool) -> Result<std::process::Command, String> {
    Err("system clipboard unavailable".into())
}

fn mouse(app: &mut App, kind: MouseEventKind, column: u16, row: u16) {
    app.handle_event(Event::Mouse(MouseEvent {
        kind,
        column,
        row,
        modifiers: KeyModifiers::NONE,
    }));
}

/// Waits for the clipboard helper's result and applies it, as the event loop does.
fn finish_copy(app: &mut App, events: &mut tokio::sync::mpsc::Receiver<AppEvent>) {
    loop {
        let event = events.blocking_recv().expect("clipboard result");
        let done = matches!(event, AppEvent::ClipboardWritten { .. });
        app.handle_background(event);
        if done {
            return;
        }
    }
}

/// A main-pane terminal showing two lines after printing `modes`, drawn so its cells can be
/// clicked.
fn terminal_pane(
    app: &mut App,
    modes: &str,
) -> (Rect, ratatui::Terminal<ratatui::backend::TestBackend>) {
    let mut terminal = spawn(
        app,
        "/bin/sh",
        &[
            "-c",
            &format!("printf '{modes}alpha beta\\ngamma delta\\n'; exec sleep 30"),
        ],
    );
    wait_until(&mut terminal, |terminal| {
        terminal.snapshot().text().contains("gamma delta")
    });
    app.workbench.insert_tab_focused(Tab::Terminal(terminal));
    app.focus = Focus::Main;
    let mut screen = ratatui::Terminal::new(ratatui::backend::TestBackend::new(120, 40)).unwrap();
    screen.draw(|frame| app.draw(frame)).unwrap();
    let pane = app.workbench.focused();
    let area = app
        .layout
        .hits
        .iter()
        .find_map(|(_, target)| match target {
            HitTarget::EditorContent(id, area) if *id == pane => Some(*area),
            _ => None,
        })
        .expect("terminal content");
    (area, screen)
}

#[test]
fn dragging_in_a_terminal_pane_selects_highlights_and_copies_text() {
    let (_home, mut app) = test_app();
    let mut events = app.take_event_receiver().unwrap();
    app.clipboard_commands = copies_succeed;
    app.clipboard_host = crate::clipboard::Host::default();
    let (area, mut screen) = terminal_pane(&mut app, "");
    app.focus = Focus::Sidebar;

    let left = MouseButton::Left;
    mouse(&mut app, MouseEventKind::Down(left), area.x + 6, area.y);
    assert_eq!(app.focus, Focus::Main, "pressing in a terminal focuses it");
    mouse(&mut app, MouseEventKind::Drag(left), area.x + 7, area.y + 1);
    // Past the right edge: the pointer stays captured and is held on the last column.
    mouse(
        &mut app,
        MouseEventKind::Drag(left),
        area.right() + 5,
        area.y,
    );
    mouse(&mut app, MouseEventKind::Drag(left), area.x + 4, area.y + 1);
    mouse(&mut app, MouseEventKind::Up(left), area.x + 4, area.y + 1);
    finish_copy(&mut app, &mut events);
    assert_eq!(app.clipboard, "beta\ngamma");
    assert_eq!(app.status, "Copied terminal selection");
    assert!(
        app.take_host_output().is_empty(),
        "OSC 52 waits for a failure"
    );

    screen.draw(|frame| app.draw(frame)).unwrap();
    let buffer = screen.backend().buffer();
    let selection = app.palette.selection;
    assert_eq!(buffer[(area.x + 6, area.y)].bg, selection);
    assert_eq!(buffer[(area.x, area.y + 1)].bg, selection);
    assert_ne!(buffer[(area.x + 5, area.y)].bg, selection);
    assert_ne!(buffer[(area.x + 5, area.y + 1)].bg, selection);

    // The palette command copies the selection again; Ctrl+C stays the shell's interrupt.
    app.clipboard.clear();
    app.handle_key(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL));
    assert!(app.clipboard.is_empty());
    let terminal = || match app.workbench.active_tab() {
        Some(Tab::Terminal(terminal)) => terminal.selection_text(),
        _ => None,
    };
    assert_eq!(
        terminal(),
        None,
        "keys sent to the program drop the selection"
    );

    // A double click selects a word; copy-on-select can be turned off.
    app.settings
        .set("terminal.copyOnSelect", serde_json::json!(false));
    mouse(&mut app, MouseEventKind::Down(left), area.x + 2, area.y + 1);
    mouse(&mut app, MouseEventKind::Up(left), area.x + 2, area.y + 1);
    mouse(&mut app, MouseEventKind::Down(left), area.x + 2, area.y + 1);
    mouse(&mut app, MouseEventKind::Up(left), area.x + 2, area.y + 1);
    assert!(app.clipboard.is_empty());
    app.execute_command(Command::TerminalCopySelection);
    finish_copy(&mut app, &mut events);
    assert_eq!(app.clipboard, "gamma");
    // A third click takes the whole line, and Edit: Copy (Cmd+C) copies in a focused terminal.
    mouse(&mut app, MouseEventKind::Down(left), area.x + 2, area.y + 1);
    mouse(&mut app, MouseEventKind::Up(left), area.x + 2, area.y + 1);
    app.execute_command(Command::EditCopy);
    finish_copy(&mut app, &mut events);
    assert_eq!(app.clipboard, "gamma delta\n");

    // A single click clears the selection.
    std::thread::sleep(Duration::from_millis(450));
    mouse(&mut app, MouseEventKind::Down(left), area.x + 2, area.y + 1);
    mouse(&mut app, MouseEventKind::Up(left), area.x + 2, area.y + 1);
    app.execute_command(Command::TerminalCopySelection);
    assert!(app.status.contains("nothing is selected"), "{}", app.status);
}

#[test]
fn a_program_with_mouse_reporting_keeps_the_mouse_until_shift_is_held() {
    let (_home, mut app) = test_app();
    let mut events = app.take_event_receiver().unwrap();
    app.clipboard_commands = copies_succeed;
    // As vim or htop would: ask for presses and drags.
    let (area, _screen) = terminal_pane(&mut app, "\\033[?1000h\\033[?1002h\\033[?1006h");
    let left = MouseButton::Left;
    mouse(&mut app, MouseEventKind::Down(left), area.x, area.y);
    mouse(&mut app, MouseEventKind::Drag(left), area.x + 4, area.y);
    mouse(&mut app, MouseEventKind::Up(left), area.x + 4, area.y);
    let selected = |app: &App| match app.workbench.active_tab() {
        Some(Tab::Terminal(terminal)) => terminal.selection_text(),
        _ => None,
    };
    assert_eq!(selected(&app), None, "the program got the drag");

    let shift = |kind, column| {
        Event::Mouse(MouseEvent {
            kind,
            column,
            row: area.y,
            modifiers: KeyModifiers::SHIFT,
        })
    };
    app.handle_event(shift(MouseEventKind::Down(left), area.x));
    app.handle_event(shift(MouseEventKind::Drag(left), area.x + 4));
    app.handle_event(shift(MouseEventKind::Up(left), area.x + 4));
    finish_copy(&mut app, &mut events);
    assert_eq!(app.clipboard, "alpha");
}

#[test]
fn copies_reach_the_host_terminal_with_osc52_over_ssh_or_when_the_clipboard_fails() {
    let (_home, mut app) = test_app();
    let mut events = app.take_event_receiver().unwrap();
    app.new_file();
    app.workbench
        .active_editor_mut()
        .unwrap()
        .insert_text("héllo");
    app.workbench.active_editor_mut().unwrap().select_all();
    let expected = crate::clipboard::osc52("héllo", crate::clipboard::Host::default()).unwrap();

    // Locally, OSC 52 is the fallback when no system clipboard can be reached.
    app.clipboard_host = crate::clipboard::Host::default();
    app.clipboard_commands = no_system_clipboard;
    app.execute_command(Command::EditCopy);
    assert!(app.take_host_output().is_empty());
    finish_copy(&mut app, &mut events);
    assert_eq!(app.take_host_output(), expected);
    assert_eq!(app.status, "Copied selection through the terminal (OSC 52)");

    // Over SSH it is sent at once, since the system clipboard is the remote machine's.
    app.clipboard_host = crate::clipboard::Host {
        remote: true,
        tmux: false,
    };
    app.clipboard_commands = copies_succeed;
    app.execute_command(Command::EditCopy);
    assert_eq!(app.take_host_output(), expected);
    finish_copy(&mut app, &mut events);
    assert!(app.take_host_output().is_empty(), "sent once");
    assert_eq!(app.status, "Copied selection");

    // "off" never writes to the host terminal, even when the clipboard fails.
    app.settings
        .set("clipboard.osc52", serde_json::json!("off"));
    app.clipboard_commands = no_system_clipboard;
    app.execute_command(Command::EditCopy);
    finish_copy(&mut app, &mut events);
    assert!(app.take_host_output().is_empty());
    assert!(
        app.status.starts_with("Copied inside Tide"),
        "{}",
        app.status
    );

    // "always" sends it alongside a working system clipboard.
    app.settings
        .set("clipboard.osc52", serde_json::json!("always"));
    app.clipboard_host = crate::clipboard::Host::default();
    app.clipboard_commands = copies_succeed;
    app.execute_command(Command::EditCopy);
    assert_eq!(app.take_host_output(), expected);
    finish_copy(&mut app, &mut events);
}
