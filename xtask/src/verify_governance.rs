//! Offline governance policy verifier: config, workflows, CODEOWNERS and tooling.
use crate::governance::{Ruleset, governance_plan, load_config};
use crate::util::read_text;
use anyhow::{Context, Result, bail, ensure};
use std::path::Path;

/// A named offline check.
pub type Scenario<'a> = (&'a str, Box<dyn Fn() -> Result<()> + 'a>);

/// `verify-github-governance`: offline policy, workflow and tooling checks.
pub fn verify(repository_root: &Path, list: bool, scenario: Option<&str>) -> Result<()> {
    let config = load_config(repository_root)?;
    let read = |path: &str| read_text(&repository_root.join(path));
    let workflow = read(".github/workflows/ci.yml")?;
    let release_workflow = read(".github/workflows/release.yml")?;
    let release_contract = read("xtask/src/workflow_contract.rs")?;
    let tool = read("xtask/src/governance.rs")?;
    let codeowners = read(".github/CODEOWNERS")?;
    let ruleset = |name: &str| {
        config
            .rulesets
            .iter()
            .find(|item| item.name == name)
            .with_context(|| format!("Missing ruleset {name}"))
    };
    let only = |values: &[String], login: &str| values.len() == 1 && values[0] == login;
    let has_rule = |set: &Ruleset, kind: &str| set.rules.iter().any(|rule| rule.r#type == kind);
    let first_bypass = |set: &Ruleset| set.bypass_actors.first().map(|actor| actor.login.clone());
    let access = &config.expected_access;
    let position = |step: &str| config.cutover_order.iter().position(|item| item == step);

    let scenarios: Vec<Scenario<'_>> = vec![
        (
            "sole-owner-and-admin",
            Box::new(|| {
                ensure!(
                    only(&access.organization_owners, "thekeith"),
                    "Organization owner policy drifted"
                );
                ensure!(
                    only(&access.repository_admins, "thekeith"),
                    "Repository admin policy drifted"
                );
                ensure!(
                    only(&access.repository_writers, "thekeith"),
                    "Repository writer policy drifted"
                );
                ensure!(
                    access.write_apps.len()
                        + access.write_teams.len()
                        + access.write_deploy_keys.len()
                        == 0,
                    "Secondary write actors are allowed"
                );
                Ok(())
            }),
        ),
        (
            "protected-branch-pr-only",
            Box::new(|| {
                let set = ruleset("Tide protected branch integrity")?;
                ensure!(
                    set.bypass_actors.is_empty(),
                    "Integrity ruleset must have no bypass"
                );
                ensure!(
                    has_rule(set, "pull_request"),
                    "Pull requests are not required"
                );
                ensure!(
                    has_rule(set, "deletion") && has_rule(set, "non_fast_forward"),
                    "Deletion or force pushes remain possible"
                );
                Ok(())
            }),
        ),
        (
            "owner-only-merge-authority",
            Box::new(|| {
                let set = ruleset("Tide owner-only protected branch updates")?;
                ensure!(
                    set.rules.len() == 1 && set.rules[0].r#type == "update",
                    "Update restriction drifted"
                );
                ensure!(
                    set.bypass_actors.len() == 1
                        && set.bypass_actors[0].login == "thekeith"
                        && set.bypass_actors[0].bypass_mode == "pull_request",
                    "Only the owner may update through PRs"
                );
                ensure!(codeowners.trim() == "* @thekeith", "CODEOWNERS drifted");
                Ok(())
            }),
        ),
        (
            "required-checks-non-bypassable",
            Box::new(|| {
                let set = ruleset("Tide protected branch integrity")?;
                ensure!(
                    set.bypass_actors.is_empty(),
                    "Required checks are bypassable"
                );
                for check in &config.required_checks {
                    // A check is a literal job name or a matrix `check:` value used as the job name.
                    ensure!(
                        workflow.contains(&format!("name: {check}"))
                            || (workflow.contains(&format!("check: {check}"))
                                && workflow.contains("name: ${{ matrix.check }}")),
                        "Stable CI job missing: {check}"
                    );
                }
                ensure!(
                    workflow.contains("branches: [main, release, 'release/**']"),
                    "CI does not cover every protected branch family"
                );
                Ok(())
            }),
        ),
        (
            "release-branch-creation-authority",
            Box::new(|| {
                let set = ruleset("Tide owner-only release branch creation")?;
                ensure!(
                    set.rules
                        .first()
                        .is_some_and(|rule| rule.r#type == "creation")
                        && first_bypass(set).as_deref() == Some("thekeith"),
                    "Release branch creation is not owner-only"
                );
                Ok(())
            }),
        ),
        (
            "owner-only-version-tag-creation",
            Box::new(|| {
                let set = ruleset("Tide owner-only version tag creation")?;
                ensure!(
                    set.include.iter().any(|item| item == "refs/tags/v*")
                        && first_bypass(set).as_deref() == Some("thekeith"),
                    "Version tag creation is not owner-only"
                );
                ensure!(
                    release_workflow.contains("release-workflow-contract preflight")
                        && release_contract.contains("context.actor == policy.publication_actor")
                        && release_contract
                            .contains("context.triggering_actor == policy.publication_actor"),
                    "Release workflow actor checks are missing"
                );
                Ok(())
            }),
        ),
        (
            "version-tag-immutability",
            Box::new(|| {
                let set = ruleset("Tide immutable version tags")?;
                ensure!(
                    set.bypass_actors.is_empty(),
                    "Version tag immutability is bypassable"
                );
                for rule in ["deletion", "update", "non_fast_forward"] {
                    ensure!(has_rule(set, rule), "Tag immutability lacks {rule}");
                }
                Ok(())
            }),
        ),
        (
            "release-environment-owner-review",
            Box::new(|| {
                for environment in &config.environments {
                    ensure!(
                        environment.reviewers.len() == 1
                            && environment.reviewers[0].login == "thekeith",
                        "{} reviewer drifted",
                        environment.name
                    );
                    ensure!(
                        !environment.prevent_self_review,
                        "{} must permit sole-owner review",
                        environment.name
                    );
                }
                ensure!(
                    config.environments.iter().any(|item| {
                        item.name == "release"
                            && item
                                .deployment_tag_patterns
                                .iter()
                                .any(|pattern| pattern == "v*")
                    }),
                    "Release environment tag restriction is missing"
                );
                Ok(())
            }),
        ),
        (
            "actions-token-least-privilege",
            Box::new(|| {
                ensure!(
                    workflow.contains("permissions:\n  contents: read"),
                    "CI token is not read-only"
                );
                ensure!(
                    release_workflow.contains("permissions:\n  contents: read"),
                    "Release workflow default token is not read-only"
                );
                ensure!(
                    config.repository_settings.default_workflow_permissions == "read"
                        && !config
                            .repository_settings
                            .workflows_may_approve_pull_requests,
                    "Repository Actions defaults are too broad"
                );
                Ok(())
            }),
        ),
        (
            "no-write-capable-secondary-actors",
            Box::new(|| {
                ensure!(
                    access.write_apps.is_empty()
                        && access.write_teams.is_empty()
                        && access.write_deploy_keys.is_empty(),
                    "A secondary release-capable actor is permitted"
                );
                ensure!(
                    tool.contains("Refusing governance changes because access is unexpected"),
                    "Apply does not fail closed on unexpected access"
                );
                Ok(())
            }),
        ),
        (
            "public-cutover-order",
            Box::new(|| {
                ensure!(
                    position("audit-governance") < position("add-public-remote-and-push-source"),
                    "Governance must be audited before source push"
                );
                ensure!(
                    position("rehearse-signed-artifacts-and-hosted-ci")
                        < position("create-release-tag"),
                    "Rehearsal and hosted CI must precede tagging"
                );
                Ok(())
            }),
        ),
        (
            "live-governance-audit",
            Box::new(|| {
                ensure!(
                    governance_plan(&config)
                        .last()
                        .is_some_and(|step| step.starts_with("audit resulting governance")),
                    "Governance plan lacks a final live audit"
                );
                ensure!(
                    tool.contains("Mode::Audit => audit_governance(&mut GhCliClient"),
                    "Read-only live audit entrypoint is missing"
                );
                ensure!(
                    tool.contains("Mode::Apply =>") && tool.contains("--confirm-apply="),
                    "Apply is not independently confirmation-gated"
                );
                Ok(())
            }),
        ),
    ];
    run_scenarios("GitHub governance", &scenarios, list, scenario)
}

/// Runs named scenario checks with `--list` / `--scenario=` semantics.
pub fn run_scenarios(
    label: &str,
    scenarios: &[Scenario<'_>],
    list: bool,
    requested: Option<&str>,
) -> Result<()> {
    if list {
        for (name, _) in scenarios {
            println!("{name}");
        }
        return Ok(());
    }
    let selected: Vec<&str> = match requested {
        Some(name) => vec![name],
        None => scenarios.iter().map(|(name, _)| *name).collect(),
    };
    for name in selected {
        let Some((_, check)) = scenarios.iter().find(|(id, _)| *id == name) else {
            bail!("Unknown {label} scenario: {name}");
        };
        check()?;
        println!("PASS {name}");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::util::repo_root;

    #[test]
    fn repository_policy_scenarios_pass() {
        verify(&repo_root(), false, None).unwrap();
        assert!(verify(&repo_root(), false, Some("no-such-scenario")).is_err());
    }
}
