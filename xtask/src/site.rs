//! The generated site-agent package under `docs/site`.
use crate::ojson::Json;
use crate::util::{capture, read_text, sha256_hex};
use anyhow::{Result, bail};
use std::path::Path;
use std::process::Command;

pub const INPUTS: [&str; 23] = [
    "config/product.json",
    "config/release.json",
    "README.md",
    "docs/getting-started.md",
    "docs/README.md",
    "docs/features/workbench.md",
    "docs/features/editor.md",
    "docs/features/git.md",
    "docs/features/terminal.md",
    "docs/features/postgresql.md",
    "docs/features/ai.md",
    "docs/features/themes.md",
    "docs/reference/keyboard-shortcuts.md",
    "docs/reference/commands.md",
    "docs/reference/configuration.md",
    "docs/reference/runtime.json",
    "rust-toolchain.toml",
    "Cargo.toml",
    "Cargo.lock",
    "docs/troubleshooting.md",
    "docs/v0.10.0-prerelease.md",
    "install.sh",
    "uninstall.sh",
];

pub fn content_hash(root: &Path) -> Result<String> {
    let mut data = Vec::new();
    for path in INPUTS {
        data.extend_from_slice(path.as_bytes());
        data.push(0);
        data.extend_from_slice(&std::fs::read(root.join(path))?);
    }
    Ok(sha256_hex(&data))
}

/// The commit that last touched the inputs, refreshed only on `--write`. Checks reuse the
/// recorded value: the content hash already proves the package matches its inputs.
fn fingerprint(root: &Path, write: bool) -> Result<String> {
    let existing = root.join("docs/site/tide-release.json");
    if !write && existing.exists() {
        let site = Json::parse(&read_text(&existing)?)?;
        return Ok(site
            .str_at(&["source", "lastVerifiedGitFingerprint"])
            .to_owned());
    }
    Ok(capture(
        Command::new("git")
            .args(["log", "--no-merges", "-1", "--format=%H", "--"])
            .args(INPUTS)
            .current_dir(root),
    )?
    .trim()
    .to_owned())
}

fn text(value: &str) -> Json {
    Json::String(value.to_owned())
}

pub fn render(root: &Path, write: bool) -> Result<Vec<(&'static str, String)>> {
    let product = Json::parse(&read_text(&root.join("config/product.json"))?)?;
    let release = Json::parse(&read_text(&root.join("config/release.json"))?)?;
    let content_hash = content_hash(root)?;
    let fingerprint = fingerprint(root, write)?;
    let field = |key: &str| release.get(key).cloned().unwrap_or(Json::Null);
    let artifacts = Json::Array(
        release
            .get("artifacts")
            .map(Json::as_array)
            .unwrap_or_default()
            .iter()
            .map(|artifact| {
                let mut artifact = artifact.clone();
                artifact.set("downloadButtonEnabled", Json::Bool(false));
                artifact
            })
            .collect(),
    );
    let site = Json::Object(vec![
        (
            "$schema".into(),
            text("../../config/site-release.schema.json"),
        ),
        ("schemaVersion".into(), Json::Number(1.0)),
        ("product".into(), product.clone()),
        ("release".into(), field("status")),
        ("website".into(), field("website")),
        ("platforms".into(), field("platforms")),
        ("artifacts".into(), artifacts),
        ("commands".into(), field("commands")),
        (
            "paths".into(),
            Json::Object(vec![
                ("config".into(), text("${XDG_CONFIG_HOME:-~/.config}/tide/")),
                (
                    "data".into(),
                    text("${XDG_DATA_HOME:-~/.local/share}/tide/"),
                ),
                (
                    "state".into(),
                    text("${XDG_STATE_HOME:-~/.local/state}/tide/"),
                ),
                ("cache".into(), text("${XDG_CACHE_HOME:-~/.cache}/tide/")),
                ("workspace".into(), text("<workspace>/.tide/")),
            ]),
        ),
        ("features".into(), field("features")),
        ("externalAiClis".into(), field("externalAiClis")),
        ("urls".into(), field("urls")),
        (
            "media".into(),
            Json::Object(vec![
                (
                    "filmFixture".into(),
                    text(
                        "Synthetic capture references require current Rust provenance before publication.",
                    ),
                ),
                ("screenshots".into(), Json::Array(Vec::new())),
            ]),
        ),
        (
            "source".into(),
            Json::Object(vec![
                ("contentHash".into(), text(&content_hash)),
                ("lastVerifiedGitFingerprint".into(), text(&fingerprint)),
                (
                    "inputFiles".into(),
                    Json::Array(INPUTS.iter().map(|path| text(path)).collect()),
                ),
            ]),
        ),
    ]);
    let headline = release
        .at(&["website", "headline"])
        .map(Json::display)
        .unwrap_or_default();
    let description = release
        .at(&["website", "description"])
        .map(Json::display)
        .unwrap_or_default();
    let version = product.str_at(&["version"]);
    let guide = format!(
        "# Tide Website Content Guide\n\n## Approved language\n\n**Headline:** {headline}\n\n**Description:** {description}\n\nUse **Tide** as the product name and **Tide by Ultra.dev** as the endorsement. Label version {version} as a prerelease.\n\n## Permitted claims\n\nThe site may claim only features listed in `features.releaseReady` in `tide-release.json`. It may describe the five external AI CLI integrations listed there.\n\n## Prohibited claims\n\n- Do not present downloads or binary install commands as available before distribution is published.\n- Do not add features that are absent from the release-ready inventory.\n- Do not claim the signed artifacts are publicly downloadable before the configured release tag workflow completes.\n- Do not claim WSL support.\n\n## Authoritative fields\n\nTreat product identity, release status, platform support, artifact filenames, commands, paths, feature inventories, provider support, and URLs in `tide-release.json` as authoritative. Download buttons stay disabled until both the artifact and distribution statuses become published.\n\n## Verification\n\nGenerated from source content hash `{content_hash}` and last verified against Git `{fingerprint}`.\n"
    );
    let homepage = release
        .at(&["urls", "homepage"])
        .map(Json::display)
        .unwrap_or_default();
    let repository = release
        .at(&["urls", "repository"])
        .map(Json::display)
        .unwrap_or_default();
    let readme = format!(
        "# Tide Site-Agent Package\n\nThis directory is generated from `config/product.json` and `config/release.json`. It is the machine-readable handoff for the Ultra.dev site agent.\n\n- `tide-release.json` contains authoritative release facts and claim boundaries.\n- `WEBSITE_CONTENT_GUIDE.md` contains approved copy and prohibited claims.\n- Rust artifacts and the common installer are under private validation. Download buttons remain disabled; signing credentials and private acceptance material are excluded from this handoff.\n\n## Installer redirects\n\nThe website does not host installer scripts or version files. Each GitHub release publishes `install.sh` and `uninstall.sh`, covered by its `SHA256SUMS` and build attestation. Configure these HTTPS redirects as temporary redirects (302 or 307), never as HTML pages or permanent (301/308) redirects, so `curl -fsSL` always follows them to the current release:\n\n| Website URL | Redirect to |\n| --- | --- |\n| `{homepage}/install.sh` | `{repository}/releases/latest/download/install.sh` |\n| `{homepage}/uninstall.sh` | `{repository}/releases/latest/download/uninstall.sh` |\n\n`{homepage}/releases/latest-version` is no longer read by the installer and can be retired. GitHub's `latest` URLs skip prereleases: until a release that is not marked as a prerelease exists, they return 404 and the redirects cannot serve an installer. Show the canonical GitHub command from `commands.binaryInstall`; the short website form is `curl -fsSL {homepage}/install.sh | sh`.\n\nRegenerate with `cargo xtask generate-tide-site-package --write`. Verify with `--check`. Do not edit generated files directly.\n"
    );
    Ok(vec![
        (
            "docs/site/tide-release.json",
            format!("{}\n", site.pretty()),
        ),
        ("docs/site/WEBSITE_CONTENT_GUIDE.md", guide),
        ("docs/site/README.md", readme),
    ])
}

/// `generate-tide-site-package --write|--check`.
pub fn generate(root: &Path, write: bool) -> Result<()> {
    for (name, expected) in render(root, write)? {
        let path = root.join(name);
        if write {
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::write(&path, expected)?;
        } else if read_text(&path)? != expected {
            bail!("Generated site artifact is stale: {}", path.display());
        }
    }
    println!(
        "{}",
        if write {
            "Generated Tide site package."
        } else {
            "Tide site package is current."
        }
    );
    Ok(())
}
