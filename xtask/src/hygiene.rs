//! Repository hygiene: tracked files must not record a contributor's machine.
use crate::util::{finish, tracked_files};
use anyhow::{Context, Result};
use regex::Regex;
use std::path::Path;

/// macOS home directories and per-user temporary directories name one machine's user.
fn machine_path() -> Result<Regex> {
    Ok(Regex::new(
        r"/(?:Users|private/var/folders)/[A-Za-z0-9._-]+/",
    )?)
}

/// Every `path:line` in a tracked text file that contains a machine-specific path.
fn findings(root: &Path) -> Result<Vec<String>> {
    let tracked = tracked_files(root)?
        .with_context(|| format!("{} is not the top of a Git work tree", root.display()))?;
    let pattern = machine_path()?;
    let mut found = Vec::new();
    for name in tracked {
        let Ok(bytes) = std::fs::read(root.join(&name)) else {
            // Deleted in the work tree but still in the index.
            continue;
        };
        if bytes.contains(&0) {
            continue;
        }
        for (index, line) in String::from_utf8_lossy(&bytes).split('\n').enumerate() {
            if pattern.is_match(line) {
                found.push(format!(
                    "{name}:{}: machine-specific absolute path",
                    index + 1
                ));
            }
        }
    }
    Ok(found)
}

/// `verify-portable-paths`.
pub fn verify_portable_paths(root: &Path) -> Result<()> {
    finish(
        &findings(root)?,
        "Tracked files contain no machine-specific absolute paths.",
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;

    #[test]
    fn only_tracked_text_with_a_machine_path_fails() {
        let root = tempfile::tempdir().unwrap();
        let git = |args: &[&str]| {
            let status = Command::new("git")
                .args(args)
                .current_dir(root.path())
                .status()
                .unwrap();
            assert!(status.success());
        };
        git(&["init", "-q"]);
        let home = format!("/{}/alice/project", "Users");
        let scratch = format!("/private/var/{}/xy/T/tide", "folders");
        let write = |name: &str, text: &str| std::fs::write(root.path().join(name), text).unwrap();
        write(
            "clean.md",
            "Use ~/.local/bin, /usr/local/bin or /private/tmp.\n",
        );
        write("leaky.md", &format!("first\n{home}/notes\n{scratch}/x\n"));
        write("binary.bin", &format!("\0{home}/"));
        write("untracked.md", &format!("{home}/"));
        git(&["add", "clean.md", "leaky.md", "binary.bin"]);
        assert_eq!(
            findings(root.path()).unwrap(),
            [
                "leaky.md:2: machine-specific absolute path",
                "leaky.md:3: machine-specific absolute path",
            ]
        );
        assert!(verify_portable_paths(root.path()).is_err());
    }

    #[test]
    fn an_untracked_tree_is_refused() {
        let root = tempfile::tempdir().unwrap();
        assert!(findings(root.path()).is_err());
    }

    #[test]
    fn the_repository_is_portable() {
        verify_portable_paths(&crate::util::repo_root()).unwrap();
    }
}
