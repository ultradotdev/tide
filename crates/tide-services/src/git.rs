pub mod diff;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum GitStatus {
    Added,
    Modified,
    Deleted,
    Renamed,
    Copied,
    TypeChanged,
    Conflict,
    Untracked,
}
impl GitStatus {
    pub fn label(self) -> &'static str {
        match self {
            Self::Added => "A",
            Self::Modified => "M",
            Self::Deleted => "D",
            Self::Renamed => "R",
            Self::Copied => "C",
            Self::TypeChanged => "T",
            Self::Conflict => "U",
            Self::Untracked => "?",
        }
    }
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GitChange {
    pub path: String,
    pub previous_path: Option<String>,
    pub index: Option<GitStatus>,
    pub worktree: Option<GitStatus>,
    /// A path that is not UTF-8 is shown lossily; path-based actions must refuse it.
    #[serde(default)]
    pub lossy_path: bool,
}
impl GitChange {
    fn new(path: &[u8]) -> Self {
        Self {
            path: String::from_utf8_lossy(path).into_owned(),
            previous_path: None,
            index: None,
            worktree: None,
            lossy_path: std::str::from_utf8(path).is_err(),
        }
    }
    fn at<'a>(changes: &'a mut BTreeMap<Vec<u8>, GitChange>, path: &[u8]) -> &'a mut GitChange {
        changes
            .entry(path.to_vec())
            .or_insert_with(|| GitChange::new(path))
    }
    fn set_previous(&mut self, previous: &[u8]) {
        self.previous_path = Some(String::from_utf8_lossy(previous).into_owned());
        self.lossy_path |= std::str::from_utf8(previous).is_err();
    }
}

/// Read-only repository state used by the Phase 0 Git spike.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitSnapshot {
    pub root: PathBuf,
    pub branch: Option<String>,
    pub head: Option<String>,
    pub dirty: bool,
    /// Local graph divergence; None means no upstream or bounded traversal unavailable.
    pub ahead_behind: Option<(usize, usize)>,
    pub changed_paths: Vec<String>,
    pub changes: Vec<GitChange>,
}

/// Inspect a repository with `gix` without shelling out to `git`.
pub fn repository_snapshot(path: impl AsRef<Path>) -> Result<GitSnapshot> {
    repository_snapshot_inner(
        path,
        std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
        false,
    )
}
pub fn repository_snapshot_cancellable(
    path: impl AsRef<Path>,
    cancel: std::sync::Arc<std::sync::atomic::AtomicBool>,
) -> Result<GitSnapshot> {
    repository_snapshot_inner(path, cancel, true)
}
fn repository_snapshot_inner(
    path: impl AsRef<Path>,
    cancel: std::sync::Arc<std::sync::atomic::AtomicBool>,
    include_divergence: bool,
) -> Result<GitSnapshot> {
    let repo = gix::discover(path.as_ref())
        .with_context(|| format!("discover a Git repository from {}", path.as_ref().display()))?;
    let root = repo
        .workdir()
        .map_or_else(|| repo.git_dir().to_path_buf(), Path::to_path_buf);
    let branch = repo
        .head_name()
        .ok()
        .flatten()
        .map(|name| name.shorten().to_string());
    let head = repo.head_id().ok().map(|id| id.to_string());
    let mut changes: BTreeMap<Vec<u8>, GitChange> = BTreeMap::new();
    let platform = repo
        .status(gix::progress::Discard)?
        .untracked_files(gix::status::UntrackedFiles::Files)
        .should_interrupt_owned(cancel.clone());
    let iter = platform.into_iter(Vec::new())?;
    for item in iter {
        anyhow::ensure!(
            !cancel.load(std::sync::atomic::Ordering::Acquire),
            "Git status cancelled"
        );
        anyhow::ensure!(
            changes.len() < 50_000,
            "Git status exceeds 50,000 paths; narrow the workspace"
        );
        match item? {
            gix::status::Item::IndexWorktree(gix::status::index_worktree::Item::Modification {
                rela_path,
                status,
                ..
            }) => {
                use gix::status::plumbing::index_as_worktree::{Change, EntryStatus};
                let status = match status {
                    EntryStatus::Conflict { .. } => GitStatus::Conflict,
                    EntryStatus::IntentToAdd => GitStatus::Added,
                    EntryStatus::Change(Change::Removed) => GitStatus::Deleted,
                    EntryStatus::Change(Change::Type { .. }) => GitStatus::TypeChanged,
                    EntryStatus::Change(_) => GitStatus::Modified,
                    EntryStatus::NeedsUpdate(_) => continue,
                };
                let change = GitChange::at(&mut changes, &rela_path);
                change.worktree = Some(status);
                if status == GitStatus::Conflict {
                    change.index = Some(status);
                }
            }
            gix::status::Item::IndexWorktree(
                gix::status::index_worktree::Item::DirectoryContents { entry, .. },
            ) => {
                if entry.status != gix::dir::entry::Status::Untracked {
                    continue;
                }
                GitChange::at(&mut changes, &entry.rela_path).worktree = Some(GitStatus::Untracked);
            }
            gix::status::Item::IndexWorktree(gix::status::index_worktree::Item::Rewrite {
                source,
                dirwalk_entry,
                copy,
                ..
            }) => {
                let change = GitChange::at(&mut changes, &dirwalk_entry.rela_path);
                change.set_previous(source.rela_path());
                change.worktree = Some(if copy {
                    GitStatus::Copied
                } else {
                    GitStatus::Renamed
                });
            }
            gix::status::Item::TreeIndex(change) => {
                use gix::diff::index::ChangeRef;
                let (path, status, previous) = match &change {
                    ChangeRef::Addition { location, .. } => (location, GitStatus::Added, None),
                    ChangeRef::Deletion { location, .. } => (location, GitStatus::Deleted, None),
                    ChangeRef::Modification {
                        location,
                        previous_entry_mode,
                        entry_mode,
                        ..
                    } => (
                        location,
                        if previous_entry_mode.bits() & 0o170000 != entry_mode.bits() & 0o170000 {
                            GitStatus::TypeChanged
                        } else {
                            GitStatus::Modified
                        },
                        None,
                    ),
                    ChangeRef::Rewrite {
                        location,
                        source_location,
                        copy,
                        ..
                    } => (
                        location,
                        if *copy {
                            GitStatus::Copied
                        } else {
                            GitStatus::Renamed
                        },
                        Some(source_location),
                    ),
                };
                let change = GitChange::at(&mut changes, path);
                change.index = Some(status);
                if let Some(previous) = previous {
                    change.set_previous(previous);
                }
            }
        }
    }
    let changes = changes.into_values().collect::<Vec<_>>();
    let changed_paths = changes
        .iter()
        .map(|change| change.path.clone())
        .collect::<Vec<_>>();
    let dirty = !changes.is_empty();

    Ok(GitSnapshot {
        root,
        branch,
        head,
        dirty: dirty || !changed_paths.is_empty(),
        ahead_behind: if include_divergence {
            divergence(&repo, &cancel)
        } else {
            None
        },
        changed_paths,
        changes,
    })
}

fn divergence(
    repo: &gix::Repository,
    cancel: &std::sync::atomic::AtomicBool,
) -> Option<(usize, usize)> {
    let head = repo.head_id().ok()?.detach();
    let branch = repo.head_ref().ok()??;
    let name = if branch
        .remote_name(gix::remote::Direction::Fetch)
        .is_some_and(|name| name.as_bstr() == gix::bstr::BStr::new("."))
    {
        branch
            .remote_ref_name(gix::remote::Direction::Fetch)?
            .ok()?
    } else {
        branch
            .remote_tracking_ref_name(gix::remote::Direction::Fetch)?
            .ok()?
    };
    let upstream = repo
        .find_reference(name.as_ref())
        .ok()?
        .peel_to_id()
        .ok()?
        .detach();
    if head == upstream {
        return Some((0, 0));
    }
    let deadline = std::time::Instant::now() + std::time::Duration::from_millis(250);
    let exclusive = |tip: gix::ObjectId, hidden: gix::ObjectId| -> Option<usize> {
        let mut count = 0;
        for item in repo.rev_walk([tip]).with_hidden([hidden]).all().ok()? {
            if cancel.load(std::sync::atomic::Ordering::Acquire)
                || std::time::Instant::now() >= deadline
            {
                return None;
            }
            item.ok()?;
            count += 1;
        }
        Some(count)
    };
    Some((exclusive(head, upstream)?, exclusive(upstream, head)?))
}

#[cfg(test)]
mod tests {
    use std::process::Command;

    use super::*;

    fn git(dir: &Path, args: &[&str]) {
        let status = Command::new("git")
            .arg("-C")
            .arg(dir)
            .args(args)
            .status()
            .unwrap();
        assert!(status.success(), "git {args:?} failed");
    }

    fn init_repo() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        git(dir.path(), &["init", "-q"]);
        git(dir.path(), &["config", "user.email", "spike@tide.local"]);
        git(dir.path(), &["config", "user.name", "Tide Spike"]);
        std::fs::write(dir.path().join("file.txt"), "one\n").unwrap();
        git(dir.path(), &["add", "file.txt"]);
        git(dir.path(), &["commit", "-q", "-m", "initial"]);
        dir
    }

    #[test]
    fn reports_local_ahead_and_behind_without_network() {
        let root = tempfile::tempdir().unwrap();
        git(root.path(), &["init", "-b", "main"]);
        git(
            root.path(),
            &[
                "-c",
                "user.name=Fixture",
                "-c",
                "user.email=fixture@example.test",
                "commit",
                "--allow-empty",
                "-m",
                "base",
            ],
        );
        git(root.path(), &["branch", "upstream"]);
        git(root.path(), &["config", "branch.main.remote", "."]);
        git(
            root.path(),
            &["config", "branch.main.merge", "refs/heads/upstream"],
        );
        git(
            root.path(),
            &[
                "-c",
                "user.name=Fixture",
                "-c",
                "user.email=fixture@example.test",
                "commit",
                "--allow-empty",
                "-m",
                "local",
            ],
        );
        git(root.path(), &["checkout", "upstream"]);
        git(
            root.path(),
            &[
                "-c",
                "user.name=Fixture",
                "-c",
                "user.email=fixture@example.test",
                "commit",
                "--allow-empty",
                "-m",
                "remote",
            ],
        );
        git(root.path(), &["checkout", "main"]);
        assert_eq!(
            repository_snapshot_cancellable(
                root.path(),
                std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false))
            )
            .unwrap()
            .ahead_behind,
            Some((1, 1))
        );
    }

    #[test]
    fn ahead_behind_is_counted_on_a_long_history() {
        use std::io::Write;
        let root = tempfile::tempdir().unwrap();
        git(root.path(), &["init", "-q", "-b", "main"]);
        let mut stream = String::new();
        let mut commit = |mark: usize, branch: &str, parent: Option<usize>| {
            stream.push_str(&format!(
                "commit refs/heads/{branch}\nmark :{mark}\ncommitter Fixture <fixture@example.test> {} +0000\ndata 0\n",
                1_000_000 + mark
            ));
            if let Some(parent) = parent {
                stream.push_str(&format!("from :{parent}\n"));
            }
            stream.push('\n');
        };
        commit(1, "main", None);
        for mark in 2..=20_000 {
            commit(mark, "main", Some(mark - 1));
        }
        commit(20_001, "upstream", Some(20_000));
        commit(20_002, "main", Some(20_000));
        commit(20_003, "main", Some(20_002));
        let mut import = Command::new("git")
            .arg("-C")
            .arg(root.path())
            .args(["fast-import", "--quiet"])
            .stdin(std::process::Stdio::piped())
            .spawn()
            .unwrap();
        import
            .stdin
            .take()
            .unwrap()
            .write_all(stream.as_bytes())
            .unwrap();
        assert!(import.wait().unwrap().success());
        git(root.path(), &["config", "branch.main.remote", "."]);
        git(
            root.path(),
            &["config", "branch.main.merge", "refs/heads/upstream"],
        );
        let cancel = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        assert_eq!(
            repository_snapshot_cancellable(root.path(), cancel)
                .unwrap()
                .ahead_behind,
            Some((2, 1))
        );
    }

    #[test]
    fn non_utf8_filename_is_a_flagged_lossy_entry() {
        use std::os::unix::ffi::OsStrExt;
        let dir = init_repo();
        std::fs::write(dir.path().join("file.txt"), "two\n").unwrap();
        let blob = Command::new("git")
            .arg("-C")
            .arg(dir.path())
            .args(["hash-object", "-w", "file.txt"])
            .output()
            .unwrap();
        let mut info =
            format!("100644,{},", String::from_utf8(blob.stdout).unwrap().trim()).into_bytes();
        info.extend_from_slice(b"bad\xff.txt");
        let status = Command::new("git")
            .arg("-C")
            .arg(dir.path())
            .args(["update-index", "--add", "--cacheinfo"])
            .arg(std::ffi::OsStr::from_bytes(&info))
            .status()
            .unwrap();
        assert!(status.success());
        let snapshot = repository_snapshot(dir.path()).unwrap();
        let lossy = snapshot
            .changes
            .iter()
            .find(|change| change.path == "bad\u{fffd}.txt")
            .unwrap();
        assert!(lossy.lossy_path);
        assert_eq!(lossy.index, Some(GitStatus::Added));
        assert_eq!(lossy.worktree, Some(GitStatus::Deleted));
        let normal = snapshot
            .changes
            .iter()
            .find(|change| change.path == "file.txt")
            .unwrap();
        assert!(!normal.lossy_path);
        assert_eq!(normal.worktree, Some(GitStatus::Modified));
    }

    #[test]
    fn status_preserves_index_worktree_untracked_and_rename_identity() {
        let dir = init_repo();
        let file = dir.path().join("file.txt");
        std::fs::write(&file, "staged\n").unwrap();
        git(dir.path(), &["add", "file.txt"]);
        std::fs::write(&file, "unstaged\n").unwrap();
        std::fs::create_dir_all(dir.path().join("nested")).unwrap();
        std::fs::write(dir.path().join("nested/untouched.txt"), "new").unwrap();
        let snapshot = repository_snapshot(dir.path()).unwrap();
        let modified = snapshot
            .changes
            .iter()
            .find(|change| change.path == "file.txt")
            .unwrap();
        assert_eq!(modified.index, Some(GitStatus::Modified));
        assert_eq!(modified.worktree, Some(GitStatus::Modified));
        assert!(
            snapshot
                .changes
                .iter()
                .any(|change| change.path == "nested/untouched.txt"
                    && change.worktree == Some(GitStatus::Untracked))
        );
        git(dir.path(), &["add", "file.txt"]);
        git(dir.path(), &["commit", "-qm", "second"]);
        git(dir.path(), &["mv", "file.txt", "renamed.txt"]);
        let renamed = repository_snapshot(dir.path()).unwrap();
        assert!(
            renamed
                .changes
                .iter()
                .any(|change| change.path == "renamed.txt"
                    && change.index == Some(GitStatus::Renamed)
                    && change.previous_path.as_deref() == Some("file.txt"))
        );
    }

    #[test]
    fn reads_repository_state_without_git_cli() {
        let dir = init_repo();
        let snapshot = repository_snapshot(dir.path()).unwrap();
        assert!(snapshot.root.ends_with(dir.path().file_name().unwrap()));
        assert!(snapshot.branch.is_some());
        assert!(snapshot.head.is_some());
        assert!(!snapshot.dirty);
        assert!(snapshot.changed_paths.is_empty());
    }

    #[test]
    fn reports_a_modified_file() {
        let dir = init_repo();
        std::fs::write(dir.path().join("file.txt"), "two\n").unwrap();
        let snapshot = repository_snapshot(dir.path()).unwrap();
        assert!(snapshot.dirty);
        assert!(snapshot.changed_paths.iter().any(|path| path == "file.txt"));
    }

    #[test]
    fn reports_an_untracked_file() {
        let dir = init_repo();
        std::fs::write(dir.path().join("new.txt"), "new\n").unwrap();
        let snapshot = repository_snapshot(dir.path()).unwrap();
        assert!(snapshot.dirty);
        assert!(snapshot.changed_paths.iter().any(|path| path == "new.txt"));
    }
}
