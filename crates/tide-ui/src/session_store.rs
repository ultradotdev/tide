use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use tide_config::{Paths, SessionState};

#[derive(Debug, Clone, Serialize, Deserialize)]
struct LastSession {
    #[serde(rename = "sessionId")]
    session_id: String,
    #[serde(rename = "workspaceRoot")]
    workspace_root: String,
    timestamp: String,
}

/// Workspace and named session snapshots under `$XDG_STATE_HOME/tide/sessions/rust`.
///
/// A save refuses to overwrite a file another instance changed since this store
/// last read or wrote it, so concurrent instances cannot discard each other's
/// recovery snapshots.
#[derive(Debug, Clone)]
pub struct SessionStore {
    paths: Paths,
    workspace: PathBuf,
    observed:
        std::sync::Arc<std::sync::Mutex<std::collections::BTreeMap<PathBuf, Option<Vec<u8>>>>>,
}

impl SessionStore {
    #[must_use]
    pub fn new(paths: Paths, workspace: PathBuf) -> Self {
        let store = Self {
            paths,
            workspace,
            observed: Default::default(),
        };
        crate::lock(&store.observed).insert(
            store.workspace_session_path(),
            std::fs::read(store.workspace_session_path()).ok(),
        );
        store
    }

    #[must_use]
    pub fn workspace(&self) -> &Path {
        &self.workspace
    }

    /// The session last saved for this workspace, named or not.
    pub fn load_last(&self) -> Result<Option<SessionState>> {
        let last = read_last_optional(&self.last_session_file())?;
        let Some(last) = last.filter(|last| Path::new(&last.workspace_root) == self.workspace)
        else {
            return self.read_session(&self.workspace_session_path());
        };
        let session_path = if let Some(hash) = last.session_id.strip_prefix("workspace-") {
            self.root().join("workspaces").join(format!("{hash}.json"))
        } else if let Some(name) = last.session_id.strip_prefix("named-") {
            self.named_session_path(name)
        } else {
            self.workspace_session_path()
        };
        self.read_session(&session_path)
    }

    /// Names of saved sessions.
    pub fn named_sessions(&self) -> Vec<String> {
        let mut names = std::collections::BTreeSet::new();
        if let Ok(entries) = std::fs::read_dir(self.root().join("named")) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().is_none_or(|extension| extension != "json") {
                    continue;
                }
                if let Some(stem) = path.file_stem().and_then(|stem| stem.to_str()) {
                    names.insert(stem.to_owned());
                }
            }
        }
        names.into_iter().collect()
    }

    pub fn load_named(&self, name: &str) -> Result<Option<SessionState>> {
        self.read_session(&self.named_session_path(name))
    }

    pub fn save(&self, state: &SessionState, name: Option<&str>) -> Result<()> {
        let bytes = serde_json::to_vec_pretty(state)?;
        let path = if let Some(name) = name {
            self.named_session_path(name)
        } else {
            self.workspace_session_path()
        };
        let parent = path.parent().context("session parent")?;
        std::fs::create_dir_all(parent)?;
        set_mode(parent, 0o700)?;
        #[cfg(unix)]
        let _lock = {
            use nix::fcntl::{Flock, FlockArg};
            let file = std::fs::OpenOptions::new()
                .create(true)
                .truncate(false)
                .read(true)
                .write(true)
                .open(path.with_extension("lock"))?;
            Flock::lock(file, FlockArg::LockExclusiveNonblock).map_err(|(_, error)| {
                anyhow::anyhow!("session is being saved by another instance: {error}")
            })?
        };
        let mut observed = self
            .observed
            .lock()
            .map_err(|_| anyhow::anyhow!("session state lock poisoned"))?;
        let current = match std::fs::read(&path) {
            Ok(bytes) => Some(bytes),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
            Err(error) => return Err(error.into()),
        };
        anyhow::ensure!(
            observed.get(&path).unwrap_or(&None) == &current,
            "session changed in another instance; reopen it before saving (existing recovery was preserved)"
        );
        write_private(&path, &bytes)?;
        observed.insert(path, Some(bytes.clone()));
        drop(observed);
        let session_id = if let Some(name) = name {
            format!("named-{name}")
        } else {
            format!("workspace-{}", self.workspace_hash())
        };
        write_private(
            &self.last_session_file(),
            &serde_json::to_vec_pretty(&LastSession {
                session_id,
                workspace_root: self.workspace.display().to_string(),
                timestamp: timestamp(),
            })?,
        )?;
        Ok(())
    }

    #[must_use]
    pub fn workspace_session_path(&self) -> PathBuf {
        self.root()
            .join("workspaces")
            .join(format!("{}.json", self.workspace_hash()))
    }

    #[must_use]
    pub fn named_session_path(&self, name: &str) -> PathBuf {
        self.root()
            .join("named")
            .join(format!("{}.json", sanitize_name(name)))
    }

    fn read_session(&self, path: &Path) -> Result<Option<SessionState>> {
        let bytes = match std::fs::read(path) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(error.into()),
        };
        let state: SessionState = serde_json::from_slice(&bytes)
            .with_context(|| format!("parse session {}", path.display()))?;
        state.validate_version()?;
        self.observed
            .lock()
            .map_err(|_| anyhow::anyhow!("session state lock poisoned"))?
            .insert(path.to_path_buf(), Some(bytes));
        Ok(Some(state))
    }

    fn root(&self) -> PathBuf {
        self.paths.sessions_dir.join("rust")
    }

    fn last_session_file(&self) -> PathBuf {
        self.root().join("last-session.json")
    }

    fn workspace_hash(&self) -> String {
        let mut hasher = Sha256::new();
        hasher.update(self.workspace.display().to_string().as_bytes());
        let digest = hasher.finalize();
        digest
            .iter()
            .take(8)
            .map(|byte| format!("{byte:02x}"))
            .collect()
    }
}

fn read_last_optional(path: &Path) -> Result<Option<LastSession>> {
    match std::fs::read_to_string(path) {
        Ok(text) => Ok(Some(serde_json::from_str(&text)?)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error.into()),
    }
}

fn write_private(path: &Path, bytes: &[u8]) -> Result<()> {
    let parent = path
        .parent()
        .context("session path must have a parent directory")?;
    std::fs::create_dir_all(parent)?;
    set_mode(parent, 0o700)?;

    if path.exists() {
        let backup = path.with_extension("json.bak");
        std::fs::copy(path, &backup)?;
        set_mode(&backup, 0o600)?;
    }

    let temporary = path.with_extension(format!("tmp.{}", std::process::id()));
    {
        use std::io::Write;
        let mut file = std::fs::File::create(&temporary)?;
        set_mode(&temporary, 0o600)?;
        file.write_all(bytes)?;
        file.sync_all()?;
    }
    std::fs::rename(&temporary, path)?;
    set_mode(path, 0o600)?;
    Ok(())
}

#[cfg(unix)]
fn set_mode(path: &Path, mode: u32) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode))?;
    Ok(())
}

#[cfg(not(unix))]
fn set_mode(_path: &Path, _mode: u32) -> Result<()> {
    Ok(())
}

fn sanitize_name(name: &str) -> String {
    let mut value = name
        .to_lowercase()
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || ch == '_' || ch == '-' {
                ch
            } else {
                '-'
            }
        })
        .collect::<String>();
    while value.contains("--") {
        value = value.replace("--", "-");
    }
    value.trim_matches('-').to_owned()
}

fn timestamp() -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    format!("{}.{:03}Z", now.as_secs(), now.subsec_millis())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tide_config::builtin;

    fn store() -> (tempfile::TempDir, SessionStore) {
        let home = tempfile::tempdir().unwrap();
        let paths = Paths::from_env(home.path(), |_| None);
        let workspace = home.path().join("workspace");
        std::fs::create_dir_all(&workspace).unwrap();
        let store = SessionStore::new(paths, workspace);
        (home, store)
    }

    fn fixture() -> SessionState {
        builtin::parse_session(
            &std::fs::read_to_string(
                Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/session-v7.json"),
            )
            .unwrap(),
        )
        .unwrap()
    }

    #[test]
    fn concurrent_instances_cannot_overwrite_each_others_recovery() {
        let (_home, first) = store();
        let second = SessionStore::new(first.paths.clone(), first.workspace.clone());
        let mut session = fixture();
        session.workspace_root = first.workspace.display().to_string();
        first.save(&session, None).unwrap();
        assert!(second.save(&session, None).is_err());
        assert_eq!(second.load_last().unwrap().unwrap(), session);
        second.save(&session, None).unwrap();
    }

    #[test]
    fn workspace_recovery_is_found_after_visiting_another_workspace() {
        let (_home, first) = store();
        let other_root = first.workspace.with_file_name("other");
        std::fs::create_dir_all(&other_root).unwrap();
        let other = SessionStore::new(first.paths.clone(), other_root);
        let mut session = fixture();
        session.workspace_root = first.workspace.display().to_string();
        first.save(&session, None).unwrap();
        let mut other_session = fixture();
        other_session.workspace_root = other.workspace.display().to_string();
        other.save(&other_session, None).unwrap();
        assert_eq!(
            first.load_last().unwrap().unwrap().workspace_root,
            session.workspace_root
        );
    }

    #[test]
    fn workspace_hash_is_stable() {
        let (_home, store) = store();
        assert_eq!(store.workspace_hash(), store.workspace_hash());
        assert_eq!(store.workspace_hash().len(), 16);
    }

    #[test]
    fn saves_and_loads_workspace_and_named_sessions() {
        let (_home, store) = store();
        let mut session = fixture();
        session.workspace_root = store.workspace().display().to_string();
        store.save(&session, None).unwrap();
        assert!(store.workspace_session_path().exists());
        let loaded = store.load_last().unwrap().expect("session should load");
        assert_eq!(loaded.workspace_root, session.workspace_root);

        store.save(&session, Some("Review")).unwrap();
        assert_eq!(store.named_sessions(), ["review"]);
        assert_eq!(store.load_named("Review").unwrap(), Some(session.clone()));
        assert_eq!(store.load_last().unwrap(), Some(session));
    }

    #[test]
    fn malformed_session_is_reported_not_silently_ignored() {
        let (_home, store) = store();
        let path = store.workspace_session_path();
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, "{ broken").unwrap();
        assert!(store.load_last().is_err());
    }

    #[test]
    fn sanitizes_named_sessions() {
        assert_eq!(sanitize_name("My Project!"), "my-project");
    }
}
