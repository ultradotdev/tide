//! The workspace boundary for user- and server-supplied paths.

use std::path::{Component, Path, PathBuf};

/// Why a path could not be resolved inside the workspace.
#[derive(Debug)]
pub enum WorkspaceError {
    /// The path, or a symlink along it, leads outside the workspace.
    Outside(String),
    Io(std::io::Error),
}

impl std::fmt::Display for WorkspaceError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Outside(path) => write!(f, "path escapes the workspace: {path}"),
            Self::Io(error) => write!(f, "io error: {error}"),
        }
    }
}

impl std::error::Error for WorkspaceError {}

impl From<std::io::Error> for WorkspaceError {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error)
    }
}

/// A canonicalized workspace root. Every resolved path is checked against it.
#[derive(Debug, Clone)]
pub struct Workspace {
    canonical_root: PathBuf,
}

impl Workspace {
    pub fn new(root: impl AsRef<Path>) -> Result<Self, WorkspaceError> {
        Ok(Self {
            canonical_root: std::fs::canonicalize(root)?,
        })
    }

    /// Resolve a workspace-relative or absolute path, rejecting anything outside the
    /// workspace, including through symlinks.
    pub fn resolve(&self, path: &str) -> Result<PathBuf, WorkspaceError> {
        let outside = || WorkspaceError::Outside(path.to_owned());
        let candidate = Path::new(path);
        let joined = if candidate.is_absolute() {
            candidate.to_path_buf()
        } else {
            self.canonical_root.join(candidate)
        };
        let normalized = normalize(&joined);
        if !normalized.starts_with(&self.canonical_root) {
            return Err(outside());
        }
        if normalized.exists() {
            let canonical = std::fs::canonicalize(&normalized)?;
            if !canonical.starts_with(&self.canonical_root) {
                return Err(outside());
            }
            return Ok(canonical);
        }
        // For new files, canonicalize the nearest existing ancestor and re-append
        // the missing components so symlinked parents cannot escape the root.
        let mut existing = normalized.as_path();
        let mut suffix = Vec::new();
        while !existing.exists() {
            suffix.push(existing.file_name().ok_or_else(outside)?.to_os_string());
            existing = existing.parent().ok_or_else(outside)?;
        }
        let mut canonical = std::fs::canonicalize(existing)?;
        if !canonical.starts_with(&self.canonical_root) {
            return Err(outside());
        }
        for component in suffix.iter().rev() {
            canonical.push(component);
        }
        if !canonical.starts_with(&self.canonical_root) {
            return Err(outside());
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
}
