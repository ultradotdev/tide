//! The runtime reference and the help-template documentation, generated from the compiled runtime.
use crate::ojson::Json;
use crate::util::{capture, read_json};
use anyhow::{Result, bail, ensure};
use std::path::Path;
use std::process::Command;

pub fn export_reference(root: &Path, option: Option<&str>) -> Result<String> {
    capture(
        Command::new("cargo")
            .args([
                "run",
                "--locked",
                "--quiet",
                "--release",
                "-p",
                "tide",
                "--example",
                "export_reference",
            ])
            .args(option.map(|option| ["--", option]).into_iter().flatten())
            .current_dir(root),
    )
}

/// Renders the runtime reference file from the exporter's JSON.
pub fn render(raw: &str, product_version: &str) -> Result<Vec<(String, String)>> {
    let reference = Json::parse(raw)?;
    ensure!(
        reference.get("schemaVersion").and_then(Json::as_f64) == Some(2.0)
            && reference.str_at(&["implementation"]) == "rust"
            && reference.str_at(&["version"]) == product_version,
        "Rust reference identity does not match product metadata"
    );
    Ok(vec![(
        "docs/reference/runtime.json".to_owned(),
        format!("{}\n", reference.pretty()),
    )])
}

/// The documentation pages rendered from the in-app help templates with default
/// keybindings and settings, as `(repository path, Markdown)`.
pub fn render_help_docs(raw: &str) -> Result<Vec<(String, String)>> {
    let docs = Json::parse(raw)?;
    let pages = docs
        .entries()
        .iter()
        .map(|(path, text)| (path.clone(), text.display()))
        .collect::<Vec<_>>();
    ensure!(
        !pages.is_empty()
            && pages.iter().all(|(path, text)| {
                path.starts_with("docs/") && path.ends_with(".md") && !text.contains("{{")
            }),
        "Help documentation export is invalid"
    );
    Ok(pages)
}

/// `generate-rust-reference --write|--check`.
pub fn generate(root: &Path, write: bool) -> Result<()> {
    let product: serde_json::Value = read_json(&root.join("config/product.json"))?;
    let version = product["version"].as_str().unwrap_or_default();
    let raw = export_reference(root, None)?;
    let mut outputs = render(&raw, version)?;
    let help = render_help_docs(&export_reference(root, Some("--help-docs"))?)?;
    let pages = help.len();
    outputs.extend(help);
    for (name, contents) in &outputs {
        let path = root.join(name);
        if write {
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::write(&path, contents)?;
        } else if std::fs::read_to_string(&path).ok().as_deref() != Some(contents.as_str()) {
            bail!(
                "Generated Rust reference is stale: {name}; run `cargo xtask generate-rust-reference --write`"
            );
        }
    }
    let reference = Json::parse(&raw)?;
    println!(
        "{} Rust reference: {} command IDs, {} effective bindings, {} settings, {pages} help pages.",
        if write { "Generated" } else { "Verified" },
        reference
            .get("commands")
            .map(Json::as_array)
            .unwrap_or_default()
            .len(),
        reference
            .get("keybindings")
            .map(Json::as_array)
            .unwrap_or_default()
            .len(),
        reference
            .get("settings")
            .map(Json::entries)
            .unwrap_or_default()
            .len(),
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"{"schemaVersion":2,"version":"0.10.0","implementation":"rust",
      "commands":[],"aliases":{},"keybindings":[],"settings":{},
      "externalCliProviders":[],"themes":[]}"#;

    #[test]
    fn renders_the_runtime_reference() {
        let outputs = render(SAMPLE, "0.10.0").unwrap();
        assert_eq!(outputs.len(), 1);
        assert_eq!(outputs[0].0, "docs/reference/runtime.json");
        assert!(
            outputs[0]
                .1
                .starts_with("{\n  \"schemaVersion\": 2,\n  \"version\": \"0.10.0\"")
        );
    }

    #[test]
    fn rejects_a_reference_for_another_version() {
        assert!(render(SAMPLE, "0.11.0").is_err());
    }

    #[test]
    fn help_docs_must_be_resolved_markdown_under_docs() {
        let pages =
            render_help_docs(r##"{"docs/welcome.md":"# Welcome\n","docs/x.md":"ok"}"##).unwrap();
        assert_eq!(pages.len(), 2);
        assert_eq!(
            pages[0],
            ("docs/welcome.md".to_owned(), "# Welcome\n".to_owned())
        );
        for invalid in [
            r#"{}"#,
            r#"{"README.md":"x"}"#,
            r#"{"docs/a.md":"{{key:file.save}}"}"#,
        ] {
            assert!(render_help_docs(invalid).is_err(), "{invalid}");
        }
    }
}
