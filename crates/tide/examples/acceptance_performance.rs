//! Fixed-fixture acceptance measurements. Run with `cargo run --release --example acceptance_performance -p tide`.
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};
use tide_terminal::{PtyConfig, PtySession, TerminalEmulator};
use tide_ui::{App, AppConfig};

/// Nearest-rank percentile: the smallest value with at least `percent`% of
/// the samples at or below it.
fn percentile(values: &mut [f64], percent: usize) -> f64 {
    values.sort_by(f64::total_cmp);
    values[(values.len() * percent)
        .div_ceil(100)
        .saturating_sub(1)
        .min(values.len() - 1)]
}
fn cpu_seconds() -> f64 {
    let output = std::process::Command::new("/bin/ps")
        .args(["-p", &std::process::id().to_string(), "-o", "time="])
        .output()
        .unwrap();
    assert!(output.status.success());
    String::from_utf8(output.stdout)
        .unwrap()
        .trim()
        .split(':')
        .fold(0., |total, part| total * 60. + part.parse::<f64>().unwrap())
}
fn rss_mb() -> Option<f64> {
    #[cfg(target_os = "linux")]
    {
        std::fs::read_to_string("/proc/self/status")
            .ok()?
            .lines()
            .find(|line| line.starts_with("VmRSS:"))?
            .split_whitespace()
            .nth(1)?
            .parse::<f64>()
            .ok()
            .map(|kb| kb / 1024.)
    }
    #[cfg(not(target_os = "linux"))]
    {
        let output = std::process::Command::new("/bin/ps")
            .args(["-p", &std::process::id().to_string(), "-o", "rss="])
            .output()
            .ok()?;
        String::from_utf8(output.stdout)
            .ok()?
            .trim()
            .parse::<f64>()
            .ok()
            .map(|kb| kb / 1024.)
    }
}
#[tokio::main(flavor = "current_thread")]
async fn main() {
    let fixture = tempfile::tempdir().unwrap();
    let root = fixture.path().join("workspace");
    std::fs::create_dir(&root).unwrap();
    let paths = tide_config::Paths::from_env(fixture.path(), |_| None);
    let mut app = App::new(AppConfig {
        workspace: root.clone(),
        initial_file: None,
        session_name: None,
        restore_session: false,
        shared_sessions: false,
        paths: Some(paths),
        jobs: tide_ui::Jobs::runtime(tokio::runtime::Handle::current()),
        agent_discovery: tide_ui::discover_agent_sessions,
    })
    .unwrap();
    // Measure rendering and hosted terminals independently of whichever
    // language servers happen to be installed on a platform runner.
    app.settings.set("lsp.enabled", serde_json::json!(false));
    let mut text = String::from("/* multiline syntax\n");
    for _ in 0..100_000 {
        text.push_str("long comment line with café and Unicode 界 0123456789 abcdefghijklmnopqrstuvwxyz 0123456789 abcdefghijklmnopqrstuvwxyz\n");
    }
    text.push_str("*/\nfn main() {}\n");
    let file = root.join("large.rs");
    std::fs::write(&file, &text).unwrap();
    let start = Instant::now();
    app.open_file(&file);
    let mut terminal = ratatui::Terminal::new(ratatui::backend::TestBackend::new(120, 40)).unwrap();
    terminal.draw(|frame| app.draw(frame)).unwrap();
    let open_ms = start.elapsed().as_secs_f64() * 1000.;
    let mut frames = Vec::new();
    for _ in 0..100 {
        let start = Instant::now();
        app.workbench.active_editor_mut().unwrap().scroll_by(7);
        terminal.draw(|frame| app.draw(frame)).unwrap();
        frames.push(start.elapsed().as_secs_f64() * 1000.);
        tokio::task::yield_now().await;
    }
    let scroll_p95 = percentile(&mut frames, 95);
    let scroll_p99 = percentile(&mut frames, 99);
    // Ten independent 1 MB files and five actual hosted terminals.
    text.clear();
    text.push_str(&"memory line\n".repeat(90_910));
    for i in 0..10 {
        let file = root.join(format!("memory{i}.txt"));
        std::fs::write(&file, &text).unwrap();
        app.open_file(file);
    }
    for _ in 0..5 {
        app.dock.open(&root, Some("/bin/sh"), 10_000).unwrap();
    }
    terminal.draw(|frame| app.draw(frame)).unwrap();
    let memory = rss_mb().expect("RSS measurement available");
    app.shutdown().await;
    drop(app);
    drop(text);
    let mut emulator = TerminalEmulator::new(120, 40, 10_000);
    let bytes =
        b"terminal throughput line 0123456789 abcdefghijklmnopqrstuvwxyz\r\n".repeat(170_000);
    let start = Instant::now();
    emulator.feed(&bytes);
    let throughput = bytes.len() as f64 / start.elapsed().as_secs_f64() / 1_000_000.;
    let mut terminal_scroll = Vec::new();
    for _ in 0..100 {
        let start = Instant::now();
        emulator.scroll_display(7);
        std::hint::black_box(emulator.snapshot());
        terminal_scroll.push(start.elapsed().as_secs_f64() * 1000.);
    }
    let terminal_scroll_p95 = percentile(&mut terminal_scroll, 95);
    drop(emulator);
    drop(bytes);
    let mut session=PtySession::spawn(PtyConfig::command("/usr/bin/python3",["-c".into(),"import os,tty;tty.setraw(0);os.write(1,b'READY');\nwhile True:\n b=os.read(0,1)\n if not b:break\n os.write(1,b)".into()])).unwrap();
    // Wait for the fixture process to initialize before measuring input. Its
    // interpreter startup is not an input round-trip and can exceed100ms on CI.
    let ready_deadline = Instant::now() + Duration::from_secs(2);
    let mut ready = Vec::new();
    while !ready.windows(5).any(|bytes| bytes == b"READY") {
        assert!(Instant::now() < ready_deadline, "PTY fixture startup");
        ready.extend(session.read_for(Duration::from_millis(10)));
    }
    let mut latencies = Vec::new();
    for _ in 0..100 {
        let start = Instant::now();
        session.write(b"x").unwrap();
        while !session.read_for(Duration::from_micros(500)).contains(&b'x') {
            assert!(start.elapsed() < Duration::from_secs(2));
        }
        latencies.push(start.elapsed().as_secs_f64() * 1000.);
    }
    session.kill().unwrap();
    drop(session);
    let input_p95 = percentile(&mut latencies, 95);
    let search_root = fixture.path().join("search");
    std::fs::create_dir(&search_root).unwrap();
    // Creation is excluded from the search clock, all files are actually visited.
    for directory in 0..100 {
        let path = search_root.join(format!("d{directory}"));
        std::fs::create_dir(&path).unwrap();
        for file in 0..1000 {
            std::fs::write(path.join(format!("f{file}.txt")), "search fixture\n").unwrap();
        }
    }
    let cancel = std::sync::Arc::new(AtomicBool::new(false));
    let search_cpu_before = cpu_seconds();
    let start = Instant::now();
    assert!(tide_ui::workspace_search::search(&search_root, "ABSENT_NEEDLE", &cancel).is_empty());
    let search_ms = start.elapsed().as_secs_f64() * 1000.;
    let search_cpu_ms = (cpu_seconds() - search_cpu_before) * 1000.;
    let worker_cancel = cancel.clone();
    let worker_root = search_root.clone();
    let worker = std::thread::spawn(move || {
        tide_ui::workspace_search::search(&worker_root, "ABSENT_NEEDLE", &worker_cancel)
    });
    std::thread::sleep(Duration::from_millis(5));
    let start = Instant::now();
    cancel.store(true, Ordering::Release);
    worker.join().unwrap();
    let cancel_ms = start.elapsed().as_secs_f64() * 1000.;
    println!(
        "{}",
        serde_json::json!({"samples":100,"large_file_bytes":std::fs::metadata(&file).unwrap().len(),"large_open_ms":open_ms,"multiline_minimap_scroll_p95_ms":scroll_p95,"multiline_minimap_scroll_p99_ms":scroll_p99,"rss_5_terminals_10_files_mb":memory,"terminal_mb_s":throughput,"terminal_scrollback_p95_ms":terminal_scroll_p95,"pty_input_p95_ms":input_p95,"search_files":100000,"search_ms":search_ms,"search_cpu_ms":search_cpu_ms,"search_cancel_ms":cancel_ms})
    );
    if std::env::args().any(|argument| argument == "--profile-search") {
        for workers in 1..=4 {
            let start = Instant::now();
            assert!(
                tide_ui::workspace_search::search_with_parallelism(
                    &search_root,
                    "ABSENT_NEEDLE",
                    &AtomicBool::new(false),
                    workers,
                )
                .is_empty()
            );
            println!(
                "{}",
                serde_json::json!({
                    "workload": "search_worker_profile", "workers": workers,
                    "search_files": 100000, "search_ms": start.elapsed().as_secs_f64() * 1000.,
                    "condition": "after primary production-policy measurement; warm filesystem",
                })
            );
        }
    }
    assert!(open_ms < 500., "large-file open budget");
    assert!(scroll_p95 < 1000. / 60., "100k-line scroll frame budget");
    assert!(memory < 250., "memory budget");
    assert!(
        terminal_scroll_p95 < 1000. / 60.,
        "terminal scroll frame budget"
    );
    assert!(throughput > 5., "terminal throughput budget");
    assert!(input_p95 < 16., "terminal input budget");
    assert!(cancel_ms < 100., "search cancellation budget");
    let defer_search = std::env::args().any(|argument| argument == "--defer-search-throughput");
    if search_ms >= 2000. && defer_search {
        println!(
            "{}",
            serde_json::json!({
                "workload": "accepted_deferral", "id": "workspace-search-throughput",
                "measured_ms": search_ms, "target_ms": 2000,
                "authorization": "Owner-approved 0.10 deferral, 2026-10-28",
            })
        );
    }
    assert!(search_ms < 2000. || defer_search, "100k-file search budget");
}
