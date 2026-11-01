//! Global and project `connections.json` records. Passwords are stored only as
//! secret references.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

#[derive(Debug, thiserror::Error)]
pub enum ConnectionError {
    #[error("connection not found: {0}")]
    NotFound(String),
    #[error("invalid connection: {0}")]
    Invalid(String),
    #[error("invalid PostgreSQL URL: {0}")]
    InvalidUrl(String),
    #[error("io error: {0}")]
    Io(String),
    #[error("json error: {0}")]
    Json(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ConnectionScope {
    Global,
    Project,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConnectionConfig {
    #[serde(default)]
    pub id: String,
    pub name: String,
    #[serde(rename = "type", default = "postgres_type")]
    pub kind: String,
    pub host: String,
    #[serde(default = "default_port")]
    pub port: u16,
    pub database: String,
    pub username: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub password_secret: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ssl: Option<serde_json::Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub connection_timeout: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub query_timeout: Option<u64>,
    #[serde(default = "default_true")]
    pub read_only: bool,
    pub scope: ConnectionScope,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project_path: Option<String>,
}

fn postgres_type() -> String {
    "postgres".to_owned()
}

const fn default_port() -> u16 {
    5432
}

const fn default_true() -> bool {
    true
}

impl Default for ConnectionConfig {
    fn default() -> Self {
        Self {
            id: String::new(),
            name: String::new(),
            kind: "postgres".to_owned(),
            host: "localhost".to_owned(),
            port: 5432,
            database: String::new(),
            username: String::new(),
            password_secret: None,
            ssl: None,
            connection_timeout: None,
            query_timeout: None,
            read_only: true,
            scope: ConnectionScope::Global,
            project_path: None,
        }
    }
}

impl ConnectionConfig {
    pub fn validate(&self) -> Result<(), ConnectionError> {
        if self.kind != "postgres" && !self.kind.is_empty() {
            return Err(ConnectionError::Invalid(format!(
                "unsupported database type {}",
                self.kind
            )));
        }
        if self.name.trim().is_empty() {
            return Err(ConnectionError::Invalid("name is required".to_owned()));
        }
        if self.host.trim().is_empty() {
            return Err(ConnectionError::Invalid("host is required".to_owned()));
        }
        if self.database.trim().is_empty() {
            return Err(ConnectionError::Invalid("database is required".to_owned()));
        }
        if self.username.trim().is_empty() {
            return Err(ConnectionError::Invalid("username is required".to_owned()));
        }
        if self.port == 0 {
            return Err(ConnectionError::Invalid("port is invalid".to_owned()));
        }
        Ok(())
    }

    /// Redacted summary safe for status lines and logs.
    pub fn summary(&self) -> String {
        format!(
            "{}@{}:{}/{}",
            self.username, self.host, self.port, self.database
        )
    }

    /// Credentials may only cross to the exact destination approved in user config.
    pub fn same_destination(&self, other: &Self) -> bool {
        self.host == other.host
            && self.port == other.port
            && self.database == other.database
            && self.username == other.username
            && self.ssl == other.ssl
    }

    /// Unset TLS settings default to `disable` only for loopback and Unix
    /// socket hosts; everything else requires TLS.
    pub fn ssl_mode(&self) -> SslPreference {
        match &self.ssl {
            None | Some(serde_json::Value::Null) if is_loopback_host(&self.host) => {
                SslPreference::Disable
            }
            None | Some(serde_json::Value::Null) => SslPreference::Require,
            Some(serde_json::Value::Bool(false)) => SslPreference::Disable,
            Some(serde_json::Value::Bool(true)) => SslPreference::Require,
            Some(serde_json::Value::Object(object))
                if object.get("mode").and_then(|v| v.as_str()) == Some("prefer") =>
            {
                SslPreference::Prefer
            }
            Some(serde_json::Value::Object(_)) => SslPreference::Require,
            Some(serde_json::Value::String(mode)) => match mode.as_str() {
                "disable" => SslPreference::Disable,
                "prefer" => SslPreference::Prefer,
                _ => SslPreference::Require,
            },
            _ => SslPreference::Require,
        }
    }
}

fn is_loopback_host(host: &str) -> bool {
    let host = host.trim();
    host.starts_with('/')
        || host.eq_ignore_ascii_case("localhost")
        || host
            .trim_start_matches('[')
            .trim_end_matches(']')
            .parse::<std::net::IpAddr>()
            .is_ok_and(|address| address.is_loopback())
}

fn percent_decode(value: &str, part: &str) -> Result<String, ConnectionError> {
    let bytes = value.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        let escaped = bytes
            .get(index + 1..index + 3)
            .filter(|hex| bytes[index] == b'%' && hex.iter().all(u8::is_ascii_hexdigit))
            .and_then(|hex| u8::from_str_radix(std::str::from_utf8(hex).ok()?, 16).ok());
        if let Some(byte) = escaped {
            decoded.push(byte);
            index += 3;
        } else {
            decoded.push(bytes[index]);
            index += 1;
        }
    }
    String::from_utf8(decoded).map_err(|_| {
        ConnectionError::InvalidUrl(format!("{part} is not valid UTF-8 after percent-decoding"))
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SslPreference {
    Disable,
    Prefer,
    Require,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoredConnections {
    #[serde(default)]
    pub connections: Vec<ConnectionConfig>,
}

#[derive(Debug, Clone, Default)]
pub struct ConnectionStore {
    global_file: PathBuf,
    project_file: Option<PathBuf>,
    connections: Vec<ConnectionConfig>,
}

impl ConnectionStore {
    pub fn new(global_file: impl Into<PathBuf>, project_file: Option<PathBuf>) -> Self {
        Self {
            global_file: global_file.into(),
            project_file,
            connections: Vec::new(),
        }
    }

    pub fn load(&mut self) -> Result<(), ConnectionError> {
        self.connections.clear();
        load_file(
            &self.global_file,
            ConnectionScope::Global,
            &mut self.connections,
        )?;
        if let Some(project_file) = &self.project_file {
            load_file(
                project_file,
                ConnectionScope::Project,
                &mut self.connections,
            )?;
        }
        Ok(())
    }

    pub fn save(&self) -> Result<(), ConnectionError> {
        let paths = std::iter::once(self.global_file.clone())
            .chain(self.project_file.clone())
            .collect::<Vec<_>>();
        let mut previous = Vec::new();
        for path in paths {
            let bytes = match std::fs::symlink_metadata(&path) {
                Ok(metadata) => {
                    if !metadata.is_file() || metadata.len() > 16 * 1024 * 1024 {
                        return Err(ConnectionError::Io(
                            "connection records must be regular files within 16 MiB".into(),
                        ));
                    }
                    Some(std::fs::read(&path).map_err(|e| ConnectionError::Io(e.to_string()))?)
                }
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
                Err(e) => return Err(ConnectionError::Io(e.to_string())),
            };
            previous.push((path, bytes));
        }
        let mut written = Vec::new();
        let saved: Result<(), ConnectionError> = (|| {
            self.save_scope(ConnectionScope::Global, &self.global_file, false)?;
            written.push(self.global_file.clone());
            if let Some(path) = &self.project_file {
                self.save_scope(ConnectionScope::Project, path, true)?;
                written.push(path.clone());
            }
            Ok(())
        })();
        if let Err(error) = saved {
            let mut failures = 0;
            for (path, bytes) in previous
                .into_iter()
                .filter(|(path, _)| written.contains(path))
            {
                let result = if let Some(bytes) = bytes {
                    crate::persistence::write(&path, &bytes)
                } else {
                    match std::fs::remove_file(&path) {
                        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
                        other => other,
                    }
                };
                if result.is_err() {
                    failures += 1;
                }
            }
            return Err(ConnectionError::Io(format!(
                "{error}; {failures} record rollback failure(s)"
            )));
        }
        Ok(())
    }

    fn save_scope(
        &self,
        scope: ConnectionScope,
        path: &Path,
        write_when_empty: bool,
    ) -> Result<(), ConnectionError> {
        let connections = self
            .connections
            .iter()
            .filter(|connection| connection.scope == scope)
            .cloned()
            .collect::<Vec<_>>();
        if connections.is_empty() && !write_when_empty && !path.exists() {
            return Ok(());
        }
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|error| ConnectionError::Io(error.to_string()))?;
        }
        let text = serde_json::to_string_pretty(&StoredConnections { connections })
            .map_err(|error| ConnectionError::Json(error.to_string()))?;
        crate::persistence::write(path, text.as_bytes())
            .map_err(|error| ConnectionError::Io(error.to_string()))?;
        set_mode(path, 0o600);
        Ok(())
    }

    pub fn add(&mut self, mut config: ConnectionConfig) -> Result<String, ConnectionError> {
        config.kind = "postgres".to_owned();
        config.validate()?;
        if config.scope == ConnectionScope::Project && self.project_file.is_none() {
            return Err(ConnectionError::Invalid(
                "project storage is unavailable".into(),
            ));
        }
        if config.id.is_empty() {
            config.id = generate_id(&config);
        }
        let before = self.connections.clone();
        if self.connections.iter().any(|old| old.id == config.id) {
            return Err(ConnectionError::Invalid(
                "connection ID already exists".into(),
            ));
        }
        let id = config.id.clone();
        self.connections.push(config);
        if let Err(error) = self.save() {
            self.connections = before;
            return Err(error);
        }
        Ok(id)
    }

    pub fn update(&mut self, config: ConnectionConfig) -> Result<(), ConnectionError> {
        config.validate()?;
        if config.scope == ConnectionScope::Project && self.project_file.is_none() {
            return Err(ConnectionError::Invalid(
                "project storage is unavailable".into(),
            ));
        }
        let before = self.connections.clone();
        let Some(existing) = self
            .connections
            .iter_mut()
            .find(|existing| existing.id == config.id)
        else {
            return Err(ConnectionError::NotFound(config.id));
        };
        *existing = config;
        if let Err(error) = self.save() {
            self.connections = before;
            return Err(error);
        }
        Ok(())
    }

    pub fn remove(&mut self, id: &str) -> Result<bool, ConnectionError> {
        let saved = self.connections.clone();
        let before = self.connections.len();
        self.connections.retain(|connection| connection.id != id);
        if self.connections.len() == before {
            return Ok(false);
        }
        if let Err(error) = self.save() {
            self.connections = saved;
            return Err(error);
        }
        Ok(true)
    }

    pub fn get(&self, id: &str) -> Option<&ConnectionConfig> {
        self.connections
            .iter()
            .find(|connection| connection.id == id)
    }

    pub fn list(&self, scope: Option<ConnectionScope>) -> Vec<&ConnectionConfig> {
        self.connections
            .iter()
            .filter(|connection| scope.is_none_or(|scope| connection.scope == scope))
            .collect()
    }

    /// Project files describe destinations, never authority to select user secrets.
    pub fn credential_reference<'a>(&'a self, connection: &ConnectionConfig) -> Option<&'a str> {
        self.connections
            .iter()
            .find(|trusted| {
                trusted.scope == ConnectionScope::Global
                    && trusted.same_destination(connection)
                    && (connection.scope == ConnectionScope::Project || trusted.id == connection.id)
            })
            .and_then(|trusted| trusted.password_secret.as_deref())
    }

    pub fn connections(&self) -> &[ConnectionConfig] {
        &self.connections
    }
}

fn load_file(
    path: &Path,
    scope: ConnectionScope,
    connections: &mut Vec<ConnectionConfig>,
) -> Result<(), ConnectionError> {
    let text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(ConnectionError::Io(error.to_string())),
    };
    let stored: StoredConnections =
        serde_json::from_str(&text).map_err(|error| ConnectionError::Json(error.to_string()))?;
    for mut config in stored.connections {
        if !config.kind.is_empty() && config.kind != "postgres" {
            continue;
        }
        config.kind = "postgres".to_owned();
        config.scope = scope;
        if config.id.is_empty() {
            config.id = generate_id(&config);
        }
        connections.push(config);
    }
    Ok(())
}

/// Parse a `postgres://`/`postgresql://` URL. Any password is returned
/// separately so the caller can move it into the secret service.
pub fn parse_postgres_url(
    input: &str,
) -> Result<(ConnectionConfig, Option<String>), ConnectionError> {
    let input = input.trim();
    let normalized = if let Some(rest) = input.strip_prefix("postgresql://") {
        format!("postgres://{rest}")
    } else if input.starts_with("postgres://") {
        input.to_owned()
    } else {
        return Err(ConnectionError::InvalidUrl(
            "expected postgres:// or postgresql://".to_owned(),
        ));
    };
    let url = url::Url::parse(&normalized)
        .map_err(|error| ConnectionError::InvalidUrl(error.to_string()))?;
    let host = url
        .host_str()
        .ok_or_else(|| ConnectionError::InvalidUrl("missing host".to_owned()))?
        .to_owned();
    let database = percent_decode(url.path().trim_start_matches('/'), "database")?;
    if database.is_empty() {
        return Err(ConnectionError::InvalidUrl("missing database".to_owned()));
    }
    let username = if url.username().is_empty() {
        "postgres".to_owned()
    } else {
        percent_decode(url.username(), "username")?
    };
    let password = url
        .password()
        .map(|password| percent_decode(password, "password"))
        .transpose()?;
    let mut name = url
        .query_pairs()
        .find(|(key, _)| key == "name")
        .map(|(_, value)| value.into_owned())
        .unwrap_or_else(|| format!("{database}@{host}"));
    if name.trim().is_empty() {
        name = format!("{database}@{host}");
    }
    let ssl = url.query_pairs().find_map(|(key, value)| {
        (key == "sslmode").then(|| match value.as_ref() {
            "disable" => serde_json::Value::Bool(false),
            "prefer" => serde_json::Value::String("prefer".to_owned()),
            _ => serde_json::Value::Bool(true),
        })
    });
    let read_only = url
        .query_pairs()
        .find(|(key, _)| key == "readOnly" || key == "readonly")
        .is_none_or(|(_, value)| value != "0" && value != "false");
    let config = ConnectionConfig {
        name,
        host,
        port: url.port().unwrap_or(5432),
        database,
        username,
        ssl,
        read_only,
        ..ConnectionConfig::default()
    };
    config.validate()?;
    Ok((config, password))
}

pub fn generate_id(config: &ConnectionConfig) -> String {
    let slug = |value: &str| {
        value
            .chars()
            .map(|ch| {
                if ch.is_ascii_alphanumeric() {
                    ch.to_ascii_lowercase()
                } else {
                    '-'
                }
            })
            .collect::<String>()
            .trim_matches('-')
            .to_owned()
    };
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |duration| duration.as_millis());
    format!(
        "pg-{}-{}-{stamp}",
        slug(&config.host),
        slug(&config.database)
    )
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

    fn config(database: &str) -> ConnectionConfig {
        ConnectionConfig {
            name: database.to_owned(),
            host: "db.example".to_owned(),
            port: 5432,
            database: database.to_owned(),
            username: "tide".to_owned(),
            password_secret: Some(format!("database.{database}.password")),
            ..ConnectionConfig::default()
        }
    }

    #[cfg(unix)]
    #[test]
    fn failed_scope_write_rolls_back_memory_and_successful_record_write() {
        use std::os::unix::fs::PermissionsExt;
        let root = tempfile::tempdir().unwrap();
        let project_dir = root.path().join("project");
        std::fs::create_dir(&project_dir).unwrap();
        let global = root.path().join("global.json");
        let project = project_dir.join("connections.json");
        let mut store = ConnectionStore::new(&global, Some(project.clone()));
        let mut original = config("original");
        original.id = "original".into();
        store.add(original.clone()).unwrap();
        let bytes = std::fs::read(&global).unwrap();
        std::fs::set_permissions(&project_dir, std::fs::Permissions::from_mode(0o500)).unwrap();
        let mut updated = original.clone();
        updated.name = "changed".into();
        let result = store.update(updated);
        std::fs::set_permissions(&project_dir, std::fs::Permissions::from_mode(0o700)).unwrap();
        assert!(result.is_err());
        assert_eq!(store.get("original"), Some(&original));
        assert_eq!(std::fs::read(&global).unwrap(), bytes);
    }

    #[test]
    fn project_secret_references_cannot_select_user_secrets() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = ConnectionStore::new(
            dir.path().join("global.json"),
            Some(dir.path().join("project.json")),
        );
        let mut project = config("project");
        project.id = "project".into();
        project.scope = ConnectionScope::Project;
        project.password_secret = Some("OPENAI_API_KEY".into());
        store.add(project.clone()).unwrap();
        assert_eq!(store.credential_reference(&project), None);
        let mut approved = project.clone();
        approved.id = "approved".into();
        approved.scope = ConnectionScope::Global;
        approved.password_secret = Some("db.approved.password".into());
        store.add(approved).unwrap();
        assert_eq!(
            store.credential_reference(&project),
            Some("db.approved.password")
        );
        project.host = "other.example".into();
        assert_eq!(store.credential_reference(&project), None);
    }

    #[test]
    fn stores_global_and_project_connections() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = ConnectionStore::new(
            dir.path().join("global.json"),
            Some(dir.path().join("project.json")),
        );
        let mut global = config("global_db");
        global.id = "global".to_owned();
        let mut project = config("project_db");
        project.id = "project".to_owned();
        project.scope = ConnectionScope::Project;
        store.add(global).unwrap();
        store.add(project).unwrap();
        assert_eq!(store.list(None).len(), 2);
        assert_eq!(store.list(Some(ConnectionScope::Project)).len(), 1);

        let mut reopened = ConnectionStore::new(
            dir.path().join("global.json"),
            Some(dir.path().join("project.json")),
        );
        reopened.load().unwrap();
        assert_eq!(reopened.list(None).len(), 2);
        assert_eq!(
            reopened.get("project").unwrap().scope,
            ConnectionScope::Project
        );
        assert!(reopened.remove("global").unwrap());
        assert!(!reopened.remove("global").unwrap());
    }

    #[test]
    fn parses_postgres_urls_and_keeps_passwords_out_of_records() {
        let (config, password) = parse_postgres_url(
            "postgresql://alice:secret@db.example:6543/app?sslmode=require&name=Prod&readOnly=0",
        )
        .unwrap();
        assert_eq!(config.username, "alice");
        assert_eq!(config.port, 6543);
        assert_eq!(config.database, "app");
        assert_eq!(config.name, "Prod");
        assert!(!config.read_only);
        assert_eq!(config.summary(), "alice@db.example:6543/app");
        assert_eq!(password.as_deref(), Some("secret"));
        let serialized = serde_json::to_string(&config).unwrap();
        assert!(!serialized.contains("secret"));
        assert!(serialized.contains("readOnly"));
    }

    #[test]
    fn unset_ssl_requires_tls_except_on_loopback() {
        let mut remote = config("app");
        assert_eq!(remote.ssl_mode(), SslPreference::Require);
        for host in [
            "localhost",
            "LOCALHOST",
            "127.0.0.1",
            "127.1.2.3",
            "::1",
            "[::1]",
            "/var/run/postgresql",
        ] {
            remote.host = host.to_owned();
            assert_eq!(remote.ssl_mode(), SslPreference::Disable, "{host}");
        }
        for host in ["db.example", "10.0.0.5", "localhost.example", "::2"] {
            remote.host = host.to_owned();
            assert_eq!(remote.ssl_mode(), SslPreference::Require, "{host}");
        }
        remote.ssl = Some(serde_json::Value::Bool(false));
        assert_eq!(remote.ssl_mode(), SslPreference::Disable);
        remote.ssl = Some(serde_json::json!("prefer"));
        assert_eq!(remote.ssl_mode(), SslPreference::Prefer);
        let (parsed, _) = parse_postgres_url("postgres://u@db.example/app").unwrap();
        assert_eq!(parsed.ssl_mode(), SslPreference::Require);
        let (parsed, _) = parse_postgres_url("postgres://u@127.0.0.1/app").unwrap();
        assert_eq!(parsed.ssl_mode(), SslPreference::Disable);
        let (parsed, _) =
            parse_postgres_url("postgres://u@db.example/app?sslmode=disable").unwrap();
        assert_eq!(parsed.ssl_mode(), SslPreference::Disable);
    }

    #[test]
    fn url_credentials_and_database_are_percent_decoded() {
        let (config, password) =
            parse_postgres_url("postgres://al%40ice:p%40ss%3A%2F%25@db.example/my%20db").unwrap();
        assert_eq!(config.username, "al@ice");
        assert_eq!(password.as_deref(), Some("p@ss:/%"));
        assert_eq!(config.database, "my db");
        let (_, password) = parse_postgres_url("postgres://u:p%40ss@db.example/app").unwrap();
        assert_eq!(password.as_deref(), Some("p@ss"));
        let (_, password) = parse_postgres_url("postgres://u:caf%C3%A9@db.example/app").unwrap();
        assert_eq!(password.as_deref(), Some("café"));
        assert!(parse_postgres_url("postgres://u:%FF@db.example/app").is_err());
    }

    #[test]
    fn rejects_unsupported_or_incomplete_connections() {
        assert!(parse_postgres_url("mysql://db/app").is_err());
        let mut invalid = config("x");
        invalid.username.clear();
        assert!(invalid.validate().is_err());
        assert!(invalid.summary().contains("db.example"));
    }

    #[test]
    fn loads_and_saves_records_with_unknown_fields() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("connections.json");
        std::fs::write(
            &path,
            r#"{"connections":[{"id":"a","name":"A","type":"postgres","host":"h","port":5432,"database":"d","username":"u","passwordSecret":"database.a.password","readOnly":true,"scope":"global","futureField":1}]}"#,
        )
        .unwrap();
        let mut store = ConnectionStore::new(&path, None);
        store.load().unwrap();
        assert_eq!(store.get("a").unwrap().database, "d");
        store.save().unwrap();
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.contains("passwordSecret"));
    }
}
