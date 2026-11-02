//! GitHub repository governance: plan, read-only audits and a confirmed apply,
//! plus the offline policy verifier that CI runs.
use crate::util::read_json;
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::BTreeSet;
use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};

pub trait GitHubClient {
    fn request(&mut self, method: &str, path: &str, body: Option<&Value>) -> Result<Value>;
}

/// Calls `gh api` with the owner's existing CLI authentication.
pub struct GhCliClient;

impl GitHubClient for GhCliClient {
    fn request(&mut self, method: &str, path: &str, body: Option<&Value>) -> Result<Value> {
        let mut command = Command::new("gh");
        command.args(["api", "--method", method, path]);
        if body.is_some() {
            command.args(["--input", "-"]);
        }
        let mut child = command
            .stdin(if body.is_some() {
                Stdio::piped()
            } else {
                Stdio::null()
            })
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .context("spawn gh")?;
        if let (Some(body), Some(mut stdin)) = (body, child.stdin.take()) {
            stdin.write_all(body.to_string().as_bytes())?;
        }
        let output = child.wait_with_output()?;
        ensure!(
            output.status.success(),
            "GitHub API {method} {path} failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        );
        let stdout = String::from_utf8_lossy(&output.stdout);
        Ok(if stdout.trim().is_empty() {
            json!({})
        } else {
            serde_json::from_str(&stdout)?
        })
    }
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BypassActor {
    pub actor_type: String,
    pub login: String,
    pub bypass_mode: String,
}

#[derive(Clone, Debug, Deserialize)]
pub struct Rule {
    pub r#type: String,
    #[serde(default)]
    pub parameters: Option<serde_json::Map<String, Value>>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Ruleset {
    pub name: String,
    pub target: String,
    pub enforcement: String,
    pub include: Vec<String>,
    pub bypass_actors: Vec<BypassActor>,
    pub rules: Vec<Rule>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct Reviewer {
    pub r#type: String,
    pub login: String,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Environment {
    pub name: String,
    pub reviewers: Vec<Reviewer>,
    pub prevent_self_review: bool,
    pub required_secrets: Vec<String>,
    pub deployment_tag_patterns: Vec<String>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExpectedAccess {
    pub organization_owners: Vec<String>,
    pub repository_admins: Vec<String>,
    pub repository_writers: Vec<String>,
    pub write_teams: Vec<String>,
    pub write_apps: Vec<String>,
    pub write_deploy_keys: Vec<String>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RepositorySettings {
    pub allow_squash_merge: bool,
    pub allow_merge_commit: bool,
    pub allow_rebase_merge: bool,
    pub allow_auto_merge: bool,
    pub delete_branch_on_merge: bool,
    pub default_workflow_permissions: String,
    pub workflows_may_approve_pull_requests: bool,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GovernanceConfig {
    pub schema_version: u8,
    pub repository: String,
    pub owner: String,
    pub required_checks: Vec<String>,
    pub expected_access: ExpectedAccess,
    pub repository_settings: RepositorySettings,
    pub rulesets: Vec<Ruleset>,
    pub environments: Vec<Environment>,
    pub cutover_order: Vec<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ObservedAccess {
    pub organization_owners: Vec<String>,
    pub repository_admins: Vec<String>,
    pub repository_writers: Vec<String>,
    pub write_teams: Vec<String>,
    pub write_deploy_keys: Vec<String>,
    pub write_apps: Vec<String>,
}

#[derive(Clone, Debug, Serialize)]
pub struct AuditResult {
    pub repository: String,
    pub status: String,
    pub findings: Vec<String>,
    pub observed: ObservedAccess,
}

fn equal_members(actual: &[String], expected: &[String]) -> bool {
    actual.iter().collect::<BTreeSet<_>>() == expected.iter().collect::<BTreeSet<_>>()
}

pub fn load_config(repository_root: &Path) -> Result<GovernanceConfig> {
    let config: GovernanceConfig =
        read_json(&repository_root.join("config/github-governance.json"))?;
    ensure!(
        config.schema_version == 1,
        "Unsupported GitHub governance schema"
    );
    ensure!(
        config.repository == "ultradotdev/tide",
        "Governance repository must be ultradotdev/tide"
    );
    ensure!(
        config.owner == "thekeith",
        "Governance owner must be thekeith"
    );
    let checks = ["CI / macOS", "CI / Linux", "CI / PostgreSQL"].map(String::from);
    ensure!(
        equal_members(&config.required_checks, &checks),
        "Required CI check names drifted"
    );
    ensure!(
        config.rulesets.len() == 5,
        "Exactly five governance rulesets are required"
    );
    let environments: Vec<&str> = config
        .environments
        .iter()
        .map(|item| item.name.as_str())
        .collect();
    ensure!(
        environments == ["release-signing", "release"],
        "Release environments drifted"
    );
    Ok(config)
}

fn strings(value: &Value, key: &str) -> Vec<String> {
    value
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|item| item.get(key).and_then(Value::as_str).map(str::to_owned))
        .collect()
}

fn flag(item: &Value, key: &str) -> bool {
    item.pointer(&format!("/permissions/{key}"))
        .and_then(Value::as_bool)
        .unwrap_or(false)
}

fn observe_access(
    client: &mut dyn GitHubClient,
    config: &GovernanceConfig,
) -> Result<ObservedAccess> {
    let (organization, repository) = config
        .repository
        .split_once('/')
        .context("Invalid repository name")?;
    ensure!(!repository.is_empty(), "Invalid repository name");
    let owners = client.request(
        "GET",
        &format!("/orgs/{organization}/members?role=admin"),
        None,
    )?;
    let collaborators = client.request(
        "GET",
        &format!("/repos/{}/collaborators?affiliation=all", config.repository),
        None,
    )?;
    let teams = client.request("GET", &format!("/repos/{}/teams", config.repository), None)?;
    let keys = client.request("GET", &format!("/repos/{}/keys", config.repository), None)?;
    let installations =
        client.request("GET", &format!("/orgs/{organization}/installations"), None)?;
    let collaborators = collaborators.as_array().cloned().unwrap_or_default();
    let login = |item: &Value| {
        item.get("login")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned()
    };
    Ok(ObservedAccess {
        organization_owners: strings(&owners, "login"),
        repository_admins: collaborators
            .iter()
            .filter(|item| flag(item, "admin"))
            .map(login)
            .collect(),
        repository_writers: collaborators
            .iter()
            .filter(|item| flag(item, "admin") || flag(item, "maintain") || flag(item, "push"))
            .map(login)
            .collect(),
        write_teams: teams
            .as_array()
            .into_iter()
            .flatten()
            .filter(|item| {
                item.get("permission")
                    .and_then(Value::as_str)
                    .is_some_and(|permission| permission != "pull")
            })
            .filter_map(|item| item.get("slug").and_then(Value::as_str).map(str::to_owned))
            .collect(),
        write_deploy_keys: keys
            .as_array()
            .into_iter()
            .flatten()
            .filter(|item| item.get("read_only").and_then(Value::as_bool) == Some(false))
            .filter_map(|item| item.get("title").and_then(Value::as_str).map(str::to_owned))
            .collect(),
        // The organization endpoint is available to the sole owner token. Treat any
        // organization-wide write-capable App as unexpected because repository
        // scope cannot be proven safely with an ordinary user token.
        write_apps: installations
            .get("installations")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter(|item| {
                item.get("permissions")
                    .and_then(Value::as_object)
                    .is_some_and(|permissions| {
                        permissions
                            .values()
                            .any(|value| value == "write" || value == "admin")
                    })
            })
            .map(|item| {
                item.get("app_slug").and_then(Value::as_str).map_or_else(
                    || item.get("app_id").map(Value::to_string).unwrap_or_default(),
                    str::to_owned,
                )
            })
            .collect(),
    })
}

fn access_findings(observed: &ObservedAccess, config: &GovernanceConfig) -> Vec<String> {
    let expected = &config.expected_access;
    [
        (
            "organizationOwners",
            &observed.organization_owners,
            &expected.organization_owners,
        ),
        (
            "repositoryAdmins",
            &observed.repository_admins,
            &expected.repository_admins,
        ),
        (
            "repositoryWriters",
            &observed.repository_writers,
            &expected.repository_writers,
        ),
        ("writeTeams", &observed.write_teams, &expected.write_teams),
        ("writeApps", &observed.write_apps, &expected.write_apps),
        (
            "writeDeployKeys",
            &observed.write_deploy_keys,
            &expected.write_deploy_keys,
        ),
    ]
    .into_iter()
    .filter(|(_, actual, wanted)| !equal_members(actual, wanted))
    .map(|(key, actual, wanted)| {
        format!(
            "{key}: expected [{}], observed [{}]",
            wanted.join(", "),
            actual.join(", ")
        )
    })
    .collect()
}

fn status(findings: &[String]) -> String {
    if findings.is_empty() { "pass" } else { "fail" }.to_owned()
}

pub fn audit_access(
    client: &mut dyn GitHubClient,
    config: &GovernanceConfig,
) -> Result<AuditResult> {
    let observed = observe_access(client, config)?;
    let findings = access_findings(&observed, config);
    Ok(AuditResult {
        repository: config.repository.clone(),
        status: status(&findings),
        findings,
        observed,
    })
}

fn repository_settings_body(config: &GovernanceConfig) -> Vec<(&'static str, bool)> {
    let settings = &config.repository_settings;
    vec![
        ("allow_squash_merge", settings.allow_squash_merge),
        ("allow_merge_commit", settings.allow_merge_commit),
        ("allow_rebase_merge", settings.allow_rebase_merge),
        ("allow_auto_merge", settings.allow_auto_merge),
        ("delete_branch_on_merge", settings.delete_branch_on_merge),
    ]
}

fn user_id(client: &mut dyn GitHubClient, login: &str) -> Result<i64> {
    client
        .request("GET", &format!("/users/{login}"), None)?
        .get("id")
        .and_then(Value::as_i64)
        .with_context(|| format!("Unable to resolve GitHub user {login}"))
}

fn snake_case(key: &str) -> String {
    let mut output = String::new();
    for character in key.chars() {
        if character.is_ascii_uppercase() {
            output.push('_');
            output.push(character.to_ascii_lowercase());
        } else {
            output.push(character);
        }
    }
    output
}

fn ruleset_body(client: &mut dyn GitHubClient, ruleset: &Ruleset) -> Result<Value> {
    let mut bypass = Vec::new();
    for actor in &ruleset.bypass_actors {
        bypass.push(json!({
            "actor_id": user_id(client, &actor.login)?,
            "actor_type": actor.actor_type,
            "bypass_mode": actor.bypass_mode,
        }));
    }
    let rules: Vec<Value> = ruleset
        .rules
        .iter()
        .map(|rule| {
            let mut body = json!({ "type": rule.r#type });
            if let Some(parameters) = &rule.parameters {
                let converted: serde_json::Map<String, Value> = parameters
                    .iter()
                    .map(|(key, value)| {
                        let value = match value {
                            Value::Array(items) if key == "requiredStatusChecks" => Value::Array(
                                items
                                    .iter()
                                    .map(|item| json!({ "context": item.get("context") }))
                                    .collect(),
                            ),
                            other => other.clone(),
                        };
                        (snake_case(key), value)
                    })
                    .collect();
                body["parameters"] = Value::Object(converted);
            }
            body
        })
        .collect();
    Ok(json!({
        "name": ruleset.name,
        "target": ruleset.target,
        "enforcement": ruleset.enforcement,
        "bypass_actors": bypass,
        "conditions": { "ref_name": { "include": ruleset.include, "exclude": [] } },
        "rules": rules,
    }))
}

fn audit_settings(
    client: &mut dyn GitHubClient,
    config: &GovernanceConfig,
    findings: &mut Vec<String>,
) -> Result<()> {
    let repository = client.request("GET", &format!("/repos/{}", config.repository), None)?;
    let actions = client.request(
        "GET",
        &format!("/repos/{}/actions/permissions/workflow", config.repository),
        None,
    )?;
    for (key, expected) in repository_settings_body(config) {
        let actual = repository.get(key);
        if actual != Some(&Value::Bool(expected)) {
            let shown = actual.map_or_else(
                || "undefined".to_owned(),
                |value| {
                    value
                        .as_str()
                        .map_or_else(|| value.to_string(), str::to_owned)
                },
            );
            findings.push(format!(
                "repository setting {key} is {shown}, expected {expected}"
            ));
        }
    }
    if actions
        .get("default_workflow_permissions")
        .and_then(Value::as_str)
        != Some("read")
    {
        findings.push("default workflow permissions are not read-only".into());
    }
    if actions.get("can_approve_pull_request_reviews") != Some(&Value::Bool(false)) {
        findings.push("workflows may approve pull requests".into());
    }
    Ok(())
}

fn audit_rulesets(
    client: &mut dyn GitHubClient,
    config: &GovernanceConfig,
    findings: &mut Vec<String>,
) -> Result<()> {
    let active = client.request(
        "GET",
        &format!(
            "/repos/{}/rulesets?includes_parents=false",
            config.repository
        ),
        None,
    )?;
    for expected in &config.rulesets {
        let current =
            active.as_array().into_iter().flatten().find(|item| {
                item.get("name").and_then(Value::as_str) == Some(expected.name.as_str())
            });
        match current {
            None => findings.push(format!("missing ruleset {}", expected.name)),
            Some(current)
                if current.get("enforcement").and_then(Value::as_str) != Some("active")
                    || current.get("target").and_then(Value::as_str)
                        != Some(expected.target.as_str()) =>
            {
                findings.push(format!(
                    "ruleset {} is not active with target {}",
                    expected.name, expected.target
                ));
            }
            Some(_) => {}
        }
    }
    Ok(())
}

fn audit_environments(
    client: &mut dyn GitHubClient,
    config: &GovernanceConfig,
    findings: &mut Vec<String>,
) -> Result<()> {
    for expected in &config.environments {
        let environment = client.request(
            "GET",
            &format!(
                "/repos/{}/environments/{}",
                config.repository, expected.name
            ),
            None,
        )?;
        let reviewers: Vec<String> = environment
            .get("protection_rules")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .flat_map(|rule| {
                rule.get("reviewers")
                    .and_then(Value::as_array)
                    .cloned()
                    .unwrap_or_default()
            })
            .filter_map(|item| {
                item.pointer("/reviewer/login")
                    .and_then(Value::as_str)
                    .filter(|login| !login.is_empty())
                    .map(str::to_owned)
            })
            .collect();
        let wanted: Vec<String> = expected
            .reviewers
            .iter()
            .map(|item| item.login.clone())
            .collect();
        if !equal_members(&reviewers, &wanted) {
            findings.push(format!("environment {} reviewers drifted", expected.name));
        }
        // GitHub reports the setting on the required-reviewers protection rule.
        let prevent_self_review = environment["protection_rules"]
            .as_array()
            .and_then(|rules| {
                rules
                    .iter()
                    .find(|rule| rule["type"] == "required_reviewers")
            })
            .and_then(|rule| rule.get("prevent_self_review"));
        if prevent_self_review != Some(&Value::Bool(expected.prevent_self_review)) {
            findings.push(format!(
                "environment {} self-review policy drifted",
                expected.name
            ));
        }
        let repository = client.request("GET", &format!("/repos/{}", config.repository), None)?;
        let id = repository
            .get("id")
            .map(Value::to_string)
            .unwrap_or_default();
        let secrets = client.request(
            "GET",
            &format!("/repositories/{id}/environments/{}/secrets", expected.name),
            None,
        )?;
        let present = strings(secrets.get("secrets").unwrap_or(&Value::Null), "name");
        if !expected
            .required_secrets
            .iter()
            .all(|name| present.contains(name))
        {
            findings.push(format!(
                "environment {} is missing required secrets",
                expected.name
            ));
        }
    }
    Ok(())
}

pub fn audit_governance(
    client: &mut dyn GitHubClient,
    config: &GovernanceConfig,
) -> Result<AuditResult> {
    let observed = observe_access(client, config)?;
    let mut findings = access_findings(&observed, config);
    let checks = audit_settings(client, config, &mut findings)
        .and_then(|()| audit_rulesets(client, config, &mut findings))
        .and_then(|()| audit_environments(client, config, &mut findings));
    if let Err(error) = checks {
        findings.push(error.to_string());
        return Ok(AuditResult {
            repository: config.repository.clone(),
            status: "unavailable".into(),
            findings,
            observed,
        });
    }
    Ok(AuditResult {
        repository: config.repository.clone(),
        status: status(&findings),
        findings,
        observed,
    })
}

pub fn governance_plan(config: &GovernanceConfig) -> Vec<String> {
    let mut plan = vec![
        format!("verify sole owner and writer for {}", config.repository),
        format!(
            "configure merge and Actions defaults for {}",
            config.repository
        ),
    ];
    plan.extend(
        config
            .rulesets
            .iter()
            .map(|item| format!("create or update ruleset: {}", item.name)),
    );
    plan.extend(
        config
            .environments
            .iter()
            .map(|item| format!("create or update environment: {}", item.name)),
    );
    plan.push("audit resulting governance without removing unexpected actors or rules".into());
    plan
}

pub fn apply_governance(
    client: &mut dyn GitHubClient,
    config: &GovernanceConfig,
    confirmation: &str,
) -> Result<()> {
    let expected = format!("{}:{}", config.repository, config.owner);
    ensure!(
        confirmation == expected,
        "Apply requires --confirm-apply={expected}"
    );
    let observed = observe_access(client, config)?;
    let errors = access_findings(&observed, config);
    ensure!(
        errors.is_empty(),
        "Refusing governance changes because access is unexpected: {}",
        errors.join("; ")
    );
    let settings: serde_json::Map<String, Value> = repository_settings_body(config)
        .into_iter()
        .map(|(key, value)| (key.to_owned(), Value::Bool(value)))
        .collect();
    client.request(
        "PATCH",
        &format!("/repos/{}", config.repository),
        Some(&Value::Object(settings)),
    )?;
    client.request(
        "PUT",
        &format!("/repos/{}/actions/permissions/workflow", config.repository),
        Some(&json!({
            "default_workflow_permissions": config.repository_settings.default_workflow_permissions,
            "can_approve_pull_request_reviews": config.repository_settings.workflows_may_approve_pull_requests,
        })),
    )?;
    let existing = client.request(
        "GET",
        &format!(
            "/repos/{}/rulesets?includes_parents=false",
            config.repository
        ),
        None,
    )?;
    for ruleset in &config.rulesets {
        let body = ruleset_body(client, ruleset)?;
        let current =
            existing.as_array().into_iter().flatten().find(|item| {
                item.get("name").and_then(Value::as_str) == Some(ruleset.name.as_str())
            });
        match current.and_then(|item| item.get("id")) {
            None => client.request(
                "POST",
                &format!("/repos/{}/rulesets", config.repository),
                Some(&body),
            )?,
            Some(id) => client.request(
                "PUT",
                &format!("/repos/{}/rulesets/{id}", config.repository),
                Some(&body),
            )?,
        };
    }
    for environment in &config.environments {
        let mut reviewers = Vec::new();
        for reviewer in &environment.reviewers {
            reviewers
                .push(json!({ "type": reviewer.r#type, "id": user_id(client, &reviewer.login)? }));
        }
        // GitHub rejects a policy whose two flags are equal; no restriction is `null`.
        let deployment_policy = if environment.deployment_tag_patterns.is_empty() {
            Value::Null
        } else {
            json!({ "protected_branches": false, "custom_branch_policies": true })
        };
        client.request(
            "PUT",
            &format!(
                "/repos/{}/environments/{}",
                config.repository, environment.name
            ),
            Some(&json!({
                "reviewers": reviewers,
                "prevent_self_review": environment.prevent_self_review,
                "deployment_branch_policy": deployment_policy,
            })),
        )?;
        let policies = if environment.deployment_tag_patterns.is_empty() {
            Value::Null
        } else {
            client.request(
                "GET",
                &format!(
                    "/repos/{}/environments/{}/deployment-branch-policies",
                    config.repository, environment.name
                ),
                None,
            )?
        };
        let existing = policies["branch_policies"]
            .as_array()
            .map(Vec::as_slice)
            .unwrap_or_default();
        for pattern in &environment.deployment_tag_patterns {
            if existing
                .iter()
                .any(|policy| policy["name"] == *pattern && policy["type"] == "tag")
            {
                continue;
            }
            client.request(
                "POST",
                &format!(
                    "/repos/{}/environments/{}/deployment-branch-policies",
                    config.repository, environment.name
                ),
                Some(&json!({ "name": pattern, "type": "tag" })),
            )?;
        }
    }
    Ok(())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, clap::ValueEnum)]
pub enum Mode {
    Plan,
    AccessAudit,
    Audit,
    Apply,
}

/// `github-governance --mode=plan|access-audit|audit|apply [--confirm-apply=ultradotdev/tide:thekeith]`.
pub fn run(repository_root: &Path, mode: Mode, confirm_apply: &str) -> Result<()> {
    let config = load_config(repository_root)?;
    let result = match mode {
        Mode::Plan => {
            let plan =
                json!({ "repository": config.repository, "operations": governance_plan(&config) });
            println!("{}", serde_json::to_string_pretty(&plan)?);
            return Ok(());
        }
        Mode::AccessAudit => audit_access(&mut GhCliClient, &config)?,
        Mode::Audit => audit_governance(&mut GhCliClient, &config)?,
        Mode::Apply => {
            apply_governance(&mut GhCliClient, &config, confirm_apply)?;
            audit_governance(&mut GhCliClient, &config)?
        }
    };
    println!("{}", serde_json::to_string_pretty(&result)?);
    ensure!(
        result.status == "pass",
        "GitHub governance status: {}",
        result.status
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::util::repo_root;

    #[derive(Default)]
    struct FakeClient {
        writes: Vec<(String, String, Value)>,
        unexpected_writer: bool,
        existing_tag_policy: bool,
    }

    impl GitHubClient for FakeClient {
        fn request(&mut self, method: &str, path: &str, body: Option<&Value>) -> Result<Value> {
            if method != "GET" {
                self.writes.push((
                    method.into(),
                    path.into(),
                    body.cloned().unwrap_or(Value::Null),
                ));
                return Ok(json!({}));
            }
            Ok(match path {
                "/orgs/ultradotdev/members?role=admin" => json!([{ "login": "thekeith" }]),
                "/orgs/ultradotdev/installations" => json!({ "installations": [] }),
                "/users/thekeith" => json!({ "id": 42 }),
                "/repos/ultradotdev/tide" => json!({
                    "id": 123,
                    "allow_squash_merge": true,
                    "allow_merge_commit": false,
                    "allow_rebase_merge": false,
                    "allow_auto_merge": false,
                    "delete_branch_on_merge": true,
                }),
                _ if path.ends_with("/collaborators?affiliation=all") => {
                    let mut people = vec![
                        json!({ "login": "thekeith", "permissions": { "admin": true, "maintain": true, "push": true } }),
                    ];
                    if self.unexpected_writer {
                        people
                            .push(json!({ "login": "intruder", "permissions": { "push": true } }));
                    }
                    Value::Array(people)
                }
                _ if path.ends_with("/teams") || path.ends_with("/keys") => json!([]),
                _ if path.ends_with("/actions/permissions/workflow") => json!({
                    "default_workflow_permissions": "read",
                    "can_approve_pull_request_reviews": false,
                }),
                _ if path.contains("/rulesets?") => Value::Array(
                    [
                        "Tide protected branch integrity",
                        "Tide owner-only protected branch updates",
                        "Tide owner-only release branch creation",
                        "Tide owner-only version tag creation",
                        "Tide immutable version tags",
                    ]
                    .iter()
                    .enumerate()
                    .map(|(index, name)| {
                        json!({
                            "id": index + 1,
                            "name": name,
                            "target": if index > 2 { "tag" } else { "branch" },
                            "enforcement": "active",
                        })
                    })
                    .collect(),
                ),
                _ if path.ends_with("/deployment-branch-policies") => json!({
                    "branch_policies": if self.existing_tag_policy {
                        vec![json!({ "name": "v*", "type": "tag" })]
                    } else {
                        Vec::new()
                    },
                }),
                _ if path.contains("/environments/") && path.ends_with("/secrets") => {
                    let secrets: Vec<Value> = [
                        "APPLE_DEVELOPER_ID_P12_BASE64",
                        "APPLE_DEVELOPER_ID_P12_PASSWORD",
                        "APPLE_TEAM_ID",
                        "APPLE_NOTARY_API_KEY_P8_BASE64",
                        "APPLE_NOTARY_API_KEY_ID",
                        "APPLE_NOTARY_API_ISSUER_ID",
                    ]
                    .iter()
                    .map(|name| json!({ "name": name }))
                    .collect();
                    json!({ "secrets": secrets })
                }
                _ if path.contains("/environments/") => json!({
                    "protection_rules": [{
                        "type": "required_reviewers",
                        "prevent_self_review": false,
                        "reviewers": [{ "type": "User", "reviewer": { "login": "thekeith" } }],
                    }],
                }),
                _ => anyhow::bail!("Unexpected fake request: {method} {path}"),
            })
        }
    }

    fn config() -> GovernanceConfig {
        load_config(&repo_root()).unwrap()
    }

    #[test]
    fn loads_the_owner_only_policy_and_stable_checks() {
        let config = config();
        assert_eq!(
            config.required_checks,
            ["CI / macOS", "CI / Linux", "CI / PostgreSQL"]
        );
        assert_eq!(config.rulesets.len(), 5);
        assert!(governance_plan(&config).contains(
            &"audit resulting governance without removing unexpected actors or rules".to_owned()
        ));
    }

    #[test]
    fn audits_the_expected_sole_owner_state_without_writing() {
        let mut client = FakeClient::default();
        assert_eq!(audit_access(&mut client, &config()).unwrap().status, "pass");
        let result = audit_governance(&mut client, &config()).unwrap();
        assert_eq!(result.status, "pass", "{:?}", result.findings);
        assert!(client.writes.is_empty());
    }

    #[test]
    fn apply_requires_an_exact_explicit_confirmation() {
        let mut client = FakeClient::default();
        let error = apply_governance(&mut client, &config(), "").unwrap_err();
        assert!(error.to_string().contains("Apply requires"));
        assert!(client.writes.is_empty());
    }

    #[test]
    fn apply_fails_before_mutation_when_an_unexpected_writer_exists() {
        let mut client = FakeClient {
            unexpected_writer: true,
            ..FakeClient::default()
        };
        let error =
            apply_governance(&mut client, &config(), "ultradotdev/tide:thekeith").unwrap_err();
        assert!(error.to_string().contains("access is unexpected"));
        assert!(client.writes.is_empty());
    }

    #[test]
    fn apply_updates_named_protections_without_deleting_actors_or_rulesets() {
        let mut client = FakeClient::default();
        apply_governance(&mut client, &config(), "ultradotdev/tide:thekeith").unwrap();
        assert!(
            client
                .writes
                .iter()
                .any(|(method, path, _)| method == "PATCH" && path == "/repos/ultradotdev/tide")
        );
        assert_eq!(
            client
                .writes
                .iter()
                .filter(|(_, path, _)| path.contains("/rulesets/"))
                .count(),
            5
        );
        assert!(
            !client
                .writes
                .iter()
                .any(|(method, _, _)| method == "DELETE")
        );
        let integrity = client
            .writes
            .iter()
            .find(|(_, path, _)| path.ends_with("/rulesets/1"))
            .map(|(_, _, body)| body.clone())
            .unwrap();
        assert_eq!(integrity["bypass_actors"], json!([]));
    }

    #[test]
    fn environments_send_a_valid_deployment_policy_and_reapply_cleanly() {
        let environment = |client: &FakeClient, name: &str| {
            client
                .writes
                .iter()
                .find(|(method, path, _)| {
                    method == "PUT" && path.ends_with(&format!("/environments/{name}"))
                })
                .map(|(_, _, body)| body["deployment_branch_policy"].clone())
                .unwrap()
        };
        let tag_policy_posts = |client: &FakeClient| {
            client
                .writes
                .iter()
                .filter(|(method, path, _)| {
                    method == "POST" && path.ends_with("/deployment-branch-policies")
                })
                .count()
        };
        let mut client = FakeClient::default();
        apply_governance(&mut client, &config(), "ultradotdev/tide:thekeith").unwrap();
        assert_eq!(environment(&client, "release-signing"), Value::Null);
        assert_eq!(
            environment(&client, "release"),
            json!({ "protected_branches": false, "custom_branch_policies": true })
        );
        assert_eq!(tag_policy_posts(&client), 1);

        let mut again = FakeClient {
            existing_tag_policy: true,
            ..FakeClient::default()
        };
        apply_governance(&mut again, &config(), "ultradotdev/tide:thekeith").unwrap();
        assert_eq!(tag_policy_posts(&again), 0);
    }

    #[test]
    fn rule_parameters_become_api_field_names() {
        assert_eq!(snake_case("requiredStatusChecks"), "required_status_checks");
        assert_eq!(snake_case("strict"), "strict");
    }
}
