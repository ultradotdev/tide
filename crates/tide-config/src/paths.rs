use std::path::{Path, PathBuf};

/// Tide's XDG roots and derived file paths.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Paths {
    pub config_dir: PathBuf,
    pub data_dir: PathBuf,
    pub state_dir: PathBuf,
    pub cache_dir: PathBuf,
    pub settings_file: PathBuf,
    pub keybindings_file: PathBuf,
    pub themes_dir: PathBuf,
    pub models_file: PathBuf,
    pub model_preferences_file: PathBuf,
    pub connections_file: PathBuf,
    pub secrets_file: PathBuf,
    pub conversations_dir: PathBuf,
    pub sessions_dir: PathBuf,
    pub workspace_sessions_dir: PathBuf,
    pub named_sessions_dir: PathBuf,
    pub last_session_file: PathBuf,
    pub logs_dir: PathBuf,
    pub debug_log_file: PathBuf,
    pub query_history_dir: PathBuf,
}

impl Paths {
    /// Discover paths from the current process environment.
    #[must_use]
    pub fn discover() -> Self {
        let home = std::env::var_os("HOME")
            .or_else(|| std::env::var_os("USERPROFILE"))
            .map_or_else(|| PathBuf::from("."), PathBuf::from);
        Self::from_env(&home, |key| std::env::var_os(key))
    }

    /// Build paths from an explicit home directory and environment lookup.
    ///
    /// This exists so tests do not mutate process-wide environment variables.
    #[must_use]
    pub fn from_env<F>(home: &Path, env: F) -> Self
    where
        F: Fn(&str) -> Option<std::ffi::OsString>,
    {
        let xdg = |key: &str, fallback: &str| {
            env(key)
                .map(PathBuf::from)
                .filter(|path| path.is_absolute())
                .unwrap_or_else(|| home.join(fallback))
        };

        let config_root = xdg("XDG_CONFIG_HOME", ".config");
        let data_root = xdg("XDG_DATA_HOME", ".local/share");
        let state_root = xdg("XDG_STATE_HOME", ".local/state");
        let cache_root = xdg("XDG_CACHE_HOME", ".cache");

        let config_dir = config_root.join("tide");
        let data_dir = data_root.join("tide");
        let state_dir = state_root.join("tide");
        let cache_dir = cache_root.join("tide");
        let sessions_dir = state_dir.join("sessions");

        Self {
            settings_file: config_dir.join("settings.jsonc"),
            keybindings_file: config_dir.join("keybindings.jsonc"),
            themes_dir: config_dir.join("themes"),
            models_file: config_dir.join("models.json"),
            model_preferences_file: config_dir.join("model-preferences.json"),
            connections_file: config_dir.join("connections.json"),
            secrets_file: data_dir.join("secrets.enc"),
            conversations_dir: data_dir.join("conversations"),
            workspace_sessions_dir: sessions_dir.join("workspaces"),
            named_sessions_dir: sessions_dir.join("named"),
            last_session_file: sessions_dir.join("last-session.json"),
            logs_dir: state_dir.join("logs"),
            debug_log_file: state_dir.join("logs/debug.log"),
            query_history_dir: data_dir.join("database/query-history"),
            sessions_dir,
            config_dir,
            data_dir,
            state_dir,
            cache_dir,
        }
    }

    /// Create the four XDG roots with owner-only permissions on Unix.
    pub fn ensure_roots(&self) -> std::io::Result<()> {
        for path in [
            &self.config_dir,
            &self.data_dir,
            &self.state_dir,
            &self.cache_dir,
        ] {
            std::fs::create_dir_all(path)?;
            set_owner_only(path)?;
        }
        Ok(())
    }

    /// Workspace-local configuration directory.
    #[must_use]
    pub fn workspace_config_dir(&self, workspace_root: &Path) -> PathBuf {
        workspace_root.join(".tide")
    }

    /// Workspace-local settings file.
    #[must_use]
    pub fn workspace_settings_file(&self, workspace_root: &Path) -> PathBuf {
        self.workspace_config_dir(workspace_root)
            .join("settings.jsonc")
    }

    /// Workspace-local database connections file.
    #[must_use]
    pub fn workspace_connections_file(&self, workspace_root: &Path) -> PathBuf {
        self.workspace_config_dir(workspace_root)
            .join("connections.json")
    }
}

#[cfg(unix)]
fn set_owner_only(path: &Path) -> std::io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    let mut permissions = std::fs::metadata(path)?.permissions();
    permissions.set_mode(0o700);
    std::fs::set_permissions(path, permissions)
}

#[cfg(not(unix))]
fn set_owner_only(_path: &Path) -> std::io::Result<()> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::ffi::OsString;

    use super::*;

    #[test]
    fn defaults_match_tide_xdg_layout() {
        let paths = Paths::from_env(Path::new("/home/alice"), |_| None);
        assert_eq!(paths.config_dir, PathBuf::from("/home/alice/.config/tide"));
        assert_eq!(
            paths.settings_file,
            PathBuf::from("/home/alice/.config/tide/settings.jsonc")
        );
        assert_eq!(
            paths.sessions_dir,
            PathBuf::from("/home/alice/.local/state/tide/sessions")
        );
    }

    #[test]
    fn xdg_roots_override_defaults() {
        let paths = Paths::from_env(Path::new("/home/alice"), |key| match key {
            "XDG_CONFIG_HOME" => Some(OsString::from("/tmp/config")),
            "XDG_DATA_HOME" => Some(OsString::from("/tmp/data")),
            _ => None,
        });
        assert_eq!(paths.config_dir, PathBuf::from("/tmp/config/tide"));
        assert_eq!(paths.data_dir, PathBuf::from("/tmp/data/tide"));
        assert_eq!(
            paths.state_dir,
            PathBuf::from("/home/alice/.local/state/tide")
        );
    }
}
