//! Release workflow preflight: build matrices and the publication context gate.
use crate::release::{archive_stem, load_contract};
use crate::util::read_text;
use anyhow::{Result, ensure};
use serde::Serialize;
use std::path::Path;
use std::process::Command;

#[derive(Clone, Debug, Default)]
pub struct WorkflowContext {
    pub event_name: String,
    pub r#ref: String,
    pub sha: String,
    pub actor: String,
    pub triggering_actor: String,
}

impl WorkflowContext {
    pub fn from_environment() -> Self {
        let var = |name: &str| std::env::var(name).unwrap_or_default();
        Self {
            event_name: var("GITHUB_EVENT_NAME"),
            r#ref: var("GITHUB_REF"),
            sha: var("GITHUB_SHA"),
            actor: var("GITHUB_ACTOR"),
            triggering_actor: var("GITHUB_TRIGGERING_ACTOR"),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MatrixEntry {
    pub artifact_id: String,
    pub filename: String,
    pub stem: String,
    pub rust_target: String,
    pub runner: String,
    pub build_image: String,
    #[serde(rename = "minimumOS")]
    pub minimum_os: String,
    pub archive_format: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct Matrix {
    pub include: Vec<MatrixEntry>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PublicationDecision {
    pub publish: bool,
    pub tag: String,
    pub title: String,
    /// Updated with the generated formula only when `publish` is true.
    pub homebrew_tap: String,
}

pub fn release_matrix(repository_root: &Path, platform: &str) -> Result<Matrix> {
    ensure!(
        platform == "macos" || platform == "linux",
        "matrix requires --platform=macos|linux"
    );
    let wanted = if platform == "macos" {
        "darwin"
    } else {
        "linux"
    };
    let contract = load_contract(repository_root)?;
    let include = contract
        .artifacts
        .iter()
        .filter(|artifact| artifact.platform == wanted)
        .map(|artifact| {
            Ok(MatrixEntry {
                artifact_id: artifact.id.clone(),
                filename: artifact.filename.clone(),
                stem: archive_stem(&artifact.filename)?.to_owned(),
                rust_target: artifact.rust_target.clone(),
                runner: artifact.runner.clone(),
                build_image: artifact.build_image.clone().unwrap_or_default(),
                minimum_os: artifact.minimum_os.clone().unwrap_or_default(),
                archive_format: artifact.archive_format.clone(),
            })
        })
        .collect::<Result<_>>()?;
    Ok(Matrix { include })
}

/// The Cargo workspace version replaces the retired `package.json` version.
fn workspace_version(repository_root: &Path) -> Result<String> {
    let manifest = read_text(&repository_root.join("Cargo.toml"))?;
    let package = manifest
        .split("[workspace.package]")
        .nth(1)
        .unwrap_or_default();
    Ok(package
        .lines()
        .find_map(|line| line.trim().strip_prefix("version = "))
        .unwrap_or_default()
        .trim_matches('"')
        .to_owned())
}

pub fn validate_context(
    repository_root: &Path,
    context: &WorkflowContext,
) -> Result<PublicationDecision> {
    let contract = load_contract(repository_root)?;
    let policy = &contract.policy;
    ensure!(
        workspace_version(repository_root)? == contract.product.version,
        "Cargo workspace version does not match release metadata"
    );
    let head = Command::new("/usr/bin/git")
        .args(["rev-parse", "HEAD"])
        .current_dir(repository_root)
        .output()?;
    ensure!(
        head.status.success() && context.sha == String::from_utf8_lossy(&head.stdout).trim(),
        "Workflow SHA does not match checked out commit"
    );
    let title = format!("# Tide v{} Prerelease Notes", contract.product.version);
    ensure!(
        read_text(&repository_root.join(&policy.notes_path))?
            .split('\n')
            .any(|line| line == title),
        "Release notes title does not match product version"
    );

    let decision = |publish| PublicationDecision {
        publish,
        tag: policy.tag.clone(),
        title: policy.title.clone(),
        homebrew_tap: policy.homebrew_tap.clone(),
    };
    if context.event_name == "workflow_dispatch" {
        return Ok(decision(false));
    }
    ensure!(
        context.event_name == "push",
        "Unsupported release workflow event: {}",
        context.event_name
    );
    ensure!(
        context.r#ref == format!("refs/tags/{}", policy.tag),
        "Only {} may publish this release",
        policy.tag
    );
    ensure!(
        context.actor == policy.publication_actor,
        "Release tag actor is not the configured owner"
    );
    ensure!(
        context.triggering_actor == policy.publication_actor,
        "Release triggering actor is not the configured owner"
    );
    Ok(decision(true))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::release::tests::head_commit;
    use crate::util::repo_root;

    fn context(event: &str, r#ref: &str, actor: &str) -> WorkflowContext {
        WorkflowContext {
            event_name: event.into(),
            r#ref: r#ref.into(),
            sha: head_commit(),
            actor: actor.into(),
            triggering_actor: actor.into(),
        }
    }

    #[test]
    fn derives_both_build_matrices_from_canonical_artifact_metadata() {
        let macos = release_matrix(&repo_root(), "macos").unwrap();
        let linux = release_matrix(&repo_root(), "linux").unwrap();
        let ids = |matrix: &Matrix| -> Vec<String> {
            matrix
                .include
                .iter()
                .map(|item| item.artifact_id.clone())
                .collect()
        };
        assert_eq!(ids(&macos), ["darwin-arm64", "darwin-x64"]);
        assert_eq!(ids(&linux), ["linux-arm64", "linux-x64"]);
        assert!(
            macos
                .include
                .iter()
                .chain(&linux.include)
                .all(|item| item.filename.starts_with("tide-v0.10.0-"))
        );
        let json = serde_json::to_value(&macos).unwrap();
        assert_eq!(json["include"][0]["minimumOS"], "14.0");
        assert!(release_matrix(&repo_root(), "windows").is_err());
    }

    #[test]
    fn manual_workflow_runs_can_verify_but_cannot_publish() {
        let decision = validate_context(
            &repo_root(),
            &context("workflow_dispatch", "refs/heads/main", "thekeith"),
        )
        .unwrap();
        assert!(!decision.publish);
        assert_eq!(decision.tag, "v0.10.0");
        let json = serde_json::to_value(&decision).unwrap();
        assert_eq!(json["publish"], false);
        assert_eq!(json["homebrewTap"], "ultradotdev/homebrew-tap");
    }

    #[test]
    fn publication_requires_the_exact_owner_created_tag_context() {
        let wrong_tag = validate_context(
            &repo_root(),
            &context("push", "refs/tags/v0.10.1", "thekeith"),
        )
        .unwrap_err();
        assert!(wrong_tag.to_string().contains("Only v0.10.0"));
        let wrong_actor = validate_context(
            &repo_root(),
            &context("push", "refs/tags/v0.10.0", "someone-else"),
        )
        .unwrap_err();
        assert!(wrong_actor.to_string().contains("actor"));
        assert_eq!(
            validate_context(
                &repo_root(),
                &context("push", "refs/tags/v0.10.0", "thekeith")
            )
            .unwrap(),
            PublicationDecision {
                publish: true,
                tag: "v0.10.0".into(),
                title: "Tide v0.10.0".into(),
                homebrew_tap: "ultradotdev/homebrew-tap".into(),
            }
        );
    }
}
