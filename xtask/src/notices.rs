//! `docs/ATTRIBUTIONS.md` from the locked four-platform native dependency graph.
use crate::util::{capture, sha256_hex};
use anyhow::{Context, Result, bail, ensure};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::{Path, PathBuf};
use std::process::Command;

const TARGETS: [&str; 4] = [
    "aarch64-apple-darwin",
    "x86_64-apple-darwin",
    "aarch64-unknown-linux-gnu",
    "x86_64-unknown-linux-gnu",
];
const PREFIXES: [&str; 6] = [
    "LICENSE",
    "LICENCE",
    "COPYING",
    "NOTICE",
    "COPYRIGHT",
    "UNLICENSE",
];

/// Python `str.isspace` for `rstrip`: Unicode whitespace plus the ASCII
/// information separators.
fn python_space(character: char) -> bool {
    character.is_whitespace() || ('\u{1c}'..='\u{1f}').contains(&character)
}

fn rstrip(text: &str) -> &str {
    text.trim_end_matches(python_space)
}

/// Python `str.splitlines()` boundaries.
fn splitlines(text: &str) -> Vec<&str> {
    let mut lines = Vec::new();
    let mut start = 0;
    let mut characters = text.char_indices().peekable();
    while let Some((index, character)) = characters.next() {
        let boundary = matches!(
            character,
            '\n' | '\r'
                | '\u{b}'
                | '\u{c}'
                | '\u{1c}'
                | '\u{1d}'
                | '\u{1e}'
                | '\u{85}'
                | '\u{2028}'
                | '\u{2029}'
        );
        if !boundary {
            continue;
        }
        lines.push(&text[start..index]);
        let mut end = index + character.len_utf8();
        if character == '\r' && characters.peek().is_some_and(|(_, next)| *next == '\n') {
            characters.next();
            end += 1;
        }
        start = end;
    }
    if start < text.len() {
        lines.push(&text[start..]);
    }
    lines
}

/// Python's text-mode read: universal newlines.
fn read_text_universal(path: &Path) -> Result<String> {
    let text = std::fs::read_to_string(path).with_context(|| format!("read {}", path.display()))?;
    Ok(text.replace("\r\n", "\n").replace('\r', "\n"))
}

#[derive(Default)]
struct Notices {
    texts: BTreeMap<String, String>,
}

impl Notices {
    fn add(&mut self, text: &str) -> Result<String> {
        let text = rstrip(&text.replace("\r\n", "\n")).to_owned();
        ensure!(
            !text.contains('\0') && text.len() <= 1_000_000,
            "Invalid notice"
        );
        let digest = sha256_hex(text.as_bytes());
        self.texts.insert(digest.clone(), text);
        Ok(digest)
    }
}

fn links(hashes: &[String]) -> String {
    hashes
        .iter()
        .collect::<BTreeSet<_>>()
        .iter()
        .map(|hash| format!("[{}](#notice-{hash})", &hash[..12]))
        .collect::<Vec<_>>()
        .join(", ")
}

/// Registry and Git packages reachable from `tide` through non-dev edges.
fn dependency_packages(
    root: &Path,
    metadata_prefix: Option<&Path>,
) -> Result<BTreeMap<(String, String), Value>> {
    let mut packages = BTreeMap::new();
    for target in TARGETS {
        let raw = match metadata_prefix {
            Some(prefix) => std::fs::read_to_string(format!("{}{target}.json", prefix.display()))?,
            None => capture(
                Command::new("cargo")
                    .args([
                        "metadata",
                        "--locked",
                        "--offline",
                        "--format-version",
                        "1",
                        "--filter-platform",
                        target,
                    ])
                    .current_dir(root),
            )?,
        };
        let metadata: Value = serde_json::from_str(&raw)?;
        let all = metadata["packages"]
            .as_array()
            .context("metadata packages")?;
        let by_id: HashMap<&str, &Value> = all
            .iter()
            .filter_map(|package| package["id"].as_str().map(|id| (id, package)))
            .collect();
        let nodes: HashMap<&str, &Value> = metadata["resolve"]["nodes"]
            .as_array()
            .context("metadata resolve")?
            .iter()
            .filter_map(|node| node["id"].as_str().map(|id| (id, node)))
            .collect();
        let mut todo: Vec<&str> = all
            .iter()
            .filter(|package| package["name"] == "tide")
            .filter_map(|package| package["id"].as_str())
            .collect();
        let mut seen = BTreeSet::new();
        while let Some(id) = todo.pop() {
            if !seen.insert(id) {
                continue;
            }
            let package = by_id.get(id).context("metadata package")?;
            if package["source"]
                .as_str()
                .is_some_and(|source| !source.is_empty())
            {
                let key = (
                    package["name"].as_str().unwrap_or_default().to_owned(),
                    package["version"].as_str().unwrap_or_default().to_owned(),
                );
                packages.insert(key, (*package).clone());
            }
            for dependency in nodes
                .get(id)
                .and_then(|node| node["deps"].as_array())
                .into_iter()
                .flatten()
            {
                let runtime = dependency["dep_kinds"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .any(|kind| kind["kind"].as_str() != Some("dev"));
                if runtime && let Some(pkg) = dependency["pkg"].as_str() {
                    todo.push(pkg);
                }
            }
        }
    }
    Ok(packages)
}

fn license_files(package: &Value) -> Result<Vec<PathBuf>> {
    let manifest = PathBuf::from(package["manifest_path"].as_str().context("manifest_path")?);
    let directory = manifest.parent().context("crate directory")?.to_path_buf();
    let mut files: Vec<PathBuf> = std::fs::read_dir(&directory)?
        .filter_map(Result::ok)
        .filter(|entry| {
            let upper = entry.file_name().to_string_lossy().to_uppercase();
            PREFIXES.iter().any(|prefix| upper.starts_with(prefix))
                && entry.file_type().is_ok_and(|kind| kind.is_file())
        })
        .map(|entry| entry.path())
        .collect();
    files.sort();
    if let Some(declared) = package["license_file"].as_str() {
        let path = if Path::new(declared).is_absolute() {
            PathBuf::from(declared)
        } else {
            directory.join(declared)
        };
        ensure!(
            path.canonicalize()?.starts_with(directory.canonicalize()?),
            "License file escaped crate"
        );
        ensure!(
            std::fs::symlink_metadata(&path)?.is_file(),
            "Non-regular license file"
        );
        if !files.contains(&path) {
            files.push(path);
        }
    }
    Ok(files)
}

pub struct NoticeInputs<'a> {
    pub metadata_prefix: Option<&'a Path>,
    pub syntax_notices_file: Option<&'a Path>,
}

pub fn render(root: &Path, inputs: &NoticeInputs<'_>) -> Result<(String, usize, usize)> {
    let packages = dependency_packages(root, inputs.metadata_prefix)?;
    let overrides: Value = serde_json::from_str(&read_text_universal(
        &root.join("config/rust-license-overrides.json"),
    )?)?;
    let mut notices = Notices::default();
    let mut rows = Vec::new();
    for ((name, version), package) in &packages {
        let mut hashes = Vec::new();
        for path in license_files(package)? {
            hashes.push(notices.add(&read_text_universal(&path)?)?);
        }
        let key = format!("{name}-{version}");
        if hashes.is_empty() {
            let entries = overrides[&key].as_array().with_context(|| {
                format!("Missing standalone notice for {key}; retain exact pinned upstream license blobs")
            })?;
            for entry in entries {
                let text = entry["text"].as_str().unwrap_or_default();
                ensure!(
                    sha256_hex(text.as_bytes()) == entry["sha256"].as_str().unwrap_or_default(),
                    "Changed upstream notice"
                );
                hashes.push(notices.add(text)?);
            }
        }
        let license = package["license"]
            .as_str()
            .filter(|license| !license.is_empty())
            .unwrap_or("License file");
        rows.push(format!(
            "| `{name}` {version} | {license} | {} |",
            links(&hashes)
        ));
    }
    let themes: Value = serde_json::from_str(&read_text_universal(
        &root.join("config/theme-provenance.json"),
    )?)?;
    let theme_notices: Value = serde_json::from_str(&read_text_universal(
        &root.join("config/theme-license-notices.json"),
    )?)?;
    let mut theme_rows = Vec::new();
    for theme in themes.as_array().into_iter().flatten() {
        let id = theme["themeId"].as_str().unwrap_or_default();
        if id == "ultra-dusk" {
            continue;
        }
        let entries = theme_notices[id]
            .as_array()
            .with_context(|| format!("Missing theme notice for {id}"))?;
        let mut hashes = Vec::new();
        for entry in entries {
            let text = entry["text"].as_str().unwrap_or_default();
            ensure!(
                sha256_hex(text.as_bytes()) == entry["sha256"].as_str().unwrap_or_default(),
                "Changed theme notice"
            );
            hashes.push(notices.add(text)?);
        }
        let attribution = theme["attribution"].as_str().unwrap_or_default();
        theme_rows.push(format!("| {id} | {attribution} | {} |", links(&hashes)));
    }
    let syntax = match inputs.syntax_notices_file {
        Some(path) => read_text_universal(path)?,
        None => capture(
            Command::new("cargo")
                .args([
                    "run",
                    "--locked",
                    "--offline",
                    "--quiet",
                    "--release",
                    "-p",
                    "tide",
                    "--example",
                    "export_reference",
                    "--",
                    "--syntax-notices",
                ])
                .current_dir(root),
        )?,
    };
    ensure!(
        syntax.starts_with("## Embedded syntax definitions\n") && syntax.len() < 1_000_000,
        "Invalid compiled grammar notices"
    );
    let mut result = String::from(HEADER);
    result.push_str(&rows.join("\n"));
    result.push_str("\n\n## Theme palette notices\n\n| Palette | Copyright | Notice text |\n| --- | --- | --- |\n");
    result.push_str(&theme_rows.join("\n"));
    result.push_str("\n\n");
    result.push_str(&syntax);
    result.push_str("\n## License and copyright texts\n\n");
    for (digest, text) in &notices.texts {
        result.push_str(&format!(
            "<a id=\"notice-{digest}\"></a>\n\n### {}\n\n````text\n{text}\n````\n\n",
            &digest[..12]
        ));
    }
    let lines: Vec<&str> = splitlines(&result).into_iter().map(rstrip).collect();
    let output = format!("{}\n", rstrip(&lines.join("\n")));
    Ok((output, packages.len(), notices.texts.len()))
}

const HEADER: &str = "# Tide Attributions

Tide is a native Rust application. This standalone file includes notices from the locked dependency graph for the four macOS/Linux release targets, including build dependencies and platform alternatives. Presence here does not imply every crate is linked on every target. Tide itself is Copyright 2025-2026 Zorz LLC and licensed under the Apache License, Version 2.0.

Native terminal integration uses portable-pty and alacritty_terminal. Ratatui/Crossterm render and route input; Ropey supplies text storage; Syntect and two-face supply syntax grammars.

The thirteen Tide themes retain the upstream palette licenses and provenance recorded in config/theme-provenance.json and docs/THEME_ATTRIBUTION.md. Ultra Dusk is an original Zorz LLC design under Tide's license. Upstream projects do not endorse Tide.

Generated with `cargo xtask generate-rust-notices` from Cargo.lock and the compiled grammar notice exporter. Exact source revisions for missing crate-root notices are retained in config/rust-license-overrides.json.

## Native dependency notices

| Dependency | Declared license | Notice text |
| --- | --- | --- |
";

/// `generate-rust-notices --write|--check`.
pub fn generate(root: &Path, write: bool, inputs: &NoticeInputs<'_>) -> Result<()> {
    let (output, packages, texts) = render(root, inputs)?;
    let path = root.join("docs/ATTRIBUTIONS.md");
    if write {
        std::fs::write(&path, &output)?;
    } else {
        let committed = read_text_universal(&path)?;
        if committed != output {
            let diff = similar::TextDiff::from_lines(&committed, &output);
            let rendered = diff
                .unified_diff()
                .context_radius(2)
                .header("committed", "generated")
                .to_string();
            let shown: Vec<&str> = rendered.lines().take(65).collect();
            println!("{}", shown.join("\n"));
            bail!("Rust notices are stale");
        }
    }
    println!(
        "{} notices for {packages} dependencies, {texts} distinct texts.",
        if write { "Generated" } else { "Verified" }
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splitlines_and_rstrip_follow_python() {
        assert_eq!(splitlines("a\r\nb\rc\u{c}d\n"), ["a", "b", "c", "d"]);
        assert_eq!(splitlines("a\n\nb"), ["a", "", "b"]);
        assert_eq!(rstrip("text \t\u{1f}\n"), "text");
    }

    #[test]
    fn notices_are_normalized_before_hashing() {
        let mut notices = Notices::default();
        let first = notices.add("MIT License\r\n\r\n").unwrap();
        let second = notices.add("MIT License").unwrap();
        assert_eq!(first, second);
        assert_eq!(notices.texts.len(), 1);
        assert!(notices.add("bad\0text").is_err());
    }

    #[test]
    fn notice_links_are_sorted_and_unique() {
        let a = "a".repeat(64);
        let b = "b".repeat(64);
        assert_eq!(
            links(&[b.clone(), a.clone(), b.clone()]),
            format!("[{}](#notice-{a}), [{}](#notice-{b})", &a[..12], &b[..12])
        );
    }
}
