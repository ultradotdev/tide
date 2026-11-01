//! Bounded JSONL query history with favorites and owner-only permissions.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::redact::redact_database_sensitive_text;

pub const MAX_HISTORY_ENTRIES: usize = 10_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum QueryStatus {
    Success,
    Error,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QueryHistoryEntry {
    pub id: String,
    pub connection_id: String,
    pub connection_name: String,
    pub sql: String,
    pub executed_at: String,
    pub duration_ms: u128,
    pub row_count: usize,
    pub status: QueryStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    #[serde(default)]
    pub is_favorite: bool,
}

#[derive(Debug, Clone)]
pub struct QueryHistory {
    dir: PathBuf,
    max_entries: usize,
    enabled: bool,
}

#[derive(Debug, Serialize, Deserialize)]
struct FavoritesFile {
    favorites: Vec<String>,
}

impl QueryHistory {
    pub fn new(dir: impl Into<PathBuf>) -> Self {
        Self {
            dir: dir.into(),
            max_entries: MAX_HISTORY_ENTRIES,
            enabled: true,
        }
    }

    pub fn with_max_entries(mut self, max_entries: usize) -> Self {
        self.max_entries = max_entries.max(1);
        self
    }

    pub fn set_enabled(&mut self, enabled: bool) {
        self.enabled = enabled;
    }
    pub fn with_enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    fn history_file(&self) -> PathBuf {
        self.dir.join("history.jsonl")
    }

    fn favorites_file(&self) -> PathBuf {
        self.dir.join("favorites.json")
    }

    pub fn init(&self) -> Result<(), String> {
        if !self.enabled {
            return Ok(());
        }
        std::fs::create_dir_all(&self.dir).map_err(|error| error.to_string())?;
        set_mode(&self.dir, 0o700);
        for path in [self.history_file(), self.favorites_file()] {
            if !path.exists() {
                std::fs::write(&path, "").map_err(|error| error.to_string())?;
            }
            set_mode(&path, 0o600);
        }
        Ok(())
    }

    /// Append an entry. Returns `None` when history is disabled.
    #[allow(clippy::too_many_arguments)]
    pub fn add_entry(
        &mut self,
        connection_id: &str,
        connection_name: &str,
        sql: &str,
        duration_ms: u128,
        row_count: usize,
        status: QueryStatus,
        error: Option<&str>,
    ) -> Result<Option<QueryHistoryEntry>, String> {
        if !self.enabled {
            return Ok(None);
        }
        let entry = QueryHistoryEntry {
            id: generate_id(),
            connection_id: connection_id.to_owned(),
            connection_name: connection_name.to_owned(),
            sql: redact_database_sensitive_text(sql, &[]),
            executed_at: chrono::Utc::now().to_rfc3339(),
            duration_ms,
            row_count,
            status,
            error: error.map(|error| redact_database_sensitive_text(error, &[])),
            is_favorite: false,
        };
        self.init()?;
        let line = serde_json::to_string(&strip_favorite(&entry))
            .map_err(|error| error.to_string())?
            + "\n";
        use std::io::Write as _;
        let mut file = std::fs::OpenOptions::new()
            .append(true)
            .open(self.history_file())
            .map_err(|error| error.to_string())?;
        file.write_all(line.as_bytes())
            .map_err(|error| error.to_string())?;
        let count = self.entries()?.len();
        if count > self.max_entries {
            self.compact()?;
        }
        Ok(Some(entry))
    }

    pub fn entries(&self) -> Result<Vec<QueryHistoryEntry>, String> {
        let text = match std::fs::read_to_string(self.history_file()) {
            Ok(text) => text,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(error) => return Err(error.to_string()),
        };
        let favorites = self.favorite_ids()?;
        let mut entries = text
            .lines()
            .filter_map(|line| serde_json::from_str::<QueryHistoryEntry>(line).ok())
            .map(|mut entry| {
                entry.is_favorite = favorites.contains(&entry.id);
                entry
            })
            .collect::<Vec<_>>();
        entries.sort_by(|left, right| right.executed_at.cmp(&left.executed_at));
        Ok(entries)
    }

    pub fn get_history(
        &self,
        connection_id: Option<&str>,
        limit: usize,
        offset: usize,
    ) -> Result<Vec<QueryHistoryEntry>, String> {
        let entries = self.entries()?;
        Ok(entries
            .into_iter()
            .filter(|entry| connection_id.is_none_or(|id| entry.connection_id == id))
            .skip(offset)
            .take(limit)
            .collect())
    }

    pub fn search(
        &self,
        query: &str,
        connection_id: Option<&str>,
    ) -> Result<Vec<QueryHistoryEntry>, String> {
        let needle = query.to_lowercase();
        let mut entries = self
            .entries()?
            .into_iter()
            .filter(|entry| entry.sql.to_lowercase().contains(&needle))
            .filter(|entry| connection_id.is_none_or(|id| entry.connection_id == id))
            .collect::<Vec<_>>();
        entries.sort_by(|left, right| {
            let left_exact = left.sql.to_lowercase().starts_with(&needle);
            let right_exact = right.sql.to_lowercase().starts_with(&needle);
            right_exact
                .cmp(&left_exact)
                .then_with(|| right.executed_at.cmp(&left.executed_at))
        });
        entries.truncate(100);
        Ok(entries)
    }

    pub fn set_favorite(&mut self, history_id: &str, favorite: bool) -> Result<(), String> {
        if !self.enabled {
            return Ok(());
        }
        self.init()?;
        let mut favorites = self.favorite_ids()?;
        if favorite {
            favorites.insert(history_id.to_owned());
        } else {
            favorites.remove(history_id);
        }
        self.write_favorites(&favorites)
    }

    pub fn favorites(&self) -> Result<Vec<QueryHistoryEntry>, String> {
        Ok(self
            .entries()?
            .into_iter()
            .filter(|entry| entry.is_favorite)
            .collect())
    }

    pub fn clear(&mut self, connection_id: Option<&str>) -> Result<(), String> {
        if !self.enabled {
            return Ok(());
        }
        self.init()?;
        match connection_id {
            Some(connection_id) => {
                let kept = self
                    .entries()?
                    .into_iter()
                    .filter(|entry| entry.connection_id != connection_id)
                    .collect::<Vec<_>>();
                let kept_ids = kept
                    .iter()
                    .map(|entry| entry.id.clone())
                    .collect::<BTreeSet<_>>();
                self.write_entries(&kept)?;
                let favorites = self
                    .favorite_ids()?
                    .into_iter()
                    .filter(|id| kept_ids.contains(id))
                    .collect::<BTreeSet<_>>();
                self.write_favorites(&favorites)?;
            }
            None => {
                crate::persistence::write(&self.history_file(), b"")
                    .map_err(|error| error.to_string())?;
                self.write_favorites(&BTreeSet::new())?;
            }
        }
        Ok(())
    }

    fn compact(&mut self) -> Result<(), String> {
        let mut entries = self.entries()?;
        entries.truncate(self.max_entries);
        self.write_entries(&entries)
    }

    fn write_entries(&self, entries: &[QueryHistoryEntry]) -> Result<(), String> {
        let mut text = String::new();
        for entry in entries {
            text.push_str(
                &serde_json::to_string(&strip_favorite(entry))
                    .map_err(|error| error.to_string())?,
            );
            text.push('\n');
        }
        crate::persistence::write(&self.history_file(), text.as_bytes())
            .map_err(|error| error.to_string())?;

        Ok(())
    }

    fn favorite_ids(&self) -> Result<BTreeSet<String>, String> {
        let text = match std::fs::read_to_string(self.favorites_file()) {
            Ok(text) => text,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Ok(BTreeSet::new());
            }
            Err(error) => return Err(error.to_string()),
        };
        let file: FavoritesFile = serde_json::from_str(&text).unwrap_or(FavoritesFile {
            favorites: Vec::new(),
        });
        Ok(file.favorites.into_iter().collect())
    }

    fn write_favorites(&self, favorites: &BTreeSet<String>) -> Result<(), String> {
        let text = serde_json::to_string(&FavoritesFile {
            favorites: favorites.iter().cloned().collect(),
        })
        .map_err(|error| error.to_string())?;
        crate::persistence::write(&self.favorites_file(), text.as_bytes())
            .map_err(|error| error.to_string())?;
        Ok(())
    }
}

fn strip_favorite(entry: &QueryHistoryEntry) -> serde_json::Value {
    let mut value = serde_json::to_value(entry).unwrap_or(serde_json::Value::Null);
    if let Some(object) = value.as_object_mut() {
        object.remove("isFavorite");
    }
    value
}

fn generate_id() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_nanos());
    format!("query-{millis}-{}", std::process::id())
}

fn set_mode(path: &Path, mode: u32) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode));
    }
    #[cfg(not(unix))]
    {
        let _ = (path, mode);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn history() -> (tempfile::TempDir, QueryHistory) {
        let dir = tempfile::tempdir().unwrap();
        let history = QueryHistory::new(dir.path().join("database"));
        history.init().unwrap();
        (dir, history)
    }

    #[test]
    fn appends_searches_and_persists_entries() {
        let (dir, mut history) = history();
        history
            .add_entry(
                "conn",
                "Local",
                "SELECT * FROM users",
                12,
                3,
                QueryStatus::Success,
                None,
            )
            .unwrap();
        history
            .add_entry(
                "conn",
                "Local",
                "DELETE FROM users WHERE id = 1",
                7,
                1,
                QueryStatus::Success,
                None,
            )
            .unwrap();
        let entries = history.entries().unwrap();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].sql, "DELETE FROM users WHERE id = 1");
        assert_eq!(history.search("users", None).unwrap().len(), 2);
        assert_eq!(history.get_history(None, 1, 0).unwrap().len(), 1);

        let reopened = QueryHistory::new(dir.path().join("database"));
        assert_eq!(reopened.entries().unwrap().len(), 2);
    }

    #[test]
    fn favorites_persist_and_clear() {
        let (_dir, mut history) = history();
        let entry = history
            .add_entry("c", "C", "SELECT 1", 1, 1, QueryStatus::Success, None)
            .unwrap()
            .unwrap();
        history.set_favorite(&entry.id, true).unwrap();
        assert_eq!(history.favorites().unwrap().len(), 1);
        history.set_favorite(&entry.id, false).unwrap();
        assert!(history.favorites().unwrap().is_empty());
        history.set_favorite(&entry.id, true).unwrap();
        history.clear(None).unwrap();
        assert!(history.entries().unwrap().is_empty());
        assert!(history.favorites().unwrap().is_empty());
    }

    #[test]
    fn compacts_to_the_configured_bound() {
        let (_dir, history) = history();
        let mut history = QueryHistory::new(history.dir().to_path_buf()).with_max_entries(5);
        history.init().unwrap();
        for index in 0..12 {
            history
                .add_entry(
                    "c",
                    "C",
                    &format!("SELECT {index}"),
                    1,
                    1,
                    QueryStatus::Success,
                    None,
                )
                .unwrap();
        }
        assert!(history.entries().unwrap().len() <= 5);
    }

    #[test]
    fn redacts_credentials_and_respects_disable_mode() {
        let (_dir, mut history) = history();
        history
            .add_entry(
                "c",
                "C",
                "postgres://user:secret@host/db",
                1,
                0,
                QueryStatus::Error,
                Some("password=hunter2"),
            )
            .unwrap();
        let entry = &history.entries().unwrap()[0];
        assert!(!entry.sql.contains("secret"));
        assert!(!entry.error.as_deref().unwrap().contains("hunter2"));
        let mut disabled = QueryHistory::new(history.dir()).with_enabled(false);
        assert!(
            disabled
                .add_entry("c", "C", "SELECT 1", 1, 1, QueryStatus::Success, None)
                .unwrap()
                .is_none()
        );
    }
}
