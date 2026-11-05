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

/// A multi-step Git command that has stopped part way, usually on a conflict.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OperationKind {
    Rebase,
    Merge,
    CherryPick,
    Revert,
    Bisect,
    /// `git am`, applying patches from a mailbox.
    ApplyPatches,
}

/// An operation in progress in the repository, with what Git records about it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitOperation {
    pub kind: OperationKind,
    /// The branch being rebased or bisected, when Git recorded one.
    pub branch: Option<String>,
    /// The commit being rebased onto, cherry-picked, reverted or merged in.
    pub target: Option<String>,
    /// The step Git is on and how many there are, for rebases and `git am`.
    pub progress: Option<(usize, usize)>,
}

impl GitOperation {
    /// One line for people, such as "Rebasing main onto 1a2b3c4 (3/7)".
    #[must_use]
    pub fn describe(&self) -> String {
        let short = |id: &str| id.chars().take(8).collect::<String>();
        let progress = self
            .progress
            .map(|(step, total)| format!(" ({step}/{total})"))
            .unwrap_or_default();
        match (self.kind, &self.branch, &self.target) {
            (OperationKind::Rebase, Some(branch), Some(onto)) => {
                format!("Rebasing {branch} onto {}{progress}", short(onto))
            }
            (OperationKind::Rebase, None, Some(onto)) => {
                format!("Rebasing onto {}{progress}", short(onto))
            }
            (OperationKind::Rebase, ..) => format!("Rebase in progress{progress}"),
            (OperationKind::Merge, _, Some(other)) => format!("Merging {}", short(other)),
            (OperationKind::Merge, ..) => "Merge in progress".into(),
            (OperationKind::CherryPick, _, Some(commit)) => {
                format!("Cherry-picking {}", short(commit))
            }
            (OperationKind::CherryPick, ..) => "Cherry-pick in progress".into(),
            (OperationKind::Revert, _, Some(commit)) => format!("Reverting {}", short(commit)),
            (OperationKind::Revert, ..) => "Revert in progress".into(),
            (OperationKind::Bisect, Some(branch), _) => format!("Bisecting {branch}"),
            (OperationKind::Bisect, ..) => "Bisect in progress".into(),
            (OperationKind::ApplyPatches, ..) => format!("Applying patches{progress}"),
        }
    }

    /// A few words for the status bar, such as "Rebasing 3/7".
    #[must_use]
    pub fn short_label(&self) -> String {
        let verb = match self.kind {
            OperationKind::Rebase => "Rebasing",
            OperationKind::Merge => "Merging",
            OperationKind::CherryPick => "Cherry-picking",
            OperationKind::Revert => "Reverting",
            OperationKind::Bisect => "Bisecting",
            OperationKind::ApplyPatches => "Applying",
        };
        match self.progress {
            Some((step, total)) => format!("{verb} {step}/{total}"),
            None => verb.into(),
        }
    }
}

/// The operation Git has in progress in `git_dir`, read from the files Git keeps there.
#[must_use]
pub fn operation_in_progress(git_dir: &Path) -> Option<GitOperation> {
    let read = |name: &str| {
        std::fs::read_to_string(git_dir.join(name))
            .ok()
            .map(|text| text.trim().to_owned())
            .filter(|text| !text.is_empty())
    };
    let number = |name: &str| read(name).and_then(|text| text.parse::<usize>().ok());
    let branch = |name: &str| {
        read(name).map(|name| {
            name.strip_prefix("refs/heads/")
                .map_or_else(|| name.clone(), str::to_owned)
        })
    };
    let operation = |kind, branch, target, progress| GitOperation {
        kind,
        branch,
        target,
        progress,
    };
    let steps = |step: Option<usize>, total: Option<usize>| step.zip(total);
    if git_dir.join("rebase-merge").is_dir() {
        return Some(operation(
            OperationKind::Rebase,
            branch("rebase-merge/head-name").filter(|name| name != "detached HEAD"),
            read("rebase-merge/onto"),
            steps(number("rebase-merge/msgnum"), number("rebase-merge/end")),
        ));
    }
    if git_dir.join("rebase-apply").is_dir() {
        let progress = steps(number("rebase-apply/next"), number("rebase-apply/last"));
        return Some(if git_dir.join("rebase-apply/applying").exists() {
            operation(OperationKind::ApplyPatches, None, None, progress)
        } else {
            operation(
                OperationKind::Rebase,
                branch("rebase-apply/head-name").filter(|name| name != "detached HEAD"),
                read("rebase-apply/onto"),
                progress,
            )
        });
    }
    for (file, kind) in [
        ("MERGE_HEAD", OperationKind::Merge),
        ("CHERRY_PICK_HEAD", OperationKind::CherryPick),
        ("REVERT_HEAD", OperationKind::Revert),
    ] {
        if git_dir.join(file).exists() {
            let target = read(file).and_then(|text| text.lines().next().map(str::to_owned));
            return Some(operation(kind, None, target, None));
        }
    }
    if git_dir.join("BISECT_LOG").exists() {
        return Some(operation(
            OperationKind::Bisect,
            read("BISECT_START").filter(|start| !start.chars().all(|ch| ch.is_ascii_hexdigit())),
            None,
            None,
        ));
    }
    None
}

/// Read-only repository state used by the Phase 0 Git spike.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitSnapshot {
    pub root: PathBuf,
    /// The checked-out branch; None when HEAD is detached, as it is during a rebase.
    pub branch: Option<String>,
    pub head: Option<String>,
    /// A rebase, merge, cherry-pick, revert, bisect or `git am` that has not finished.
    pub operation: Option<GitOperation>,
    pub dirty: bool,
    /// Local graph divergence; None means no upstream or bounded traversal unavailable.
    pub ahead_behind: Option<(usize, usize)>,
    pub changed_paths: Vec<String>,
    pub changes: Vec<GitChange>,
}

/// Inspect a repository with `gix` without shelling out to `git`.
/// The project folder for `path`: the working tree of the nearest Git repository at or
/// above it, or `None` when there is none.
///
/// A submodule gives way to the repository that contains it, so a file in a submodule
/// opens the superproject. A linked worktree is its own project. The search never looks
/// in `home` or above it: a repository there (a dotfiles repo, say) would make every file
/// under the home folder open the whole home folder.
#[must_use]
pub fn project_root(path: &Path, home: Option<&Path>) -> Option<PathBuf> {
    let mut found = None;
    for dir in path.ancestors() {
        if home.is_some_and(|home| home.starts_with(dir)) {
            break;
        }
        if std::fs::symlink_metadata(dir.join(".git")).is_err() {
            continue;
        }
        let Ok(repo) = gix::open(dir) else {
            continue;
        };
        found = Some(dir.to_path_buf());
        if repo.kind() != gix::repository::Kind::Submodule {
            break;
        }
    }
    found
}

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
        operation: operation_in_progress(repo.git_dir()),
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
    fn detached_head_and_a_stopped_rebase_are_reported_with_their_changes() {
        let dir = init_repo();
        let main = String::from_utf8(
            Command::new("git")
                .arg("-C")
                .arg(dir.path())
                .args(["branch", "--show-current"])
                .output()
                .unwrap()
                .stdout,
        )
        .unwrap()
        .trim()
        .to_owned();
        git(dir.path(), &["checkout", "-q", "--detach"]);
        let detached = repository_snapshot(dir.path()).unwrap();
        assert_eq!(detached.branch, None);
        assert!(detached.head.is_some());
        assert_eq!(detached.operation, None);

        git(dir.path(), &["checkout", "-q", &main]);
        git(dir.path(), &["checkout", "-q", "-b", "other"]);
        std::fs::write(dir.path().join("file.txt"), "other\n").unwrap();
        git(dir.path(), &["commit", "-qam", "other"]);
        git(dir.path(), &["checkout", "-q", &main]);
        for (index, content) in ["two\n", "three\n"].into_iter().enumerate() {
            std::fs::write(dir.path().join("file.txt"), content).unwrap();
            git(dir.path(), &["commit", "-qam", &format!("main {index}")]);
        }
        let onto = String::from_utf8(
            Command::new("git")
                .arg("-C")
                .arg(dir.path())
                .args(["rev-parse", "other"])
                .output()
                .unwrap()
                .stdout,
        )
        .unwrap();
        let status = Command::new("git")
            .arg("-C")
            .arg(dir.path())
            .args(["rebase", "-q", "other"])
            .output()
            .unwrap();
        assert!(!status.status.success(), "the rebase stops on a conflict");
        let rebasing = repository_snapshot(dir.path()).unwrap();
        assert_eq!(rebasing.branch, None);
        let operation = rebasing.operation.expect("a rebase in progress");
        assert_eq!(operation.kind, OperationKind::Rebase);
        assert_eq!(operation.branch.as_deref(), Some(main.as_str()));
        assert_eq!(operation.progress, Some((1, 2)));
        assert_eq!(
            operation.describe(),
            format!("Rebasing {main} onto {} (1/2)", &onto[..8])
        );
        assert_eq!(operation.short_label(), "Rebasing 1/2");
        assert!(rebasing.changes.iter().any(
            |change| change.path == "file.txt" && change.worktree == Some(GitStatus::Conflict)
        ));

        git(dir.path(), &["rebase", "--abort"]);
        // The merge conflicts too; Git leaves MERGE_HEAD for the commit that finishes it.
        let merge = Command::new("git")
            .arg("-C")
            .arg(dir.path())
            .args(["merge", "-q", "other"])
            .output()
            .unwrap();
        assert!(!merge.status.success());
        let merging = repository_snapshot(dir.path()).unwrap();
        assert_eq!(merging.branch.as_deref(), Some(main.as_str()));
        assert_eq!(
            merging.operation.map(|operation| operation.kind),
            Some(OperationKind::Merge)
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

    fn canonical(dir: &tempfile::TempDir) -> PathBuf {
        std::fs::canonicalize(dir.path()).unwrap()
    }

    #[test]
    fn project_root_is_the_nearest_repository() {
        let outer = init_repo();
        let outer_root = canonical(&outer);
        let file = outer_root.join("src/deep/main.rs");
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        std::fs::write(&file, "").unwrap();
        assert_eq!(project_root(&file, None), Some(outer_root.clone()));
        assert_eq!(
            project_root(file.parent().unwrap(), None),
            Some(outer_root.clone())
        );

        let inner = outer_root.join("vendor/clone");
        std::fs::create_dir_all(&inner).unwrap();
        git(&inner, &["init", "-q"]);
        let inner_file = inner.join("lib.rs");
        std::fs::write(&inner_file, "").unwrap();
        assert_eq!(project_root(&inner_file, None), Some(inner));
    }

    #[test]
    fn project_root_without_a_repository_is_none() {
        let dir = tempfile::tempdir().unwrap();
        let file = canonical(&dir).join("notes.txt");
        std::fs::write(&file, "").unwrap();
        // A temporary folder may sit inside some repository; only check when it doesn't.
        if gix::discover(dir.path()).is_err() {
            assert_eq!(project_root(&file, None), None);
        }
    }

    #[test]
    fn project_root_never_reaches_the_home_folder() {
        let home = init_repo();
        let home_root = canonical(&home);
        let file = home_root.join("Documents/notes/todo.md");
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        std::fs::write(&file, "").unwrap();
        assert_eq!(project_root(&file, Some(&home_root)), None);
        assert_eq!(project_root(&home_root, Some(&home_root)), None);

        let project = home_root.join("Development/app");
        std::fs::create_dir_all(&project).unwrap();
        git(&project, &["init", "-q"]);
        assert_eq!(
            project_root(&project.join("src"), Some(&home_root)),
            Some(project)
        );
    }

    #[test]
    fn project_root_climbs_out_of_a_submodule() {
        let library = init_repo();
        let parent = init_repo();
        let parent_root = canonical(&parent);
        git(
            &parent_root,
            &[
                "-c",
                "protocol.file.allow=always",
                "submodule",
                "add",
                "-q",
                library.path().to_str().unwrap(),
                "libs/library",
            ],
        );
        let file = parent_root.join("libs/library/file.txt");
        assert!(file.is_file());
        assert_eq!(project_root(&file, None), Some(parent_root));
    }

    #[test]
    fn project_root_of_a_linked_worktree_is_the_worktree() {
        let main = init_repo();
        let scratch = tempfile::tempdir().unwrap();
        let worktree = canonical(&scratch).join("feature");
        git(
            main.path(),
            &[
                "worktree",
                "add",
                "-q",
                "-b",
                "feature",
                worktree.to_str().unwrap(),
            ],
        );
        assert_eq!(
            project_root(&worktree.join("file.txt"), None),
            Some(worktree)
        );
    }
}
