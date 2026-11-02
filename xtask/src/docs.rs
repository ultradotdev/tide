//! Documentation gates: relative links, documented claims and product identity.
use crate::ojson::Json;
use crate::util::{finish, read_text, resolve_lexically, run_inherited, tracked_files};
use anyhow::{Result, bail};
use regex::Regex;
use std::path::{Path, PathBuf};
use std::process::Command;

/// Directories whose Markdown is not documentation: dependencies, build output and
/// test fixtures. Hidden directories other than `.github` are skipped as well.
fn skipped_directory(name: &str) -> bool {
    matches!(name, ".git" | "node_modules" | "dist" | "tests" | "target")
        || (name.starts_with('.') && name != ".github")
}

/// Markdown files whose links are checked: the tracked files when `root` is a Git
/// work tree, otherwise every Markdown file outside skipped directories.
fn markdown_files(root: &Path) -> Result<Vec<PathBuf>> {
    let is_markdown = |path: &Path| {
        path.extension()
            .is_some_and(|ext| ext.eq_ignore_ascii_case("md"))
    };
    if let Some(tracked) = tracked_files(root)? {
        return Ok(tracked
            .iter()
            .map(Path::new)
            .filter(|path| {
                is_markdown(path)
                    && !path
                        .parent()
                        .into_iter()
                        .flat_map(Path::components)
                        .any(|part| skipped_directory(&part.as_os_str().to_string_lossy()))
            })
            .map(|path| root.join(path))
            .filter(|path| path.is_file())
            .collect());
    }
    let mut files = Vec::new();
    let mut pending = vec![root.to_path_buf()];
    while let Some(directory) = pending.pop() {
        let mut entries = std::fs::read_dir(&directory)?.collect::<Result<Vec<_>, _>>()?;
        entries.sort_by_key(std::fs::DirEntry::file_name);
        for entry in entries {
            let path = entry.path();
            if entry.file_type()?.is_dir() {
                if !skipped_directory(&entry.file_name().to_string_lossy()) {
                    pending.push(path);
                }
            } else if is_markdown(&path) {
                files.push(path);
            }
        }
    }
    Ok(files)
}

/// Removes fenced code blocks: examples and verbatim license notices are text.
fn prose(content: &str) -> String {
    let delimiter = Regex::new(r"^\s*(`{3,}|~{3,})").expect("valid fence pattern");
    let mut fence: Option<(char, usize)> = None;
    content
        .split('\n')
        .filter(|line| {
            if let Some(found) = delimiter.captures(line) {
                let run = &found[1];
                let marker = run.chars().next().unwrap_or('`');
                match fence {
                    None => fence = Some((marker, run.len())),
                    Some((open, length)) if open == marker && run.len() >= length => fence = None,
                    Some(_) => {}
                }
                return false;
            }
            fence.is_none()
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// `decodeURIComponent`: rejects malformed escapes and invalid UTF-8.
pub fn percent_decode(value: &str) -> Option<String> {
    let bytes = value.as_bytes();
    let mut output = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' {
            let hex = value.get(index + 1..index + 3)?;
            output.push(u8::from_str_radix(hex, 16).ok()?);
            index += 3;
        } else {
            output.push(bytes[index]);
            index += 1;
        }
    }
    String::from_utf8(output).ok()
}

/// Checks one Markdown file's relative links; returns failure messages.
fn link_failures(root: &Path, file: &Path) -> Result<Vec<String>> {
    let link = Regex::new(r"!?\[[^\]]*\]\(([^)]+)\)")?;
    let external = Regex::new(r"(?i)^(?:https?:|mailto:|tel:|data:)")?;
    let title = Regex::new(r#"\s+["']"#)?;
    let content = read_text(file)?;
    let mut failures = Vec::new();
    let shown = file.display();
    for found in link.captures_iter(&prose(&content)) {
        let trimmed = found[1].trim();
        let trimmed = trimmed.strip_prefix('<').unwrap_or(trimmed);
        let raw = trimmed.strip_suffix('>').unwrap_or(trimmed);
        let destination = title.split(raw).next().unwrap_or_default();
        let destination = destination.split('#').next().unwrap_or_default();
        if destination.is_empty() || external.is_match(destination) {
            continue;
        }
        let Some(decoded) = percent_decode(destination) else {
            failures.push(format!("{shown}: invalid percent encoding in {raw}"));
            continue;
        };
        let target = resolve_lexically(file.parent().unwrap_or(root), Path::new(&decoded));
        if !target.starts_with(root) {
            failures.push(format!("{shown}: link escapes repository: {raw}"));
            continue;
        }
        if std::fs::metadata(&target).is_err() {
            failures.push(format!("{shown}: missing link target: {raw}"));
        }
    }
    Ok(failures)
}

/// `validate-doc-links [root]`.
pub fn validate_doc_links(root: &Path) -> Result<()> {
    let root = std::path::absolute(root)?;
    let files = markdown_files(&root)?;
    let mut failures = Vec::new();
    for file in &files {
        failures.extend(link_failures(&root, file)?);
    }
    if failures.is_empty() {
        println!(
            "Validated relative links in {} Markdown files.",
            files.len()
        );
        return Ok(());
    }
    bail!("{}", failures.join("\n"))
}

/// `verify-tide-docs [root]`.
pub fn verify_tide_docs(root: &Path) -> Result<()> {
    let root = std::path::absolute(root)?;
    let read = |path: &str| read_text(&root.join(path));
    let json = |path: &str| -> Result<Json> { Json::parse(&read(path)?) };
    let product = json("config/product.json")?;
    let release = json("config/release.json")?;
    let runtime = json("docs/reference/runtime.json")?;
    let site = json("docs/site/tide-release.json")?;
    let readme = read("README.md")?;
    let version = product.str_at(&["version"]).to_owned();
    let mut failures: Vec<String> = Vec::new();
    let mut require = |condition: bool, message: &str| {
        if !condition {
            failures.push(message.to_owned());
        }
    };
    require(
        product.str_at(&["name"]) == "Tide"
            && product.str_at(&["binary"]) == "tide"
            && version == crate::util::VERSION,
        "Product identity drifted",
    );
    require(
        runtime.get("schemaVersion").and_then(Json::as_f64) == Some(2.0)
            && runtime.str_at(&["implementation"]) == "rust"
            && runtime.str_at(&["version"]) == version,
        "Compiled Rust reference identity drifted",
    );
    require(
        read("Cargo.toml")?.contains(&format!("version = \"{version}\"")),
        "Cargo workspace version drifted",
    );
    require(
        read("LICENSE")?
            .contains("Apache License\n                           Version 2.0, January 2004")
            && read("NOTICE")?.starts_with("Tide\nCopyright "),
        "LICENSE must be the Apache License 2.0 text and NOTICE must name Tide",
    );
    require(
        readme.starts_with("# Tide by Ultra.dev\n"),
        "README identity drifted",
    );
    let headline = release
        .at(&["website", "headline"])
        .map(Json::display)
        .unwrap_or_default();
    let description = release
        .at(&["website", "description"])
        .map(Json::display)
        .unwrap_or_default();
    require(
        readme.contains(&headline) && readme.contains(&description),
        "README approved copy drifted",
    );
    let published = release.str_at(&["status", "distribution"]) == "published";
    require(
        readme.contains("Public downloads are unavailable.") != published,
        if published {
            "README still says downloads are unavailable"
        } else {
            "Unpublished download boundary is missing"
        },
    );
    let getting_started = read("docs/getting-started.md")?;
    let install_commands: Vec<String> = [
        "homebrewInstall",
        "homebrewUpgrade",
        "homebrewUninstall",
        "binaryInstall",
        "binaryUninstall",
    ]
    .iter()
    .flat_map(|set| {
        release
            .at(&["commands", set, "commands"])
            .map(Json::as_array)
            .unwrap_or_default()
            .iter()
            .map(Json::display)
            .collect::<Vec<_>>()
    })
    .collect();
    require(
        install_commands.len() >= 6
            && install_commands
                .iter()
                .all(|command| readme.contains(command) && getting_started.contains(command)),
        "README or getting started install commands drifted from release metadata",
    );
    let distribution = release.str_at(&["status", "distribution"]);
    require(
        ["in-validation", "implemented-unpublished", "published"].contains(&distribution)
            && release
                .at(&["status", "prerelease"])
                .and_then(Json::as_bool)
                == Some(false),
        "Distribution claim boundary drifted",
    );
    let experimental = release
        .at(&["features", "experimental"])
        .map(Json::as_array)
        .unwrap_or_default();
    let deferred = release
        .at(&["features", "deferred"])
        .map(Json::as_array)
        .unwrap_or_default();
    require(
        experimental.is_empty() && deferred.iter().any(|item| item.str_at(&["id"]) == "wsl"),
        "Excluded/deferred feature boundary drifted",
    );
    let ids = |value: Option<&Json>| {
        let mut ids: Vec<String> = value
            .map(Json::as_array)
            .unwrap_or_default()
            .iter()
            .map(|item| item.str_at(&["id"]).to_owned())
            .collect();
        ids.sort();
        ids
    };
    require(
        ids(runtime.get("externalCliProviders")) == ids(release.get("externalAiClis")),
        "Runtime/release provider inventory drifted",
    );
    require(
        runtime
            .get("themes")
            .map(Json::as_array)
            .unwrap_or_default()
            .len()
            == 13
            && runtime
                .get("settings")
                .map(Json::entries)
                .unwrap_or_default()
                .len()
                > 40,
        "Runtime reference is incomplete",
    );
    let bindings = runtime
        .get("keybindings")
        .map(Json::as_array)
        .unwrap_or_default();
    require(
        bindings.iter().any(|binding| {
            binding.str_at(&["key"]) == "ctrl+k"
                && binding.str_at(&["command"]) == "workbench.commandPalette"
        }),
        "Terminal-safe palette binding is absent",
    );
    let artifacts = site
        .get("artifacts")
        .map(Json::as_array)
        .unwrap_or_default();
    require(
        site.str_at(&["product", "version"]) == version
            && artifacts.iter().all(|artifact| {
                artifact
                    .get("downloadButtonEnabled")
                    .and_then(Json::as_bool)
                    == Some(published && artifact.str_at(&["status"]) == "published")
            }),
        "Site download claim drifted",
    );
    let inputs: Vec<String> = site
        .at(&["source", "inputFiles"])
        .map(Json::as_array)
        .unwrap_or_default()
        .iter()
        .map(Json::display)
        .collect();
    let hash = site.str_at(&["source", "contentHash"]);
    require(
        inputs
            .iter()
            .any(|input| input == "docs/reference/runtime.json")
            && hash.len() == 64
            && hash
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)),
        "Site must fingerprint compiled runtime references",
    );
    if let Err(error) = crate::reference::generate(&root, false) {
        failures.push(format!("Compiled Rust reference is stale: {error}"));
    }
    if let Err(error) = crate::site::generate(&root, false) {
        failures.push(format!("Site package is stale: {error}"));
    }
    finish(
        &failures,
        "Rust runtime references, documentation and site claims agree with the release status.",
    )
}

/// `verify-tide-identity`.
pub fn verify_tide_identity(root: &Path) -> Result<()> {
    let product = Json::parse(&read_text(&root.join("config/product.json"))?)?;
    let reference = Json::parse(&read_text(&root.join("docs/reference/runtime.json"))?)?;
    let version = product.str_at(&["version"]);
    if product.str_at(&["name"]) != "Tide"
        || product.str_at(&["binary"]) != "tide"
        || version != crate::util::VERSION
        || reference.str_at(&["version"]) != version
        || reference.str_at(&["implementation"]) != "rust"
    {
        bail!("Product identity mismatch");
    }
    if !read_text(&root.join("Cargo.toml"))?.contains(&format!("version = \"{version}\"")) {
        bail!("Cargo version mismatch");
    }
    println!("Tide identity verified.");
    Ok(())
}

/// `validate`: formatting, strict Clippy and the documentation gate.
pub fn validate(root: &Path) -> Result<()> {
    run_inherited(
        Command::new("cargo")
            .args(["fmt", "--all", "--", "--check"])
            .current_dir(root),
    )?;
    run_inherited(
        Command::new("cargo")
            .args([
                "clippy",
                "--locked",
                "--workspace",
                "--all-targets",
                "--",
                "-D",
                "warnings",
            ])
            .current_dir(root),
    )?;
    verify_tide_docs(root)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn verify(markdown: &str) -> Result<()> {
        let root = tempfile::tempdir()?;
        std::fs::write(root.path().join("README.md"), markdown)?;
        validate_doc_links(root.path())
    }

    #[test]
    fn verbatim_examples_and_license_blocks_do_not_create_repository_links() {
        verify("````text\n[upstream](LICENSE.txt)\n```\n[still literal](missing.md)\n````\n")
            .unwrap();
        verify("~~~md\n[example](missing.md)\n~~~\n").unwrap();
    }

    #[test]
    fn authored_missing_links_still_fail_after_a_fenced_example() {
        assert!(verify("```text\n[literal](not-a-link)\n```\n[broken](missing.md)\n").is_err());
        assert!(verify("[escape](../outside.md)\n").is_err());
    }

    #[test]
    fn existing_anchored_titled_and_external_links_pass() {
        let root = tempfile::tempdir().unwrap();
        std::fs::create_dir(root.path().join("docs")).unwrap();
        std::fs::write(root.path().join("docs/guide one.md"), "# Guide\n").unwrap();
        std::fs::write(
            root.path().join("README.md"),
            "[a](docs/guide%20one.md#top) [b](<docs/guide one.md>) [c](https://example.com/x) [d](docs/guide%20one.md \"Title\")\n",
        )
        .unwrap();
        validate_doc_links(root.path()).unwrap();
        std::fs::write(root.path().join("README.md"), "[bad](docs/%zz.md)\n").unwrap();
        let error = validate_doc_links(root.path()).unwrap_err().to_string();
        assert!(error.contains("invalid percent encoding"));
    }

    #[test]
    fn repository_documentation_links_resolve() {
        validate_doc_links(&crate::util::repo_root()).unwrap();
    }

    #[test]
    fn percent_decoding_matches_decode_uri_component() {
        assert_eq!(percent_decode("a%20b").as_deref(), Some("a b"));
        assert_eq!(percent_decode("%E2%80%93").as_deref(), Some("–"));
        assert!(percent_decode("%ff").is_none());
        assert!(percent_decode("%2").is_none());
    }
}
