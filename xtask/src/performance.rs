//! Measures actual editor cursor responses in a PTY, not startup or
//! control-sequence output.
use anyhow::{Context, Result, bail, ensure};
use portable_pty::{CommandBuilder, PtySize, native_pty_system};
use regex::bytes::Regex;
use serde::Serialize;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::mpsc::Receiver;
use std::time::{Duration, Instant};

#[derive(Debug, Serialize)]
pub struct Measurement {
    pub file_type: String,
    pub bytes: u64,
    pub samples: usize,
    pub startup_first_frame_ms: f64,
    pub cursor_p99_ms: f64,
    pub cursor_p50_ms: f64,
    pub cursor_p95_ms: f64,
    pub idle_cpu_cores: f64,
    pub palette_open_ms: f64,
}

#[derive(Debug, Serialize)]
pub struct StartupDistribution {
    pub workload: &'static str,
    pub samples: usize,
    pub condition: &'static str,
    pub p50_ms: f64,
    pub p95_ms: f64,
    pub p99_ms: f64,
}

#[derive(Clone, Debug)]
pub struct Budgets {
    pub max_p95_ms: Option<f64>,
    pub max_p99_ms: Option<f64>,
    pub max_startup_ms: Option<f64>,
    pub max_idle_cpu: Option<f64>,
}

fn round2(value: f64) -> f64 {
    (value * 100.0).round() / 100.0
}

fn millis(duration: Duration) -> f64 {
    duration.as_secs_f64() * 1000.0
}

/// The order statistic used for p95/p99: `sorted[min(n - 1, n * percent / 100)]`.
pub fn percentile(sorted: &[f64], percent: usize) -> f64 {
    let last = sorted.len().saturating_sub(1);
    let index = sorted.len() * percent / 100;
    sorted.get(index.min(last)).copied().unwrap_or_default()
}

fn clock_ticks() -> f64 {
    Command::new("getconf")
        .arg("CLK_TCK")
        .output()
        .ok()
        .and_then(|output| String::from_utf8_lossy(&output.stdout).trim().parse().ok())
        .unwrap_or(100.0)
}

/// CPU seconds consumed by `pid`: `/proc` on Linux, `ps` elsewhere.
fn cpu_seconds(pid: u32) -> Result<f64> {
    let stat = PathBuf::from(format!("/proc/{pid}/stat"));
    if stat.exists() {
        let text = std::fs::read_to_string(&stat)?;
        let fields: Vec<&str> = text
            .rsplit_once(')')
            .map(|(_, rest)| rest)
            .unwrap_or_default()
            .split_whitespace()
            .collect();
        let ticks: f64 = fields.get(11).context("utime")?.parse::<f64>()?
            + fields.get(12).context("stime")?.parse::<f64>()?;
        return Ok(ticks / clock_ticks());
    }
    let output = Command::new("/bin/ps")
        .args(["-p", &pid.to_string(), "-o", "time="])
        .output()?;
    parse_ps_time(String::from_utf8_lossy(&output.stdout).trim())
}

pub fn parse_ps_time(value: &str) -> Result<f64> {
    value.split(':').try_fold(0.0, |total, part| {
        Ok(total * 60.0
            + part
                .trim()
                .parse::<f64>()
                .with_context(|| format!("ps time {value}"))?)
    })
}

struct Session {
    child: Box<dyn portable_pty::Child + Send + Sync>,
    writer: Box<dyn Write + Send>,
    output: Receiver<Vec<u8>>,
    pid: u32,
}

impl Session {
    fn read(&self, timeout: Duration) -> Vec<u8> {
        // A timeout or a closed PTY both read as "no output".
        self.output.recv_timeout(timeout).unwrap_or_default()
    }

    fn write(&mut self, bytes: &[u8]) -> Result<()> {
        self.writer.write_all(bytes)?;
        self.writer.flush()?;
        Ok(())
    }

    /// Ctrl+Q, then a process-group kill if the editor does not exit in 2s.
    fn stop(mut self) {
        let _ = self.write(b"\x11");
        let deadline = Instant::now() + Duration::from_secs(2);
        while Instant::now() < deadline {
            if matches!(self.child.try_wait(), Ok(Some(_))) {
                return;
            }
            let _ = self.read(Duration::from_millis(20));
        }
        if let Some(pid) = i32::try_from(self.pid)
            .ok()
            .and_then(rustix::process::Pid::from_raw)
        {
            let _ = rustix::process::kill_process_group(pid, rustix::process::Signal::KILL);
        }
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn spawn(binary: &Path, workspace: &Path, file: &Path, scratch: &Path) -> Result<Session> {
    let pair = native_pty_system()
        .openpty(PtySize {
            rows: 40,
            cols: 120,
            pixel_width: 0,
            pixel_height: 0,
        })
        .map_err(|error| anyhow::anyhow!("openpty: {error}"))?;
    let mut command = CommandBuilder::new(binary);
    command.arg("--no-session");
    command.arg(file);
    command.cwd(workspace);
    command.env_clear();
    command.env("PATH", "/usr/bin:/bin");
    command.env("TERM", "xterm-256color");
    command.env("HOME", scratch);
    for kind in ["CONFIG", "DATA", "STATE", "CACHE"] {
        command.env(
            format!("XDG_{kind}_HOME"),
            scratch.join(kind.to_lowercase()),
        );
    }
    let child = pair
        .slave
        .spawn_command(command)
        .map_err(|error| anyhow::anyhow!("spawn {}: {error}", binary.display()))?;
    drop(pair.slave);
    let mut reader = pair
        .master
        .try_clone_reader()
        .map_err(|error| anyhow::anyhow!("pty reader: {error}"))?;
    let writer = pair
        .master
        .take_writer()
        .map_err(|error| anyhow::anyhow!("pty writer: {error}"))?;
    let (sender, output) = std::sync::mpsc::channel();
    let master = pair.master;
    std::thread::spawn(move || {
        let _keep_master_open = master;
        let mut buffer = vec![0_u8; 65536];
        loop {
            match std::io::Read::read(&mut reader, &mut buffer) {
                Ok(0) | Err(_) => break,
                Ok(read) => {
                    if sender.send(buffer[..read].to_vec()).is_err() {
                        break;
                    }
                }
            }
        }
    });
    let pid = child.process_id().context("child pid")?;
    Ok(Session {
        child,
        writer,
        output,
        pid,
    })
}

pub enum Outcome {
    Startup(f64),
    Full(Measurement),
}

pub fn measure(
    binary: &Path,
    extension: &str,
    lines: usize,
    samples: usize,
    response_timeout: Duration,
    startup_only: bool,
) -> Result<Outcome> {
    let scratch = tempfile::Builder::new().prefix("tide-input-").tempdir()?;
    let root = scratch.path();
    let workspace = root.join("workspace");
    std::fs::create_dir(&workspace)?;
    let file = workspace.join(format!("input.{extension}"));
    {
        let mut output = std::io::BufWriter::new(std::fs::File::create(&file)?);
        output.write_all(b"// BENCHMARK_READY xxxxxxxxxxxxxxxxxxxxxxxxxx\n")?;
        for index in 0..lines {
            writeln!(output, "fn benchmark_{index}() {{ let value = {index}; }}")?;
        }
    }
    let position = Regex::new(r"\x1b\[(\d+);(\d+)H")?;
    let launched = Instant::now();
    let mut session = spawn(binary, &workspace, &file, root)?;
    let result = (|| -> Result<Outcome> {
        let mut output = Vec::new();
        let deadline = launched + Duration::from_secs(15);
        loop {
            ensure!(
                Instant::now() < deadline,
                "editor did not render the benchmark file"
            );
            output.extend(session.read(Duration::from_millis(50)));
            if output
                .windows(15)
                .any(|window| window == b"BENCHMARK_READY")
                && position.is_match(&output)
            {
                break;
            }
        }
        let startup_ms = millis(launched.elapsed());
        if startup_only {
            return Ok(Outcome::Startup(round2(startup_ms)));
        }
        // Drain the complete initial frame before sending any input.
        loop {
            let chunk = session.read(Duration::from_millis(100));
            if chunk.is_empty() {
                break;
            }
            output.extend(chunk);
            ensure!(Instant::now() <= deadline, "initial frame never settled");
        }
        let last = position
            .captures_iter(&output)
            .last()
            .context("no cursor position in the initial frame")?;
        let row: i64 = String::from_utf8_lossy(&last[1]).parse()?;
        let mut column: i64 = String::from_utf8_lossy(&last[2]).parse()?;
        let before = cpu_seconds(session.pid)?;
        let idle_started = Instant::now();
        while idle_started.elapsed() < Duration::from_secs(1) {
            let _ = session.read(Duration::from_millis(50));
        }
        let idle_cpu = (cpu_seconds(session.pid)? - before) / idle_started.elapsed().as_secs_f64();
        let mut latencies = Vec::with_capacity(samples);
        for index in 0..samples {
            let right = index % 2 == 0;
            column += if right { 1 } else { -1 };
            let expected = format!("\x1b[{row};{column}H").into_bytes();
            let started = Instant::now();
            session.write(if right { b"\x1b[C" } else { b"\x1b[D" })?;
            let mut response: Vec<u8> = Vec::new();
            loop {
                if started.elapsed() >= response_timeout {
                    let tail = &response[response.len().saturating_sub(500)..];
                    bail!(
                        "no matching cursor response for sample {index}: expected {:?}, tail {:?}, prior ms {latencies:?}",
                        String::from_utf8_lossy(&expected),
                        String::from_utf8_lossy(tail)
                    );
                }
                response.extend(session.read(Duration::from_millis(10)));
                if response.len() > 65536 {
                    response.drain(..response.len() - 65536);
                }
                if response
                    .windows(expected.len())
                    .any(|window| window == expected.as_slice())
                {
                    latencies.push(millis(started.elapsed()));
                    break;
                }
            }
        }
        let started = Instant::now();
        session.write(b"\x0b")?; // Ctrl+K must open immediately while preserving chords.
        let mut response = Vec::new();
        let palette_ms = loop {
            ensure!(
                started.elapsed() < Duration::from_secs(2),
                "command palette did not render"
            );
            response.extend(session.read(Duration::from_millis(10)));
            if response
                .windows(15)
                .any(|window| window == b"Command Palette")
            {
                break millis(started.elapsed());
            }
        };
        session.write(b"\x1b")?;
        let mut ordered = latencies.clone();
        ordered.sort_by(f64::total_cmp);
        Ok(Outcome::Full(Measurement {
            file_type: extension.to_owned(),
            bytes: std::fs::metadata(&file)?.len(),
            samples,
            startup_first_frame_ms: round2(startup_ms),
            cursor_p99_ms: round2(percentile(&ordered, 99)),
            cursor_p50_ms: round2(ordered[ordered.len() / 2]),
            cursor_p95_ms: round2(percentile(&ordered, 95)),
            idle_cpu_cores: (idle_cpu * 1000.0).round() / 1000.0,
            palette_open_ms: round2(palette_ms),
        }))
    })();
    session.stop();
    result
}

pub struct PerformanceArgs {
    pub binary: PathBuf,
    pub lines: usize,
    pub samples: usize,
    pub startup_samples: usize,
    pub response_timeout: Duration,
    pub budgets: Budgets,
}

/// Returns budget violations for one full measurement.
pub fn violations(result: &Measurement, budgets: &Budgets) -> Vec<String> {
    let mut failed = Vec::new();
    let over = |value: f64, limit: Option<f64>| limit.is_some_and(|limit| value > limit);
    if over(
        result.cursor_p95_ms.max(result.palette_open_ms),
        budgets.max_p95_ms,
    ) {
        failed.push(format!("{}: p95/palette over budget", result.file_type));
    }
    if over(result.cursor_p99_ms, budgets.max_p99_ms) {
        failed.push(format!("{}: p99 over budget", result.file_type));
    }
    if over(result.startup_first_frame_ms, budgets.max_startup_ms) {
        failed.push(format!("{}: first frame over budget", result.file_type));
    }
    if over(result.idle_cpu_cores, budgets.max_idle_cpu) {
        failed.push(format!("{}: idle CPU over budget", result.file_type));
    }
    failed
}

/// `measure-rust-performance`: prints one JSON line per workload.
pub fn run(args: &PerformanceArgs) -> Result<()> {
    ensure!(
        args.samples >= 2 && args.lines >= 1,
        "samples must be >= 2 and lines >= 1"
    );
    ensure!(args.startup_samples >= 2, "startup-samples must be >= 2");
    let binary = std::path::absolute(&args.binary)?.canonicalize()?;
    let mut failed = Vec::new();
    for extension in ["rs", "txt"] {
        let Outcome::Full(result) = measure(
            &binary,
            extension,
            args.lines,
            args.samples,
            args.response_timeout,
            false,
        )?
        else {
            bail!("expected a full measurement");
        };
        println!("{}", serde_json::to_string(&result)?);
        failed.extend(violations(&result, &args.budgets));
    }
    let mut startups = Vec::new();
    for _ in 0..args.startup_samples {
        if let Outcome::Startup(ms) = measure(&binary, "txt", 100, 2, args.response_timeout, true)?
        {
            startups.push(ms);
        }
    }
    startups.sort_by(f64::total_cmp);
    let distribution = StartupDistribution {
        workload: "startup_distribution",
        samples: startups.len(),
        condition: "fresh process and HOME, warm OS filesystem cache",
        p50_ms: startups[startups.len() / 2],
        p95_ms: percentile(&startups, 95),
        p99_ms: startups.last().copied().unwrap_or_default(),
    };
    println!("{}", serde_json::to_string(&distribution)?);
    if args
        .budgets
        .max_startup_ms
        .is_some_and(|limit| distribution.p99_ms > limit)
    {
        failed.push("startup distribution p99 over budget".into());
    }
    ensure!(
        failed.is_empty(),
        "Performance budgets exceeded: {}",
        failed.join("; ")
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn percentiles_use_the_floor_index_capped_at_the_last_sample() {
        let samples: Vec<f64> = (1..=100).map(f64::from).collect();
        assert!((percentile(&samples, 95) - 96.0).abs() < f64::EPSILON);
        assert!((percentile(&samples, 99) - 100.0).abs() < f64::EPSILON);
        assert!((percentile(&[1.0, 2.0], 99) - 2.0).abs() < f64::EPSILON);
    }

    #[test]
    fn ps_cpu_times_parse_minutes_and_hours() {
        assert!((parse_ps_time("0:01.50").unwrap() - 1.5).abs() < 1e-9);
        assert!((parse_ps_time("1:02:03").unwrap() - 3723.0).abs() < 1e-9);
    }

    #[test]
    fn budgets_report_each_exceeded_limit() {
        let result = Measurement {
            file_type: "rs".into(),
            bytes: 1,
            samples: 2,
            startup_first_frame_ms: 300.0,
            cursor_p99_ms: 10.0,
            cursor_p50_ms: 1.0,
            cursor_p95_ms: 2.0,
            idle_cpu_cores: 0.01,
            palette_open_ms: 40.0,
        };
        let budgets = Budgets {
            max_p95_ms: Some(33.0),
            max_p99_ms: Some(50.0),
            max_startup_ms: Some(250.0),
            max_idle_cpu: Some(0.25),
        };
        assert_eq!(violations(&result, &budgets).len(), 2);
        let open = Budgets {
            max_p95_ms: None,
            max_p99_ms: None,
            max_startup_ms: None,
            max_idle_cpu: None,
        };
        assert!(violations(&result, &open).is_empty());
    }
}
