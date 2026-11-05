use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use tide_config::{Paths, SessionState};

/// A named session saved for this workspace.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SavedSession {
    /// The file name, which opens and deletes it.
    pub name: String,
    /// The name it was saved under.
    pub title: String,
    pub saved_millis: Option<u128>,
    pub documents: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct LastSession {
    #[serde(rename = "sessionId")]
    session_id: String,
    #[serde(rename = "workspaceRoot")]
    workspace_root: String,
    timestamp: String,
}

/// Workspace and named session snapshots under `$XDG_STATE_HOME/tide/sessions`.
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
        if let Err(error) = migrate_layout(&paths.sessions_dir) {
            tracing::warn!(%error, "could not move sessions out of sessions/rust");
        }
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

    /// Named sessions saved for this workspace, newest first. Each file is read only as far
    /// as its summary; a file that does not parse is listed as unreadable.
    pub fn workspace_sessions(&self) -> Vec<SavedSession> {
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct Summary {
            workspace_root: String,
            #[serde(default)]
            session_name: Option<String>,
            #[serde(default)]
            timestamp: String,
            #[serde(default)]
            documents: Vec<serde::de::IgnoredAny>,
        }
        let mut sessions = self
            .named_sessions()
            .into_iter()
            .filter_map(|name| {
                let bytes = std::fs::read(self.named_session_path(&name)).ok()?;
                let summary = serde_json::from_slice::<Summary>(&bytes).ok()?;
                (Path::new(&summary.workspace_root) == self.workspace).then(|| SavedSession {
                    title: summary.session_name.unwrap_or_else(|| name.clone()),
                    name,
                    saved_millis: summary.timestamp.parse().ok(),
                    documents: summary.documents.len(),
                })
            })
            .collect::<Vec<_>>();
        sessions.sort_by_key(|session| std::cmp::Reverse(session.saved_millis));
        sessions
    }

    /// Whether `a` and `b` name the same saved session file.
    #[must_use]
    pub fn same_name(a: &str, b: &str) -> bool {
        sanitize_name(a) == sanitize_name(b)
    }

    /// Deletes a named session with its backup. A last-session record that pointed at it is
    /// dropped too, so the next start restores this workspace's recovery session instead.
    pub fn delete_named(&self, name: &str) -> Result<()> {
        let path = self.named_session_path(name);
        match std::fs::remove_file(&path) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
        for extra in [path.with_extension("json.bak"), path.with_extension("lock")] {
            let _ = std::fs::remove_file(extra);
        }
        crate::lock(&self.observed).insert(path, None);
        let last = read_last_optional(&self.last_session_file()).ok().flatten();
        if last.is_some_and(|last| last.session_id == format!("named-{}", sanitize_name(name))) {
            let _ = std::fs::remove_file(self.last_session_file());
        }
        Ok(())
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
        self.paths.sessions_dir.clone()
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

/// Marks a sessions folder that uses the current layout, with sessions at its top.
const LAYOUT_MARKER: &str = ".layout";

/// Moves sessions out of `sessions/rust`, where Tide 0.10.0 to 0.12.2 kept them apart
/// from the TypeScript-era Tide's files, which use the same names at the top of
/// `sessions/`.
///
/// The first time, anything already at the top (those old files) is moved into
/// `sessions/legacy/`, and the `.layout` marker is written so it never happens again. Then
/// the contents of `rust/` move up. An older Tide still running can recreate `rust/`;
/// later starts move its files up too, but never over a file that is already there.
fn migrate_layout(sessions: &Path) -> Result<()> {
    let old = sessions.join("rust");
    let marker = sessions.join(LAYOUT_MARKER);
    if marker.exists() && !old.is_dir() {
        return Ok(());
    }
    if !marker.exists() {
        if old.is_dir() {
            let legacy = sessions.join("legacy");
            for entry in std::fs::read_dir(sessions)? {
                let entry = entry?;
                let name = entry.file_name();
                if name == "rust" || name == "legacy" || name == LAYOUT_MARKER {
                    continue;
                }
                std::fs::create_dir_all(&legacy)?;
                set_mode(&legacy, 0o700)?;
                move_missing(&entry.path(), &legacy.join(&name))?;
            }
        }
        std::fs::create_dir_all(sessions)?;
        set_mode(sessions, 0o700)?;
        write_private(&marker, b"2\n")?;
    }
    if old.is_dir() {
        move_missing(&old, sessions)?;
    }
    Ok(())
}

/// Moves `from` to `to`, merging folders: what already exists at `to` is kept, and what
/// could not move stays at `from`. Emptied folders are removed.
fn move_missing(from: &Path, to: &Path) -> Result<()> {
    let is_dir = std::fs::symlink_metadata(from)?.is_dir();
    if std::fs::symlink_metadata(to).is_err() {
        std::fs::rename(from, to)
            .with_context(|| format!("move {} to {}", from.display(), to.display()))?;
        return Ok(());
    }
    if is_dir && std::fs::symlink_metadata(to)?.is_dir() {
        for entry in std::fs::read_dir(from)? {
            let entry = entry?;
            move_missing(&entry.path(), &to.join(entry.file_name()))?;
        }
        // Fails, harmlessly, when something was left behind.
        let _ = std::fs::remove_dir(from);
    }
    Ok(())
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

    fn write(path: &Path, text: &str) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }

    fn read(path: &Path) -> String {
        std::fs::read_to_string(path).unwrap()
    }

    #[test]
    fn sessions_move_out_of_the_rust_folder_and_old_files_are_archived() {
        let home = tempfile::tempdir().unwrap();
        let paths = Paths::from_env(home.path(), |_| None);
        let sessions = paths.sessions_dir.clone();
        write(&sessions.join("last-session.json"), "typescript last");
        write(&sessions.join("workspaces/abc.json"), "typescript abc");
        write(&sessions.join("rust/last-session.json"), "rust last");
        write(&sessions.join("rust/workspaces/abc.json"), "rust abc");
        write(&sessions.join("rust/named/demo.json"), "rust named");

        let workspace = home.path().join("workspace");
        std::fs::create_dir_all(&workspace).unwrap();
        let _store = SessionStore::new(paths.clone(), workspace.clone());

        assert_eq!(read(&sessions.join("last-session.json")), "rust last");
        assert_eq!(read(&sessions.join("workspaces/abc.json")), "rust abc");
        assert_eq!(read(&sessions.join("named/demo.json")), "rust named");
        assert_eq!(
            read(&sessions.join("legacy/last-session.json")),
            "typescript last"
        );
        assert_eq!(
            read(&sessions.join("legacy/workspaces/abc.json")),
            "typescript abc"
        );
        assert!(!sessions.join("rust").exists());
        assert!(sessions.join(LAYOUT_MARKER).is_file());

        // An older Tide still running writes to rust/ again: its new files move up on the
        // next start, nothing current is archived, and nothing is overwritten.
        write(
            &sessions.join("rust/workspaces/abc.json"),
            "older instance abc",
        );
        write(
            &sessions.join("rust/workspaces/new.json"),
            "older instance new",
        );
        let _store = SessionStore::new(paths, workspace);
        assert_eq!(read(&sessions.join("workspaces/abc.json")), "rust abc");
        assert_eq!(
            read(&sessions.join("workspaces/new.json")),
            "older instance new"
        );
        assert_eq!(read(&sessions.join("last-session.json")), "rust last");
        assert_eq!(
            read(&sessions.join("rust/workspaces/abc.json")),
            "older instance abc",
            "kept where it was rather than overwrite"
        );
        assert!(!sessions.join("legacy/workspaces/new.json").exists());
    }

    #[test]
    fn a_new_profile_needs_no_migration() {
        let home = tempfile::tempdir().unwrap();
        let paths = Paths::from_env(home.path(), |_| None);
        let sessions = paths.sessions_dir.clone();
        let workspace = home.path().join("workspace");
        std::fs::create_dir_all(&workspace).unwrap();
        let store = SessionStore::new(paths, workspace);
        store.save(&fixture(), None).unwrap();
        assert!(
            store
                .workspace_session_path()
                .starts_with(sessions.join("workspaces"))
        );
        assert!(!sessions.join("legacy").exists());
        assert!(!sessions.join("rust").exists());
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
    fn lists_this_workspaces_sessions_and_deletes_them_with_their_last_session_record() {
        let (_home, store) = store();
        let other_root = store.workspace.with_file_name("other");
        std::fs::create_dir_all(&other_root).unwrap();
        let other = SessionStore::new(store.paths.clone(), other_root);
        let mut session = fixture();
        session.workspace_root = store.workspace().display().to_string();
        session.timestamp = "1000".into();
        store.save(&session, Some("Older")).unwrap();
        session.timestamp = "2000".into();
        session.session_name = Some("Review Work".into());
        store.save(&session, Some("Review Work")).unwrap();
        let mut elsewhere = fixture();
        elsewhere.workspace_root = other.workspace().display().to_string();
        other.save(&elsewhere, Some("Elsewhere")).unwrap();

        let listed = store.workspace_sessions();
        let names = listed.iter().map(|s| s.name.as_str()).collect::<Vec<_>>();
        assert_eq!(names, ["review-work", "older"]);
        assert_eq!(listed[0].title, "Review Work");
        assert_eq!(listed[0].saved_millis, Some(2000));
        assert!(SessionStore::same_name("Review Work", "review-work"));

        // The last save named "Review Work", so deleting it drops that record too.
        store.delete_named("review-work").unwrap();
        assert!(!store.named_session_path("review-work").exists());
        assert_eq!(store.workspace_sessions().len(), 1);
        assert_eq!(store.load_last().unwrap(), None);
        // A deleted name can be saved again without a conflict.
        store.save(&session, Some("Review Work")).unwrap();
        assert!(other.named_session_path("elsewhere").exists());
    }

    #[test]
    fn sanitizes_named_sessions() {
        assert_eq!(sanitize_name("My Project!"), "my-project");
    }
}
