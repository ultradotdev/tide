use std::cell::Cell;
use std::io::{self, Write};
use std::panic::AssertUnwindSafe;
use std::time::{Duration, Instant};

use anyhow::Result;
use crossterm::event::{
    DisableBracketedPaste, DisableFocusChange, DisableMouseCapture, EnableBracketedPaste,
    EnableFocusChange, EnableMouseCapture, EventStream,
};
use crossterm::execute;
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use futures_util::{FutureExt, Stream, StreamExt};
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;

use crate::app::App;

const ACTIVE_TICK: Duration = Duration::from_millis(8);
const IDLE_TICK: Duration = Duration::from_millis(50);
const ACTIVE_WINDOW: Duration = Duration::from_millis(500);

/// Ticks quickly while input, terminal output or background work is arriving,
/// and slowly once nothing has happened for [`ACTIVE_WINDOW`].
struct TickPacer {
    last_tick: Instant,
    last_activity: Instant,
}

impl TickPacer {
    fn new(now: Instant) -> Self {
        Self {
            last_tick: now,
            last_activity: now,
        }
    }

    fn activity(&mut self, now: Instant) {
        self.last_activity = now;
    }

    fn ticked(&mut self, now: Instant, changed: bool) {
        self.last_tick = now;
        if changed {
            self.last_activity = now;
        }
    }

    fn deadline(&self) -> Instant {
        let idle = self.last_tick.saturating_duration_since(self.last_activity);
        self.last_tick
            + if idle < ACTIVE_WINDOW {
                ACTIVE_TICK
            } else {
                IDLE_TICK
            }
    }
}

const ENABLE_KEYBOARD: &str = "\x1b[>4;2m\x1b[>1u";
const DISABLE_KEYBOARD: &str = "\x1b[>4;0m\x1b[<u";

thread_local! {
    /// Set while this thread has the terminal in raw mode on the alternate screen.
    static OWNS_TERMINAL: Cell<bool> = const { Cell::new(false) };
}

/// How long the terminal must stay quiet before Tide trusts it has stopped sending.
/// Reports sent just before it stopped can still be on their way, over SSH for longer.
const QUIET: u16 = 30;
/// The longest Tide waits for that quiet on exit.
const MAX_SETTLE: Duration = Duration::from_millis(300);

/// Asks the terminal to stop sending mouse movements and focus changes. Tide stops
/// reading input when it quits, and anything the terminal keeps sending after that
/// would reach the shell as text.
fn stop_reports() {
    let mut stdout = io::stdout();
    let _ = execute!(stdout, DisableMouseCapture, DisableFocusChange);
    let _ = stdout.flush();
}

/// Throws away input Tide never read, such as mouse movements queued while it shut
/// down, until the terminal has been quiet for a moment, so none of it reaches the
/// shell. Keys typed while Tide was closing go too.
fn discard_unread_input() {
    use nix::poll::{PollFd, PollFlags, PollTimeout, poll};
    use nix::sys::termios::{FlushArg, tcflush};
    use std::os::fd::AsFd;
    let Ok(tty) = std::fs::File::open("/dev/tty") else {
        return;
    };
    let started = Instant::now();
    loop {
        let _ = tcflush(&tty, FlushArg::TCIFLUSH);
        if started.elapsed() >= MAX_SETTLE {
            break;
        }
        let mut fds = [PollFd::new(tty.as_fd(), PollFlags::POLLIN)];
        if !matches!(poll(&mut fds, PollTimeout::from(QUIET)), Ok(ready) if ready > 0) {
            break;
        }
    }
}

fn restore_terminal() {
    let mut stdout = io::stdout();
    // The kitty keyboard protocol keeps one flag stack per screen: pop Tide's flags on the
    // alternate screen, where they were pushed, before leaving it.
    let _ = write!(stdout, "{DISABLE_KEYBOARD}");
    let _ = execute!(stdout, DisableBracketedPaste);
    stop_reports();
    // Still in raw mode, so nothing queued is echoed; then nothing is left to echo.
    discard_unread_input();
    let _ = disable_raw_mode();
    let _ = execute!(stdout, LeaveAlternateScreen, crossterm::cursor::Show);
    let _ = stdout.flush();
}

/// How long quitting may take before Tide says it is still closing.
const QUIT_NOTICE_AFTER: Duration = Duration::from_millis(500);

/// Draws "Quitting Tide…" in the middle of the last frame, so a slow shutdown does not
/// look like a hang. Drawn straight to the backend, since the app is busy shutting down.
fn paint_quit_notice<B: ratatui::backend::Backend>(
    backend: &mut B,
    palette: crate::palette::Palette,
    indexed: bool,
) -> Result<(), B::Error> {
    use ratatui::buffer::Buffer;
    use ratatui::layout::Rect;
    use ratatui::style::{Modifier, Style};
    use ratatui::text::Line;
    use ratatui::widgets::{Block, Paragraph, Widget, Wrap};
    let size = backend.size()?;
    let width = size.width.min(52);
    let height = size.height.min(4);
    if width < 10 || height < 3 {
        return Ok(());
    }
    let area = Rect::new(
        (size.width - width) / 2,
        (size.height - height) / 2,
        width,
        height,
    );
    let mut buffer = Buffer::empty(area);
    Paragraph::new(vec![
        Line::styled(
            "Quitting Tide…",
            Style::default()
                .fg(palette.foreground)
                .add_modifier(Modifier::BOLD),
        ),
        Line::styled(
            "Closing language servers and background work",
            Style::default().fg(palette.description),
        ),
    ])
    .wrap(Wrap { trim: true })
    .block(
        Block::bordered()
            .padding(ratatui::widgets::Padding::horizontal(1))
            .border_style(Style::default().fg(palette.modal_border))
            .style(Style::default().bg(palette.modal_bg)),
    )
    .render(area, &mut buffer);
    if indexed {
        crate::display::reduce_colors(buffer.content.iter_mut());
    }
    let cells = buffer.content.iter().enumerate().map(|(index, cell)| {
        let (x, y) = buffer.pos_of(index);
        (x, y, cell)
    });
    backend.draw(cells)?;
    ratatui::backend::Backend::flush(backend)
}

struct TerminalModeGuard;
impl Drop for TerminalModeGuard {
    fn drop(&mut self) {
        OWNS_TERMINAL.set(false);
        restore_terminal();
    }
}

/// Leaves the alternate screen before the panic message prints, so it is still
/// visible after exit. Panics on other threads (background tasks the UI
/// survives) leave the screen alone.
fn install_panic_hook() {
    static INSTALL: std::sync::Once = std::sync::Once::new();
    INSTALL.call_once(|| {
        let previous = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            if OWNS_TERMINAL.replace(false) {
                restore_terminal();
            }
            previous(info);
        }));
    });
}

/// Runs the event loop; if it panics, writes the session recovery snapshot
/// (which carries unsaved buffers) before resuming the panic.
pub(crate) async fn guard_panics<T>(app: &mut App, work: impl AsyncFnOnce(&mut App) -> T) -> T {
    let outcome = AssertUnwindSafe(work(&mut *app)).catch_unwind().await;
    outcome.unwrap_or_else(|panic| {
        // State may be mid-update; a failing snapshot must not replace the original panic.
        let saved = std::panic::catch_unwind(AssertUnwindSafe(|| app.save_recovery()));
        if matches!(saved, Ok(Ok(true))) {
            eprintln!("Tide crashed; unsaved edits were kept in the session recovery snapshot.");
        } else if matches!(saved, Ok(Ok(false))) {
            eprintln!("Tide crashed; it was started with --no-session, so no recovery snapshot was written.");
        } else {
            eprintln!("Tide crashed and the session recovery snapshot could not be written.");
        }
        std::panic::resume_unwind(panic)
    })
}

/// The workspace this debug binary was built from, for "Restart and Rebuild".
#[cfg(debug_assertions)]
pub(crate) fn rebuild_source() -> Option<&'static std::path::Path> {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()?
        .parent()?;
    root.join("Cargo.toml").is_file().then_some(root)
}

/// Release builds never embed or rebuild from a build-machine path.
#[cfg(not(debug_assertions))]
pub(crate) fn rebuild_source() -> Option<&'static std::path::Path> {
    None
}

/// Why the event loop stopped without the user quitting.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Interrupted {
    Hangup,
    Terminated,
    LostTerminal,
}

impl Interrupted {
    fn describe(self) -> &'static str {
        match self {
            Self::Hangup => "the terminal hung up (SIGHUP)",
            Self::Terminated => "it received SIGTERM",
            Self::LostTerminal => "it lost its terminal",
        }
    }
}

/// Keeps unsaved edits in the recovery snapshot and stops, without asking.
fn interrupt(app: &mut App, reason: Interrupted, stopped: &mut Option<(Interrupted, bool)>) {
    let saved = match app.hang_up() {
        Ok(saved) => saved,
        Err(error) => {
            tracing::error!("recovery snapshot after {}: {error:#}", reason.describe());
            false
        }
    };
    *stopped = Some((reason, saved));
}

pub async fn run(mut app: App) -> Result<()> {
    install_panic_hook();
    app.terminal = crate::display::TerminalTraits::detect(|name| std::env::var(name).ok());
    // Registered before the terminal changes mode, so a hangup is never the default
    // action (exit at once, with unsaved edits only as fresh as the last autosave).
    let mut hangup = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::hangup())?;
    let mut terminate = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())?;
    let mut stopped: Option<(Interrupted, bool)> = None;
    enable_raw_mode()?;
    let _mode_guard = TerminalModeGuard;
    OWNS_TERMINAL.set(true);
    let mut stdout = io::stdout();
    execute!(
        stdout,
        EnterAlternateScreen,
        EnableMouseCapture,
        EnableFocusChange,
        EnableBracketedPaste
    )?;
    // After the switch: flags pushed on the main screen do not apply to the alternate one.
    write!(stdout, "{ENABLE_KEYBOARD}")?;
    stdout.flush()?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;
    let mut events = EventStream::new();
    let mut background = app
        .take_event_receiver()
        .expect("app background receiver is available");
    let mut pacer = TickPacer::new(Instant::now());
    // Packagers and test harnesses turn the daily check off without touching settings.
    if std::env::var_os("TIDE_NO_UPDATE_CHECK").is_none() {
        app.check_for_updates_automatically();
    }
    let mut output_armed = true;

    let result = guard_panics(&mut app, async |app: &mut App| -> Result<()> {
        terminal.draw(|frame| app.draw(frame))?;
        crate::display::paint_curly(terminal.backend_mut(),&app.layout.curly_cells)?;
        let mut curly=app.curly_underlines_enabled();
        loop {
            let mut redraw = false;
            let deadline = tokio::time::Instant::from_std(pacer.deadline());
            tokio::select! {
                _ = hangup.recv() => {
                    interrupt(app, Interrupted::Hangup, &mut stopped);
                    break;
                }
                _ = terminate.recv() => {
                    interrupt(app, Interrupted::Terminated, &mut stopped);
                    break;
                }
                maybe_event = events.next() => {
                    pacer.activity(Instant::now());
                    let event = match maybe_event {
                        Some(Ok(event)) => event,
                        Some(Err(error)) => {
                            interrupt(app, Interrupted::LostTerminal, &mut stopped);
                            return Err(error.into());
                        }
                        None => {
                            interrupt(app, Interrupted::LostTerminal, &mut stopped);
                            break;
                        }
                    };
                    redraw |= app.handle_event(event);
                    // Handle a bounded burst together, rather than rendering once
                    // for every queued key or mouse movement.
                    for _ in 0..63 {
                        // Preserve the executor waker. now_or_never() installs
                        // a noop waker when this drain reaches Pending, which
                        // delays the next key until the maintenance timer.
                        let event = std::future::poll_fn(|cx| {
                            std::task::Poll::Ready(std::pin::Pin::new(&mut events).poll_next(cx))
                        }).await;
                        let std::task::Poll::Ready(Some(event)) = event else { break };
                        let event = match event {
                            Ok(event) => event,
                            Err(error) => {
                                interrupt(app, Interrupted::LostTerminal, &mut stopped);
                                return Err(error.into());
                            }
                        };
                        redraw |= app.handle_event(event);
                        if app.should_quit() { break; }
                    }
                }
                event = background.recv() => {
                    pacer.activity(Instant::now());
                    if let Some(event) = event {
                        app.handle_background(event);
                        for _ in 0..63 {let Ok(event)=background.try_recv() else{break;};app.handle_background(event);}
                        redraw = true;
                    }
                }
                // Terminal output pulls the next tick in; the tick drains it.
                () = crate::terminal_tab::OUTPUT.notified(), if output_armed => {
                    output_armed = false;
                    pacer.activity(Instant::now());
                }
                () = tokio::time::sleep_until(deadline) => {
                    let changed = app.on_tick();
                    pacer.ticked(Instant::now(), changed);
                    output_armed = true;
                    redraw |= changed;
                }
            }
            if app.should_quit() {
                break;
            }
            if redraw {
                if curly!=app.curly_underlines_enabled() {curly=app.curly_underlines_enabled();terminal.clear()?;}
                terminal.draw(|frame| app.draw(frame))?;
                crate::display::paint_curly(terminal.backend_mut(),&app.layout.curly_cells)?;
            }
            // Clipboard sequences (OSC 52) go to the host terminal as they are, outside the
            // frame, which never carries control characters.
            let host_output = app.take_host_output();
            if !host_output.is_empty() {
                let backend = terminal.backend_mut();
                backend.write_all(&host_output)?;
                backend.flush()?;
            }
        }
        Ok(())
    })
    .await;

    let restart = app.should_quit().then_some(app.restart).flatten();
    let workspace = app.workspace.clone();
    let name = app.session_name.clone();
    // Shutting down can take a couple of seconds; the terminal must not queue mouse
    // movements for the shell meanwhile.
    stop_reports();
    let notice = (app.should_quit() && stopped.is_none() && result.is_ok()).then(|| {
        (
            app.palette,
            app.color_depth() == crate::display::ColorDepth::Indexed,
        )
    });
    {
        let shutdown = app.shutdown();
        tokio::pin!(shutdown);
        tokio::select! {
            () = &mut shutdown => {}
            () = tokio::time::sleep(QUIT_NOTICE_AFTER), if notice.is_some() => {
                if let Some((palette, indexed)) = notice {
                    let _ = paint_quit_notice(terminal.backend_mut(), palette, indexed);
                }
                shutdown.await;
            }
        }
    }
    drop(terminal);
    drop(_mode_guard);
    if let Some((reason, saved)) = stopped {
        // Usually nobody sees this after a hangup, but a SIGTERM in a live terminal does.
        eprintln!(
            "Tide stopped because {}. {}",
            reason.describe(),
            if saved {
                "Unsaved edits were kept in the session recovery snapshot."
            } else {
                "No recovery snapshot was written (--no-session, or it failed)."
            }
        );
        return Ok(());
    }
    if result.is_ok()
        && let Some(rebuild) = restart
    {
        let executable = std::env::current_exe()?;
        if rebuild {
            let source =
                rebuild_source().ok_or_else(|| anyhow::anyhow!("Missing Tide source checkout"))?;
            let mut args = vec!["build", "--locked", "--offline", "-p", "tide"];
            if executable
                .parent()
                .and_then(|p| p.file_name())
                .is_some_and(|p| p == "release")
            {
                args.push("--release");
            }
            eprintln!("Rebuilding Tide from {}…", source.display());
            let output = tide_services::git_process::local_command(
                std::ffi::OsStr::new("cargo"),
                source,
                &args,
                &std::sync::atomic::AtomicBool::new(false),
                Duration::from_secs(120),
            )?;
            anyhow::ensure!(
                output.success,
                "Rebuild failed; recovery snapshot preserved: {}",
                String::from_utf8_lossy(&output.stderr)
            );
        }
        let mut command = std::process::Command::new(executable);
        command.arg(workspace);
        if let Some(name) = name {
            command.args(["--session", &name]);
        }
        if std::env::args().any(|arg| arg == "--debug") {
            command.arg("--debug");
        }
        #[cfg(unix)]
        {
            use std::os::unix::process::CommandExt;
            return Err(command.exec().into());
        }
        #[cfg(not(unix))]
        {
            return command.status().map(|_| ()).map_err(Into::into);
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_tick_slows_down_only_after_a_quiet_spell() {
        let start = Instant::now();
        let mut pacer = TickPacer::new(start);
        assert_eq!(pacer.deadline(), start + ACTIVE_TICK);
        let mut now = start;
        while now + ACTIVE_TICK < start + ACTIVE_WINDOW {
            now += ACTIVE_TICK;
            pacer.ticked(now, false);
            assert_eq!(pacer.deadline(), now + ACTIVE_TICK);
        }
        now += ACTIVE_TICK;
        pacer.ticked(now, false);
        assert_eq!(pacer.deadline(), now + IDLE_TICK);
        pacer.activity(now + Duration::from_millis(20));
        assert_eq!(pacer.deadline(), now + ACTIVE_TICK);
        now += ACTIVE_TICK;
        pacer.ticked(now, true);
        assert_eq!(pacer.deadline(), now + ACTIVE_TICK);
    }
}

#[cfg(test)]
mod quit_notice_tests {
    use super::*;
    use ratatui::backend::TestBackend;

    #[test]
    fn the_quit_notice_is_drawn_over_the_last_frame() {
        let mut terminal = Terminal::new(TestBackend::new(80, 20)).unwrap();
        terminal
            .draw(|frame| {
                frame.render_widget(
                    ratatui::widgets::Paragraph::new("x".repeat(80 * 20))
                        .wrap(ratatui::widgets::Wrap { trim: false }),
                    frame.area(),
                );
            })
            .unwrap();
        paint_quit_notice(
            terminal.backend_mut(),
            crate::palette::Palette::ultra_dusk(),
            false,
        )
        .unwrap();
        let buffer = terminal.backend().buffer();
        let rows = (0..20)
            .map(|y| (0..80).map(|x| buffer[(x, y)].symbol()).collect::<String>())
            .collect::<Vec<_>>();
        let text = rows.join("\n");
        assert!(text.contains("Quitting Tide…"), "{text}");
        assert!(text.contains("Closing language servers"), "{text}");
        // Only the middle is painted over.
        assert_eq!(rows[0], "x".repeat(80));
        assert_eq!(rows[19], "x".repeat(80));
        assert!(rows[10].starts_with("xxxxx") && rows[10].ends_with("xxxxx"));
    }

    #[test]
    fn a_tiny_terminal_gets_no_quit_notice() {
        let mut backend = TestBackend::new(8, 2);
        paint_quit_notice(&mut backend, crate::palette::Palette::ultra_dusk(), true).unwrap();
        assert!(
            backend
                .buffer()
                .content
                .iter()
                .all(|cell| cell.symbol() == " ")
        );
    }
}
