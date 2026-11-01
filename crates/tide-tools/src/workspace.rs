//! Workspace boundary and read-only workspace views for AI tools.

use std::collections::BTreeMap;
use std::path::{Component, Path, PathBuf};

use serde::{Deserialize, Serialize};

pub const MAX_FILE_BYTES: u64 = 2 * 1024 * 1024;
pub const MAX_RESULTS: usize = 500;

#[derive(Debug, thiserror::Error)]
pub enum ToolError {
    #[error("path escapes the workspace: {0}")]
    OutsideWorkspace(String),
    #[error("file is too large to read: {0}")]
    TooLarge(String),
    #[error("not a file: {0}")]
    NotAFile(String),
    #[error("unknown tool: {0}")]
    UnknownTool(String),
    #[error("invalid tool input: {0}")]
    InvalidInput(String),
    #[error("tool command execution is disabled until the user opts in")]
    CommandsDisabled,
    #[error("change set is stale; the file changed after it was staged")]
    Stale,
    #[error("validation failed to start: {0}")]
    Validation(String),
    #[error("io error: {0}")]
    Io(String),
}

/// A canonicalized workspace root. Every tool path is checked against it.
#[derive(Debug, Clone)]
pub struct Workspace {
    root: PathBuf,
    canonical_root: PathBuf,
}

impl Workspace {
    pub fn new(root: impl AsRef<Path>) -> Result<Self, ToolError> {
        let root = root.as_ref().to_path_buf();
        let canonical_root =
            std::fs::canonicalize(&root).map_err(|error| ToolError::Io(error.to_string()))?;
        Ok(Self {
            root,
            canonical_root,
        })
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Resolve a tool-supplied path, rejecting anything outside the workspace.
    pub fn resolve(&self, path: &str) -> Result<PathBuf, ToolError> {
        let candidate = Path::new(path);
        let joined = if candidate.is_absolute() {
            candidate.to_path_buf()
        } else {
            self.canonical_root.join(candidate)
        };
        let normalized = normalize(&joined);
        if !normalized.starts_with(&self.canonical_root) {
            return Err(ToolError::OutsideWorkspace(path.to_owned()));
        }
        if normalized.exists() {
            let canonical = std::fs::canonicalize(&normalized)
                .map_err(|error| ToolError::Io(error.to_string()))?;
            if !canonical.starts_with(&self.canonical_root) {
                return Err(ToolError::OutsideWorkspace(path.to_owned()));
            }
            return Ok(canonical);
        }
        // For new files, canonicalize the nearest existing ancestor and re-append
        // the missing components so symlinked parents cannot escape the root.
        let mut existing = normalized.as_path();
        let mut suffix = Vec::new();
        while !existing.exists() {
            suffix.push(
                existing
                    .file_name()
                    .ok_or_else(|| ToolError::OutsideWorkspace(path.to_owned()))?
                    .to_os_string(),
            );
            existing = existing
                .parent()
                .ok_or_else(|| ToolError::OutsideWorkspace(path.to_owned()))?;
        }
        let mut canonical =
            std::fs::canonicalize(existing).map_err(|error| ToolError::Io(error.to_string()))?;
        if !canonical.starts_with(&self.canonical_root) {
            return Err(ToolError::OutsideWorkspace(path.to_owned()));
        }
        for component in suffix.iter().rev() {
            canonical.push(component);
        }
        if !canonical.starts_with(&self.canonical_root) {
            return Err(ToolError::OutsideWorkspace(path.to_owned()));
        }
        Ok(canonical)
    }

    /// Workspace-relative display path.
    pub fn relative(&self, path: &Path) -> String {
        path.strip_prefix(&self.canonical_root)
            .unwrap_or(path)
            .display()
            .to_string()
    }

    pub fn read_text(&self, path: &str) -> Result<String, ToolError> {
        let resolved = self.resolve(path)?;
        let metadata =
            std::fs::metadata(&resolved).map_err(|error| ToolError::Io(error.to_string()))?;
        if !metadata.is_file() {
            return Err(ToolError::NotAFile(path.to_owned()));
        }
        if metadata.len() > MAX_FILE_BYTES {
            return Err(ToolError::TooLarge(path.to_owned()));
        }
        std::fs::read_to_string(&resolved).map_err(|error| ToolError::Io(error.to_string()))
    }

    /// Read the current on-disk bytes and hash without interpreting the file.
    pub fn read_bytes(&self, path: &Path) -> Result<Option<Vec<u8>>, ToolError> {
        match std::fs::read(path) {
            Ok(bytes) => Ok(Some(bytes)),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(ToolError::Io(error.to_string())),
        }
    }

    pub fn list_files(&self, path: Option<&str>, limit: usize) -> Result<Vec<String>, ToolError> {
        let start = path.map_or_else(
            || Ok(self.canonical_root.clone()),
            |path| self.resolve(path),
        )?;
        let limit = limit.clamp(1, MAX_RESULTS);
        let mut files = Vec::new();
        let walker = walk(&start)
            .sort_by_file_name(|left, right| left.cmp(right))
            .build();
        for entry in walker.flatten() {
            if !entry.file_type().is_some_and(|kind| kind.is_file()) {
                continue;
            }
            let relative = self.relative(entry.path());
            if is_git_path(Path::new(&relative)) {
                continue;
            }
            files.push(relative);
            if files.len() >= limit {
                break;
            }
        }
        files.sort();
        Ok(files)
    }

    pub fn search(&self, query: &str, limit: usize) -> Result<Vec<SearchMatch>, ToolError> {
        if query.is_empty() {
            return Ok(Vec::new());
        }
        let limit = limit.clamp(1, MAX_RESULTS);
        let needle = query.to_lowercase();
        let mut matches = Vec::new();
        'outer: for entry in walk(&self.canonical_root).build().flatten() {
            if !entry.file_type().is_some_and(|kind| kind.is_file()) {
                continue;
            }
            let path = entry.path();
            if is_sensitive(Path::new(&self.relative(path))) {
                continue;
            }
            let Ok(metadata) = std::fs::metadata(path) else {
                continue;
            };
            if metadata.len() > MAX_FILE_BYTES {
                continue;
            }
            let Ok(text) = std::fs::read_to_string(path) else {
                continue;
            };
            for (index, line) in text.lines().enumerate() {
                if line.to_lowercase().contains(&needle) {
                    matches.push(SearchMatch {
                        path: self.relative(path),
                        line: index + 1,
                        text: line.chars().take(240).collect(),
                    });
                    if matches.len() >= limit {
                        break 'outer;
                    }
                }
            }
        }
        Ok(matches)
    }

    /// Cheap workspace view used by providers to describe the project.
    pub fn view(&self, max_files: usize) -> WorkspaceView {
        let files = self.list_files(None, max_files).unwrap_or_default();
        WorkspaceView {
            root: self.root.display().to_string(),
            files,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SearchMatch {
    pub path: String,
    pub line: usize,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkspaceView {
    pub root: String,
    pub files: Vec<String>,
}

/// Supplies the current editor buffer for a path, overriding the disk view.
pub trait BufferSource: Send + Sync {
    fn get(&self, path: &Path) -> Option<String>;
}

#[derive(Debug, Default, Clone)]
pub struct MapBuffers {
    buffers: BTreeMap<PathBuf, String>,
}

impl MapBuffers {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn insert(&mut self, path: impl Into<PathBuf>, text: impl Into<String>) {
        self.buffers.insert(path.into(), text.into());
    }
}

impl BufferSource for MapBuffers {
    fn get(&self, path: &Path) -> Option<String> {
        self.buffers.get(path).cloned()
    }
}

/// Paths AI tools may only read after explicit user approval: anything in
/// `.git`, dotenv files, private keys, and credential files.
pub fn is_sensitive(path: &Path) -> bool {
    is_git_path(path)
        || path
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| {
                let name = name.to_ascii_lowercase();
                name.starts_with(".env")
                    || is_ssh_private_key(&name)
                    || name.starts_with("credentials")
                    || name.ends_with(".pem")
                    || name.ends_with(".key")
            })
}

/// `id_rsa`, `id_ed25519_sk` and similar; public halves (`.pub`) are not secret.
fn is_ssh_private_key(name: &str) -> bool {
    name.strip_prefix("id_").is_some_and(|kind| {
        ["rsa", "dsa", "ecdsa", "ed25519"]
            .iter()
            .any(|algorithm| kind == *algorithm || kind == format!("{algorithm}_sk"))
    })
}

fn is_git_path(path: &Path) -> bool {
    path.components()
        .any(|component| component.as_os_str() == ".git")
}

fn walk(start: &Path) -> ignore::WalkBuilder {
    let mut builder = ignore::WalkBuilder::new(start);
    builder
        .hidden(false)
        .git_ignore(true)
        .filter_entry(|entry| entry.file_name() != ".git");
    builder
}

fn normalize(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                out.pop();
            }
            other => out.push(other.as_os_str()),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_paths_outside_the_workspace() {
        let dir = tempfile::tempdir().unwrap();
        let workspace = Workspace::new(dir.path()).unwrap();
        assert!(workspace.resolve("../escape").is_err());
        assert!(workspace.resolve("/etc/passwd").is_err());
        assert!(workspace.resolve("src/main.rs").is_ok());
    }

    #[test]
    fn resolves_nested_new_files_inside_the_workspace() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("src")).unwrap();
        let workspace = Workspace::new(dir.path()).unwrap();
        let path = workspace.resolve("src/new.rs").unwrap();
        assert_eq!(workspace.relative(&path), "src/new.rs");
    }

    #[test]
    fn lists_and_searches_workspace_files() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("src")).unwrap();
        std::fs::write(dir.path().join("src/a.rs"), "fn main() {}\n").unwrap();
        std::fs::write(dir.path().join("README.md"), "Main docs\n").unwrap();
        let workspace = Workspace::new(dir.path()).unwrap();
        let files = workspace.list_files(None, 100).unwrap();
        assert!(files.contains(&"src/a.rs".to_owned()));
        let matches = workspace.search("main", 100).unwrap();
        assert_eq!(matches.len(), 2);
        assert!(workspace.search("", 100).unwrap().is_empty());
    }

    #[test]
    fn walks_skip_git_and_search_skips_sensitive_files() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".git")).unwrap();
        std::fs::write(
            dir.path().join(".git/config"),
            "url = https://token@example.com\n",
        )
        .unwrap();
        std::fs::write(dir.path().join(".env"), "TOKEN=hunter2\n").unwrap();
        std::fs::write(dir.path().join("server.pem"), "TOKEN pem\n").unwrap();
        std::fs::write(dir.path().join("a.rs"), "// TOKEN usage\n").unwrap();
        let workspace = Workspace::new(dir.path()).unwrap();
        let files = workspace.list_files(None, 100).unwrap();
        assert!(files.iter().all(|file| !file.starts_with(".git/")));
        assert!(files.contains(&".env".to_owned()));
        assert!(workspace.list_files(Some(".git"), 100).unwrap().is_empty());
        let matches = workspace.search("token", 100).unwrap();
        assert_eq!(
            matches
                .iter()
                .map(|found| found.path.as_str())
                .collect::<Vec<_>>(),
            ["a.rs"]
        );
    }

    #[test]
    fn classifies_sensitive_paths() {
        for path in [
            ".env",
            ".env.local",
            "config/server.PEM",
            "keys/id_rsa",
            "keys/id_ed25519_sk",
            "tls/site.key",
            "credentials.json",
            ".git/config",
            "vendor/.git/HEAD",
        ] {
            assert!(is_sensitive(Path::new(path)), "{path}");
        }
        for path in [
            "src/main.rs",
            ".gitignore",
            ".github/ci.yml",
            "keyboard.rs",
            "src/id_utils.rs",
            "keys/id_rsa.pub",
        ] {
            assert!(!is_sensitive(Path::new(path)), "{path}");
        }
    }

    #[test]
    fn buffers_override_the_disk_view() {
        let mut buffers = MapBuffers::new();
        buffers.insert(PathBuf::from("/tmp/a.rs"), "buffer text");
        assert_eq!(
            buffers.get(Path::new("/tmp/a.rs")).as_deref(),
            Some("buffer text")
        );
        assert!(buffers.get(Path::new("/tmp/b.rs")).is_none());
    }
}
