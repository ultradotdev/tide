//! Workspace file watching that skips ignored trees.
//!
//! Build output and dependency trees (`target`, `node_modules`, anything the
//! root `.gitignore` excludes, and Git's object store) never refresh the
//! workbench. On Linux, where every inotify watch covers one directory, those
//! trees are not even registered, so a large `node_modules` cannot exhaust the
//! per-user watch limit.
use std::path::{Path, PathBuf};

use ignore::gitignore::{Gitignore, GitignoreBuilder};

/// inotify needs one watch per directory; other backends watch a tree natively.
pub(crate) const PER_DIRECTORY: bool = cfg!(target_os = "linux");

pub(crate) struct WatchFilter {
    roots: Vec<PathBuf>,
    gitignore: Gitignore,
}

impl WatchFilter {
    pub(crate) fn new(root: &Path) -> Self {
        let mut roots = vec![root.to_path_buf()];
        if let Ok(canonical) = std::fs::canonicalize(root)
            && canonical != root
        {
            roots.push(canonical);
        }
        let mut builder = GitignoreBuilder::new(root);
        builder.add(root.join(".gitignore"));
        builder.add(root.join(".git/info/exclude"));
        Self {
            roots,
            gitignore: builder.build().unwrap_or_else(|_| Gitignore::empty()),
        }
    }

    /// Whether a change at `path` can never matter to the workbench.
    pub(crate) fn ignores(&self, path: &Path) -> bool {
        let Some(relative) = self
            .roots
            .iter()
            .find_map(|root| path.strip_prefix(root).ok())
        else {
            return false;
        };
        let mut parts = relative.components().map(|part| part.as_os_str());
        match parts.next() {
            None => false,
            Some(first) if first == "target" || first == "node_modules" => true,
            Some(first) if first == ".git" => parts
                .next()
                .is_some_and(|part| part == "objects" || part == "logs"),
            Some(_) => self
                .gitignore
                .matched_path_or_any_parents(relative, path.is_dir())
                .is_ignore(),
        }
    }
}

/// Directories to watch one by one: every directory the `.gitignore` files
/// keep, plus `.git` and its refs for branch and index changes.
pub(crate) fn directories(root: &Path, filter: &WatchFilter) -> Vec<PathBuf> {
    let mut directories = walk(root, filter);
    let git = root.join(".git");
    if git.is_dir() {
        directories.push(git.clone());
        directories.extend(walk(&git.join("refs"), filter));
    }
    directories
}

pub(crate) fn walk(start: &Path, filter: &WatchFilter) -> Vec<PathBuf> {
    ignore::WalkBuilder::new(start)
        .hidden(false)
        .require_git(false)
        .filter_entry(|entry| entry.file_name() != ".git")
        .build()
        .filter_map(Result::ok)
        .filter(|entry| entry.file_type().is_some_and(|kind| kind.is_dir()))
        .map(ignore::DirEntry::into_path)
        .filter(|path| path == start || !filter.ignores(path))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ignored_trees_are_neither_watched_nor_reported() {
        let root = tempfile::tempdir().unwrap();
        let root = root.path();
        for dir in [
            "src/nested",
            "node_modules/pkg/lib",
            "target/debug",
            "dist/bundle",
            ".git/objects/ab",
            ".git/refs/heads",
        ] {
            std::fs::create_dir_all(root.join(dir)).unwrap();
        }
        std::fs::write(root.join(".gitignore"), "dist/\n*.log\n").unwrap();
        let filter = WatchFilter::new(root);
        let watched = directories(root, &filter);
        for kept in [
            "",
            "src",
            "src/nested",
            ".git",
            ".git/refs",
            ".git/refs/heads",
        ] {
            assert!(watched.contains(&root.join(kept)), "{kept} is not watched");
        }
        for skipped in [
            "node_modules",
            "node_modules/pkg",
            "target",
            "dist",
            ".git/objects",
        ] {
            assert!(
                !watched.contains(&root.join(skipped)),
                "{skipped} is watched"
            );
        }
        assert!(filter.ignores(&root.join("dist/bundle/app.js")));
        assert!(filter.ignores(&root.join("src/debug.log")));
        assert!(filter.ignores(&root.join("node_modules/pkg/index.js")));
        assert!(filter.ignores(&root.join(".git/objects/ab/cd")));
        assert!(!filter.ignores(&root.join(".git/HEAD")));
        assert!(!filter.ignores(&root.join("src/main.rs")));
        assert!(!filter.ignores(Path::new("/elsewhere/file")));
    }
}
