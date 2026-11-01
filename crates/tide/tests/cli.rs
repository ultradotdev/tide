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
fn help_mentions_the_terminal_editor() {
    Command::cargo_bin("tide")
        .unwrap()
        .arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("terminal-native"))
        .stdout(predicate::str::contains("--check-terminal"));
}

/// A copy of the built executable at a Homebrew keg path: `<prefix>/Cellar/tide/<v>/bin/tide`.
fn homebrew_keg(root: &std::path::Path) -> std::path::PathBuf {
    let bin = root.join("Cellar/tide/0.10.0/bin");
    std::fs::create_dir_all(&bin).unwrap();
    let keg = bin.join("tide");
    std::fs::copy(assert_cmd::cargo::cargo_bin("tide"), &keg).unwrap();
    keg
}

#[test]
fn homebrew_installs_refuse_managed_installation_commands() {
    let scratch = tempfile::tempdir().unwrap();
    let root = scratch.path().canonicalize().unwrap();
    let keg = homebrew_keg(&root.join("homebrew"));
    let store = root.join("home/.local/lib/tide");
    let run = |args: &[&str]| {
        let mut command = Command::new(&keg);
        command
            .args(args)
            .env("HOME", root.join("home"))
            .env("TIDE_INSTALL_ROOT", &store)
            .env("TIDE_BIN_DIR", root.join("home/.local/bin"));
        command.assert()
    };
    run(&["--uninstall"])
        .failure()
        .stderr(predicate::str::contains("brew uninstall tide"));
    run(&["--uninstall", "--purge", "--yes"])
        .failure()
        .stderr(predicate::str::contains("brew uninstall tide"));
    run(&["--install-local"])
        .failure()
        .stderr(predicate::str::contains("brew upgrade tide"));
    run(&["--activate-existing", "0.10.0"])
        .failure()
        .stderr(predicate::str::contains("brew upgrade tide"));
    assert!(!root.join("home/.local").exists());
    run(&["--version"])
        .success()
        .stdout(predicate::str::contains(env!("CARGO_PKG_VERSION")));
}
