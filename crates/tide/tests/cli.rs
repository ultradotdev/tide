use assert_cmd::Command;
use predicates::prelude::*;

#[test]
fn version_reports_the_crate_version() {
    Command::cargo_bin("tide")
        .unwrap()
        .arg("--version")
        .assert()
        .success()
        .stdout(predicate::str::contains(env!("CARGO_PKG_VERSION")));
}

#[test]
fn shared_session_writes_are_refused_until_compatibility_is_proven() {
    Command::cargo_bin("tide")
        .unwrap()
        .arg("--shared-sessions")
        .arg(".")
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "shared TypeScript session writes are disabled",
        ));
}

#[test]
fn help_mentions_the_terminal_editor() {
    Command::cargo_bin("tide")
        .unwrap()
        .arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("terminal-native"))
        .stdout(predicate::str::contains("--check-terminal"));
}
