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
