//! Release contract, deterministic payload preparation, archives and checksums.
use crate::argv;
use crate::util::{
    CommandRunner, RunOptions, chmod, is_full_git_sha, locale_compare, pretty_json, read_json,
    set_times, sha256_file,
};
use anyhow::{Context, Result, bail, ensure};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::io::Write;
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ArtifactSpec {
    pub id: String,
    pub filename: String,
    pub platform: String,
    pub architecture: String,
    pub status: String,
    pub rust_target: String,
    #[serde(default)]
    pub build_image: Option<String>,
    #[serde(default)]
    pub minimum_glibc: Option<String>,
    #[serde(default, rename = "minimumOS")]
    pub minimum_os: Option<String>,
    pub runner: String,
    pub archive_format: String,
    pub signing: String,
    pub notarization: String,
    pub smoke_checks: Vec<String>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReleasePolicy {
    pub tag: String,
    pub title: String,
    pub notes_path: String,
    pub manual_runs_may_publish: bool,
    pub publication_actor: String,
    pub checksums_filename: String,
    pub attestation: String,
    pub replace_existing_assets: bool,
    /// The Homebrew tap that receives the generated formula after publication.
    pub homebrew_tap: String,
}

#[derive(Clone, Debug, Deserialize)]
pub struct ProductMetadata {
    pub name: String,
    pub version: String,
    pub binary: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ReleaseMetadata {
    release_policy: ReleasePolicy,
    artifacts: Vec<serde_json::Value>,
}

#[derive(Clone, Debug)]
pub struct ReleaseContract {
    pub root: PathBuf,
    pub product: ProductMetadata,
    pub policy: ReleasePolicy,
    pub artifacts: Vec<ArtifactSpec>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
pub struct PayloadEntry {
    pub path: String,
    pub sha256: String,
    pub mode: String,
    pub size: u64,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VersionManifest {
    pub schema_version: u8,
    pub product: String,
    pub version: String,
    pub source_commit: String,
    pub target: String,
    pub platform: String,
    pub architecture: String,
    pub prerelease: bool,
    pub implementation: String,
    pub local: bool,
    pub payloads: Vec<PayloadEntry>,
}

pub const PAYLOAD_FILES: [&str; 5] = [
    "ATTRIBUTIONS.md",
    "LICENSE",
    "VERSION.json",
    "tide",
    "uninstall.sh",
];

/// Installer scripts published beside the archives, so
/// `releases/latest/download/install.sh` is the canonical one-line installer.
pub const INSTALLER_ASSETS: [&str; 2] = ["install.sh", "uninstall.sh"];

pub fn archive_stem(filename: &str) -> Result<&str> {
    if let Some(stem) = filename.strip_suffix(".tar.gz") {
        return Ok(stem);
    }
    if let Some(stem) = filename.strip_suffix(".zip") {
        return Ok(stem);
    }
    bail!("Unsupported release archive filename: {filename}")
}

pub fn load_contract(repository_root: &Path) -> Result<ReleaseContract> {
    let root = std::path::absolute(repository_root)?;
    let product: ProductMetadata = read_json(&root.join("config/product.json"))?;
    let release: ReleaseMetadata = read_json(&root.join("config/release.json"))?;
    let artifacts = release
        .artifacts
        .into_iter()
        .filter(|entry| entry.get("id").and_then(serde_json::Value::as_str) != Some("checksums"))
        .map(serde_json::from_value)
        .collect::<Result<Vec<ArtifactSpec>, _>>()
        .context("parse release artifacts")?;
    let policy = release.release_policy;

    ensure!(
        product.name == "Tide" && product.binary == "tide",
        "Release product identity must be Tide/tide"
    );
    ensure!(
        policy.tag == format!("v{}", product.version),
        "Release tag must match product version"
    );
    ensure!(
        policy.title == format!("Tide v{}", product.version),
        "Release title must match product version"
    );
    ensure!(
        !policy.manual_runs_may_publish,
        "Manual release runs must not publish"
    );
    ensure!(
        !policy.replace_existing_assets,
        "Release assets must not be replaceable"
    );
    ensure!(
        policy.homebrew_tap == "ultradotdev/homebrew-tap",
        "Homebrew tap must be ultradotdev/homebrew-tap"
    );
    ensure!(
        artifacts.len() == 4,
        "Release contract must contain four platform artifacts"
    );
    let ids: HashSet<_> = artifacts.iter().map(|item| &item.id).collect();
    ensure!(
        ids.len() == artifacts.len(),
        "Release artifact IDs must be unique"
    );
    let names: HashSet<_> = artifacts.iter().map(|item| &item.filename).collect();
    ensure!(
        names.len() == artifacts.len(),
        "Release filenames must be unique"
    );

    for artifact in &artifacts {
        let extension = if artifact.archive_format == "zip" {
            ".zip"
        } else {
            ".tar.gz"
        };
        let expected = format!(
            "tide-v{}-{}-{}{extension}",
            product.version, artifact.platform, artifact.architecture
        );
        ensure!(
            artifact.filename == expected,
            "{} filename does not match its target",
            artifact.id
        );
        let arch = if artifact.architecture == "arm64" {
            "aarch64"
        } else {
            "x86_64"
        };
        let os = if artifact.platform == "darwin" {
            "apple-darwin"
        } else {
            "unknown-linux-gnu"
        };
        ensure!(
            artifact.rust_target == format!("{arch}-{os}"),
            "{} Rust target mismatch",
            artifact.id
        );
        let policy_ok = if artifact.platform == "darwin" {
            artifact.signing == "developer-id"
                && artifact.notarization == "notarytool"
                && artifact.archive_format == "zip"
        } else {
            artifact.signing == "none"
                && artifact.notarization == "none"
                && artifact.archive_format == "tar.gz"
        };
        ensure!(
            policy_ok,
            "{} signing or archive policy mismatch",
            artifact.id
        );
    }

    Ok(ReleaseContract {
        root,
        product,
        policy,
        artifacts,
    })
}

impl ReleaseContract {
    pub fn artifact(&self, artifact_id: &str) -> Result<&ArtifactSpec> {
        self.artifacts
            .iter()
            .find(|item| item.id == artifact_id)
            .with_context(|| format!("Unknown release artifact: {artifact_id}"))
    }
}

/// An isolated XDG environment for the model-catalog smoke check.
pub fn isolated_smoke_env(home: &Path) -> Vec<(String, String)> {
    vec![
        ("HOME".into(), home.display().to_string()),
        (
            "XDG_CONFIG_HOME".into(),
            home.join(".config").display().to_string(),
        ),
        (
            "XDG_DATA_HOME".into(),
            home.join(".local/share").display().to_string(),
        ),
        (
            "XDG_STATE_HOME".into(),
            home.join(".local/state").display().to_string(),
        ),
        (
            "XDG_CACHE_HOME".into(),
            home.join(".cache").display().to_string(),
        ),
        (
            "PATH".into(),
            std::env::var("PATH").unwrap_or_else(|_| "/usr/bin:/bin".into()),
        ),
        ("TERM".into(), "xterm-256color".into()),
        ("LANG".into(), "C.UTF-8".into()),
    ]
}

fn run_release_smoke_checks(
    binary: &Path,
    version: &str,
    runner: &mut dyn CommandRunner,
) -> Result<()> {
    let output = runner.run_plain(&argv![binary, "--version"])?;
    ensure!(
        output.stdout.contains(version),
        "Binary version output does not contain {version}"
    );
    runner.run_plain(&argv![binary, "--check-terminal"])?;
    runner.run_plain(&argv![binary, "--check-catalogs"])?;
    let isolated = tempfile::Builder::new()
        .prefix("tide-release-models-")
        .tempdir()?;
    runner.run(
        &argv![binary, "--check-models"],
        &RunOptions::env(isolated_smoke_env(isolated.path())),
    )?;
    Ok(())
}

pub fn source_commit_timestamp(
    root: &Path,
    source_commit: &str,
    runner: &mut dyn CommandRunner,
) -> Result<u64> {
    ensure!(
        is_full_git_sha(source_commit),
        "Source commit must be a full lowercase Git object ID"
    );
    let head = runner.run(
        &argv!["/usr/bin/git", "rev-parse", "HEAD"],
        &RunOptions::cwd(root),
    )?;
    let head = head.stdout.trim();
    ensure!(
        head == source_commit,
        "Source commit {source_commit} does not match checked out HEAD {head}"
    );
    commit_timestamp(root, source_commit, runner)
}

pub fn commit_timestamp(
    root: &Path,
    source_commit: &str,
    runner: &mut dyn CommandRunner,
) -> Result<u64> {
    let output = runner.run(
        &argv!["/usr/bin/git", "show", "-s", "--format=%ct", source_commit],
        &RunOptions::cwd(root),
    )?;
    let timestamp: u64 = output.stdout.trim().parse().unwrap_or(0);
    ensure!(timestamp > 0, "Source commit timestamp is invalid");
    Ok(timestamp)
}

pub fn ensure_plain_file(path: &Path, label: &str) -> Result<()> {
    let info = std::fs::symlink_metadata(path)
        .with_context(|| format!("{label} is missing: {}", path.display()))?;
    ensure!(info.is_file(), "{label} must be a plain file");
    Ok(())
}

fn normalize_file(path: &Path, mode: u32, timestamp: u64) -> Result<()> {
    chmod(path, mode)?;
    set_times(path, timestamp)
}

pub fn payload_entries(payload_root: &Path) -> Result<Vec<PayloadEntry>> {
    use std::os::unix::fs::PermissionsExt;
    fn visit(root: &Path, directory: &Path, entries: &mut Vec<PayloadEntry>) -> Result<()> {
        let mut children = std::fs::read_dir(directory)
            .with_context(|| format!("list {}", directory.display()))?
            .collect::<Result<Vec<_>, _>>()?;
        children.sort_by(|left, right| {
            locale_compare(
                &left.file_name().to_string_lossy(),
                &right.file_name().to_string_lossy(),
            )
        });
        for child in children {
            let path = child.path();
            let relative = path
                .strip_prefix(root)?
                .to_string_lossy()
                .replace('\\', "/");
            let info = std::fs::symlink_metadata(&path)?;
            ensure!(
                !info.file_type().is_symlink(),
                "Release payload must not contain symlink: {relative}"
            );
            if info.is_dir() {
                visit(root, &path, entries)?;
            } else {
                ensure!(
                    info.is_file(),
                    "Release payload contains unsupported entry: {relative}"
                );
                if relative != "VERSION.json" {
                    entries.push(PayloadEntry {
                        sha256: sha256_file(&path)?,
                        mode: if info.permissions().mode() & 0o111 != 0 {
                            "0755"
                        } else {
                            "0644"
                        }
                        .into(),
                        size: info.len(),
                        path: relative,
                    });
                }
            }
        }
        Ok(())
    }
    let mut entries = Vec::new();
    visit(payload_root, payload_root, &mut entries)?;
    Ok(entries)
}

fn build_manifest(
    contract: &ReleaseContract,
    artifact: &ArtifactSpec,
    payload_root: &Path,
    source_commit: &str,
) -> Result<VersionManifest> {
    Ok(VersionManifest {
        schema_version: 2,
        product: "Tide".into(),
        version: contract.product.version.clone(),
        source_commit: source_commit.into(),
        target: artifact.rust_target.clone(),
        platform: artifact.platform.clone(),
        architecture: artifact.architecture.clone(),
        prerelease: true,
        implementation: "rust".into(),
        local: false,
        payloads: payload_entries(payload_root)?,
    })
}

fn write_manifest(payload_root: &Path, manifest: &VersionManifest, timestamp: u64) -> Result<()> {
    let path = payload_root.join("VERSION.json");
    std::fs::write(&path, pretty_json(manifest)?)?;
    normalize_file(&path, 0o644, timestamp)
}

pub struct PrepareOptions<'a> {
    pub repository_root: &'a Path,
    pub artifact_id: &'a str,
    pub binary: &'a Path,
    pub output_directory: &'a Path,
    pub source_commit: &'a str,
    pub run_smoke_checks: bool,
}

pub fn prepare_payload(
    options: &PrepareOptions<'_>,
    runner: &mut dyn CommandRunner,
) -> Result<PathBuf> {
    let contract = load_contract(options.repository_root)?;
    let artifact = contract.artifact(options.artifact_id)?.clone();
    let binary = std::path::absolute(options.binary)?;
    ensure_plain_file(&binary, "Tide binary")?;
    if options.run_smoke_checks {
        run_release_smoke_checks(&binary, &contract.product.version, runner)?;
    }
    let source_date_epoch = source_commit_timestamp(&contract.root, options.source_commit, runner)?;
    let stem = archive_stem(&artifact.filename)?;
    let output = std::path::absolute(options.output_directory)?;
    let payload_root = output.join("payloads").join(stem);
    ensure!(
        payload_root.starts_with(&output) && payload_root != output,
        "Release payload path escaped its output directory"
    );
    if payload_root.exists() {
        std::fs::remove_dir_all(&payload_root)?;
    }
    std::fs::create_dir_all(&payload_root)?;

    let copies = [
        (binary.clone(), "tide", 0o755),
        (contract.root.join("uninstall.sh"), "uninstall.sh", 0o755),
        (contract.root.join("LICENSE"), "LICENSE", 0o644),
        (
            contract.root.join("docs/ATTRIBUTIONS.md"),
            "ATTRIBUTIONS.md",
            0o644,
        ),
    ];
    for (source, name, _) in &copies {
        std::fs::copy(source, payload_root.join(name))
            .with_context(|| format!("copy {}", source.display()))?;
    }
    for (_, name, mode) in &copies {
        normalize_file(&payload_root.join(name), *mode, source_date_epoch)?;
    }
    chmod(&payload_root, 0o755)?;
    set_times(&payload_root, source_date_epoch)?;

    let manifest = build_manifest(&contract, &artifact, &payload_root, options.source_commit)?;
    write_manifest(&payload_root, &manifest, source_date_epoch)?;
    if options.run_smoke_checks {
        runner.run_plain(&argv![binary, "--verify-install-manifest", payload_root])?;
    }
    Ok(payload_root)
}

pub fn validate_prepared_payload(
    contract: &ReleaseContract,
    artifact: &ArtifactSpec,
    payload_root: &Path,
) -> Result<VersionManifest> {
    let actual = payload_entries(payload_root)?;
    let mut found: Vec<&str> = actual.iter().map(|entry| entry.path.as_str()).collect();
    found.push("VERSION.json");
    found.sort_unstable();
    let mut expected = PAYLOAD_FILES.to_vec();
    expected.sort_unstable();
    ensure!(
        found == expected,
        "Release payload layout mismatch: {}",
        found.join(", ")
    );
    let manifest: VersionManifest = read_json(&payload_root.join("VERSION.json"))?;
    ensure!(
        manifest.schema_version == 2 && manifest.implementation == "rust" && !manifest.local,
        "VERSION.json implementation mismatch"
    );
    ensure!(
        manifest.version == contract.product.version,
        "VERSION.json version mismatch"
    );
    ensure!(
        manifest.target == artifact.rust_target,
        "VERSION.json target mismatch"
    );
    ensure!(
        manifest.platform == artifact.platform && manifest.architecture == artifact.architecture,
        "VERSION.json platform mismatch"
    );
    ensure!(
        manifest.payloads == actual,
        "VERSION.json payload hashes are stale"
    );
    Ok(manifest)
}

pub fn refresh_prepared_manifest(
    repository_root: &Path,
    artifact_id: &str,
    payload_root: &Path,
    source_commit: &str,
    source_date_epoch: u64,
) -> Result<VersionManifest> {
    let contract = load_contract(repository_root)?;
    let artifact = contract.artifact(artifact_id)?;
    let manifest = build_manifest(&contract, artifact, payload_root, source_commit)?;
    write_manifest(payload_root, &manifest, source_date_epoch)?;
    validate_prepared_payload(&contract, artifact, payload_root)?;
    Ok(manifest)
}

pub fn archive_prepared_payload(
    repository_root: &Path,
    artifact_id: &str,
    payload_root: &Path,
    output_directory: &Path,
    runner: &mut dyn CommandRunner,
) -> Result<PathBuf> {
    let contract = load_contract(repository_root)?;
    let artifact = contract.artifact(artifact_id)?;
    validate_prepared_payload(&contract, artifact, payload_root)?;
    let stem = archive_stem(&artifact.filename)?;
    ensure!(
        payload_root.file_name().is_some_and(|name| name == stem),
        "Release payload directory must match the archive stem"
    );
    let output = std::path::absolute(output_directory)?;
    std::fs::create_dir_all(&output)?;
    let archive = output.join(&artifact.filename);
    if archive.exists() {
        std::fs::remove_file(&archive)?;
    }
    if artifact.archive_format == "zip" {
        write_release_zip(payload_root, &archive)?;
    } else {
        let modified = std::fs::metadata(payload_root.join("VERSION.json"))?
            .modified()?
            .duration_since(std::time::UNIX_EPOCH)?
            .as_secs();
        let parent = payload_root
            .parent()
            .context("Release payload has no parent directory")?;
        runner.run_plain(&argv![
            "/usr/bin/tar",
            "--sort=name",
            "--format=ustar",
            format!("--mtime=@{modified}"),
            "--owner=0",
            "--group=0",
            "--numeric-owner",
            "-czf",
            archive,
            "-C",
            parent,
            stem,
        ])?;
    }
    ensure_plain_file(&archive, "Release archive")?;
    Ok(archive)
}

/// Writes `SHA256SUMS` for the four archives, two notarization receipts and
/// the two installer scripts.
pub fn write_release_checksums(asset_paths: &[PathBuf], output: &Path) -> Result<()> {
    ensure!(
        asset_paths.len() == 8,
        "Exactly four archives, two notarization receipts and two installer scripts are required for release checksums"
    );
    let mut named: Vec<(String, &PathBuf)> = asset_paths
        .iter()
        .map(|path| {
            path.file_name()
                .map(|name| (name.to_string_lossy().into_owned(), path))
                .context("Release asset has no file name")
        })
        .collect::<Result<_>>()?;
    let unique: HashSet<_> = named.iter().map(|(name, _)| name.clone()).collect();
    ensure!(
        unique.len() == named.len(),
        "Release archive filenames must be unique"
    );
    named.sort_by(|left, right| locale_compare(&left.0, &right.0));
    let mut lines = String::new();
    for (name, path) in named {
        lines.push_str(&format!("{}  {name}\n", sha256_file(path)?));
    }
    std::fs::write(output, lines)?;
    chmod(output, 0o644)
}

/// Converts seconds since the epoch to a UTC civil date and time.
pub fn utc_civil(epoch_seconds: u64) -> (i64, u32, u32, u32, u32, u32) {
    let days = i64::try_from(epoch_seconds / 86_400).unwrap_or(i64::MAX);
    let seconds = epoch_seconds % 86_400;
    // Howard Hinnant's days-from-civil inverse.
    let shifted = days + 719_468;
    let era = shifted.div_euclid(146_097);
    let day_of_era = shifted.rem_euclid(146_097);
    let year_of_era =
        (day_of_era - day_of_era / 1460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_index = (5 * day_of_year + 2) / 153;
    let day = u32::try_from(day_of_year - (153 * month_index + 2) / 5 + 1).unwrap_or(1);
    let month = u32::try_from(if month_index < 10 {
        month_index + 3
    } else {
        month_index - 9
    })
    .unwrap_or(1);
    let year = year_of_era + era * 400 + i64::from(month <= 2);
    let hour = u32::try_from(seconds / 3600).unwrap_or(0);
    let minute = u32::try_from(seconds % 3600 / 60).unwrap_or(0);
    let second = u32::try_from(seconds % 60).unwrap_or(0);
    (year, month, day, hour, minute, second)
}

/// Writes the normalized payload as a deterministic, unsigned ZIP archive.
///
/// Every entry is `<payload directory>/<name>`, stamped with the UTC time of
/// `VERSION.json`, compressed with DEFLATE level 9 and carries its Unix mode.
pub fn write_release_zip(payload_root: &Path, output: &Path) -> Result<()> {
    use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
    let mut names: Vec<String> = std::fs::read_dir(payload_root)?
        .map(|entry| entry.map(|entry| entry.file_name().to_string_lossy().into_owned()))
        .collect::<Result<_, _>>()?;
    names.sort();
    ensure!(
        names == PAYLOAD_FILES,
        "Release ZIP payload must contain exactly {}",
        PAYLOAD_FILES.join(", ")
    );
    let root_name = payload_root
        .file_name()
        .context("Payload root has no name")?
        .to_string_lossy()
        .into_owned();
    let stamp = std::fs::metadata(payload_root.join("VERSION.json"))?
        .modified()?
        .duration_since(std::time::UNIX_EPOCH)?
        .as_secs();
    let (year, month, day, hour, minute, second) = utc_civil(stamp);
    ensure!(
        (1980..=2107).contains(&year),
        "ZIP timestamps must fall between 1980 and 2107"
    );
    let dos_time = u16::try_from((hour << 11) | (minute << 5) | (second / 2))?;
    let dos_date = u16::try_from(((year - 1980) << 9) | i64::from(month << 5) | i64::from(day))?;

    let mut body = Vec::new();
    let mut central = Vec::new();
    for name in &names {
        let path = payload_root.join(name);
        let metadata = std::fs::symlink_metadata(&path)?;
        ensure!(
            metadata.is_file() && metadata.len() <= 512 * 1024 * 1024,
            "Release ZIP entries must be regular files no larger than 512 MiB: {name}"
        );
        let data = std::fs::read(&path)?;
        let mut crc = flate2::Crc::new();
        crc.update(&data);
        let mut encoder =
            flate2::write::DeflateEncoder::new(Vec::new(), flate2::Compression::new(9));
        encoder.write_all(&data)?;
        let compressed = encoder.finish()?;
        let entry_name = format!("{root_name}/{name}");
        let offset = u32::try_from(body.len())?;
        let header = ZipHeader {
            name: entry_name.as_bytes(),
            crc: crc.sum(),
            compressed: u32::try_from(compressed.len())?,
            size: u32::try_from(data.len())?,
            dos_time,
            dos_date,
        };
        header.local(&mut body)?;
        body.extend_from_slice(&compressed);
        header.central(&mut central, metadata.mode() << 16, offset)?;
    }
    let central_offset = u32::try_from(body.len())?;
    let central_size = u32::try_from(central.len())?;
    let count = u16::try_from(names.len())?;
    body.extend_from_slice(&central);
    body.extend_from_slice(&0x0605_4b50_u32.to_le_bytes());
    body.extend_from_slice(&[0, 0, 0, 0]);
    body.extend_from_slice(&count.to_le_bytes());
    body.extend_from_slice(&count.to_le_bytes());
    body.extend_from_slice(&central_size.to_le_bytes());
    body.extend_from_slice(&central_offset.to_le_bytes());
    body.extend_from_slice(&0_u16.to_le_bytes());

    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o644)
        .open(output)
        .with_context(|| format!("create {}", output.display()))?;
    file.write_all(&body)?;
    file.sync_all()?;
    Ok(())
}

struct ZipHeader<'a> {
    name: &'a [u8],
    crc: u32,
    compressed: u32,
    size: u32,
    dos_time: u16,
    dos_date: u16,
}

impl ZipHeader<'_> {
    const VERSION_NEEDED: u16 = 20;
    const DEFLATE: u16 = 8;

    fn common(&self, output: &mut Vec<u8>) -> Result<()> {
        output.extend_from_slice(&Self::VERSION_NEEDED.to_le_bytes());
        output.extend_from_slice(&0_u16.to_le_bytes());
        output.extend_from_slice(&Self::DEFLATE.to_le_bytes());
        output.extend_from_slice(&self.dos_time.to_le_bytes());
        output.extend_from_slice(&self.dos_date.to_le_bytes());
        output.extend_from_slice(&self.crc.to_le_bytes());
        output.extend_from_slice(&self.compressed.to_le_bytes());
        output.extend_from_slice(&self.size.to_le_bytes());
        output.extend_from_slice(&u16::try_from(self.name.len())?.to_le_bytes());
        output.extend_from_slice(&0_u16.to_le_bytes());
        Ok(())
    }

    fn local(&self, output: &mut Vec<u8>) -> Result<()> {
        output.extend_from_slice(&0x0403_4b50_u32.to_le_bytes());
        self.common(output)?;
        output.extend_from_slice(self.name);
        Ok(())
    }

    fn central(&self, output: &mut Vec<u8>, external: u32, offset: u32) -> Result<()> {
        output.extend_from_slice(&0x0201_4b50_u32.to_le_bytes());
        // Made by Unix (3), specification 2.0.
        output.extend_from_slice(&((3_u16 << 8) | Self::VERSION_NEEDED).to_le_bytes());
        self.common(output)?;
        output.extend_from_slice(&0_u16.to_le_bytes()); // comment
        output.extend_from_slice(&0_u16.to_le_bytes()); // disk
        output.extend_from_slice(&0_u16.to_le_bytes()); // internal attributes
        output.extend_from_slice(&external.to_le_bytes());
        output.extend_from_slice(&offset.to_le_bytes());
        output.extend_from_slice(self.name);
        Ok(())
    }
}

/// `package-release-artifact <prepare|finalize|package>`.
pub struct PackageArgs {
    pub action: String,
    pub repository_root: PathBuf,
    pub artifact_id: String,
    pub output_directory: PathBuf,
    pub source_commit: String,
    pub binary: Option<PathBuf>,
    pub payload_root: Option<PathBuf>,
}

pub fn package_release_artifact(args: &PackageArgs, runner: &mut dyn CommandRunner) -> Result<()> {
    ensure!(
        ["prepare", "finalize", "package"].contains(&args.action.as_str()),
        "Usage: package-release-artifact <prepare|finalize|package> --artifact-id=<id> --repository-root=<path> --output-directory=<path> --source-commit=<sha> [--binary=<path> --payload-root=<path>]"
    );
    let root = std::path::absolute(&args.repository_root)?;
    let output = std::path::absolute(&args.output_directory)?;
    let contract = load_contract(&root)?;
    let artifact = contract.artifact(&args.artifact_id)?;
    let payload_root = if args.action == "finalize" {
        let payload_root = std::path::absolute(
            args.payload_root
                .as_ref()
                .context("Missing --payload-root=<value>")?,
        )?;
        let timestamp = commit_timestamp(&root, &args.source_commit, runner)?;
        refresh_prepared_manifest(
            &root,
            &args.artifact_id,
            &payload_root,
            &args.source_commit,
            timestamp,
        )?;
        payload_root
    } else {
        let binary = args.binary.as_ref().context("Missing --binary=<value>")?;
        prepare_payload(
            &PrepareOptions {
                repository_root: &root,
                artifact_id: &args.artifact_id,
                binary,
                output_directory: &output,
                source_commit: &args.source_commit,
                run_smoke_checks: true,
            },
            runner,
        )?
    };
    let mut result = serde_json::json!({
        "artifact": artifact.id,
        "payloadRoot": payload_root,
    });
    if args.action != "prepare" {
        let archive =
            archive_prepared_payload(&root, &args.artifact_id, &payload_root, &output, runner)?;
        result["archive"] = serde_json::json!(archive);
    }
    println!("{result}");
    Ok(())
}

/// `generate-release-checksums`: hashes exactly the expected public assets.
pub fn generate_release_checksums(repository_root: &Path, assets_directory: &Path) -> Result<()> {
    let contract = load_contract(repository_root)?;
    let assets = std::path::absolute(assets_directory)?;
    let names: Vec<String> = std::fs::read_dir(&assets)?
        .map(|entry| entry.map(|entry| entry.file_name().to_string_lossy().into_owned()))
        .collect::<Result<_, _>>()?;
    let mut paths = Vec::new();
    for artifact in &contract.artifacts {
        ensure!(
            names.contains(&artifact.filename),
            "Missing release archive: {}",
            artifact.filename
        );
        paths.push(assets.join(&artifact.filename));
    }
    let receipts: Vec<String> = contract
        .artifacts
        .iter()
        .filter(|artifact| artifact.platform == "darwin")
        .map(|artifact| {
            format!(
                "{}.notarization.json",
                artifact.filename.trim_end_matches(".zip")
            )
        })
        .collect();
    for receipt in &receipts {
        ensure!(
            names.contains(receipt),
            "Missing notarization receipt: {receipt}"
        );
        paths.push(assets.join(receipt));
    }
    // Published installers must be exactly the scripts of the released source.
    for script in INSTALLER_ASSETS {
        let asset = assets.join(script);
        ensure!(
            names.iter().any(|name| name == script),
            "Missing installer script: {script}"
        );
        ensure_plain_file(&asset, "Installer script")?;
        ensure!(
            sha256_file(&asset)? == sha256_file(&contract.root.join(script))?,
            "{script} differs from the released source"
        );
        paths.push(asset);
    }
    let unexpected: Vec<&String> = names
        .iter()
        .filter(|name| {
            !contract
                .artifacts
                .iter()
                .any(|artifact| &artifact.filename == *name)
                && !receipts.contains(name)
                && !INSTALLER_ASSETS.contains(&name.as_str())
        })
        .collect();
    ensure!(
        unexpected.is_empty(),
        "Unexpected release asset before checksums: {}",
        unexpected
            .iter()
            .map(|name| name.as_str())
            .collect::<Vec<_>>()
            .join(", ")
    );
    write_release_checksums(&paths, &assets.join(&contract.policy.checksums_filename))?;
    println!("Generated {}", contract.policy.checksums_filename);
    Ok(())
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use crate::util::testing::{RecordingRunner, stdout};
    use crate::util::{SystemRunner, repo_root};

    pub fn head_commit() -> String {
        SystemRunner
            .run(
                &argv!["git", "rev-parse", "HEAD"],
                &RunOptions::cwd(&repo_root()),
            )
            .unwrap()
            .stdout
            .trim()
            .to_owned()
    }

    /// Answers git and binary smoke checks without executing anything.
    pub fn fake_release_runner(head: String) -> RecordingRunner {
        RecordingRunner::new(move |argv, _| {
            Ok(match argv.get(1).map(String::as_str) {
                Some("rev-parse") => stdout(&format!("{head}\n")),
                Some("show") => stdout("1767225600\n"),
                Some("--version") => stdout("Tide 0.10.0\n"),
                _ => stdout(""),
            })
        })
    }

    pub fn fixture_binary(directory: &Path) -> PathBuf {
        let binary = directory.join("tide");
        std::fs::write(&binary, "#!/bin/sh\necho fixture\n").unwrap();
        chmod(&binary, 0o755).unwrap();
        binary
    }

    fn prepare(output: &Path, binary: &Path, head: &str) -> (PathBuf, RecordingRunner) {
        let mut runner = fake_release_runner(head.to_owned());
        let prepared = prepare_payload(
            &PrepareOptions {
                repository_root: &repo_root(),
                artifact_id: "darwin-arm64",
                binary,
                output_directory: output,
                source_commit: head,
                run_smoke_checks: true,
            },
            &mut runner,
        )
        .unwrap();
        (prepared, runner)
    }

    #[test]
    fn resolves_one_exact_four_platform_artifact_matrix() {
        let contract = load_contract(&repo_root()).unwrap();
        assert!(!contract.policy.manual_runs_may_publish);
        assert!(!contract.policy.replace_existing_assets);
        let ids: Vec<_> = contract
            .artifacts
            .iter()
            .map(|item| item.id.as_str())
            .collect();
        assert_eq!(
            ids,
            ["darwin-arm64", "darwin-x64", "linux-arm64", "linux-x64"]
        );
        let darwin = contract.artifact("darwin-arm64").unwrap();
        assert_eq!(darwin.filename, "tide-v0.10.0-darwin-arm64.zip");
        assert_eq!(darwin.rust_target, "aarch64-apple-darwin");
        assert_eq!(darwin.signing, "developer-id");
        assert_eq!(darwin.notarization, "notarytool");
        assert_eq!(
            archive_stem("tide-v0.10.0-linux-x64.tar.gz").unwrap(),
            "tide-v0.10.0-linux-x64"
        );
    }

    #[test]
    fn prepares_an_exact_normalized_payload_with_self_verifying_hashes() {
        use std::os::unix::fs::PermissionsExt;
        let scratch = tempfile::tempdir().unwrap();
        let binary = fixture_binary(scratch.path());
        let head = head_commit();
        let (prepared, runner) = prepare(scratch.path(), &binary, &head);
        let contract = load_contract(&repo_root()).unwrap();
        let manifest = validate_prepared_payload(
            &contract,
            contract.artifact("darwin-arm64").unwrap(),
            &prepared,
        )
        .unwrap();
        assert_eq!(manifest.source_commit, head);
        let paths: Vec<_> = manifest
            .payloads
            .iter()
            .map(|item| item.path.as_str())
            .collect();
        assert_eq!(
            paths,
            ["ATTRIBUTIONS.md", "LICENSE", "tide", "uninstall.sh"]
        );
        let mode = |name: &str| {
            std::fs::metadata(prepared.join(name))
                .unwrap()
                .permissions()
                .mode()
                & 0o777
        };
        assert_eq!(mode("tide"), 0o755);
        assert_eq!(mode("uninstall.sh"), 0o755);
        assert_eq!(mode("VERSION.json"), 0o644);
        let commands = runner.commands();
        assert!(commands[0].ends_with("tide --version"));
        assert!(
            commands
                .last()
                .unwrap()
                .contains("--verify-install-manifest")
        );
        let isolated = &runner.calls[3];
        assert!(isolated.0[1] == "--check-models" && isolated.1.env.is_some());
    }

    #[test]
    fn rejects_a_source_commit_other_than_the_checkout() {
        let scratch = tempfile::tempdir().unwrap();
        let binary = fixture_binary(scratch.path());
        let mut runner = fake_release_runner("f".repeat(40));
        let error = prepare_payload(
            &PrepareOptions {
                repository_root: &repo_root(),
                artifact_id: "darwin-arm64",
                binary: &binary,
                output_directory: scratch.path(),
                source_commit: &"e".repeat(40),
                run_smoke_checks: false,
            },
            &mut runner,
        )
        .unwrap_err();
        assert!(
            error
                .to_string()
                .contains("does not match checked out HEAD")
        );
    }

    #[test]
    fn produces_deterministic_unsigned_macos_archives_from_identical_payloads() {
        let scratch = tempfile::tempdir().unwrap();
        let binary = fixture_binary(scratch.path());
        let head = head_commit();
        let mut digests = Vec::new();
        for name in ["a", "b"] {
            let output = scratch.path().join(name);
            let (prepared, mut runner) = prepare(&output, &binary, &head);
            let archive = archive_prepared_payload(
                &repo_root(),
                "darwin-arm64",
                &prepared,
                &output.join("archives"),
                &mut runner,
            )
            .unwrap();
            digests.push(sha256_file(&archive).unwrap());
        }
        assert_eq!(digests[0], digests[1]);
    }

    #[test]
    fn zip_entries_keep_the_stem_root_unix_modes_and_contents() {
        let scratch = tempfile::tempdir().unwrap();
        let binary = fixture_binary(scratch.path());
        let (prepared, _) = prepare(scratch.path(), &binary, &head_commit());
        let archive = scratch.path().join("out.zip");
        write_release_zip(&prepared, &archive).unwrap();
        let listing = std::process::Command::new("unzip")
            .args(["-Z", "-l"])
            .arg(&archive)
            .output();
        if let Ok(listing) = listing {
            let text = String::from_utf8_lossy(&listing.stdout);
            assert!(listing.status.success(), "{text}");
            assert!(text.contains("-rwxr-xr-x") && text.contains("tide-v0.10.0-darwin-arm64/tide"));
            assert!(
                text.contains("-rw-r--r--")
                    && text.contains("tide-v0.10.0-darwin-arm64/VERSION.json")
            );
        }
        let extracted = scratch.path().join("extracted");
        std::fs::create_dir(&extracted).unwrap();
        let status = std::process::Command::new("unzip")
            .arg("-q")
            .arg(&archive)
            .arg("-d")
            .arg(&extracted)
            .status();
        if let Ok(status) = status {
            assert!(status.success());
            assert_eq!(
                std::fs::read(extracted.join("tide-v0.10.0-darwin-arm64/tide")).unwrap(),
                std::fs::read(&binary).unwrap()
            );
        }
        assert!(write_release_zip(&prepared, &archive).is_err());
    }

    #[test]
    fn checksums_all_archives_receipts_and_installer_scripts() {
        let scratch = tempfile::tempdir().unwrap();
        let names = [
            "tide-v0.10.0-darwin-arm64.zip",
            "tide-v0.10.0-darwin-arm64.notarization.json",
            "tide-v0.10.0-darwin-x64.zip",
            "tide-v0.10.0-darwin-x64.notarization.json",
            "tide-v0.10.0-linux-arm64.tar.gz",
            "tide-v0.10.0-linux-x64.tar.gz",
            "install.sh",
            "uninstall.sh",
        ];
        let paths: Vec<PathBuf> = names.iter().map(|name| scratch.path().join(name)).collect();
        for (index, path) in paths.iter().enumerate() {
            std::fs::write(path, format!("asset-{index}")).unwrap();
        }
        let sums = scratch.path().join("SHA256SUMS");
        write_release_checksums(&paths, &sums).unwrap();
        let text = std::fs::read_to_string(&sums).unwrap();
        let listed: Vec<_> = text
            .lines()
            .map(|line| line.split("  ").nth(1).unwrap())
            .collect();
        let mut sorted = names.to_vec();
        sorted.sort_unstable();
        assert_eq!(listed, sorted);
        assert!(write_release_checksums(&paths[..7], &sums).is_err());
    }

    #[test]
    fn generated_checksums_require_the_released_installer_scripts() {
        let scratch = tempfile::tempdir().unwrap();
        let assets = scratch.path();
        let contract = load_contract(&repo_root()).unwrap();
        for artifact in &contract.artifacts {
            std::fs::write(assets.join(&artifact.filename), &artifact.id).unwrap();
            if artifact.platform == "darwin" {
                let receipt = format!(
                    "{}.notarization.json",
                    archive_stem(&artifact.filename).unwrap()
                );
                std::fs::write(assets.join(receipt), "{}").unwrap();
            }
        }
        let missing = generate_release_checksums(&repo_root(), assets).unwrap_err();
        assert!(
            missing
                .to_string()
                .contains("Missing installer script: install.sh")
        );
        for script in INSTALLER_ASSETS {
            std::fs::copy(repo_root().join(script), assets.join(script)).unwrap();
        }
        std::fs::write(assets.join("uninstall.sh"), "#!/bin/sh\nexit 0\n").unwrap();
        let changed = generate_release_checksums(&repo_root(), assets).unwrap_err();
        assert!(changed.to_string().contains("uninstall.sh differs"));
        std::fs::copy(
            repo_root().join("uninstall.sh"),
            assets.join("uninstall.sh"),
        )
        .unwrap();
        generate_release_checksums(&repo_root(), assets).unwrap();
        let sums = std::fs::read_to_string(assets.join("SHA256SUMS")).unwrap();
        assert_eq!(sums.lines().count(), 8);
        assert!(sums.contains("  install.sh\n") && sums.contains("  uninstall.sh\n"));
    }

    #[test]
    fn civil_dates_match_known_epochs() {
        assert_eq!(utc_civil(0), (1970, 1, 1, 0, 0, 0));
        assert_eq!(utc_civil(1_767_225_600), (2026, 1, 1, 0, 0, 0));
        assert_eq!(utc_civil(951_782_400 + 3661), (2000, 2, 29, 1, 1, 1));
    }
}
