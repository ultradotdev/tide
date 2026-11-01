//! Structured single-file diffs and guarded hunk editing.
use crate::git_process::git_cancellable;
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    io::Read,
    path::{Component, Path, PathBuf},
    sync::{Arc, Mutex, PoisonError, atomic::AtomicBool},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DiffKind {
    Context,
    Added,
    Deleted,
    Meta,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiffLine {
    pub kind: DiffKind,
    pub old_line: Option<usize>,
    pub new_line: Option<usize>,
    pub text: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiffHunk {
    pub old_start: usize,
    pub old_count: usize,
    pub new_start: usize,
    pub new_count: usize,
    pub header: String,
    pub lines: Vec<DiffLine>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileDiff {
    pub path: String,
    pub previous_path: Option<String>,
    pub staged: bool,
    pub historical: Option<String>,
    pub old_text: String,
    pub new_text: String,
    pub old_exists: bool,
    pub new_exists: bool,
    pub index_oid: Option<String>,
    pub hunks: Vec<DiffHunk>,
    pub patch: String,
    pub binary: bool,
    pub editable: bool,
    pub worktree_mode: String,
}

pub fn relative_path(path: &str) -> Result<PathBuf> {
    let value = Path::new(path);
    ensure!(
        !path.is_empty()
            && value
                .components()
                .all(|part| matches!(part, Component::Normal(_))),
        "Git path must be relative to this repository"
    );
    Ok(value.to_owned())
}
pub fn working_text(root: &Path, path: &str) -> Result<(String, bool, bool)> {
    let path = root.join(relative_path(path)?);
    let metadata = match std::fs::symlink_metadata(&path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok((String::new(), false, false));
        }
        Err(error) => return Err(error.into()),
    };
    let parent = path.parent().context("Missing diff parent")?;
    ensure!(
        std::fs::canonicalize(parent)?.starts_with(std::fs::canonicalize(root)?),
        "Git target leaves the repository through a symlink"
    );
    if metadata.is_symlink() {
        return Ok((
            std::fs::read_link(&path)?.to_string_lossy().into(),
            true,
            true,
        ));
    }
    ensure!(metadata.is_file(), "This change is not a regular file");
    let mut bytes = Vec::new();
    std::fs::File::open(&path)?
        .take(8 * 1024 * 1024 + 1)
        .read_to_end(&mut bytes)?;
    ensure!(bytes.len() <= 8 * 1024 * 1024, "File diff exceeds 8 MiB");
    Ok((
        String::from_utf8_lossy(&bytes).into_owned(),
        true,
        bytes.contains(&0) || std::str::from_utf8(&bytes).is_err(),
    ))
}
fn blob(root: &Path, spec: &str, cancel: &AtomicBool) -> Result<Option<Vec<u8>>> {
    let result = git_cancellable(
        root,
        &["show", "--no-textconv", "--no-ext-diff", spec],
        None,
        cancel,
    )?;
    if result.success {
        Ok(Some(result.stdout))
    } else {
        Ok(None)
    }
}
pub fn index_oid(root: &Path, path: &str) -> Result<Option<String>> {
    index_oid_cancellable(root, path, &AtomicBool::new(false))
}
fn index_oid_cancellable(root: &Path, path: &str, cancel: &AtomicBool) -> Result<Option<String>> {
    relative_path(path)?;
    let output = git_cancellable(
        root,
        &["ls-files", "--stage", "-z", "--", path],
        None,
        cancel,
    )?;
    ensure!(output.success, "Unable to inspect the index");
    if let Some(item) = output
        .stdout
        .split(|byte| *byte == 0)
        .find(|entry| !entry.is_empty())
    {
        let header = item.split(|byte| *byte == b'\t').next().unwrap_or_default();
        let text = std::str::from_utf8(header)?;
        let fields = text.split_whitespace().collect::<Vec<_>>();
        ensure!(
            fields.len() == 3 && fields[2] == "0",
            "Resolve index conflicts before editing this diff"
        );
        return Ok(Some(fields[1].to_owned()));
    }
    Ok(None)
}

type Ancestry = Arc<HashMap<String, usize>>;
#[cfg(test)]
thread_local! {
    static ANCESTRY_LOADS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}
/// Log positions of the bounded HEAD ancestry, cached per repository and HEAD commit.
fn ancestry(root: &Path, cancel: &AtomicBool) -> Result<Ancestry> {
    static CACHE: Mutex<Vec<(PathBuf, String, Ancestry)>> = Mutex::new(Vec::new());
    let head = gix::discover(root)
        .ok()
        .and_then(|repo| repo.head_id().ok().map(|id| id.to_string()));
    if let Some(head) = &head
        && let Some((_, _, positions)) = CACHE
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .iter()
            .find(|(cached_root, cached_head, _)| cached_root == root && cached_head == head)
    {
        return Ok(positions.clone());
    }
    #[cfg(test)]
    ANCESTRY_LOADS.with(|loads| loads.set(loads.get() + 1));
    let tip = head.as_deref().unwrap_or("HEAD");
    let commits = git_cancellable(
        root,
        &["log", "--format=%H", "-n", "10000", tip, "--"],
        None,
        cancel,
    )?;
    ensure!(commits.success, "Unable to read commit ancestry");
    let positions: Ancestry = Arc::new(
        String::from_utf8(commits.stdout)?
            .lines()
            .enumerate()
            .map(|(i, h)| (h.to_owned(), i))
            .collect(),
    );
    if let Some(head) = head {
        let mut cache = CACHE.lock().unwrap_or_else(PoisonError::into_inner);
        cache.retain(|(cached_root, _, _)| cached_root != root);
        if cache.len() >= 4 {
            cache.remove(0);
        }
        cache.push((root.to_owned(), head, positions.clone()));
    }
    Ok(positions)
}

/// Resolve the name belonging to the selected revision while following renames.
/// History is bounded; an out-of-window target fails instead of opening another file.
pub fn historical_path(
    root: &Path,
    path: &str,
    commit: &str,
    cancel: &AtomicBool,
) -> Result<String> {
    relative_path(path)?;
    ensure!(
        commit.len() >= 40 && commit.chars().all(|c| c.is_ascii_hexdigit()),
        "History requires a full commit ID"
    );
    let positions = ancestry(root, cancel)?;
    let target = *positions
        .get(commit)
        .context("Historical target is outside the bounded ancestry window")?;
    let history = git_cancellable(
        root,
        &[
            "log",
            "--follow",
            "--format=%x00COMMIT%x00%H%x00",
            "--name-status",
            "-z",
            "-n",
            "2000",
            "--",
            path,
        ],
        None,
        cancel,
    )?;
    ensure!(history.success, "Unable to follow historical filename");
    let fields = history.stdout.split(|b| *b == 0).collect::<Vec<_>>();
    let mut name = path.to_owned();
    let mut newer = false;
    let mut i = 0;
    while i < fields.len() {
        if fields[i] == b"COMMIT" && i + 1 < fields.len() {
            let hash = std::str::from_utf8(fields[i + 1])?;
            newer = positions
                .get(hash)
                .is_some_and(|position| *position < target);
            if !newer {
                break;
            }
            i += 2;
            continue;
        }
        let status = fields[i].strip_prefix(b"\n").unwrap_or(fields[i]);
        if newer && status.first() == Some(&b'R') && i + 2 < fields.len() {
            let old = std::str::from_utf8(fields[i + 1])?;
            let new = std::str::from_utf8(fields[i + 2])?;
            if new == name {
                relative_path(old)?;
                name = old.to_owned();
            }
            i += 3;
        } else {
            i += 1;
        }
    }
    Ok(name)
}

pub fn load(
    root: &Path,
    path: &str,
    previous: Option<&str>,
    staged: bool,
    historical: Option<&str>,
    cancel: &AtomicBool,
) -> Result<FileDiff> {
    load_with_context(root, path, previous, staged, historical, 3, cancel)
}
#[allow(clippy::too_many_arguments)]
pub fn load_with_context(
    root: &Path,
    path: &str,
    previous: Option<&str>,
    staged: bool,
    historical: Option<&str>,
    context: usize,
    cancel: &AtomicBool,
) -> Result<FileDiff> {
    let resolved = historical
        .map(|commit| historical_path(root, path, commit, cancel))
        .transpose()?;
    let path = resolved.as_deref().unwrap_or(path);
    relative_path(path)?;
    if let Some(previous) = previous {
        relative_path(previous)?;
    }
    let old_spec = if let Some(commit) = historical {
        format!("{commit}^:{}", previous.unwrap_or(path))
    } else if staged {
        format!("HEAD:{}", previous.unwrap_or(path))
    } else {
        format!(":{path}")
    };
    let old = blob(root, &old_spec, cancel)?;
    let (new, new_exists, unsafe_file) = if let Some(commit) = historical {
        let value = blob(root, &format!("{commit}:{path}"), cancel)?;
        let exists = value.is_some();
        let value = value.unwrap_or_default();
        let binary = value.contains(&0) || std::str::from_utf8(&value).is_err();
        (String::from_utf8_lossy(&value).into_owned(), exists, binary)
    } else if staged {
        let value = blob(root, &format!(":{path}"), cancel)?;
        let exists = value.is_some();
        let value = value.unwrap_or_default();
        let binary = value.contains(&0) || std::str::from_utf8(&value).is_err();
        (String::from_utf8_lossy(&value).into_owned(), exists, binary)
    } else {
        working_text(root, path)?
    };
    let old_exists = old.is_some();
    let old = old.unwrap_or_default();
    let binary = unsafe_file || old.contains(&0) || std::str::from_utf8(&old).is_err();
    let old_text = String::from_utf8_lossy(&old).into_owned();
    let unified = format!("--unified={}", context.min(20));
    let mut args = vec![
        "diff",
        "--no-ext-diff",
        "--no-textconv",
        "--no-color",
        unified.as_str(),
    ];
    if staged {
        args.push("--cached");
    }
    let parent;
    if let Some(commit) = historical {
        parent = format!("{commit}^");
        args.extend([&parent, commit]);
    }
    args.extend(["--", path]);
    let output = git_cancellable(root, &args, None, cancel)?;
    let patch = if !old_exists {
        full_patch(path, &old_text, &new, false)
    } else {
        ensure!(
            output.success,
            "Unable to load Git diff: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8_lossy(&output.stdout).into_owned()
    };
    let hunks = parse(&patch);
    let (oid, index_editable) = if historical.is_none() {
        match index_oid_cancellable(root, path, cancel) {
            Ok(value) => (value, true),
            Err(_) => (None, false),
        }
    } else {
        (None, false)
    };
    #[cfg(unix)]
    let executable = {
        use std::os::unix::fs::PermissionsExt;
        std::fs::symlink_metadata(root.join(path))
            .ok()
            .is_some_and(|metadata| metadata.permissions().mode() & 0o111 != 0)
    };
    #[cfg(not(unix))]
    let executable = false;
    let worktree_mode = if executable { "100755" } else { "100644" }.to_owned();
    ensure!(
        !cancel.load(std::sync::atomic::Ordering::Acquire),
        "Diff cancelled"
    );
    Ok(FileDiff {
        path: path.into(),
        previous_path: previous.map(str::to_owned),
        staged,
        historical: historical.map(str::to_owned),
        old_text,
        new_text: new,
        old_exists,
        new_exists,
        index_oid: oid,
        hunks,
        patch,
        binary,
        editable: !staged && historical.is_none() && !binary && new_exists && index_editable,
        worktree_mode,
    })
}

pub fn parse(patch: &str) -> Vec<DiffHunk> {
    let mut hunks: Vec<DiffHunk> = Vec::new();
    let (mut old, mut new) = (0, 0);
    for line in patch.lines() {
        if line.starts_with("@@ ") {
            let fields = line.split_whitespace().collect::<Vec<_>>();
            let pair = |field: &str| -> Option<(usize, usize)> {
                let field = field.get(1..)?;
                let (start, count) = field.split_once(',').unwrap_or((field, "1"));
                Some((start.parse().ok()?, count.parse().ok()?))
            };
            if let (Some((old_start, old_count)), Some((new_start, new_count))) = (
                fields.get(1).and_then(|field| pair(field)),
                fields.get(2).and_then(|field| pair(field)),
            ) {
                old = old_start;
                new = new_start;
                hunks.push(DiffHunk {
                    old_start,
                    old_count,
                    new_start,
                    new_count,
                    header: line.into(),
                    lines: Vec::new(),
                });
            }
            continue;
        }
        let Some(hunk) = hunks.last_mut() else {
            continue;
        };
        let (kind, old_line, new_line) = match line.as_bytes().first().copied() {
            Some(b'+') => {
                let at = new;
                new += 1;
                (DiffKind::Added, None, Some(at))
            }
            Some(b'-') => {
                let at = old;
                old += 1;
                (DiffKind::Deleted, Some(at), None)
            }
            Some(b' ') => {
                let (at_old, at_new) = (old, new);
                old += 1;
                new += 1;
                (DiffKind::Context, Some(at_old), Some(at_new))
            }
            Some(b'\\') => (DiffKind::Meta, None, None),
            _ => continue,
        };
        hunk.lines.push(DiffLine {
            kind,
            old_line,
            new_line,
            text: line.get(1..).unwrap_or_default().into(),
        });
    }
    hunks
}

fn line_slice(text: &str, start: usize, count: usize) -> Result<(usize, usize)> {
    let starts = std::iter::once(0)
        .chain(text.match_indices('\n').map(|(index, _)| index + 1))
        .collect::<Vec<_>>();
    let begin = starts.get(start).copied().unwrap_or(text.len());
    let end = starts
        .get(start.saturating_add(count))
        .copied()
        .unwrap_or(text.len());
    Ok((begin, end))
}

pub fn hunk_text(diff: &FileDiff, index: usize) -> Result<String> {
    let hunk = diff.hunks.get(index).context("Select a hunk to edit")?;
    let (start, end) = line_slice(
        &diff.new_text,
        hunk.new_start.saturating_sub(1),
        hunk.new_count,
    )?;
    Ok(diff.new_text[start..end].into())
}
fn replace_hunk(text: &str, start: usize, count: usize, replacement: &str) -> Result<String> {
    let (start, end) = line_slice(text, start, count)?;
    let mut replacement = replacement.to_owned();
    if end < text.len() && !replacement.is_empty() && !replacement.ends_with('\n') {
        replacement.push('\n');
    }
    let mut result = text[..start].to_owned();
    result.push_str(&replacement);
    result.push_str(&text[end..]);
    Ok(result)
}
pub fn edited_worktree(
    root: &Path,
    diff: &FileDiff,
    index: usize,
    replacement: &str,
) -> Result<String> {
    ensure!(diff.editable, "This diff cannot be edited");
    let current = working_text(root, &diff.path)?.0;
    ensure!(
        current == diff.new_text,
        "Working file changed; refresh the diff before saving"
    );
    let hunk = diff.hunks.get(index).context("Missing edited hunk")?;
    replace_hunk(
        &diff.new_text,
        hunk.new_start.saturating_sub(1),
        hunk.new_count,
        replacement,
    )
}
pub fn stage_edited_hunk(
    root: &Path,
    diff: &FileDiff,
    index: usize,
    replacement: &str,
    cancel: &AtomicBool,
) -> Result<()> {
    ensure!(diff.editable, "This diff cannot be edited");
    ensure!(
        index_oid_cancellable(root, &diff.path, cancel)? == diff.index_oid,
        "Index changed; refresh the diff before staging"
    );
    ensure!(
        working_text(root, &diff.path)?.0 == diff.new_text,
        "Working file changed; refresh the diff before staging"
    );
    let hunk = diff.hunks.get(index).context("Missing edited hunk")?;
    let content = replace_hunk(
        &diff.old_text,
        hunk.old_start.saturating_sub(1),
        hunk.old_count,
        replacement,
    )?;
    let patch = full_patch(&diff.path, &diff.old_text, &content, diff.old_exists).replace(
        "new file mode 100644",
        &format!("new file mode {}", diff.worktree_mode),
    );
    let output = git_cancellable(
        root,
        &[
            "apply",
            "--cached",
            "--unidiff-zero",
            "--whitespace=nowarn",
            "-",
        ],
        Some(patch.as_bytes()),
        cancel,
    )?;
    ensure!(
        output.success,
        "Staging edited hunk failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    Ok(())
}
fn quote_path(path: &str) -> String {
    let mut result = String::from("\"");
    for byte in path.bytes() {
        match byte {
            b'"' => result.push_str("\\\""),
            b'\\' => result.push_str("\\\\"),
            b'\n' => result.push_str("\\n"),
            b'\r' => result.push_str("\\r"),
            b'\t' => result.push_str("\\t"),
            32..=126 => result.push(byte as char),
            _ => result.push_str(&format!("\\{byte:03o}")),
        }
    }
    result.push('"');
    result
}
pub fn full_patch(path: &str, old: &str, new: &str, old_exists: bool) -> String {
    let a = quote_path(&format!("a/{path}"));
    let b = quote_path(&format!("b/{path}"));
    let old_lines = old.split_inclusive('\n').collect::<Vec<_>>();
    let new_lines = new.split_inclusive('\n').collect::<Vec<_>>();
    let mut patch = format!(
        "diff --git {a} {b}\n{}--- {}\n+++ {b}\n@@ -{},{} +{},{} @@\n",
        if old_exists {
            ""
        } else {
            "new file mode 100644\n"
        },
        if old_exists { a.as_str() } else { "/dev/null" },
        usize::from(!old_lines.is_empty()),
        old_lines.len(),
        usize::from(!new_lines.is_empty()),
        new_lines.len()
    );
    for (prefix, lines) in [('-', old_lines), ('+', new_lines)] {
        for line in lines {
            patch.push(prefix);
            patch.push_str(line);
            if !line.ends_with('\n') {
                patch.push_str("\n\\ No newline at end of file\n");
            }
        }
    }
    patch
}

#[cfg(test)]
mod tests {
    #[test]
    fn history_follows_a_renamed_file_to_its_original_content() {
        let dir = repo();
        std::fs::write(dir.path().join("old.txt"), "original\n").unwrap();
        run(dir.path(), &["add", "--", "old.txt"]);
        run(dir.path(), &["commit", "-qm", "original"]);
        let hash = String::from_utf8(
            crate::git_process::git(dir.path(), &["rev-parse", "HEAD"], None)
                .unwrap()
                .stdout,
        )
        .unwrap()
        .trim()
        .to_owned();
        run(dir.path(), &["mv", "old.txt", "new.txt"]);
        run(dir.path(), &["commit", "-qm", "rename"]);
        let cancel = std::sync::atomic::AtomicBool::new(false);
        assert_eq!(
            super::historical_path(dir.path(), "new.txt", &hash, &cancel).unwrap(),
            "old.txt"
        );
        let diff = super::load(dir.path(), "new.txt", None, false, Some(&hash), &cancel).unwrap();
        assert_eq!(diff.new_text, "original\n");
        assert!(diff.historical.is_some());
        assert!(!diff.editable);
    }
    #[test]
    fn historical_ancestry_is_cached_until_head_moves() {
        let dir = repo();
        std::fs::write(dir.path().join("old.txt"), "original\n").unwrap();
        run(dir.path(), &["add", "--", "old.txt"]);
        run(dir.path(), &["commit", "-qm", "original"]);
        let first = run(dir.path(), &["rev-parse", "HEAD"]).trim().to_owned();
        run(dir.path(), &["mv", "old.txt", "new.txt"]);
        run(dir.path(), &["commit", "-qm", "rename"]);
        let cancel = AtomicBool::new(false);
        let loads = || ANCESTRY_LOADS.with(std::cell::Cell::get);
        let before = loads();
        for _ in 0..3 {
            assert_eq!(
                historical_path(dir.path(), "new.txt", &first, &cancel).unwrap(),
                "old.txt"
            );
        }
        assert_eq!(loads() - before, 1);
        std::fs::write(dir.path().join("new.txt"), "changed\n").unwrap();
        run(dir.path(), &["commit", "-qam", "change"]);
        let latest = run(dir.path(), &["rev-parse", "HEAD"]).trim().to_owned();
        assert_eq!(
            historical_path(dir.path(), "new.txt", &latest, &cancel).unwrap(),
            "new.txt"
        );
        assert_eq!(loads() - before, 2);
    }
    use super::*;
    use crate::git_process::git;
    fn run(root: &Path, args: &[&str]) -> String {
        let result = git(root, args, None).unwrap();
        assert!(
            result.success,
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
        String::from_utf8(result.stdout).unwrap()
    }
    fn repo() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        run(dir.path(), &["init", "-q"]);
        run(dir.path(), &["config", "user.name", "Tide"]);
        run(dir.path(), &["config", "user.email", "test@tide.local"]);
        dir
    }
    #[test]
    fn staged_modified_hunk_changes_only_the_index_and_stale_sources_are_rejected() {
        let dir = repo();
        let path = dir.path().join("file.txt");
        std::fs::write(&path, "one\nsecond\nthird\n").unwrap();
        run(dir.path(), &["add", "--", "file.txt"]);
        run(dir.path(), &["commit", "-qm", "initial"]);
        std::fs::write(&path, "one\nchanged\nthird\n").unwrap();
        let cancel = AtomicBool::new(false);
        let diff = load(dir.path(), "file.txt", None, false, None, &cancel).unwrap();
        assert_eq!(diff.hunks.len(), 1);
        assert_eq!(hunk_text(&diff, 0).unwrap(), "one\nchanged\nthird\n");
        stage_edited_hunk(dir.path(), &diff, 0, "one\nreviewed\nthird\n", &cancel).unwrap();
        assert_eq!(
            run(dir.path(), &["show", ":file.txt"]),
            "one\nreviewed\nthird\n"
        );
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            "one\nchanged\nthird\n"
        );
        assert!(stage_edited_hunk(dir.path(), &diff, 0, "bad", &cancel).is_err());
        let staged = load(dir.path(), "file.txt", None, true, None, &cancel).unwrap();
        assert!(!staged.editable);
        assert!(staged.new_text.contains("reviewed"));
        let direct = load(dir.path(), "file.txt", None, false, None, &cancel).unwrap();
        assert_eq!(
            edited_worktree(dir.path(), &direct, 0, "direct\n").unwrap(),
            "direct\n"
        );
        std::fs::write(&path, "external\n").unwrap();
        assert!(edited_worktree(dir.path(), &direct, 0, "bad").is_err());
    }
    #[test]
    fn literal_untracked_filenames_and_newlines_do_not_select_another_path() {
        let dir = repo();
        let name = "file[1] new\nname.txt";
        let sibling = "file1 new\nname.txt";
        std::fs::write(dir.path().join(name), "first\n").unwrap();
        std::fs::write(dir.path().join(sibling), "keep\n").unwrap();
        let cancel = AtomicBool::new(false);
        let diff = load(dir.path(), name, None, false, None, &cancel).unwrap();
        assert!(!diff.old_exists);
        assert_eq!(diff.hunks.len(), 1);
        stage_edited_hunk(dir.path(), &diff, 0, "reviewed\n", &cancel).unwrap();
        assert_eq!(
            run(dir.path(), &["show", &format!(":{name}")]),
            "reviewed\n"
        );
        assert!(index_oid(dir.path(), sibling).unwrap().is_none());
        assert_eq!(
            std::fs::read_to_string(dir.path().join(sibling)).unwrap(),
            "keep\n"
        );
    }
    #[test]
    fn historical_root_commit_and_symlink_targets_are_read_only() {
        let dir = repo();
        std::fs::write(dir.path().join("file.txt"), "initial\n").unwrap();
        run(dir.path(), &["add", "--", "file.txt"]);
        run(dir.path(), &["commit", "-qm", "first"]);
        let commit = run(dir.path(), &["rev-parse", "HEAD"]).trim().to_owned();
        let cancel = AtomicBool::new(false);
        let historical = load(dir.path(), "file.txt", None, false, Some(&commit), &cancel).unwrap();
        assert_eq!(historical.new_text, "initial\n");
        assert!(!historical.editable);
        assert_eq!(historical.hunks.len(), 1);
        #[cfg(unix)]
        {
            let outside = tempfile::tempdir().unwrap();
            std::fs::write(outside.path().join("secret"), "private").unwrap();
            std::os::unix::fs::symlink(outside.path().join("secret"), dir.path().join("link"))
                .unwrap();
            let link = load(dir.path(), "link", None, false, None, &cancel).unwrap();
            assert!(!link.editable);
            assert!(!link.new_text.contains("private"));
            std::os::unix::fs::symlink(outside.path(), dir.path().join("escape")).unwrap();
            assert!(working_text(dir.path(), "escape/secret").is_err());
        }
    }
}
