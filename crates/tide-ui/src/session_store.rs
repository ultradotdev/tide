use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use tide_config::{Paths, SessionState};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionOrigin {
    Rust,
    TypeScript,
}

#[derive(Debug, Clone)]
pub struct LoadedSession {
    pub state: SessionState,
    pub origin: SessionOrigin,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct LastSession {
    #[serde(rename = "sessionId")]
    session_id: String,
    #[serde(rename = "workspaceRoot")]
    workspace_root: String,
    timestamp: String,
}

/// Session storage with a Rust-owned default and opt-in TypeScript writes.
///
/// Until bidirectional compatibility is proven, Rust writes its own session
/// files. TypeScript sessions are read for migration but are never overwritten
/// unless `shared_writes` is explicitly enabled.
#[derive(Debug, Clone)]
pub struct SessionStore {
    paths: Paths,
    workspace: PathBuf,
    shared_writes: bool,
    observed:
        std::sync::Arc<std::sync::Mutex<std::collections::BTreeMap<PathBuf, Option<Vec<u8>>>>>,
}

impl SessionStore {
    #[must_use]
    pub fn new(paths: Paths, workspace: PathBuf) -> Self {
        let store = Self {
            paths,
            workspace,
            shared_writes: false,
            observed: Default::default(),
        };
        crate::lock(&store.observed).insert(
            store.rust_workspace_path(),
            std::fs::read(store.rust_workspace_path()).ok(),
        );
        store
    }

    #[must_use]
    pub fn with_shared_writes(mut self, shared: bool) -> Self {
        self.shared_writes = shared;
        self
    }

    #[must_use]
    pub fn workspace(&self) -> &Path {
        &self.workspace
    }

    pub fn load_last(&self) -> Result<Option<LoadedSession>> {
        if let Some(session) = self.load_rust_last()? {
            return Ok(Some(session));
        }
        self.load_typescript_last()
    }

    /// Names of saved sessions from both Rust-owned and read-only TypeScript stores.
    pub fn named_sessions(&self) -> Vec<String> {
        let mut names = std::collections::BTreeSet::new();
        for directory in [
            self.rust_root().join("named"),
            self.paths.named_sessions_dir.clone(),
        ] {
            let Ok(entries) = std::fs::read_dir(directory) else {
                continue;
            };
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

    pub fn load_named(&self, name: &str) -> Result<Option<LoadedSession>> {
        let rust_path = self.rust_named_path(name);
        if let Some(state) = self.read_rust(&rust_path)? {
            return Ok(Some(LoadedSession {
                state,
                origin: SessionOrigin::Rust,
            }));
        }
        let ts_path = self.named_session_path(name);
        if let Some(state) = read_session_optional(&ts_path)? {
            return Ok(Some(LoadedSession {
                state,
                origin: SessionOrigin::TypeScript,
            }));
        }
        Ok(None)
    }

    pub fn save(&self, state: &SessionState, name: Option<&str>) -> Result<()> {
        let bytes = serde_json::to_vec_pretty(state)?;
        let rust_path = if let Some(name) = name {
            self.rust_named_path(name)
        } else {
            self.rust_workspace_path()
        };
        let parent = rust_path.parent().context("session parent")?;
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
                .open(rust_path.with_extension("lock"))?;
            Flock::lock(file, FlockArg::LockExclusiveNonblock).map_err(|(_, error)| {
                anyhow::anyhow!("session is being saved by another instance: {error}")
            })?
        };
        let mut observed = self
            .observed
            .lock()
            .map_err(|_| anyhow::anyhow!("session state lock poisoned"))?;
        let current = match std::fs::read(&rust_path) {
            Ok(bytes) => Some(bytes),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
            Err(error) => return Err(error.into()),
        };
        anyhow::ensure!(
            observed.get(&rust_path).unwrap_or(&None) == &current,
            "session changed in another instance; reopen it before saving (existing recovery was preserved)"
        );
        write_private(&rust_path, &bytes)?;
        observed.insert(rust_path, Some(bytes.clone()));
        drop(observed);
        let rust_id = if let Some(name) = name {
            format!("named-{name}")
        } else {
            format!("workspace-{}", self.workspace_hash())
        };
        write_private(
            &self.rust_last_file(),
            &serde_json::to_vec_pretty(&LastSession {
                session_id: rust_id,
                workspace_root: self.workspace.display().to_string(),
                timestamp: timestamp(),
            })?,
        )?;

        if self.shared_writes {
            let ts_path = if let Some(name) = name {
                self.named_session_path(name)
            } else {
                self.workspace_session_path()
            };
            write_private(&ts_path, &bytes)?;
            let ts_id = if let Some(name) = name {
                format!("named-{name}")
            } else {
                format!("workspace-{}", self.workspace_hash())
            };
            write_private(
                &self.paths.last_session_file,
                &serde_json::to_vec_pretty(&LastSession {
                    session_id: ts_id,
                    workspace_root: self.workspace.display().to_string(),
                    timestamp: timestamp(),
                })?,
            )?;
        }
        Ok(())
    }

    #[must_use]
    pub fn workspace_session_path(&self) -> PathBuf {
        self.paths
            .workspace_sessions_dir
            .join(format!("{}.json", self.workspace_hash()))
    }

    #[must_use]
    pub fn named_session_path(&self, name: &str) -> PathBuf {
        self.paths
            .named_sessions_dir
            .join(format!("{}.json", sanitize_name(name)))
    }

    #[must_use]
    pub fn rust_workspace_path(&self) -> PathBuf {
        self.rust_root()
            .join("workspaces")
            .join(format!("{}.json", self.workspace_hash()))
    }

    #[must_use]
    pub fn rust_named_path(&self, name: &str) -> PathBuf {
        self.rust_root()
            .join("named")
            .join(format!("{}.json", sanitize_name(name)))
    }

    fn read_rust(&self, path: &Path) -> Result<Option<SessionState>> {
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

    fn rust_root(&self) -> PathBuf {
        self.paths.state_dir.join("sessions").join("rust")
    }

    fn rust_last_file(&self) -> PathBuf {
        self.rust_root().join("last-session.json")
    }

    fn load_rust_last(&self) -> Result<Option<LoadedSession>> {
        let path = self.rust_last_file();
        let last = read_last_optional(&path)?;
        if last
            .as_ref()
            .is_none_or(|last| Path::new(&last.workspace_root) != self.workspace)
        {
            return Ok(self
                .read_rust(&self.rust_workspace_path())?
                .map(|state| LoadedSession {
                    state,
                    origin: SessionOrigin::Rust,
                }));
        }
        let last = last.expect("matched last session");
        let session_path = if let Some(hash) = last.session_id.strip_prefix("workspace-") {
            self.rust_root()
                .join("workspaces")
                .join(format!("{hash}.json"))
        } else if let Some(name) = last.session_id.strip_prefix("named-") {
            self.rust_named_path(name)
        } else {
            self.rust_workspace_path()
        };
        let Some(state) = self.read_rust(&session_path)? else {
            return Ok(None);
        };
        Ok(Some(LoadedSession {
            state,
            origin: SessionOrigin::Rust,
        }))
    }

    fn load_typescript_last(&self) -> Result<Option<LoadedSession>> {
        if let Some(last) = read_last_optional(&self.paths.last_session_file)?
            && Path::new(&last.workspace_root) == self.workspace
        {
            let path = self.typescript_session_path_for_id(&last.session_id);
            if let Some(state) = read_session_optional(&path)? {
                return Ok(Some(LoadedSession {
                    state,
                    origin: SessionOrigin::TypeScript,
                }));
            }
        }
        if let Some(state) = read_session_optional(&self.workspace_session_path())? {
            return Ok(Some(LoadedSession {
                state,
                origin: SessionOrigin::TypeScript,
            }));
        }
        Ok(None)
    }

    fn typescript_session_path_for_id(&self, session_id: &str) -> PathBuf {
        if let Some(hash) = session_id.strip_prefix("workspace-") {
            self.paths
                .workspace_sessions_dir
                .join(format!("{hash}.json"))
        } else if let Some(name) = session_id.strip_prefix("named-") {
            self.named_session_path(name)
        } else {
            self.workspace_session_path()
        }
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

fn read_session_optional(path: &Path) -> Result<Option<SessionState>> {
    let text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.into()),
    };
    let session: SessionState =
        serde_json::from_str(&text).with_context(|| format!("parse session {}", path.display()))?;
    session
        .validate_version()
        .with_context(|| format!("validate session {}", path.display()))?;
    Ok(Some(session))
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
                Path::new(env!("CARGO_MANIFEST_DIR"))
                    .join("../../tests/fixtures/rust-parity/session-v7.json"),
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
        assert_eq!(second.load_last().unwrap().unwrap().state, session);
        second.save(&session, None).unwrap();
    }

    #[test]
    fn rust_workspace_recovery_is_found_after_visiting_another_workspace() {
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
            first.load_last().unwrap().unwrap().state.workspace_root,
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
    fn saves_to_rust_storage_and_does_not_touch_typescript_state() {
        let (_home, store) = store();
        let mut session = fixture();
        session.workspace_root = store.workspace().display().to_string();
        store.save(&session, None).unwrap();
        assert!(store.rust_workspace_path().exists());
        assert!(!store.workspace_session_path().exists());
        let loaded = store.load_last().unwrap().expect("session should load");
        assert_eq!(loaded.origin, SessionOrigin::Rust);
        assert_eq!(loaded.state.workspace_root, session.workspace_root);
    }

    #[test]
    fn reads_typescript_session_without_writing_it() {
        let (_home, store) = store();
        let mut session = fixture();
        session.workspace_root = store.workspace().display().to_string();
        write_private(
            &store.workspace_session_path(),
            &serde_json::to_vec_pretty(&session).unwrap(),
        )
        .unwrap();
        let before = std::fs::read(store.workspace_session_path()).unwrap();
        let loaded = store.load_last().unwrap().expect("session should load");
        assert_eq!(loaded.origin, SessionOrigin::TypeScript);
        let after = std::fs::read(store.workspace_session_path()).unwrap();
        assert_eq!(before, after);
    }

    #[test]
    fn malformed_typescript_session_is_reported_not_silently_ignored() {
        let (_home, store) = store();
        let path = store.workspace_session_path();
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, "{ broken").unwrap();
        assert!(store.load_last().is_err());
    }

    #[test]
    fn shared_write_opt_in_writes_both_stores() {
        let (_home, store) = store();
        let shared = store.with_shared_writes(true);
        let mut session = fixture();
        session.workspace_root = shared.workspace().display().to_string();
        shared.save(&session, None).unwrap();
        assert!(shared.rust_workspace_path().exists());
        assert!(shared.workspace_session_path().exists());
        let loaded = shared.load_last().unwrap().expect("session");
        assert_eq!(loaded.origin, SessionOrigin::Rust);
    }

    #[test]
    fn sanitizes_named_sessions() {
        assert_eq!(sanitize_name("My Project!"), "my-project");
    }
}
