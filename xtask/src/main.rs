//! `cargo xtask`: Tide's maintainer tooling.
//!
//! Each subcommand replaces a former `scripts/*.ts`, `*.py` or `*.sh` tool and
//! keeps its name. Signing, notarization, Docker and GitHub API commands run
//! through injectable runners and are never exercised by the test suite.
mod docs;
mod gates;
mod governance;
mod homebrew;
mod hygiene;
mod install_checks;
mod linux_build;
mod macos_signing;
mod notices;
mod ojson;
mod performance;
mod reference;
mod release;
mod site;
mod util;
mod verify_governance;
mod verify_release;
mod workflow_contract;

use anyhow::{Result, ensure};
use clap::{Args, Parser, Subcommand};
use std::path::PathBuf;
use std::process::ExitCode;
use std::time::Duration;
use util::{SystemRunner, repo_root};

#[derive(Parser)]
#[command(name = "cargo xtask", about = "Tide maintainer tooling", version)]
struct Cli {
    #[command(subcommand)]
    command: Task,
}

#[derive(Args)]
#[group(required = true, multiple = false)]
struct WriteOrCheck {
    /// Regenerate the committed files.
    #[arg(long)]
    write: bool,
    /// Fail when the committed files are stale.
    #[arg(long)]
    check: bool,
}

#[derive(Args)]
struct ScenarioArgs {
    /// Print scenario names and exit.
    #[arg(long)]
    list: bool,
    /// Run one scenario.
    #[arg(long)]
    scenario: Option<String>,
}

#[derive(Subcommand)]
enum Task {
    /// Prepare, finalize or package a deterministic release payload and archive.
    PackageReleaseArtifact {
        #[arg(value_parser = ["prepare", "finalize", "package"])]
        action: String,
        #[arg(long)]
        artifact_id: String,
        #[arg(long)]
        repository_root: PathBuf,
        #[arg(long)]
        output_directory: PathBuf,
        #[arg(long)]
        source_commit: String,
        #[arg(long)]
        binary: Option<PathBuf>,
        #[arg(long)]
        payload_root: Option<PathBuf>,
    },
    /// Write SHA256SUMS for the four archives, two notarization receipts and installer scripts.
    GenerateReleaseChecksums {
        #[arg(long)]
        repository_root: PathBuf,
        #[arg(long)]
        assets_directory: PathBuf,
    },
    /// Generate the Homebrew tap formula from a release's SHA256SUMS.
    HomebrewFormula {
        /// Release version without the leading `v`.
        #[arg(long)]
        version: String,
        /// The release's SHA256SUMS file.
        #[arg(long)]
        checksums: PathBuf,
        /// Write the formula here instead of standard output.
        #[arg(long)]
        output: Option<PathBuf>,
        /// Archive base URL (default: the repository's GitHub release downloads).
        #[arg(long)]
        download_base_url: Option<String>,
    },
    /// Check the release contract, workflow and packaging/signing tooling.
    VerifyReleaseArtifacts(ScenarioArgs),
    /// Run the native source and installation release gates.
    VerifyReleaseMatrix,
    /// Write a prepared payload as a deterministic unsigned ZIP.
    WriteReleaseZip {
        payload_root: PathBuf,
        output: PathBuf,
    },
    /// Sign, notarize and validate a macOS payload (macOS hosts only).
    SignNotarizeMacos {
        #[arg(long)]
        artifact_id: String,
        #[arg(long)]
        repository_root: PathBuf,
        #[arg(long)]
        payload_root: PathBuf,
        #[arg(long)]
        output_directory: PathBuf,
        #[arg(long)]
        source_commit: String,
        /// Saved notarytool keychain profile for local rehearsal.
        #[arg(long)]
        local_keychain_profile: Option<String>,
        /// Apple team ID for local rehearsal.
        #[arg(long)]
        team_id: Option<String>,
    },
    /// Store a team App Store Connect API key in the local Keychain.
    ConfigureNotaryApiKey {
        #[arg(long)]
        key: PathBuf,
        #[arg(long)]
        key_id: String,
        #[arg(long)]
        issuer: String,
        #[arg(long, default_value = "tide-rust-notary-api")]
        profile: String,
    },
    /// Print release build matrices or validate the publication context.
    ReleaseWorkflowContract {
        #[arg(value_parser = ["matrix", "preflight"])]
        command: String,
        #[arg(long)]
        platform: Option<String>,
        #[arg(long)]
        repository_root: Option<PathBuf>,
    },
    /// Plan, audit or apply GitHub repository governance through `gh api`.
    GithubGovernance {
        #[arg(long, value_enum, default_value = "audit")]
        mode: governance::Mode,
        #[arg(long, default_value = "")]
        confirm_apply: String,
        #[arg(long)]
        repository_root: Option<PathBuf>,
    },
    /// Check governance policy, workflows, CODEOWNERS and tooling offline.
    VerifyGithubGovernance(ScenarioArgs),
    /// Generate or check the compiled runtime references.
    GenerateRustReference(WriteOrCheck),
    /// Generate or check the site-agent package.
    GenerateTideSitePackage(WriteOrCheck),
    /// Check documented claims against the compiled runtime and release metadata.
    VerifyTideDocs { root: Option<PathBuf> },
    /// Check relative Markdown links.
    ValidateDocLinks { root: Option<PathBuf> },
    /// Check product identity across metadata, references and Cargo.
    VerifyTideIdentity,
    /// Fail when tracked files contain machine-specific absolute paths.
    VerifyPortablePaths,
    /// Formatting, strict Clippy and the documentation gate.
    Validate,
    /// Two clean release builds must be byte-identical.
    VerifyDeterministicBuild,
    /// Generate or check docs/ATTRIBUTIONS.md.
    GenerateRustNotices {
        #[command(flatten)]
        mode: WriteOrCheck,
        /// Precomputed trusted cargo metadata, for offline generation.
        #[arg(long)]
        metadata_prefix: Option<PathBuf>,
        /// Precomputed trusted compiled syntax notice output.
        #[arg(long)]
        syntax_notices_file: Option<PathBuf>,
    },
    /// Install, roll back or uninstall through the common managed installer.
    InstallRustLocal {
        #[arg(long)]
        prefix: Option<PathBuf>,
        #[arg(long)]
        migrate_legacy: bool,
        #[arg(long, group = "action")]
        binary: Option<PathBuf>,
        #[arg(long, group = "action")]
        rollback: bool,
        #[arg(long, group = "action")]
        uninstall: bool,
    },
    /// Exercise install, upgrade, rollback and removal in a disposable prefix.
    VerifyRustLocalInstall {
        #[arg(long, default_value = "target/debug/tide")]
        debug: PathBuf,
        #[arg(long, default_value = "target/release/tide")]
        release: PathBuf,
    },
    /// Exercise a real release payload and the Linux download bootstrap.
    VerifyRustReleaseInstall {
        #[arg(long)]
        binary: PathBuf,
    },
    /// Measure editor input latency, palette latency, startup and idle CPU in a PTY.
    MeasureRustPerformance {
        #[arg(long, default_value = "target/release/tide")]
        binary: PathBuf,
        #[arg(long, default_value_t = 100_000)]
        lines: usize,
        #[arg(long, default_value_t = 100)]
        samples: usize,
        #[arg(long, default_value_t = 20)]
        startup_samples: usize,
        #[arg(long, default_value_t = 2.0)]
        response_timeout: f64,
        #[arg(long)]
        max_p95_ms: Option<f64>,
        #[arg(long)]
        max_p99_ms: Option<f64>,
        #[arg(long)]
        max_startup_ms: Option<f64>,
        #[arg(long)]
        max_idle_cpu: Option<f64>,
    },
    /// Record exact build provenance and sizes for a candidate archive.
    RecordRustArtifact {
        #[arg(long)]
        binary: PathBuf,
        #[arg(long)]
        artifact_directory: PathBuf,
        #[arg(long)]
        target: String,
        #[arg(long)]
        source_commit: String,
    },
    /// The full readiness gate against disposable database and LSP fixtures.
    VerifyRustTestReadiness,
    /// Build a Linux release binary in an isolated Ubuntu 22.04 container.
    BuildRustLinuxRelease {
        target: String,
        output: PathBuf,
        #[arg(long)]
        repeat: bool,
    },
}

fn main() -> ExitCode {
    match run(Cli::parse().command) {
        Ok(code) => code,
        Err(error) => {
            eprintln!("error: {error:#}");
            ExitCode::FAILURE
        }
    }
}

fn run(task: Task) -> Result<ExitCode> {
    let root = repo_root();
    let cwd_or_root = |path: Option<PathBuf>| path.unwrap_or_else(|| root.clone());
    match task {
        Task::PackageReleaseArtifact {
            action,
            artifact_id,
            repository_root,
            output_directory,
            source_commit,
            binary,
            payload_root,
        } => release::package_release_artifact(
            &release::PackageArgs {
                action,
                repository_root,
                artifact_id,
                output_directory,
                source_commit,
                binary,
                payload_root,
            },
            &mut SystemRunner,
        )?,
        Task::GenerateReleaseChecksums {
            repository_root,
            assets_directory,
        } => release::generate_release_checksums(&repository_root, &assets_directory)?,
        Task::HomebrewFormula {
            version,
            checksums,
            output,
            download_base_url,
        } => homebrew::generate(
            &root,
            &version,
            &checksums,
            output.as_deref(),
            download_base_url.as_deref(),
        )?,
        Task::VerifyReleaseArtifacts(args) => {
            verify_release::verify_release_artifacts(&root, args.list, args.scenario.as_deref())?;
        }
        Task::VerifyReleaseMatrix => verify_release::verify_release_matrix(&root)?,
        Task::WriteReleaseZip {
            payload_root,
            output,
        } => release::write_release_zip(&payload_root, &output)?,
        Task::SignNotarizeMacos {
            artifact_id,
            repository_root,
            payload_root,
            output_directory,
            source_commit,
            local_keychain_profile,
            team_id,
        } => {
            ensure!(
                cfg!(target_os = "macos"),
                "macOS signing requires a macOS host"
            );
            ensure!(
                local_keychain_profile.is_some() == team_id.is_some(),
                "Local signing requires both --local-keychain-profile and --team-id"
            );
            let credentials = if local_keychain_profile.is_some() {
                macos_signing::CredentialSource::LocalKeychain(
                    macos_signing::require_local_keychain_credentials(
                        team_id.as_deref(),
                        local_keychain_profile.as_deref(),
                    )?,
                )
            } else {
                macos_signing::CredentialSource::Release(
                    macos_signing::require_release_credentials(|name| std::env::var(name).ok())?,
                )
            };
            let result = macos_signing::sign_and_notarize(
                &macos_signing::SigningOptions {
                    repository_root: &repository_root,
                    artifact_id: &artifact_id,
                    payload_root: &payload_root,
                    output_directory: &output_directory,
                    source_commit: &source_commit,
                    credentials,
                    temporary_root: None,
                },
                &mut SystemRunner,
            )?;
            println!(
                "{}",
                serde_json::json!({ "archive": result.archive, "receipt": result.receipt_path })
            );
        }
        Task::ConfigureNotaryApiKey {
            key,
            key_id,
            issuer,
            profile,
        } => {
            ensure!(
                cfg!(target_os = "macos"),
                "Apple Keychain setup requires macOS."
            );
            let command =
                macos_signing::configure_notary_key_arguments(&key, &key_id, &issuer, &profile)?;
            // Key contents stay with the owner and Apple tooling; no copies or tracing.
            util::run_inherited(std::process::Command::new(&command[0]).args(&command[1..]))?;
        }
        Task::ReleaseWorkflowContract {
            command,
            platform,
            repository_root,
        } => {
            let repository = cwd_or_root(repository_root);
            if command == "matrix" {
                let matrix = workflow_contract::release_matrix(
                    &repository,
                    platform.as_deref().unwrap_or_default(),
                )?;
                println!("{}", serde_json::to_string(&matrix)?);
            } else {
                let decision = workflow_contract::validate_context(
                    &repository,
                    &workflow_contract::WorkflowContext::from_environment(),
                )?;
                println!("{}", serde_json::to_string(&decision)?);
            }
        }
        Task::GithubGovernance {
            mode,
            confirm_apply,
            repository_root,
        } => governance::run(&cwd_or_root(repository_root), mode, &confirm_apply)?,
        Task::VerifyGithubGovernance(args) => {
            verify_governance::verify(&root, args.list, args.scenario.as_deref())?;
        }
        Task::GenerateRustReference(mode) => reference::generate(&root, mode.write)?,
        Task::GenerateTideSitePackage(mode) => site::generate(&root, mode.write)?,
        Task::VerifyTideDocs { root: tree } => docs::verify_tide_docs(&cwd_or_root(tree))?,
        Task::ValidateDocLinks { root: tree } => {
            docs::validate_doc_links(&tree.unwrap_or(std::env::current_dir()?))?;
        }
        Task::VerifyTideIdentity => docs::verify_tide_identity(&root)?,
        Task::VerifyPortablePaths => hygiene::verify_portable_paths(&root)?,
        Task::Validate => docs::validate(&root)?,
        Task::VerifyDeterministicBuild => gates::verify_deterministic_build(&root)?,
        Task::GenerateRustNotices {
            mode,
            metadata_prefix,
            syntax_notices_file,
        } => notices::generate(
            &root,
            mode.write,
            &notices::NoticeInputs {
                metadata_prefix: metadata_prefix.as_deref(),
                syntax_notices_file: syntax_notices_file.as_deref(),
            },
        )?,
        Task::InstallRustLocal {
            prefix,
            migrate_legacy,
            binary,
            rollback,
            uninstall,
        } => {
            let action = match (binary, rollback, uninstall) {
                (Some(binary), false, false) => install_checks::LocalAction::Binary(binary),
                (None, true, false) => install_checks::LocalAction::Rollback,
                (None, false, true) => install_checks::LocalAction::Uninstall,
                _ => anyhow::bail!("Choose exactly one of --binary, --rollback or --uninstall"),
            };
            let prefix = prefix.unwrap_or_else(|| {
                PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(".local")
            });
            let code = install_checks::install_local(&prefix, &action, migrate_legacy)?;
            return Ok(ExitCode::from(u8::try_from(code).unwrap_or(1)));
        }
        Task::VerifyRustLocalInstall { debug, release } => {
            install_checks::verify_local_install(&debug, &release)?
        }
        Task::VerifyRustReleaseInstall { binary } => {
            install_checks::verify_release_install(&binary)?
        }
        Task::MeasureRustPerformance {
            binary,
            lines,
            samples,
            startup_samples,
            response_timeout,
            max_p95_ms,
            max_p99_ms,
            max_startup_ms,
            max_idle_cpu,
        } => performance::run(&performance::PerformanceArgs {
            binary,
            lines,
            samples,
            startup_samples,
            response_timeout: Duration::from_secs_f64(response_timeout),
            budgets: performance::Budgets {
                max_p95_ms,
                max_p99_ms,
                max_startup_ms,
                max_idle_cpu,
            },
        })?,
        Task::RecordRustArtifact {
            binary,
            artifact_directory,
            target,
            source_commit,
        } => gates::record_artifact(&binary, &artifact_directory, &target, &source_commit)?,
        Task::VerifyRustTestReadiness => gates::verify_test_readiness(&root)?,
        Task::BuildRustLinuxRelease {
            target,
            output,
            repeat,
        } => {
            linux_build::build(&target, &output, repeat, &mut linux_build::StreamingRunner)?;
        }
    }
    Ok(ExitCode::SUCCESS)
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::CommandFactory;

    #[test]
    fn the_cli_is_well_formed_and_lists_every_former_script() {
        Cli::command().debug_assert();
        let names: Vec<String> = Cli::command()
            .get_subcommands()
            .map(|command| command.get_name().to_owned())
            .collect();
        for former in [
            "package-release-artifact",
            "generate-release-checksums",
            "homebrew-formula",
            "verify-release-artifacts",
            "verify-release-matrix",
            "write-release-zip",
            "sign-notarize-macos",
            "configure-notary-api-key",
            "release-workflow-contract",
            "github-governance",
            "verify-github-governance",
            "generate-rust-reference",
            "generate-tide-site-package",
            "verify-tide-docs",
            "validate-doc-links",
            "verify-tide-identity",
            "verify-portable-paths",
            "validate",
            "verify-deterministic-build",
            "generate-rust-notices",
            "install-rust-local",
            "verify-rust-local-install",
            "verify-rust-release-install",
            "measure-rust-performance",
            "record-rust-artifact",
            "verify-rust-test-readiness",
            "build-rust-linux-release",
        ] {
            assert!(names.contains(&former.to_owned()), "missing {former}");
        }
    }

    #[test]
    fn workflow_style_equals_arguments_parse() {
        let cli = Cli::try_parse_from([
            "xtask",
            "package-release-artifact",
            "package",
            "--artifact-id=linux-x64",
            "--repository-root=/repo",
            "--binary=/build/tide",
            "--output-directory=/out",
            "--source-commit=abc",
        ])
        .unwrap();
        assert!(matches!(cli.command, Task::PackageReleaseArtifact { .. }));
        assert!(Cli::try_parse_from(["xtask", "generate-rust-reference"]).is_err());
        assert!(
            Cli::try_parse_from(["xtask", "generate-rust-reference", "--write", "--check"])
                .is_err()
        );
    }
}
