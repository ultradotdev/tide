//! Staged, reviewable file changes with atomic apply and guarded rollback.

use std::collections::BTreeMap;
use std::path::Path;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use similar::TextDiff;

use crate::router::{MAX_READ_BYTES, truncate};
use crate::workspace::{BufferSource, ToolError, Workspace};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileChange {
    /// Workspace-relative path.
    pub path: String,
    /// `None` means the file is created.
    pub before: Option<String>,
    /// `None` means the file is deleted.
    pub after: Option<String>,
    #[serde(skip)]
    pub before_hash: Option<String>,
}

impl FileChange {
    pub fn write(
        workspace: &Workspace,
        path: &str,
        content: &str,
        buffers: &dyn BufferSource,
    ) -> Result<Self, ToolError> {
        let resolved = workspace.resolve(path)?;
        let before = match buffers.get(&resolved) {
            Some(text) => Some(text),
            None => match workspace.read_bytes(&resolved)? {
                Some(bytes) => Some(
                    String::from_utf8(bytes)
                        .map_err(|_| ToolError::InvalidInput(format!("{path} is not UTF-8")))?,
                ),
                None => None,
            },
        };
        Ok(Self {
            path: workspace.relative(&resolved),
            before_hash: before.as_deref().map(hash_text),
            before,
            after: Some(content.to_owned()),
        })
    }

    pub fn delete(workspace: &Workspace, path: &str) -> Result<Self, ToolError> {
        let resolved = workspace.resolve(path)?;
        let before = workspace
            .read_bytes(&resolved)?
            .map(|bytes| {
                String::from_utf8(bytes)
                    .map_err(|_| ToolError::InvalidInput(format!("{path} is not UTF-8")))
            })
            .transpose()?;
        if before.is_none() {
            return Err(ToolError::Io(format!("{path} does not exist")));
        }
        Ok(Self {
            path: workspace.relative(&resolved),
            before_hash: before.as_deref().map(hash_text),
            before,
            after: None,
        })
    }

    pub fn is_creation(&self) -> bool {
        self.before.is_none() && self.after.is_some()
    }

    pub fn is_deletion(&self) -> bool {
        self.before.is_some() && self.after.is_none()
    }
}

#[derive(Debug, Clone, Default)]
pub struct AppliedChange {
    pub path: String,
    pub before: Option<String>,
    pub after_hash: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ApplyReport {
    pub applied: Vec<String>,
    /// Approved sensitive reads, keyed by workspace-relative path.
    #[serde(default)]
    pub reads: BTreeMap<String, Result<String, String>>,
}

pub const PENDING_READ_PREFIX: &str = "Pending approval for read_file ";

impl ApplyReport {
    /// The tool result that replaces a "Pending approval" result once the
    /// user approves the change set.
    pub fn resolve_pending(&self, pending: &str) -> (String, bool) {
        let Some(path) = pending
            .strip_prefix(PENDING_READ_PREFIX)
            .and_then(|rest| rest.lines().next())
        else {
            return (
                format!("Applied changes to {}", self.applied.join(", ")),
                false,
            );
        };
        match self.reads.get(path) {
            Some(Ok(content)) => (content.clone(), false),
            Some(Err(error)) => (error.clone(), true),
            None => (format!("read of {path} was not approved"), true),
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct ChangeSet {
    pub changes: Vec<FileChange>,
    /// Workspace-relative sensitive files the model asked to read.
    pub reads: Vec<String>,
    pub applied: Vec<AppliedChange>,
    pub rolled_back: bool,
}

impl ChangeSet {
    pub fn new(changes: Vec<FileChange>) -> Self {
        Self {
            changes,
            reads: Vec::new(),
            applied: Vec::new(),
            rolled_back: false,
        }
    }

    pub fn sensitive_read(path: String) -> Self {
        Self {
            reads: vec![path],
            ..Self::new(Vec::new())
        }
    }

    pub fn merge(&mut self, mut other: Self) {
        self.changes.append(&mut other.changes);
        for read in other.reads {
            if !self.reads.contains(&read) {
                self.reads.push(read);
            }
        }
    }

    pub fn is_empty(&self) -> bool {
        self.changes.is_empty() && self.reads.is_empty()
    }

    /// Unified diff across all staged files.
    pub fn preview(&self) -> String {
        let mut out = String::new();
        for path in &self.reads {
            out.push_str(&format!(
                "read {path}: sensitive file, shared with the AI only if approved\n"
            ));
        }
        for change in &self.changes {
            out.push_str(&format!("--- {}\n+++ {}\n", change.path, change.path));
            let before = change.before.as_deref().unwrap_or("");
            let after = change.after.as_deref().unwrap_or("");
            if change.before.is_some() && change.after.is_some() {
                let diff = TextDiff::from_lines(before, after);
                out.push_str(&diff.unified_diff().context_radius(3).to_string());
            } else if change.after.is_none() {
                for line in before.lines() {
                    out.push_str(&format!("-{line}\n"));
                }
            } else {
                for line in after.lines() {
                    out.push_str(&format!("+{line}\n"));
                }
            }
        }
        out
    }

    /// Re-check staged bases against the current workspace and open buffers.
    pub fn refresh_bases(
        &mut self,
        workspace: &Workspace,
        buffers: &dyn BufferSource,
    ) -> Result<(), ToolError> {
        for change in &mut self.changes {
            let resolved = workspace.resolve(&change.path)?;
            let current = match buffers.get(&resolved) {
                Some(text) => Some(text),
                None => workspace
                    .read_bytes(&resolved)?
                    .map(|bytes| String::from_utf8_lossy(&bytes).into_owned()),
            };
            let current_hash = current.as_deref().map(hash_text);
            if current_hash != change.before_hash {
                return Err(ToolError::Stale);
            }
            change.before = current;
        }
        Ok(())
    }

    /// Apply every staged change. On failure, already-written files are restored.
    pub fn apply(&mut self, workspace: &Workspace) -> Result<ApplyReport, ToolError> {
        let reads = self
            .reads
            .iter()
            .map(|path| {
                let content = workspace
                    .read_text(path)
                    .map(|text| truncate(&text, MAX_READ_BYTES))
                    .map_err(|error| error.to_string());
                (path.clone(), content)
            })
            .collect();
        if self.changes.is_empty() {
            return Ok(ApplyReport {
                applied: Vec::new(),
                reads,
            });
        }
        let mut applied = Vec::new();
        for change in &self.changes {
            let resolved = workspace.resolve(&change.path)?;
            let current = workspace.read_bytes(&resolved)?;
            let current_hash = current.as_ref().map(|bytes| hash_bytes(bytes));
            let expected_hash = change.before.as_ref().map(|text| hash_text(text));
            if current_hash != expected_hash {
                self.restore_all(workspace, &applied)?;
                self.applied.clear();
                return Err(ToolError::Stale);
            }
            if let Err(error) = write_change(&resolved, change.after.as_deref()) {
                self.restore_all(workspace, &applied)?;
                self.applied.clear();
                return Err(error);
            }
            let record = AppliedChange {
                path: change.path.clone(),
                before: change.before.clone(),
                after_hash: change.after.as_deref().map(hash_text),
            };
            applied.push(record.clone());
            self.applied.push(record);
        }
        self.rolled_back = false;
        Ok(ApplyReport {
            applied: self
                .changes
                .iter()
                .map(|change| change.path.clone())
                .collect(),
            reads,
        })
    }

    /// Roll back only if the files still match what the change set wrote.
    pub fn rollback(&mut self, workspace: &Workspace) -> Result<(), ToolError> {
        if self.applied.is_empty() {
            return Ok(());
        }
        for record in &self.applied {
            let resolved = workspace.resolve(&record.path)?;
            let current = workspace.read_bytes(&resolved)?;
            let current_hash = current.as_ref().map(|bytes| hash_bytes(bytes));
            if current_hash != record.after_hash {
                return Err(ToolError::Stale);
            }
        }
        self.restore_all(workspace, &self.applied)?;
        self.applied.clear();
        self.rolled_back = true;
        Ok(())
    }

    pub fn rollback_eligible(&self, workspace: &Workspace) -> bool {
        self.applied.iter().all(|record| {
            let Ok(resolved) = workspace.resolve(&record.path) else {
                return false;
            };
            let current_hash = workspace
                .read_bytes(&resolved)
                .ok()
                .flatten()
                .as_ref()
                .map(|bytes| hash_bytes(bytes));
            current_hash == record.after_hash
        })
    }

    fn restore_all(
        &self,
        workspace: &Workspace,
        records: &[AppliedChange],
    ) -> Result<(), ToolError> {
        for record in records.iter().rev() {
            let resolved = workspace.resolve(&record.path)?;
            write_change(&resolved, record.before.as_deref())?;
        }
        Ok(())
    }
}

fn write_change(path: &Path, content: Option<&str>) -> Result<(), ToolError> {
    match content {
        Some(content) => tide_core::fs::write_atomic(path, content.as_bytes())
            .map_err(|error| ToolError::Io(error.to_string())),
        None => match std::fs::remove_file(path) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(ToolError::Io(error.to_string())),
        },
    }
}

fn hash_text(text: &str) -> String {
    hash_bytes(text.as_bytes())
}

fn hash_bytes(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    digest.iter().map(|byte| format!("{byte:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::workspace::MapBuffers;

    #[test]
    fn applies_and_rolls_back_a_multi_file_change_set() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("a.txt"), "old\n").unwrap();
        std::fs::write(dir.path().join("b.txt"), "keep\n").unwrap();
        let workspace = Workspace::new(dir.path()).unwrap();
        let buffers = MapBuffers::new();
        let mut changes = ChangeSet::new(vec![
            FileChange::write(&workspace, "a.txt", "new\n", &buffers).unwrap(),
            FileChange::write(&workspace, "c.txt", "created\n", &buffers).unwrap(),
            FileChange::delete(&workspace, "b.txt").unwrap(),
        ]);
        assert!(changes.preview().contains("-old"));
        let report = changes.apply(&workspace).unwrap();
        assert_eq!(report.applied.len(), 3);
        assert_eq!(
            std::fs::read_to_string(dir.path().join("a.txt")).unwrap(),
            "new\n"
        );
        assert_eq!(
            std::fs::read_to_string(dir.path().join("c.txt")).unwrap(),
            "created\n"
        );
        assert!(!dir.path().join("b.txt").exists());
        assert!(changes.rollback_eligible(&workspace));
        changes.rollback(&workspace).unwrap();
        assert_eq!(
            std::fs::read_to_string(dir.path().join("a.txt")).unwrap(),
            "old\n"
        );
        assert_eq!(
            std::fs::read_to_string(dir.path().join("b.txt")).unwrap(),
            "keep\n"
        );
        assert!(!dir.path().join("c.txt").exists());
    }

    #[test]
    fn refuses_stale_changes() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("a.txt"), "old\n").unwrap();
        let workspace = Workspace::new(dir.path()).unwrap();
        let buffers = MapBuffers::new();
        let mut changes = ChangeSet::new(vec![
            FileChange::write(&workspace, "a.txt", "new\n", &buffers).unwrap(),
        ]);
        std::fs::write(dir.path().join("a.txt"), "external\n").unwrap();
        assert!(matches!(changes.apply(&workspace), Err(ToolError::Stale)));
        assert_eq!(
            std::fs::read_to_string(dir.path().join("a.txt")).unwrap(),
            "external\n"
        );
    }

    #[test]
    fn refuses_rollback_after_external_edit() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("a.txt"), "old\n").unwrap();
        let workspace = Workspace::new(dir.path()).unwrap();
        let buffers = MapBuffers::new();
        let mut changes = ChangeSet::new(vec![
            FileChange::write(&workspace, "a.txt", "new\n", &buffers).unwrap(),
        ]);
        changes.apply(&workspace).unwrap();
        std::fs::write(dir.path().join("a.txt"), "someone else\n").unwrap();
        assert!(!changes.rollback_eligible(&workspace));
        assert!(matches!(
            changes.rollback(&workspace),
            Err(ToolError::Stale)
        ));
        assert_eq!(
            std::fs::read_to_string(dir.path().join("a.txt")).unwrap(),
            "someone else\n"
        );
    }

    #[cfg(unix)]
    #[test]
    fn planted_temp_symlink_cannot_redirect_an_approved_write() {
        let dir = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let target = outside.path().join("zshrc");
        std::fs::write(&target, "outside\n").unwrap();
        std::fs::create_dir_all(dir.path().join("src")).unwrap();
        std::fs::write(dir.path().join("src/main.rs"), "old\n").unwrap();
        std::os::unix::fs::symlink(&target, dir.path().join("src/main.rs.tmp")).unwrap();
        let workspace = Workspace::new(dir.path()).unwrap();
        let mut changes = ChangeSet::new(vec![
            FileChange::write(&workspace, "src/main.rs", "new\n", &MapBuffers::new()).unwrap(),
        ]);
        changes.apply(&workspace).unwrap();
        assert_eq!(std::fs::read_to_string(&target).unwrap(), "outside\n");
        let written = dir.path().join("src/main.rs");
        assert!(!written.symlink_metadata().unwrap().is_symlink());
        assert_eq!(std::fs::read_to_string(&written).unwrap(), "new\n");
    }

    #[cfg(unix)]
    #[test]
    fn applying_and_rolling_back_preserve_the_executable_bit() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let script = dir.path().join("run.sh");
        std::fs::write(&script, "#!/bin/sh\n").unwrap();
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
        let workspace = Workspace::new(dir.path()).unwrap();
        let mut changes = ChangeSet::new(vec![
            FileChange::write(
                &workspace,
                "run.sh",
                "#!/bin/sh\necho hi\n",
                &MapBuffers::new(),
            )
            .unwrap(),
        ]);
        changes.apply(&workspace).unwrap();
        let mode = || std::fs::metadata(&script).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode(), 0o755);
        changes.rollback(&workspace).unwrap();
        assert_eq!(mode(), 0o755);
    }

    #[test]
    fn rejects_writes_that_escape_the_workspace() {
        let dir = tempfile::tempdir().unwrap();
        let workspace = Workspace::new(dir.path()).unwrap();
        assert!(FileChange::write(&workspace, "../escape.txt", "x", &MapBuffers::new()).is_err());
    }
}
