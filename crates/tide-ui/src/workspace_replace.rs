//! Workspace find-and-replace: a preview of every file a replacement changes, then the writes
//! for files that are not open in an editor. Open files change through their buffers instead.
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

use sha2::{Digest, Sha256};
use tide_core::Workspace;

use crate::search::SearchOptions;

const MAX_FILE_BYTES: u64 = 1_000_000;
const MAX_ENTRIES: usize = 200_000;
const MAX_MATCHES: usize = 100_000;

#[derive(Debug, Clone)]
pub struct Replace {
    pub query: String,
    pub replacement: String,
    pub options: SearchOptions,
}

impl Replace {
    fn expression(&self) -> Result<regex::Regex, String> {
        crate::search::expression(
            &self.query,
            SearchOptions {
                selection: None,
                ..self.options
            },
        )
    }

    /// `text` with every match replaced; regex replacements expand `$1`-style groups.
    fn apply(&self, expression: &regex::Regex, text: &str) -> String {
        if self.options.regex {
            expression
                .replace_all(text, self.replacement.as_str())
                .into_owned()
        } else {
            expression
                .replace_all(text, regex::NoExpand(&self.replacement))
                .into_owned()
        }
    }
}

#[derive(Debug)]
pub struct PlannedFile {
    pub path: PathBuf,
    pub relative: String,
    pub matches: usize,
    /// The disk content the preview counted, so a write can refuse a file that changed since.
    /// `None` for a file that is open in an editor, whose buffer is edited instead.
    pub digest: Option<[u8; 32]>,
}

#[derive(Debug)]
pub struct Plan {
    pub replace: Replace,
    pub files: Vec<PlannedFile>,
}

impl Plan {
    #[must_use]
    pub fn matches(&self) -> usize {
        self.files.iter().map(|file| file.matches).sum()
    }
}

/// What writing the closed files did.
#[derive(Debug, Default)]
pub struct Written {
    pub files: usize,
    pub matches: usize,
    /// Files left alone because they changed after the preview, or could not be written.
    pub skipped: Vec<String>,
}

fn digest(text: &str) -> [u8; 32] {
    Sha256::digest(text.as_bytes()).into()
}

/// Reads a regular UTF-8 text file inside the workspace, or `None`.
fn read_text(workspace: &Workspace, path: &Path) -> Option<String> {
    let resolved = workspace.resolve(&path.display().to_string()).ok()?;
    let metadata = std::fs::symlink_metadata(&resolved).ok()?;
    if !metadata.is_file() || metadata.len() > MAX_FILE_BYTES {
        return None;
    }
    let text = String::from_utf8(std::fs::read(&resolved).ok()?).ok()?;
    (!text.contains('\0')).then_some(text)
}

/// Counts the matches in every workspace file that ignore rules admit. `open` holds the buffer
/// text of files open in an editor, by canonical path; those are counted from the buffer.
pub fn plan(
    root: &Path,
    replace: Replace,
    open: &HashMap<PathBuf, String>,
    cancel: &AtomicBool,
) -> Result<Plan, String> {
    if replace.query.is_empty() {
        return Err("Replace in files needs a search term".into());
    }
    let expression = replace.expression()?;
    let workspace = Workspace::new(root).map_err(|error| error.to_string())?;
    let mut files = Vec::new();
    let mut total = 0;
    let walk = ignore::WalkBuilder::new(root)
        .hidden(false)
        .git_ignore(true)
        .git_global(false)
        .git_exclude(true)
        // Repository metadata is never search text.
        .filter_entry(|entry| entry.file_name() != ".git")
        .build();
    for (index, entry) in walk.enumerate() {
        if cancel.load(Ordering::Acquire) {
            return Err("Replace in files was cancelled".into());
        }
        if index >= MAX_ENTRIES {
            return Err("The workspace has too many files to replace in; narrow it first".into());
        }
        let Ok(entry) = entry else {
            continue;
        };
        if !entry.file_type().is_some_and(|kind| kind.is_file()) {
            continue;
        }
        let path = tide_core::fs::canonical_target(entry.path());
        let (text, digest) = match open.get(&path) {
            Some(text) => (text.clone(), None),
            None => {
                let Some(text) = read_text(&workspace, &path) else {
                    continue;
                };
                let digest = digest(&text);
                (text, Some(digest))
            }
        };
        let matches = expression.find_iter(&text).count();
        if matches == 0 {
            continue;
        }
        total += matches;
        if total > MAX_MATCHES {
            return Err("More than 100,000 matches; narrow the search".into());
        }
        let relative = path
            .strip_prefix(root)
            .unwrap_or(&path)
            .display()
            .to_string();
        files.push(PlannedFile {
            path,
            relative,
            matches,
            digest,
        });
    }
    files.sort_by(|a, b| a.relative.cmp(&b.relative));
    Ok(Plan { replace, files })
}

/// Writes the replacement into each file atomically, skipping any file whose content changed
/// since the preview or that was open in an editor then.
pub fn write_files(root: &Path, replace: &Replace, files: &[PlannedFile]) -> Written {
    let mut written = Written::default();
    let (Ok(expression), Ok(workspace)) = (replace.expression(), Workspace::new(root)) else {
        written.skipped = files.iter().map(|file| file.relative.clone()).collect();
        return written;
    };
    for file in files {
        let current = read_text(&workspace, &file.path);
        let Some(text) = current.filter(|text| file.digest == Some(digest(text))) else {
            written.skipped.push(file.relative.clone());
            continue;
        };
        let updated = replace.apply(&expression, &text);
        if crate::editor::write_atomic(&file.path, updated.as_bytes()).is_ok() {
            written.files += 1;
            written.matches += file.matches;
        } else {
            written.skipped.push(file.relative.clone());
        }
    }
    written
}

#[cfg(test)]
mod tests {
    use super::*;

    fn replace(query: &str, replacement: &str, regex: bool) -> Replace {
        Replace {
            query: query.into(),
            replacement: replacement.into(),
            options: SearchOptions {
                case_sensitive: true,
                regex,
                ..SearchOptions::default()
            },
        }
    }

    #[test]
    fn plans_ignored_hidden_and_escaping_files_out_and_writes_only_unchanged_ones() {
        let root = tempfile::tempdir().unwrap();
        let root_path = std::fs::canonicalize(root.path()).unwrap();
        let outside = tempfile::tempdir().unwrap();
        std::fs::write(outside.path().join("secret.txt"), "needle").unwrap();
        std::fs::create_dir_all(root_path.join(".git")).unwrap();
        std::fs::create_dir_all(root_path.join("src")).unwrap();
        std::fs::write(root_path.join(".git/config"), "needle").unwrap();
        std::fs::write(root_path.join(".ignore"), "ignored.txt\n").unwrap();
        std::fs::write(root_path.join("ignored.txt"), "needle").unwrap();
        std::fs::write(root_path.join("src/a.txt"), "needle needle\r\nneedle\r\n").unwrap();
        std::fs::write(root_path.join("b.txt"), "needle").unwrap();
        std::fs::write(root_path.join("binary.bin"), b"needle\0").unwrap();
        #[cfg(unix)]
        std::os::unix::fs::symlink(outside.path().join("secret.txt"), root_path.join("link"))
            .unwrap();
        let open = HashMap::from([(root_path.join("b.txt"), "needle in buffer needle".into())]);
        let cancel = AtomicBool::new(false);
        let plan = plan(&root_path, replace("needle", "pin", false), &open, &cancel).unwrap();
        let summary = plan
            .files
            .iter()
            .map(|file| (file.relative.as_str(), file.matches, file.digest.is_some()))
            .collect::<Vec<_>>();
        assert_eq!(summary, [("b.txt", 2, false), ("src/a.txt", 3, true)]);
        assert_eq!(plan.matches(), 5);

        let written = write_files(&root_path, &plan.replace, &plan.files);
        assert_eq!((written.files, written.matches), (1, 3));
        assert_eq!(
            written.skipped,
            ["b.txt"],
            "open files are edited in their buffers"
        );
        assert_eq!(
            std::fs::read_to_string(root_path.join("src/a.txt")).unwrap(),
            "pin pin\r\npin\r\n",
            "line endings are kept"
        );
        assert_eq!(
            std::fs::read_to_string(root_path.join("b.txt")).unwrap(),
            "needle"
        );
        assert_eq!(
            std::fs::read_to_string(outside.path().join("secret.txt")).unwrap(),
            "needle"
        );

        // A file edited after the preview is skipped rather than overwritten.
        std::fs::write(root_path.join("src/a.txt"), "needle changed").unwrap();
        let stale = Plan {
            replace: replace("needle", "pin", false),
            files: vec![PlannedFile {
                path: root_path.join("src/a.txt"),
                relative: "src/a.txt".into(),
                matches: 1,
                digest: Some(digest("needle")),
            }],
        };
        let written = write_files(&root_path, &stale.replace, &stale.files);
        assert_eq!(written.skipped, ["src/a.txt"]);
        assert_eq!(
            std::fs::read_to_string(root_path.join("src/a.txt")).unwrap(),
            "needle changed"
        );
    }

    #[test]
    fn regex_replacements_expand_groups_and_literal_ones_do_not() {
        let regex = replace(r"(\w+)@", "$1 at ", true);
        let expression = regex.expression().unwrap();
        assert_eq!(regex.apply(&expression, "me@ you@"), "me at  you at ");
        let literal = replace("a.b", "$1", false);
        let expression = literal.expression().unwrap();
        assert_eq!(literal.apply(&expression, "a.b axb"), "$1 axb");
        assert!(replace("(", "", true).expression().is_err());
    }
}
