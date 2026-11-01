//! Developer ID signing and Apple notarization orchestration.
//!
//! Every external tool (`security`, `codesign`, `xcrun notarytool`, `ditto`)
//! runs through a [`CommandRunner`], so tests record the exact argv vectors
//! without touching a keychain, Apple's notary service or the network.
use crate::argv;
use crate::release::{
    archive_prepared_payload, commit_timestamp, isolated_smoke_env, load_contract,
    refresh_prepared_manifest,
};
use crate::util::{
    CommandRunner, RunOptions, locale_compare, pretty_json, sha256_file, write_private,
};
use anyhow::{Context, Result, ensure};
use base64::Engine as _;
use regex::Regex;
use serde::Serialize;
use serde_json::Value;
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReleaseCredentials {
    pub developer_id_p12_base64: String,
    pub developer_id_p12_password: String,
    pub team_id: String,
    pub notary_api_key_p8_base64: String,
    pub notary_api_key_id: String,
    pub notary_api_issuer_id: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LocalKeychainCredentials {
    pub team_id: String,
    pub notary_keychain_profile: String,
}

/// Exactly one notarization credential source, enforced by the type.
#[derive(Clone, Debug)]
pub enum CredentialSource {
    Release(ReleaseCredentials),
    LocalKeychain(LocalKeychainCredentials),
}

impl CredentialSource {
    fn team_id(&self) -> &str {
        match self {
            Self::Release(credentials) => &credentials.team_id,
            Self::LocalKeychain(credentials) => &credentials.team_id,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct NotaryTicketReceipt {
    pub path: String,
    pub architecture: String,
    pub cdhash: String,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NotarizationReceipt {
    pub schema_version: u8,
    pub artifact_id: String,
    pub source_commit: String,
    pub signing_identity: String,
    pub signing_order: Vec<String>,
    pub hardened_runtime: bool,
    pub secure_timestamp: bool,
    pub executable_entitlements: Vec<String>,
    pub notary_submission_id: String,
    pub notary_status: String,
    pub notary_ticket_validation: String,
    pub ticket_contents: Vec<NotaryTicketReceipt>,
    pub runtime_validation: String,
    pub archive: String,
    pub archive_sha256: String,
    pub notary_log_sha256: String,
}

fn matches(pattern: &str, value: &str) -> bool {
    Regex::new(pattern).is_ok_and(|regex| regex.is_match(value))
}

pub fn require_release_credentials(
    environment: impl Fn(&str) -> Option<String>,
) -> Result<ReleaseCredentials> {
    let fields = [
        ("developerIdP12Base64", "APPLE_DEVELOPER_ID_P12_BASE64"),
        ("developerIdP12Password", "APPLE_DEVELOPER_ID_P12_PASSWORD"),
        ("teamId", "APPLE_TEAM_ID"),
        ("notaryApiKeyP8Base64", "APPLE_NOTARY_API_KEY_P8_BASE64"),
        ("notaryApiKeyId", "APPLE_NOTARY_API_KEY_ID"),
        ("notaryApiIssuerId", "APPLE_NOTARY_API_ISSUER_ID"),
    ];
    let values: Vec<String> = fields
        .iter()
        .map(|(_, variable)| environment(variable).unwrap_or_default())
        .collect();
    let missing: Vec<&str> = fields
        .iter()
        .zip(&values)
        .filter(|(_, value)| value.is_empty())
        .map(|((name, _), _)| *name)
        .collect();
    ensure!(
        missing.is_empty(),
        "Missing required Apple release credentials: {}",
        missing.join(", ")
    );
    let [p12, password, team_id, p8, key_id, issuer] =
        <[String; 6]>::try_from(values).map_err(|_| anyhow::anyhow!("credential count"))?;
    ensure!(
        matches("^[A-Z0-9]{10}$", &team_id),
        "APPLE_TEAM_ID must be a 10-character Apple team ID"
    );
    ensure!(
        matches("^[A-Z0-9]{10}$", &key_id),
        "APPLE_NOTARY_API_KEY_ID must be a 10-character key ID"
    );
    ensure!(
        matches("(?i)^[a-f0-9-]{36}$", &issuer),
        "APPLE_NOTARY_API_ISSUER_ID must be a UUID"
    );
    Ok(ReleaseCredentials {
        developer_id_p12_base64: p12,
        developer_id_p12_password: password,
        team_id,
        notary_api_key_p8_base64: p8,
        notary_api_key_id: key_id,
        notary_api_issuer_id: issuer,
    })
}

pub fn require_local_keychain_credentials(
    team_id: Option<&str>,
    notary_keychain_profile: Option<&str>,
) -> Result<LocalKeychainCredentials> {
    let team_id = team_id.filter(|value| !value.is_empty());
    let profile = notary_keychain_profile.filter(|value| !value.is_empty());
    let team_id = team_id.context("Local macOS signing requires --team-id")?;
    let profile = profile.context("Local macOS signing requires --local-keychain-profile")?;
    ensure!(
        matches("^[A-Z0-9]{10}$", team_id),
        "Local macOS signing team ID must be a 10-character Apple team ID"
    );
    ensure!(
        matches("^[A-Za-z0-9._-]+$", profile),
        "Local notarization profile contains unsupported characters"
    );
    Ok(LocalKeychainCredentials {
        team_id: team_id.to_owned(),
        notary_keychain_profile: profile.to_owned(),
    })
}

pub fn parse_developer_id_identity(output: &str, team_id: &str) -> Result<String> {
    let pattern = Regex::new(r#"^\s*\d+\)\s+[A-F0-9]+\s+"(Developer ID Application:[^"]+)""#)?;
    let suffix = format!("({team_id})");
    let identities: Vec<String> = output
        .split('\n')
        .filter_map(|line| pattern.captures(line).map(|found| found[1].to_owned()))
        .filter(|identity| identity.ends_with(&suffix))
        .collect();
    ensure!(
        identities.len() == 1,
        "Expected exactly one Developer ID Application identity for team {team_id}, found {}",
        identities.len()
    );
    Ok(identities[0].clone())
}

pub fn codesign_arguments(
    identity: &str,
    target: &Path,
    keychain: Option<&Path>,
    entitlements: Option<&Path>,
) -> Vec<String> {
    let mut command = argv![
        "/usr/bin/codesign",
        "--force",
        "--options",
        "runtime",
        "--timestamp"
    ];
    if let Some(keychain) = keychain {
        command.extend(argv!["--keychain", keychain]);
    }
    if let Some(entitlements) = entitlements {
        command.extend(argv!["--entitlements", entitlements]);
    }
    command.extend(argv!["--sign", identity, target]);
    command
}

pub fn remove_signature_arguments(target: &Path) -> Vec<String> {
    argv!["/usr/bin/codesign", "--remove-signature", target]
}

pub fn parse_code_directory_hash(output: &str) -> Result<String> {
    let pattern = Regex::new(r"(?im)^CDHash=([a-f0-9]+)$")?;
    let hashes: Vec<String> = pattern
        .captures_iter(output)
        .map(|found| found[1].to_lowercase())
        .collect();
    ensure!(
        hashes.len() == 1,
        "Expected exactly one code-directory hash, found {}",
        hashes.len()
    );
    Ok(hashes[0].clone())
}

pub struct NotaryLogExpectation<'a> {
    pub submission_id: &'a str,
    pub archive_filename: &'a str,
    pub archive_sha256: &'a str,
    pub expected_tickets: &'a [NotaryTicketReceipt],
}

pub fn validate_notary_log(
    log: &Value,
    expected: &NotaryLogExpectation<'_>,
) -> Result<Vec<NotaryTicketReceipt>> {
    let text = |key: &str| log.get(key).and_then(Value::as_str);
    let format = log.get("logFormatVersion");
    ensure!(
        format.and_then(Value::as_f64) == Some(1.0),
        "Unsupported Apple notary log format: {}",
        format.map_or_else(|| "missing".to_owned(), Value::to_string)
    );
    ensure!(
        text("jobId") == Some(expected.submission_id),
        "Apple notary log submission ID does not match the accepted submission"
    );
    ensure!(
        text("status") == Some("Accepted")
            && log.get("statusCode").and_then(Value::as_f64) == Some(0.0),
        "Apple notary log is not accepted: {}",
        text("status")
            .or_else(|| text("statusSummary"))
            .unwrap_or("unknown status")
    );
    ensure!(
        text("archiveFilename") == Some(expected.archive_filename),
        "Apple notary log archive filename does not match the submitted archive"
    );
    ensure!(
        text("sha256").map(str::to_lowercase) == Some(expected.archive_sha256.to_lowercase()),
        "Apple notary log SHA-256 does not match the submitted archive"
    );
    let issues_ok = match log.get("issues") {
        Some(Value::Null) => true,
        Some(Value::Array(items)) => items.is_empty(),
        _ => false,
    };
    ensure!(issues_ok, "Apple notary log contains issues");
    let tickets = log
        .get("ticketContents")
        .and_then(Value::as_array)
        .context("Apple notary log omitted ticket contents")?;

    let mut actual = Vec::new();
    for ticket in tickets {
        let path = ticket
            .get("path")
            .and_then(Value::as_str)
            .filter(|path| !path.is_empty())
            .context("Apple notary ticket omitted its path")?;
        ensure!(
            ticket.get("digestAlgorithm").and_then(Value::as_str) == Some("SHA-256"),
            "Apple notary ticket for {path} used an unexpected digest algorithm"
        );
        let arch = ticket.get("arch").and_then(Value::as_str);
        let architecture = arch
            .filter(|arch| *arch == "arm64" || *arch == "x86_64")
            .with_context(|| {
                format!("Apple notary ticket for {path} used an unexpected architecture")
            })?;
        let cdhash = ticket
            .get("cdhash")
            .and_then(Value::as_str)
            .filter(|hash| matches("(?i)^[a-f0-9]{40,64}$", hash))
            .with_context(|| format!("Apple notary ticket for {path} omitted a valid cdhash"))?;
        actual.push(NotaryTicketReceipt {
            path: path.to_owned(),
            architecture: architecture.to_owned(),
            cdhash: cdhash.to_lowercase(),
        });
    }
    actual.sort_by(|left, right| locale_compare(&left.path, &right.path));
    let mut wanted: Vec<NotaryTicketReceipt> = expected
        .expected_tickets
        .iter()
        .map(|ticket| NotaryTicketReceipt {
            cdhash: ticket.cdhash.to_lowercase(),
            ..ticket.clone()
        })
        .collect();
    wanted.sort_by(|left, right| locale_compare(&left.path, &right.path));
    ensure!(
        actual == wanted,
        "Apple notary ticket contents do not match the Rust Tide executable"
    );
    Ok(actual)
}

pub fn validate_release_entitlements(contents: &str) -> Result<()> {
    let keys = Regex::new(r"<key>([^<]+)</key>")?
        .find_iter(contents)
        .count();
    ensure!(
        keys == 0,
        "Rust Tide must not request JIT or other executable entitlements"
    );
    Ok(())
}

pub fn validate_signed_entitlements(output: &str, expected: &str) -> Result<()> {
    let keys: Vec<String> = Regex::new(r"\[Key\]\s+([^\s]+)")?
        .captures_iter(output)
        .chain(Regex::new(r"<key>([^<]+)</key>")?.captures_iter(output))
        .map(|found| found[1].to_owned())
        .collect();
    ensure!(
        keys.is_empty(),
        "Signed {expected} executable must not carry JIT or other entitlements: {}",
        keys.join(", ")
    );
    Ok(())
}

fn notary_credentials(
    source: &CredentialSource,
    api_key_path: Option<&Path>,
) -> Result<Vec<String>> {
    Ok(match source {
        CredentialSource::LocalKeychain(local) => {
            argv!["--keychain-profile", local.notary_keychain_profile]
        }
        CredentialSource::Release(credentials) => {
            let key =
                api_key_path.context("Notary API key path is required for secret-based signing")?;
            argv![
                "--key",
                key,
                "--key-id",
                credentials.notary_api_key_id,
                "--issuer",
                credentials.notary_api_issuer_id,
            ]
        }
    })
}

pub fn notary_submit_arguments(
    archive: &Path,
    source: &CredentialSource,
    api_key_path: Option<&Path>,
) -> Result<Vec<String>> {
    let mut command = argv!["/usr/bin/xcrun", "notarytool", "submit", archive];
    command.extend(notary_credentials(source, api_key_path)?);
    command.extend(argv!["--wait", "--output-format", "json"]);
    Ok(command)
}

pub fn notary_log_arguments(
    submission_id: &str,
    output_path: &Path,
    source: &CredentialSource,
    api_key_path: Option<&Path>,
) -> Result<Vec<String>> {
    let mut command = argv![
        "/usr/bin/xcrun",
        "notarytool",
        "log",
        submission_id,
        output_path
    ];
    command.extend(notary_credentials(source, api_key_path)?);
    Ok(command)
}

fn decode_base64(value: &str, label: &str) -> Result<Vec<u8>> {
    let normalized: String = value.chars().filter(|c| !c.is_whitespace()).collect();
    ensure!(
        !normalized.is_empty() && matches("^[A-Za-z0-9+/]+={0,2}$", &normalized),
        "{label} is not valid base64"
    );
    let engine = base64::engine::GeneralPurpose::new(
        &base64::alphabet::STANDARD,
        base64::engine::GeneralPurposeConfig::new()
            .with_decode_padding_mode(base64::engine::DecodePaddingMode::Indifferent)
            .with_decode_allow_trailing_bits(true),
    );
    let decoded = engine
        .decode(normalized)
        .with_context(|| format!("{label} is not valid base64"))?;
    ensure!(!decoded.is_empty(), "{label} decoded to an empty file");
    Ok(decoded)
}

fn random_password() -> Result<String> {
    use std::io::Read;
    let mut bytes = [0_u8; 16];
    std::fs::File::open("/dev/urandom")?.read_exact(&mut bytes)?;
    Ok(bytes.iter().map(|byte| format!("{byte:02x}")).collect())
}

fn smoke_extracted_application(
    binary: &Path,
    version: &str,
    runner: &mut dyn CommandRunner,
    isolated_root: &Path,
) -> Result<()> {
    let output = runner.run_plain(&argv![binary, "--version"])?;
    ensure!(
        output.stdout.contains(version),
        "Notarized binary version smoke failed"
    );
    runner.run_plain(&argv![binary, "--check-terminal"])?;
    runner.run_plain(&argv![binary, "--check-catalogs"])?;
    runner.run(
        &argv![binary, "--check-models"],
        &RunOptions::env(isolated_smoke_env(&isolated_root.join("home"))),
    )?;
    Ok(())
}

pub struct SigningOptions<'a> {
    pub repository_root: &'a Path,
    pub artifact_id: &'a str,
    pub payload_root: &'a Path,
    pub output_directory: &'a Path,
    pub source_commit: &'a str,
    pub credentials: CredentialSource,
    pub temporary_root: Option<&'a Path>,
}

#[derive(Debug)]
pub struct SigningResult {
    pub archive: PathBuf,
    pub receipt_path: PathBuf,
}

pub fn sign_and_notarize(
    options: &SigningOptions<'_>,
    runner: &mut dyn CommandRunner,
) -> Result<SigningResult> {
    ensure!(
        options.artifact_id == "darwin-arm64" || options.artifact_id == "darwin-x64",
        "macOS signing supports only darwin-arm64 and darwin-x64"
    );
    let repository_root = std::path::absolute(options.repository_root)?;
    let payload_root = std::path::absolute(options.payload_root)?;
    let output_directory = std::path::absolute(options.output_directory)?;
    let entitlements = repository_root.join("config/macos-release-entitlements.plist");
    validate_release_entitlements(&std::fs::read_to_string(&entitlements)?)?;
    let contract = load_contract(&repository_root)?;
    let artifact = contract.artifact(options.artifact_id)?;
    ensure!(
        artifact.platform == "darwin"
            && artifact.signing == "developer-id"
            && artifact.notarization == "notarytool",
        "Artifact does not require macOS signing and notarization"
    );

    let mut builder = tempfile::Builder::new();
    builder.prefix("tide-notary-");
    let temporary = match options.temporary_root {
        Some(root) => builder.tempdir_in(root)?,
        None => builder.tempdir()?,
    };
    let mut keychain_created = false;
    let result = sign_in_scratch(
        options,
        runner,
        &SigningContext {
            repository_root: &repository_root,
            payload_root: &payload_root,
            output_directory: &output_directory,
            entitlements: &entitlements,
            version: &contract.product.version,
            architecture: &artifact.architecture,
            scratch: temporary.path(),
        },
        &mut keychain_created,
    );
    if keychain_created {
        let _ = runner.run_plain(&argv![
            "/usr/bin/security",
            "delete-keychain",
            temporary.path().join("release.keychain-db"),
        ]);
    }
    drop(temporary);
    result
}

struct SigningContext<'a> {
    repository_root: &'a Path,
    payload_root: &'a Path,
    output_directory: &'a Path,
    entitlements: &'a Path,
    version: &'a str,
    architecture: &'a str,
    scratch: &'a Path,
}

fn sign_in_scratch(
    options: &SigningOptions<'_>,
    runner: &mut dyn CommandRunner,
    context: &SigningContext<'_>,
    keychain_created: &mut bool,
) -> Result<SigningResult> {
    let scratch = context.scratch;
    let keychain = scratch.join("release.keychain-db");
    let certificate = scratch.join("developer-id.p12");
    let extracted_root = scratch.join("extracted");
    let notary_log_path = scratch.join("notary-log.json");
    let api_key_path = match &options.credentials {
        CredentialSource::Release(credentials) => {
            Some(scratch.join(format!("AuthKey_{}.p8", credentials.notary_api_key_id)))
        }
        CredentialSource::LocalKeychain(_) => None,
    };

    if let CredentialSource::Release(credentials) = &options.credentials {
        let api_key = api_key_path.as_deref().context("API key path")?;
        write_private(
            &certificate,
            &decode_base64(
                &credentials.developer_id_p12_base64,
                "Developer ID certificate",
            )?,
        )?;
        write_private(
            api_key,
            &decode_base64(&credentials.notary_api_key_p8_base64, "Notary API key")?,
        )?;
        let password = random_password()?;
        runner.run_plain(&argv![
            "/usr/bin/security",
            "create-keychain",
            "-p",
            password,
            keychain
        ])?;
        *keychain_created = true;
        runner.run_plain(&argv![
            "/usr/bin/security",
            "unlock-keychain",
            "-p",
            password,
            keychain
        ])?;
        runner.run_plain(&argv![
            "/usr/bin/security",
            "set-keychain-settings",
            "-lut",
            "21600",
            keychain
        ])?;
        runner.run_plain(&argv![
            "/usr/bin/security",
            "import",
            certificate,
            "-k",
            keychain,
            "-P",
            credentials.developer_id_p12_password,
            "-T",
            "/usr/bin/codesign",
        ])?;
        runner.run_plain(&argv![
            "/usr/bin/security",
            "set-key-partition-list",
            "-S",
            "apple-tool:,apple:",
            "-s",
            "-k",
            password,
            keychain,
        ])?;
    }
    let release_keychain = matches!(options.credentials, CredentialSource::Release(_));
    let identity_command = if release_keychain {
        argv![
            "/usr/bin/security",
            "find-identity",
            "-v",
            "-p",
            "codesigning",
            keychain
        ]
    } else {
        argv![
            "/usr/bin/security",
            "find-identity",
            "-v",
            "-p",
            "codesigning"
        ]
    };
    let identities = runner.run_plain(&identity_command)?;
    let identity = parse_developer_id_identity(&identities.stdout, options.credentials.team_id())?;

    let binary = context.payload_root.join("tide");
    // A fresh Rust executable may be unsigned or carry an ad-hoc linker signature.
    let existing = runner
        .run_plain(&argv![
            "/usr/bin/codesign",
            "--display",
            "--verbose=2",
            binary
        ])
        .and_then(|_| runner.run_plain(&remove_signature_arguments(&binary)));
    if let Err(error) = existing
        && !error
            .to_string()
            .contains("code object is not signed at all")
    {
        return Err(error);
    }
    runner.run_plain(&codesign_arguments(
        &identity,
        &binary,
        release_keychain.then_some(keychain.as_path()),
        Some(context.entitlements),
    ))?;
    runner.run_plain(&argv![
        "/usr/bin/codesign",
        "--verify",
        "--strict",
        "--verbose=2",
        binary
    ])?;
    let signed = runner.run_plain(&argv![
        "/usr/bin/codesign",
        "--display",
        "--entitlements",
        "-",
        binary
    ])?;
    validate_signed_entitlements(&format!("{}\n{}", signed.stdout, signed.stderr), "tide")?;
    smoke_extracted_application(
        &binary,
        context.version,
        runner,
        &scratch.join("pre-notary-smoke"),
    )?;
    runner.run_plain(&argv![
        "/usr/bin/codesign",
        "--verify",
        "--strict",
        "--verbose=2",
        binary
    ])?;

    let timestamp = commit_timestamp(context.repository_root, options.source_commit, runner)?;
    refresh_prepared_manifest(
        context.repository_root,
        options.artifact_id,
        context.payload_root,
        options.source_commit,
        timestamp,
    )?;
    let archive = archive_prepared_payload(
        context.repository_root,
        options.artifact_id,
        context.payload_root,
        context.output_directory,
        runner,
    )?;

    let submission = runner.run_plain(&notary_submit_arguments(
        &archive,
        &options.credentials,
        api_key_path.as_deref(),
    )?)?;
    let notary: Value = serde_json::from_str(&submission.stdout)
        .context("Apple notarization returned malformed JSON")?;
    let status = notary.get("status").and_then(Value::as_str);
    ensure!(
        status == Some("Accepted"),
        "Apple notarization was not accepted: {}",
        status
            .or_else(|| notary.get("message").and_then(Value::as_str))
            .unwrap_or("unknown result")
    );
    let submission_id = notary
        .get("id")
        .and_then(Value::as_str)
        .filter(|id| !id.is_empty())
        .context("Apple notarization result omitted its submission ID")?
        .to_owned();

    let archive_sha256 = sha256_file(&archive)?;
    runner.run_plain(&notary_log_arguments(
        &submission_id,
        &notary_log_path,
        &options.credentials,
        api_key_path.as_deref(),
    )?)?;
    let notary_log: Value = serde_json::from_str(&std::fs::read_to_string(&notary_log_path)?)?;

    runner.run_plain(&argv![
        "/usr/bin/ditto",
        "-x",
        "-k",
        archive,
        extracted_root
    ])?;
    let stem = context
        .payload_root
        .file_name()
        .context("Payload root has no name")?
        .to_string_lossy()
        .into_owned();
    let archive_name = archive
        .file_name()
        .context("Archive has no name")?
        .to_string_lossy()
        .into_owned();
    let extracted_binary = extracted_root.join(&stem).join("tide");
    runner.run_plain(&argv![
        "/usr/bin/codesign",
        "--verify",
        "--strict",
        "--verbose=2",
        extracted_binary
    ])?;
    let signature = runner.run_plain(&argv![
        "/usr/bin/codesign",
        "--display",
        "--verbose=4",
        extracted_binary
    ])?;
    let architecture = if context.architecture == "x64" {
        "x86_64"
    } else {
        "arm64"
    };
    let expected = [NotaryTicketReceipt {
        path: format!("{archive_name}/{stem}/tide"),
        architecture: architecture.to_owned(),
        cdhash: parse_code_directory_hash(&format!("{}\n{}", signature.stdout, signature.stderr))?,
    }];
    let ticket_contents = validate_notary_log(
        &notary_log,
        &NotaryLogExpectation {
            submission_id: &submission_id,
            archive_filename: &archive_name,
            archive_sha256: &archive_sha256,
            expected_tickets: &expected,
        },
    )?;
    smoke_extracted_application(&extracted_binary, context.version, runner, scratch)?;

    let receipt = NotarizationReceipt {
        schema_version: 3,
        artifact_id: options.artifact_id.to_owned(),
        source_commit: options.source_commit.to_owned(),
        signing_identity: identity,
        signing_order: vec!["tide".into()],
        hardened_runtime: true,
        secure_timestamp: true,
        executable_entitlements: Vec::new(),
        notary_submission_id: submission_id,
        notary_status: "Accepted".into(),
        notary_ticket_validation: "archive-sha256-and-executable-cdhashes".into(),
        ticket_contents,
        runtime_validation: "strict-signature-and-functional-cli-smoke".into(),
        archive: archive_name,
        archive_sha256,
        notary_log_sha256: sha256_file(&notary_log_path)?,
    };
    let receipt_path = context
        .output_directory
        .join(format!("{stem}.notarization.json"));
    write_private(&receipt_path, pretty_json(&receipt)?.as_bytes())?;
    Ok(SigningResult {
        archive,
        receipt_path,
    })
}

/// `configure-notary-api-key`: stores a team API key in the local Keychain
/// through `notarytool store-credentials`. The key contents are never read.
pub fn configure_notary_key_arguments(
    key: &Path,
    key_id: &str,
    issuer: &str,
    profile: &str,
) -> Result<Vec<String>> {
    let metadata = std::fs::symlink_metadata(key).ok();
    ensure!(
        metadata.is_some_and(|metadata| metadata.is_file())
            && !key_id.is_empty()
            && !issuer.is_empty(),
        "Provide a regular .p8 file, team Key ID and Issuer ID. Never paste the key contents into logs."
    );
    ensure!(
        matches("^[A-Za-z0-9._-]+$", profile),
        "Invalid profile name"
    );
    Ok(argv![
        "/usr/bin/xcrun",
        "notarytool",
        "store-credentials",
        profile,
        "--key",
        key,
        "--key-id",
        key_id,
        "--issuer",
        issuer,
    ])
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::release::tests::{fake_release_runner, fixture_binary, head_commit};
    use crate::release::{PrepareOptions, prepare_payload};
    use crate::util::repo_root;
    use crate::util::testing::{RecordingRunner, stdout};
    use std::collections::HashMap;

    const ISSUER: &str = "11111111-2222-3333-4444-555555555555";

    fn ci_environment() -> HashMap<&'static str, String> {
        HashMap::from([
            ("APPLE_DEVELOPER_ID_P12_BASE64", "Y2VydA==".to_owned()),
            ("APPLE_DEVELOPER_ID_P12_PASSWORD", "password".to_owned()),
            ("APPLE_TEAM_ID", "ABCDEFGHIJ".to_owned()),
            ("APPLE_NOTARY_API_KEY_P8_BASE64", "a2V5".to_owned()),
            ("APPLE_NOTARY_API_KEY_ID", "KLMNOPQRST".to_owned()),
            ("APPLE_NOTARY_API_ISSUER_ID", ISSUER.to_owned()),
        ])
    }

    fn credentials() -> ReleaseCredentials {
        let environment = ci_environment();
        require_release_credentials(|name| environment.get(name).cloned()).unwrap()
    }

    fn local() -> LocalKeychainCredentials {
        require_local_keychain_credentials(Some("WN53UUCY94"), Some("tide-rust-notary-api"))
            .unwrap()
    }

    #[test]
    fn fails_closed_when_any_apple_credential_is_missing() {
        let error = require_release_credentials(|_| None).unwrap_err();
        assert!(
            error
                .to_string()
                .contains("Missing required Apple release credentials")
        );
        let environment = ci_environment();
        assert_eq!(
            require_release_credentials(|name| environment.get(name).cloned())
                .unwrap()
                .notary_api_key_id,
            "KLMNOPQRST"
        );
        let mut empty = ci_environment();
        empty.insert("APPLE_TEAM_ID", String::new());
        let error = require_release_credentials(|name| empty.get(name).cloned()).unwrap_err();
        assert!(error.to_string().ends_with("teamId"));
    }

    #[test]
    fn requires_exactly_one_matching_developer_id_application_identity() {
        let identity = "Developer ID Application: Ultra.dev (ABCDEFGHIJ)";
        assert_eq!(
            parse_developer_id_identity(
                &format!("  1) ABCDEF0123 \"{identity}\"\n     1 valid identities found"),
                "ABCDEFGHIJ"
            )
            .unwrap(),
            identity
        );
        let none = parse_developer_id_identity("0 valid identities found", "ABCDEFGHIJ");
        assert!(none.unwrap_err().to_string().contains("exactly one"));
        let two = parse_developer_id_identity(
            &format!("1) ABCDEF0123 \"{identity}\"\n2) FEDCBA3210 \"{identity}\""),
            "ABCDEFGHIJ",
        );
        assert!(two.unwrap_err().to_string().contains("found 2"));
    }

    #[test]
    fn validates_explicit_local_keychain_credentials() {
        assert_eq!(
            local(),
            LocalKeychainCredentials {
                team_id: "WN53UUCY94".into(),
                notary_keychain_profile: "tide-rust-notary-api".into(),
            }
        );
        let missing_team = require_local_keychain_credentials(None, Some("tide-rust-notary-api"));
        assert!(missing_team.unwrap_err().to_string().contains("team-id"));
        let missing_profile = require_local_keychain_credentials(Some("WN53UUCY94"), None);
        assert!(
            missing_profile
                .unwrap_err()
                .to_string()
                .contains("local-keychain-profile")
        );
        let bad = require_local_keychain_credentials(Some("WN53UUCY94"), Some("bad profile"));
        assert!(bad.unwrap_err().to_string().contains("unsupported"));
    }

    #[test]
    fn uses_the_login_keychain_and_saved_notary_profile_only_in_local_rehearsal_mode() {
        let codesign = codesign_arguments(
            "Developer ID Application: Zorz LLC (WN53UUCY94)",
            Path::new("/tmp/tide"),
            None,
            None,
        );
        assert_eq!(
            codesign,
            [
                "/usr/bin/codesign",
                "--force",
                "--options",
                "runtime",
                "--timestamp",
                "--sign",
                "Developer ID Application: Zorz LLC (WN53UUCY94)",
                "/tmp/tide",
            ]
        );
        assert!(!codesign.contains(&"--keychain".to_owned()));
        let source = CredentialSource::LocalKeychain(local());
        let notary = notary_submit_arguments(Path::new("/tmp/tide.zip"), &source, None).unwrap();
        assert_eq!(
            notary,
            [
                "/usr/bin/xcrun",
                "notarytool",
                "submit",
                "/tmp/tide.zip",
                "--keychain-profile",
                "tide-rust-notary-api",
                "--wait",
                "--output-format",
                "json",
            ]
        );
        assert!(!notary.contains(&"--key".to_owned()));
        assert_eq!(
            notary_log_arguments(ISSUER, Path::new("/tmp/notary-log.json"), &source, None).unwrap(),
            [
                "/usr/bin/xcrun",
                "notarytool",
                "log",
                ISSUER,
                "/tmp/notary-log.json",
                "--keychain-profile",
                "tide-rust-notary-api",
            ]
        );
    }

    #[test]
    fn signs_rust_with_hardened_runtime_and_no_jit_entitlements() {
        assert_eq!(
            remove_signature_arguments(Path::new("/tmp/tide")),
            ["/usr/bin/codesign", "--remove-signature", "/tmp/tide"]
        );
        assert_eq!(
            codesign_arguments("identity", Path::new("/tmp/tide"), None, None),
            [
                "/usr/bin/codesign",
                "--force",
                "--options",
                "runtime",
                "--timestamp",
                "--sign",
                "identity",
                "/tmp/tide"
            ]
        );
        validate_release_entitlements("<plist><dict/></plist>").unwrap();
        for key in [
            "com.apple.security.cs.allow-jit",
            "com.apple.security.cs.allow-unsigned-executable-memory",
            "com.apple.security.cs.disable-library-validation",
        ] {
            let plist = format!("<plist><dict><key>{key}</key><true/></dict></plist>");
            let requested = validate_release_entitlements(&plist).unwrap_err();
            assert!(requested.to_string().contains("must not request"));
            let carried =
                validate_signed_entitlements(&format!("[Key] {key}"), "tide").unwrap_err();
            assert!(carried.to_string().contains("must not carry"));
        }
        validate_signed_entitlements("Executable=/tmp/tide", "tide").unwrap();
        let repository =
            std::fs::read_to_string(repo_root().join("config/macos-release-entitlements.plist"))
                .unwrap();
        validate_release_entitlements(&repository).unwrap();
    }

    #[test]
    fn retains_api_key_notarization_and_explicit_keychain_selection_for_ci() {
        let source = CredentialSource::Release(credentials());
        let key = Path::new("/tmp/AuthKey_KLMNOPQRST.p8");
        assert!(
            codesign_arguments(
                "identity",
                Path::new("/tmp/tide"),
                Some(Path::new("/tmp/release.keychain-db")),
                None
            )
            .contains(&"--keychain".to_owned())
        );
        assert_eq!(
            notary_submit_arguments(Path::new("/tmp/tide.zip"), &source, Some(key)).unwrap(),
            [
                "/usr/bin/xcrun",
                "notarytool",
                "submit",
                "/tmp/tide.zip",
                "--key",
                "/tmp/AuthKey_KLMNOPQRST.p8",
                "--key-id",
                "KLMNOPQRST",
                "--issuer",
                ISSUER,
                "--wait",
                "--output-format",
                "json",
            ]
        );
        assert_eq!(
            notary_log_arguments(
                ISSUER,
                Path::new("/tmp/notary-log.json"),
                &source,
                Some(key)
            )
            .unwrap(),
            [
                "/usr/bin/xcrun",
                "notarytool",
                "log",
                ISSUER,
                "/tmp/notary-log.json",
                "--key",
                "/tmp/AuthKey_KLMNOPQRST.p8",
                "--key-id",
                "KLMNOPQRST",
                "--issuer",
                ISSUER,
            ]
        );
        let missing =
            notary_submit_arguments(Path::new("/tmp/tide.zip"), &source, None).unwrap_err();
        assert!(
            missing
                .to_string()
                .contains("Notary API key path is required")
        );
    }

    fn accepted_log(
        submission: &str,
        archive: &str,
        sha: &str,
        tickets: &[NotaryTicketReceipt],
    ) -> Value {
        serde_json::json!({
            "logFormatVersion": 1,
            "jobId": submission,
            "status": "Accepted",
            "statusSummary": "Ready for distribution",
            "statusCode": 0,
            "archiveFilename": archive,
            "sha256": sha,
            "ticketContents": tickets.iter().map(|ticket| serde_json::json!({
                "path": ticket.path,
                "digestAlgorithm": "SHA-256",
                "cdhash": ticket.cdhash,
                "arch": ticket.architecture,
            })).collect::<Vec<_>>(),
            "issues": null,
        })
    }

    #[test]
    fn matches_accepted_apple_ticket_contents_to_the_archive_and_code_directory_hashes() {
        let archive_sha = "a".repeat(64);
        let tickets = [NotaryTicketReceipt {
            path: "tide-v0.10.0-darwin-arm64.zip/tide-v0.10.0-darwin-arm64/tide".into(),
            architecture: "arm64".into(),
            cdhash: "b".repeat(40),
        }];
        let log = accepted_log(
            ISSUER,
            "tide-v0.10.0-darwin-arm64.zip",
            &archive_sha,
            &tickets,
        );
        let validated = validate_notary_log(
            &log,
            &NotaryLogExpectation {
                submission_id: ISSUER,
                archive_filename: "tide-v0.10.0-darwin-arm64.zip",
                archive_sha256: &archive_sha,
                expected_tickets: &tickets,
            },
        )
        .unwrap();
        assert_eq!(validated, tickets);
        assert_eq!(
            parse_code_directory_hash(&format!(
                "Executable=/tmp/tide\nCDHash={}\n",
                "B".repeat(40)
            ))
            .unwrap(),
            "b".repeat(40)
        );
    }

    #[test]
    fn fails_closed_when_apple_ticket_provenance_does_not_match_the_payload() {
        let tickets = [NotaryTicketReceipt {
            path: "tide.zip/tide/tide".into(),
            architecture: "arm64".into(),
            cdhash: "b".repeat(40),
        }];
        let sha = "a".repeat(64);
        let base = accepted_log(ISSUER, "tide.zip", &sha, &tickets);
        let validate = |log: &Value| {
            validate_notary_log(
                log,
                &NotaryLogExpectation {
                    submission_id: ISSUER,
                    archive_filename: "tide.zip",
                    archive_sha256: &sha,
                    expected_tickets: &tickets,
                },
            )
            .unwrap_err()
            .to_string()
        };
        let mut wrong_sha = base.clone();
        wrong_sha["sha256"] = serde_json::json!("f".repeat(64));
        assert!(validate(&wrong_sha).contains("SHA-256 does not match"));
        let mut no_tickets = base.clone();
        no_tickets["ticketContents"] = serde_json::json!([]);
        assert!(validate(&no_tickets).contains("ticket contents do not match"));
        let mut issues = base.clone();
        issues["issues"] = serde_json::json!([{ "severity": "warning" }]);
        assert!(validate(&issues).contains("contains issues"));
        let mut omitted = base;
        omitted.as_object_mut().unwrap().remove("issues");
        assert!(validate(&omitted).contains("contains issues"));
        let twice = parse_code_directory_hash("CDHash=abc\nCDHash=def").unwrap_err();
        assert!(twice.to_string().contains("exactly one"));
    }

    /// Drives the whole flow with a recording runner: nothing touches a keychain,
    /// codesign, Apple's notary service or the network.
    #[test]
    fn orchestrates_ci_signing_in_order_and_writes_a_receipt_without_executing_tools() {
        let scratch = tempfile::tempdir().unwrap();
        let binary = fixture_binary(scratch.path());
        let head = head_commit();
        let mut prepare_runner = fake_release_runner(head.clone());
        let prepared = prepare_payload(
            &PrepareOptions {
                repository_root: &repo_root(),
                artifact_id: "darwin-arm64",
                binary: &binary,
                output_directory: scratch.path(),
                source_commit: &head,
                run_smoke_checks: true,
            },
            &mut prepare_runner,
        )
        .unwrap();
        let cdhash = "c".repeat(40);
        let stem = "tide-v0.10.0-darwin-arm64";
        let archive_name = format!("{stem}.zip");
        let commit = head.clone();
        let mut recorder = RecordingRunner::new(move |argv, _| {
            let joined = argv.join(" ");
            Ok(if joined.contains("find-identity") {
                stdout("  1) ABCDEF0123 \"Developer ID Application: Ultra.dev (ABCDEFGHIJ)\"\n")
            } else if joined.contains("--display --verbose=2") {
                anyhow::bail!("codesign failed with exit 1: code object is not signed at all")
            } else if joined.contains("--display --verbose=4") {
                crate::util::CommandOutput {
                    stdout: String::new(),
                    stderr: format!("CDHash={cdhash}\n"),
                }
            } else if joined.contains("notarytool submit") {
                stdout(&format!("{{\"id\":\"{ISSUER}\",\"status\":\"Accepted\"}}"))
            } else if joined.contains("rev-parse") {
                stdout(&format!("{commit}\n"))
            } else if joined.contains("show -s") {
                stdout("1767225600\n")
            } else if joined.ends_with("--version") {
                stdout("Tide 0.10.0\n")
            } else {
                stdout("")
            })
        });
        // notarytool writes the log file itself; the wrapper stands in for it.
        let mut runner = LogWritingRunner {
            inner: &mut recorder,
            archive_name: archive_name.clone(),
            stem: stem.to_owned(),
            pending_log: None,
        };
        let output = scratch.path().join("final");
        let result = sign_and_notarize(
            &SigningOptions {
                repository_root: &repo_root(),
                artifact_id: "darwin-arm64",
                payload_root: &prepared,
                output_directory: &output,
                source_commit: &head,
                credentials: CredentialSource::Release(credentials()),
                temporary_root: Some(scratch.path()),
            },
            &mut runner,
        )
        .unwrap();

        let commands = runner.inner.commands();
        let position = |needle: &str| {
            commands
                .iter()
                .position(|command| command.contains(needle))
                .unwrap_or_else(|| panic!("missing {needle}: {commands:#?}"))
        };
        assert!(position("security create-keychain") < position("security import"));
        assert!(position("codesign --force") < position("notarytool submit"));
        assert!(position("notarytool submit") < position("notarytool log"));
        assert!(position("notarytool log") < position("ditto -x -k"));
        assert!(
            commands
                .last()
                .unwrap()
                .contains("security delete-keychain")
        );
        assert!(commands[position("codesign --force")].contains("--keychain"));
        assert!(
            commands[position("codesign --force")].contains("macos-release-entitlements.plist")
        );
        assert!(!commands.iter().any(|command| command.contains("spctl")));
        let receipt: Value =
            serde_json::from_str(&std::fs::read_to_string(&result.receipt_path).unwrap()).unwrap();
        assert_eq!(receipt["notaryStatus"], "Accepted");
        assert_eq!(
            receipt["ticketContents"][0]["path"],
            format!("{archive_name}/{stem}/tide")
        );
        assert_eq!(
            result.archive.file_name().unwrap().to_string_lossy(),
            archive_name
        );
        let written: Value =
            serde_json::from_str(&std::fs::read_to_string(&result.receipt_path).unwrap()).unwrap();
        assert_eq!(written["schemaVersion"], 3);
        assert_eq!(
            written["runtimeValidation"],
            "strict-signature-and-functional-cli-smoke"
        );
        assert!(!scratch.path().read_dir().unwrap().any(|entry| {
            entry
                .unwrap()
                .file_name()
                .to_string_lossy()
                .starts_with("tide-notary-")
        }));
    }

    #[test]
    fn local_rehearsal_never_creates_or_deletes_a_keychain() {
        let source = CredentialSource::LocalKeychain(local());
        let mut runner = RecordingRunner::new(|_, _| Ok(stdout("0 valid identities found")));
        let scratch = tempfile::tempdir().unwrap();
        let error = sign_and_notarize(
            &SigningOptions {
                repository_root: &repo_root(),
                artifact_id: "darwin-x64",
                payload_root: scratch.path(),
                output_directory: scratch.path(),
                source_commit: &"a".repeat(40),
                credentials: source,
                temporary_root: Some(scratch.path()),
            },
            &mut runner,
        )
        .unwrap_err();
        assert!(
            error
                .to_string()
                .contains("exactly one Developer ID Application")
        );
        assert_eq!(
            runner.commands(),
            ["/usr/bin/security find-identity -v -p codesigning"]
        );
    }

    #[test]
    fn notary_key_setup_validates_inputs_before_building_the_command() {
        let scratch = tempfile::tempdir().unwrap();
        let key = scratch.path().join("AuthKey.p8");
        std::fs::write(&key, "placeholder").unwrap();
        let command =
            configure_notary_key_arguments(&key, "KLMNOPQRST", ISSUER, "tide-rust-notary-api")
                .unwrap();
        assert_eq!(
            &command[..4],
            [
                "/usr/bin/xcrun",
                "notarytool",
                "store-credentials",
                "tide-rust-notary-api"
            ]
        );
        assert!(configure_notary_key_arguments(&key, "KLMNOPQRST", ISSUER, "bad profile").is_err());
        assert!(
            configure_notary_key_arguments(&scratch.path().join("missing.p8"), "K", ISSUER, "p")
                .is_err()
        );
    }

    /// Writes an accepted notary log for the archive the flow submitted.
    struct LogWritingRunner<'a> {
        inner: &'a mut RecordingRunner,
        archive_name: String,
        stem: String,
        pending_log: Option<Value>,
    }

    impl CommandRunner for LogWritingRunner<'_> {
        fn run(
            &mut self,
            argv: &[String],
            options: &RunOptions,
        ) -> Result<crate::util::CommandOutput> {
            let output = self.inner.run(argv, options)?;
            if argv.get(2).is_some_and(|word| word == "submit") {
                let archive = PathBuf::from(&argv[3]);
                let sha = sha256_file(&archive)?;
                let log = accepted_log(
                    ISSUER,
                    &self.archive_name,
                    &sha,
                    &[NotaryTicketReceipt {
                        path: format!("{}/{}/tide", self.archive_name, self.stem),
                        architecture: "arm64".into(),
                        cdhash: "c".repeat(40),
                    }],
                );
                self.pending_log = Some(log);
            }
            if argv.get(2).is_some_and(|word| word == "log")
                && let Some(log) = self.pending_log.take()
            {
                std::fs::write(&argv[4], log.to_string())?;
            }
            Ok(output)
        }
    }
}
