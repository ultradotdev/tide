//! Isolated Ubuntu 22.04 container builds for the Linux release targets.
//!
//! Dependencies and the builder image are fetched first; the build itself runs
//! read-only, without network or credentials. Command construction is separate
//! from execution so it can be tested without Docker.
use crate::argv;
use crate::util::{CommandRunner, RunOptions, capture, pretty_json, sha256_file};
use anyhow::{Context, Result, ensure};
use regex::Regex;
use serde::Serialize;
use std::path::{Path, PathBuf};
use std::process::Command;

pub const IMAGE_TAG: &str = "tide-rust-jammy-builder";
const MAXIMUM_GLIBC: (u32, u32) = (2, 35);

/// The script that runs inside the container.
const CONTAINER_SCRIPT: &str = r#"
set -euo pipefail
mkdir -p /scratch/home /scratch/tmp /scratch/cargo
cp -a --no-preserve=ownership /cache/registry /scratch/cargo/
timeout 1800 cargo build --locked --offline --release --target "$TIDE_RUST_TARGET" -p tide
cp "/scratch/target/$TIDE_RUST_TARGET/release/tide" /out/tide
/out/tide --version
/out/tide --check-catalogs
/out/tide --check-terminal
ldd /out/tide > /out/dependencies.txt
readelf --version-info /out/tide > /out/elf-versions.txt
chmod 755 /out/tide
chmod 644 /out/dependencies.txt /out/elf-versions.txt
"#;

pub struct BuildPaths {
    pub source: PathBuf,
    pub sysroot: PathBuf,
    pub cargo_home: PathBuf,
    pub output: PathBuf,
}

pub fn validate_target(target: &str) -> Result<()> {
    ensure!(
        matches!(
            target,
            "aarch64-unknown-linux-gnu" | "x86_64-unknown-linux-gnu"
        ),
        "Unsupported Linux release target"
    );
    Ok(())
}

/// The fully isolated `docker run` argv for one build.
pub fn docker_run_arguments(
    image: &str,
    target: &str,
    uid: &str,
    gid: &str,
    paths: &BuildPaths,
) -> Vec<String> {
    let mount = |source: &Path, destination: &str, readonly: bool| {
        format!(
            "type=bind,source={},target={destination}{}",
            source.display(),
            if readonly { ",readonly" } else { "" }
        )
    };
    argv![
        "docker",
        "run",
        "--rm",
        "--read-only",
        "--network",
        "none",
        "--cap-drop",
        "ALL",
        "--security-opt",
        "no-new-privileges",
        "--cpus",
        "2",
        "--memory",
        "6g",
        "--pids-limit",
        "512",
        "--ulimit",
        "fsize=1073741824:1073741824",
        "--user",
        format!("{uid}:{gid}"),
        "--tmpfs",
        format!("/scratch:rw,exec,size=10g,uid={uid},gid={gid}"),
        "--tmpfs",
        format!("/tmp:rw,size=64m,uid={uid},gid={gid}"),
        "--mount",
        mount(&paths.source, "/source", true),
        "--mount",
        mount(&paths.sysroot, "/opt/toolchain", true),
        "--mount",
        mount(&paths.cargo_home.join("registry"), "/cache/registry", true),
        "--mount",
        mount(&paths.output, "/out", false),
        "--workdir",
        "/source",
        image,
        "/usr/bin/env",
        "-i",
        "PATH=/opt/toolchain/bin:/usr/bin:/bin",
        "HOME=/scratch/home",
        "TMPDIR=/scratch/tmp",
        "CARGO_HOME=/scratch/cargo",
        "CARGO_TARGET_DIR=/scratch/target",
        "CARGO_BUILD_JOBS=2",
        "CARGO_INCREMENTAL=0",
        format!("TIDE_RUST_TARGET={target}"),
        "/bin/bash",
        "-c",
        CONTAINER_SCRIPT,
    ]
}

/// The highest `GLIBC_x.y` symbol version that `readelf --version-info` reports.
pub fn maximum_glibc(version_info: &str) -> Result<(u32, u32)> {
    let pattern = Regex::new(r"GLIBC_([0-9.]+)")?;
    pattern
        .captures_iter(version_info)
        .map(|found| {
            let mut parts = found[1]
                .split('.')
                .map(|part| part.parse::<u32>().unwrap_or(0));
            (parts.next().unwrap_or(0), parts.next().unwrap_or(0))
        })
        .max()
        .context("No GLIBC symbol versions were reported")
}

fn build_once(
    runner: &mut dyn CommandRunner,
    image: &str,
    target: &str,
    ids: (&str, &str),
    paths: &BuildPaths,
) -> Result<()> {
    runner.run_plain(&docker_run_arguments(image, target, ids.0, ids.1, paths))?;
    let versions = std::fs::read_to_string(paths.output.join("elf-versions.txt"))?;
    let maximum = maximum_glibc(&versions)?;
    ensure!(
        maximum <= MAXIMUM_GLIBC,
        "Release requires glibc {}.{}, above the supported {}.{}",
        maximum.0,
        maximum.1,
        MAXIMUM_GLIBC.0,
        MAXIMUM_GLIBC.1
    );
    println!("Maximum required glibc: {}.{}", maximum.0, maximum.1);
    Ok(())
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Reproducibility<'a> {
    independent_builds: u8,
    target: &'a str,
    builder_image: &'a str,
    binary_sha256: String,
    result: &'static str,
    scope: &'static str,
}

/// `build-rust-linux-release TARGET OUTPUT [--repeat]`.
pub fn build(
    target: &str,
    output: &Path,
    repeat: bool,
    runner: &mut dyn CommandRunner,
) -> Result<()> {
    validate_target(target)?;
    let source =
        PathBuf::from(capture(Command::new("git").args(["rev-parse", "--show-toplevel"]))?.trim());
    std::fs::create_dir_all(output)?;
    let output = output.canonicalize()?;
    ensure!(
        !output.join("tide").exists(),
        "Output already contains a binary"
    );
    let sysroot =
        PathBuf::from(capture(Command::new("rustc").args(["--print", "sysroot"]))?.trim());
    let cargo_home = std::env::var_os("CARGO_HOME").map_or_else(
        || PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(".cargo"),
        PathBuf::from,
    );
    let uid = capture(Command::new("id").arg("-u"))?.trim().to_owned();
    let gid = capture(Command::new("id").arg("-g"))?.trim().to_owned();
    runner.run_plain(&argv!["cargo", "fetch", "--locked", "--target", target])?;
    // The image needs no build context; an empty directory keeps the source out of it.
    let empty_context = tempfile::tempdir()?;
    runner.run_plain(&argv![
        "docker",
        "build",
        "--pull",
        "-t",
        IMAGE_TAG,
        "-f",
        source.join("scripts/rust-release-linux.Dockerfile"),
        empty_context.path(),
    ])?;
    let image = runner
        .run_plain(&argv![
            "docker", "image", "inspect", IMAGE_TAG, "--format", "{{.Id}}"
        ])?
        .stdout
        .trim()
        .to_owned();
    let mut paths = BuildPaths {
        source,
        sysroot,
        cargo_home,
        output: output.clone(),
    };
    build_once(runner, &image, target, (&uid, &gid), &paths)?;
    if repeat {
        // A fresh container sees only its own output directory. It cannot modify the
        // first build while the parent compares the two completed results.
        let parent = output.parent().context("output parent")?;
        let name = output
            .file_name()
            .context("output name")?
            .to_string_lossy()
            .into_owned();
        let repeat_output = tempfile::Builder::new()
            .prefix(&format!("{name}.repro."))
            .tempdir_in(parent)?;
        paths.output = repeat_output.path().to_path_buf();
        build_once(runner, &image, target, (&uid, &gid), &paths)?;
        for candidate in [output.join("tide"), paths.output.join("tide")] {
            ensure!(
                std::fs::symlink_metadata(&candidate).is_ok_and(|metadata| metadata.is_file()),
                "Non-regular build output"
            );
        }
        ensure!(
            std::fs::read(output.join("tide"))? == std::fs::read(paths.output.join("tide"))?,
            "Repeat Linux builds differ"
        );
        let proof = Reproducibility {
            independent_builds: 2,
            target,
            builder_image: &image,
            binary_sha256: sha256_file(&output.join("tide"))?,
            result: "byte-identical",
            scope: "same source, compiler and immutable Ubuntu 22.04 builder image; fresh isolated containers",
        };
        let text = pretty_json(&proof)?;
        std::fs::write(output.join("reproducibility.json"), &text)?;
        println!("{}", serde_json::to_string(&proof)?);
    }
    Ok(())
}

/// Runs Docker with inherited output so long builds stream their logs.
pub struct StreamingRunner;

impl CommandRunner for StreamingRunner {
    fn run(&mut self, argv: &[String], options: &RunOptions) -> Result<crate::util::CommandOutput> {
        let (program, args) = argv.split_first().context("empty argv")?;
        let mut command = Command::new(program);
        command.args(args);
        if let Some(cwd) = &options.cwd {
            command.current_dir(cwd);
        }
        // `docker image inspect` output is parsed; everything else streams.
        if argv.get(1).is_some_and(|word| word == "image") {
            return crate::util::SystemRunner.run(argv, options);
        }
        crate::util::run_inherited(&mut command)?;
        Ok(crate::util::CommandOutput::default())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn docker_runs_offline_read_only_and_unprivileged() {
        let paths = BuildPaths {
            source: PathBuf::from("/src"),
            sysroot: PathBuf::from("/toolchain"),
            cargo_home: PathBuf::from("/cargo"),
            output: PathBuf::from("/out-dir"),
        };
        let command = docker_run_arguments(
            "sha256:abc",
            "x86_64-unknown-linux-gnu",
            "501",
            "20",
            &paths,
        );
        let joined = command.join(" ");
        for flag in [
            "--read-only",
            "--network none",
            "--cap-drop ALL",
            "--security-opt no-new-privileges",
            "--user 501:20",
        ] {
            assert!(joined.contains(flag), "{flag}");
        }
        assert!(joined.contains("type=bind,source=/src,target=/source,readonly"));
        assert!(
            joined.contains("type=bind,source=/cargo/registry,target=/cache/registry,readonly")
        );
        assert!(joined.contains("type=bind,source=/out-dir,target=/out "));
        assert!(joined.contains("TIDE_RUST_TARGET=x86_64-unknown-linux-gnu"));
        assert!(
            command
                .last()
                .unwrap()
                .contains("cargo build --locked --offline")
        );
    }

    #[test]
    fn only_linux_gnu_targets_are_accepted() {
        validate_target("aarch64-unknown-linux-gnu").unwrap();
        assert!(validate_target("x86_64-apple-darwin").is_err());
    }

    #[test]
    fn glibc_ceiling_reads_the_highest_symbol_version() {
        let info = "Name: GLIBC_2.17  Name: GLIBC_2.34 Name: GLIBC_2.2.5";
        assert_eq!(maximum_glibc(info).unwrap(), (2, 34));
        assert!(maximum_glibc("no versions").is_err());
        assert!(maximum_glibc("GLIBC_2.36").unwrap() > MAXIMUM_GLIBC);
    }
}
