//! Build provenance, reproducibility and readiness gates.
use crate::util::{capture, is_full_git_sha, pretty_json, read_json, run_inherited, sha256_file};
use anyhow::{Context, Result, ensure};
use serde::Serialize;
use serde_json::{Value, json};
use std::path::Path;
use std::process::Command;

#[derive(Serialize)]
struct FileRecord {
    name: String,
    bytes: u64,
    sha256: String,
}

fn record(path: &Path) -> Result<FileRecord> {
    let metadata = std::fs::symlink_metadata(path)?;
    ensure!(
        metadata.is_file(),
        "{} must be a regular file",
        path.display()
    );
    Ok(FileRecord {
        name: path
            .file_name()
            .context("file name")?
            .to_string_lossy()
            .into_owned(),
        bytes: metadata.len(),
        sha256: sha256_file(path)?,
    })
}

fn build_host() -> String {
    let uname = Command::new("uname")
        .args(["-sr"])
        .output()
        .map(|output| {
            String::from_utf8_lossy(&output.stdout)
                .trim()
                .replace(' ', "-")
        })
        .unwrap_or_default();
    format!("{uname}-{}", std::env::consts::ARCH)
}

/// `record-rust-artifact`: exact build provenance and sizes; not release approval.
pub fn record_artifact(
    binary: &Path,
    artifact_directory: &Path,
    target: &str,
    source_commit: &str,
) -> Result<()> {
    ensure!(
        is_full_git_sha(source_commit),
        "Source commit must be a full lowercase Git object ID"
    );
    let mut archives: Vec<_> = std::fs::read_dir(artifact_directory)?
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| {
            let name = path.to_string_lossy();
            name.ends_with(".zip") || name.ends_with(".tar.gz")
        })
        .collect();
    archives.sort();
    ensure!(archives.len() == 1, "Expected one platform archive");
    let binary_record = record(binary)?;
    let rustc = capture(Command::new("rustc").arg("--version"))?
        .trim()
        .to_owned();
    let mut data = json!({
        "schemaVersion": 1,
        "sourceCommit": source_commit,
        "target": target,
        "buildHost": build_host(),
        "rustc": rustc,
        "binary": binary_record,
        "archive": record(&archives[0])?,
        "candidate": true,
        "signing": if target.contains("apple-darwin") { "pending-local-api-rehearsal" } else { "not-required" },
        "publication": "unpublished",
    });
    if target.contains("linux") {
        let proof: Value = read_json(&binary.with_file_name("reproducibility.json"))?;
        ensure!(
            proof["independentBuilds"] == 2 && proof["result"] == "byte-identical",
            "Linux build lacks a byte-identical repeat build"
        );
        ensure!(
            proof["target"] == target && proof["binarySha256"] == data["binary"]["sha256"],
            "Reproducibility proof does not match this binary"
        );
        data["reproducibility"] = proof;
    }
    let text = pretty_json(&data)?;
    std::fs::write(artifact_directory.join("build-evidence.json"), &text)?;
    print!("{text}");
    Ok(())
}

/// `verify-deterministic-build`: two clean builds at the same absolute path must
/// be byte-identical, and the relocated binary must pass native smoke checks.
pub fn verify_deterministic_build(root: &Path) -> Result<()> {
    let scratch = tempfile::tempdir()?;
    let build = scratch.path().join("build");
    // Delete all build output between passes to force independent compilations.
    // Keep the absolute build path fixed: Apple's linker includes output-path data
    // in the Mach-O UUID and ad-hoc signature. This checks reproducibility for the
    // same source, host, toolchain and build path, before timestamped release signing.
    for pass in ["first", "second"] {
        ensure!(
            !build.exists(),
            "Build directory was not removed between passes"
        );
        run_inherited(
            Command::new("cargo")
                .args(["build", "--locked", "--release", "-p", "tide"])
                .env("CARGO_TARGET_DIR", &build)
                .env("CARGO_INCREMENTAL", "0")
                .current_dir(root),
        )?;
        let copy = scratch.path().join(format!("{pass}-tide"));
        std::fs::copy(build.join("release/tide"), &copy)?;
        println!("{}  {}", sha256_file(&copy)?, copy.display());
        std::fs::remove_dir_all(&build)?;
    }
    ensure!(
        std::fs::read(scratch.path().join("first-tide"))?
            == std::fs::read(scratch.path().join("second-tide"))?,
        "Two clean Rust builds differ"
    );
    let relocated = scratch.path().join("relocated");
    let home = scratch.path().join("home");
    std::fs::create_dir_all(&relocated)?;
    std::fs::create_dir_all(&home)?;
    std::fs::copy(scratch.path().join("second-tide"), relocated.join("tide"))?;
    for flag in ["--version", "--check-catalogs", "--check-terminal"] {
        run_inherited(
            Command::new(relocated.join("tide"))
                .arg(flag)
                .env_clear()
                .env("HOME", &home)
                .env("PATH", "/usr/bin:/bin:/usr/sbin:/sbin")
                .env("TERM", "xterm-256color"),
        )?;
    }
    println!(
        "Two clean Rust builds are byte-identical with the same host/toolchain/build path; relocated native smoke passed."
    );
    Ok(())
}

/// `verify-rust-test-readiness`: the full gate against disposable fixtures only.
pub fn verify_test_readiness(root: &Path) -> Result<()> {
    for (name, purpose) in [
        (
            "TIDE_TEST_DATABASE_URL",
            "set a disposable PostgreSQL fixture URL",
        ),
        ("TIDE_TEST_DATABASE_TLS_URL", "set its TLS fixture URL"),
        ("TIDE_TEST_DATABASE_CA", "set its test CA certificate path"),
    ] {
        ensure!(
            std::env::var(name).is_ok_and(|value| !value.is_empty()),
            "{name}: {purpose}"
        );
    }
    let threads = std::env::var("RUST_TEST_THREADS").unwrap_or_else(|_| "2".into());
    let cargo = |args: &[&str]| {
        run_inherited(
            Command::new("cargo")
                .args(args)
                .env("RUST_TEST_THREADS", &threads)
                .current_dir(root),
        )
    };
    let xtask = std::env::current_exe()?;
    let tool = |args: &[&str]| run_inherited(Command::new(&xtask).args(args).current_dir(root));
    cargo(&["fmt", "--all", "--", "--check"])?;
    cargo(&[
        "clippy",
        "--locked",
        "--workspace",
        "--all-targets",
        "--",
        "-D",
        "warnings",
    ])?;
    // Ignored tests need live PostgreSQL, real language servers and vim; all are required here.
    cargo(&["test", "--locked", "--workspace", "--", "--include-ignored"])?;
    cargo(&[
        "run",
        "--locked",
        "-q",
        "-p",
        "tide",
        "--",
        "--check-catalogs",
    ])?;
    cargo(&[
        "run",
        "--locked",
        "-q",
        "-p",
        "tide",
        "--",
        "--check-terminal",
    ])?;
    cargo(&["build", "--locked", "--release", "-p", "tide"])?;
    tool(&[
        "measure-rust-performance",
        "--max-p95-ms",
        "33",
        "--max-p99-ms",
        "50",
        "--max-startup-ms",
        "250",
        "--max-idle-cpu",
        "0.25",
    ])?;
    cargo(&[
        "run",
        "--locked",
        "--release",
        "-p",
        "tide",
        "--example",
        "acceptance_performance",
        "--",
        "--defer-search-throughput",
    ])?;
    tool(&["verify-rust-local-install"])?;
    run_inherited(
        Command::new("cargo")
            .args(["deny", "check"])
            .current_dir(root),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn provenance_requires_exactly_one_archive_and_a_full_commit() {
        let scratch = tempfile::tempdir().unwrap();
        let binary = scratch.path().join("tide");
        std::fs::write(&binary, "binary").unwrap();
        let short = record_artifact(&binary, scratch.path(), "x86_64-apple-darwin", "abc");
        assert!(short.is_err());
        let none = record_artifact(
            &binary,
            scratch.path(),
            "x86_64-apple-darwin",
            &"a".repeat(40),
        );
        assert!(
            none.unwrap_err()
                .to_string()
                .contains("one platform archive")
        );
    }
}
