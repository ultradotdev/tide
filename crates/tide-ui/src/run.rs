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

fn restore_terminal() {
    let _ = disable_raw_mode();
    let mut stdout = io::stdout();
    // The kitty keyboard protocol keeps one flag stack per screen: pop Tide's flags on the
    // alternate screen, where they were pushed, before leaving it.
    let _ = write!(stdout, "{DISABLE_KEYBOARD}");
    let _ = execute!(
        stdout,
        LeaveAlternateScreen,
        DisableMouseCapture,
        DisableFocusChange,
        DisableBracketedPaste,
        crossterm::cursor::Show
    );
    let _ = stdout.flush();
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

pub async fn run(mut app: App) -> Result<()> {
    install_panic_hook();
    app.terminal = crate::display::TerminalTraits::detect(|name| std::env::var(name).ok());
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
                maybe_event = events.next() => {
                    pacer.activity(Instant::now());
                    let Some(event) = maybe_event else { break };
                    redraw |= app.handle_event(event?);
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
                        redraw |= app.handle_event(event?);
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
        }
        Ok(())
    })
    .await;

    let restart = app.should_quit().then_some(app.restart).flatten();
    let workspace = app.workspace.clone();
    let name = app.session_name.clone();
    app.shutdown().await;
    drop(terminal);
    drop(_mode_guard);
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
