//! `verify-release-artifacts` scenarios and the `verify-release-matrix` gate.
use crate::release::{archive_stem, load_contract};
use crate::util::read_text;
use crate::verify_governance::{Scenario, run_scenarios};
use anyhow::{Result, ensure};
use regex::Regex;
use std::path::Path;
use std::process::Command;

fn compact(text: &str) -> String {
    text.chars()
        .filter(|character| !character.is_whitespace())
        .collect()
}

pub fn verify_release_artifacts(root: &Path, list: bool, scenario: Option<&str>) -> Result<()> {
    let workflow = read_text(&root.join(".github/workflows/release.yml"))?;
    // Source checks ignore formatting: rustfmt may wrap any call.
    let signing = compact(&read_text(&root.join("xtask/src/macos_signing.rs"))?);
    let packaging = compact(&read_text(&root.join("xtask/src/release.rs"))?);
    let entitlements = read_text(&root.join("config/macos-release-entitlements.plist"))?;
    let contract = load_contract(root)?;
    let version = contract.product.version.clone();

    let scenarios: Vec<Scenario<'_>> = vec![
        (
            "release-tag-version-contract",
            Box::new(|| {
                ensure!(version == "0.10.0", "Product version drifted");
                ensure!(contract.policy.tag == "v0.10.0", "Release tag drifted");
                ensure!(
                    contract.policy.title == "Tide v0.10.0",
                    "Release title drifted"
                );
                Ok(())
            }),
        ),
        (
            "cross-platform-artifact-matrix",
            Box::new(|| {
                ensure!(
                    contract.artifacts.len() == 4,
                    "Exactly four platform artifacts are required"
                );
                let targets: std::collections::BTreeSet<String> = contract
                    .artifacts
                    .iter()
                    .map(|item| format!("{}-{}", item.platform, item.architecture))
                    .collect();
                ensure!(targets.len() == 4, "Artifact targets are not unique");
                Ok(())
            }),
        ),
        (
            "archive-layout-and-permissions",
            Box::new(|| {
                for item in &contract.artifacts {
                    ensure!(
                        archive_stem(&item.filename)?
                            == format!("tide-v{version}-{}-{}", item.platform, item.architecture),
                        "Archives must use their filename stem as root"
                    );
                }
                for name in [
                    "tide",
                    "uninstall.sh",
                    "LICENSE",
                    "ATTRIBUTIONS.md",
                    "VERSION.json",
                ] {
                    ensure!(
                        packaging.contains(&format!("\"{name}\"")),
                        "Packaging does not enforce {name}"
                    );
                }
                ensure!(
                    packaging.contains("0o755") && packaging.contains("0o644"),
                    "Archive permission normalization is absent"
                );
                Ok(())
            }),
        ),
        (
            "deterministic-linux-packaging",
            Box::new(|| {
                ensure!(
                    packaging.contains("--sort=name")
                        && packaging.contains("--owner=0")
                        && packaging.contains("--group=0"),
                    "Linux tar determinism flags are incomplete"
                );
                Ok(())
            }),
        ),
        (
            "deterministic-unsigned-macos-payload",
            Box::new(|| {
                ensure!(
                    packaging.contains("source_commit_timestamp("),
                    "Unsigned payload timestamps are not source-controlled"
                );
                ensure!(
                    packaging.contains("write_release_zip(payload_root,&archive)"),
                    "Deterministic ZIP construction is not used"
                );
                Ok(())
            }),
        ),
        (
            "macos-signing-order",
            Box::new(|| {
                let remove = signing.find("remove_signature_arguments(&binary)");
                let sign = signing.find("runner.run_plain(&codesign_arguments(&identity,&binary");
                ensure!(
                    remove.is_some() && sign > remove,
                    "An existing signature must be removed before Rust Tide is signed"
                );
                ensure!(
                    !signing.contains("tide.runtime"),
                    "Rust signing must not expect a PTY sidecar"
                );
                ensure!(
                    signing.contains("validate_signed_entitlements("),
                    "Executable entitlement verification is missing"
                );
                ensure!(
                    signing.rfind("smoke_extracted_application(&binary")
                        < signing.rfind("refresh_prepared_manifest(context"),
                    "Signed payload smoke must run before archive creation and notarization"
                );
                ensure!(
                    !entitlements.contains("<key>"),
                    "Rust release must have no JIT or other executable entitlements"
                );
                Ok(())
            }),
        ),
        (
            "macos-credential-fail-closed",
            Box::new(|| {
                ensure!(
                    signing.contains("require_release_credentials(")
                        && signing.contains("MissingrequiredApplereleasecredentials"),
                    "CI credentials must fail closed"
                );
                ensure!(
                    signing.contains("require_local_keychain_credentials(")
                        && signing.contains("Exactlyonenotarizationcredentialsource")
                        && signing.contains("pubenumCredentialSource"),
                    "Local rehearsal credentials must be explicit and exclusive"
                );
                Ok(())
            }),
        ),
        (
            "macos-notarization-acceptance",
            Box::new(|| {
                ensure!(
                    signing.contains("status==Some(\"Accepted\")"),
                    "Notarization does not require Accepted"
                );
                ensure!(
                    signing.contains("\"notarytool\",\"submit\""),
                    "notarytool submit is missing"
                );
                Ok(())
            }),
        ),
        (
            "macos-notary-ticket-provenance",
            Box::new(|| {
                ensure!(
                    signing.contains("\"notarytool\",\"log\"")
                        && signing.contains("validate_notary_log("),
                    "Accepted notarization must be verified against the Apple notary log"
                );
                ensure!(
                    signing.contains("archive_sha256")
                        && signing.contains("parse_code_directory_hash("),
                    "Notary ticket validation must bind the archive hash and executable cdhashes"
                );
                ensure!(
                    signing.contains(
                        "notary_ticket_validation:\"archive-sha256-and-executable-cdhashes\""
                    ),
                    "Receipt does not record notary ticket provenance validation"
                );
                ensure!(
                    signing.contains(
                        "runtime_validation:\"strict-signature-and-functional-cli-smoke\""
                    ),
                    "Receipt does not record extracted CLI runtime validation"
                );
                Ok(())
            }),
        ),
        (
            "macos-standalone-cli-gatekeeper-boundary",
            Box::new(|| {
                ensure!(
                    !signing.contains("/usr/sbin/spctl"),
                    "spctl must not assess a standalone CLI as an app bundle"
                );
                ensure!(
                    !signing.contains("com.apple.quarantine"),
                    "The signing process must not manufacture quarantine metadata or launch a quarantined CLI"
                );
                ensure!(
                signing.contains("argv![\"/usr/bin/codesign\",\"--verify\",\"--strict\",\"--verbose=2\",extracted_binary]"),
                "Extracted Tide signature verification is missing"
            );
                ensure!(
                    signing.contains("smoke_extracted_application(&extracted_binary"),
                    "Extracted Tide functional smoke is missing"
                );
                Ok(())
            }),
        ),
        (
            "release-checksums",
            Box::new(|| {
                ensure!(
                    contract.policy.checksums_filename == "SHA256SUMS",
                    "Checksum filename drifted"
                );
                ensure!(
                    workflow.contains("generate-release-checksums"),
                    "Final checksum generation is missing"
                );
                ensure!(
                    workflow.contains("-name '*.notarization.json'")
                        && workflow.contains("darwin-arm64.notarization.json")
                        && workflow.contains("darwin-x64.notarization.json"),
                    "Public notarization receipts are not checksummed and published"
                );
                Ok(())
            }),
        ),
        (
            "github-attestation-contract",
            Box::new(|| {
                ensure!(
                    contract.policy.attestation == "github-artifact-attestation",
                    "Attestation must be required"
                );
                ensure!(
                    workflow.contains("actions/attest@1e69f48acb82d1966a394da916b4c1698aa569d6"),
                    "Pinned official attestation action is missing"
                );
                Ok(())
            }),
        ),
        (
            "manual-run-no-publication",
            Box::new(|| {
                ensure!(
                    !contract.policy.manual_runs_may_publish,
                    "Manual publication is enabled"
                );
                ensure!(
                    workflow.contains("github.event_name == 'push'"),
                    "Publication jobs are not restricted to tag pushes"
                );
                Ok(())
            }),
        ),
        (
            "existing-release-refusal",
            Box::new(|| {
                ensure!(
                    workflow.contains("existing publication")
                        && workflow.contains("gh release view"),
                    "Existing release refusal step is missing"
                );
                ensure!(
                    !contract.policy.replace_existing_assets,
                    "Asset replacement must be disabled"
                );
                Ok(())
            }),
        ),
        (
            "immutable-action-pins",
            Box::new(|| {
                ensure!(
                    action_refs_are_pinned(&workflow)?,
                    "Every action must be pinned to a commit SHA"
                );
                Ok(())
            }),
        ),
    ];
    run_scenarios("release artifact", &scenarios, list, scenario)
}

/// True when the workflow uses actions and every `uses:` ref is a commit SHA.
pub fn action_refs_are_pinned(workflow: &str) -> Result<bool> {
    let uses = Regex::new(r"uses:\s+[^@\s]+@(\S+)")?;
    let sha = Regex::new("^[a-f0-9]{40}$")?;
    let refs: Vec<String> = uses
        .captures_iter(workflow)
        .map(|found| found[1].to_owned())
        .collect();
    Ok(!refs.is_empty() && refs.iter().all(|reference| sha.is_match(reference)))
}

/// `verify-release-matrix`: native source gates. Signed platform artifacts and
/// human approval are separate gates.
pub fn verify_release_matrix(root: &Path) -> Result<()> {
    let xtask = std::env::current_exe()?;
    let cargo = |args: &[&str]| {
        let mut command = Command::new("cargo");
        command.args(args);
        command
    };
    let tool = |args: &[&str]| {
        let mut command = Command::new(&xtask);
        command.args(args);
        command
    };
    let commands = [
        cargo(&["fmt", "--all", "--", "--check"]),
        cargo(&[
            "clippy",
            "--locked",
            "--workspace",
            "--all-targets",
            "--",
            "-D",
            "warnings",
        ]),
        cargo(&["test", "--locked", "--workspace"]),
        tool(&["verify-tide-docs"]),
        tool(&["validate-doc-links"]),
        cargo(&["build", "--locked", "--release", "-p", "tide"]),
        tool(&["verify-rust-local-install"]),
        tool(&[
            "verify-rust-release-install",
            "--binary",
            "target/release/tide",
        ]),
        tool(&["verify-release-artifacts"]),
    ];
    for mut command in commands {
        let shown: Vec<String> = std::iter::once(command.get_program())
            .chain(command.get_args())
            .map(|part| part.to_string_lossy().into_owned())
            .collect();
        println!("Checking {}", shown.join(" "));
        let status = command.current_dir(root).status()?;
        ensure!(status.success(), "{} failed with {status}", shown.join(" "));
    }
    println!(
        "Native source and installation gates passed. Platform signing, publication authority and human acceptance remain separate."
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::util::repo_root;

    #[test]
    fn repository_release_contract_scenarios_pass() {
        verify_release_artifacts(&repo_root(), false, None).unwrap();
        assert!(verify_release_artifacts(&repo_root(), false, Some("missing")).is_err());
    }

    #[test]
    fn unpinned_actions_are_rejected() {
        assert!(
            action_refs_are_pinned(
                "uses: actions/checkout@11d5960a326750d5838078e36cf38b85af677262"
            )
            .unwrap()
        );
        assert!(!action_refs_are_pinned("uses: actions/checkout@v4").unwrap());
        assert!(!action_refs_are_pinned("no actions").unwrap());
    }
}
