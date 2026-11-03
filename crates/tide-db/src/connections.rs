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

    /// The TLS mode, with libpq's meanings: `disable` never encrypts, `prefer` encrypts
    /// when the server offers it, `require` always encrypts but does not check the
    /// server's certificate, `verify-ca` also checks the certificate was issued by a
    /// trusted authority, and `verify-full` also checks it names the host.
    ///
    /// Unset means `disable` for loopback and Unix socket hosts and `verify-full`
    /// everywhere else. Records saved before Tide had named modes (`true`, or an object
    /// without `mode`) were always fully verified, so they still are unless they set
    /// `rejectUnauthorized: false`.
    pub fn ssl_mode(&self) -> SslPreference {
        match &self.ssl {
            None | Some(serde_json::Value::Null) if is_loopback_host(&self.host) => {
                SslPreference::Disable
            }
            None | Some(serde_json::Value::Null) => SslPreference::VerifyFull,
            Some(serde_json::Value::Bool(false)) => SslPreference::Disable,
            Some(serde_json::Value::String(mode)) => {
                SslPreference::parse(mode).unwrap_or(SslPreference::VerifyFull)
            }
            Some(serde_json::Value::Object(object)) => {
                match object.get("mode").and_then(|v| v.as_str()) {
                    Some(mode) => SslPreference::parse(mode).unwrap_or(SslPreference::VerifyFull),
                    None if object.get("rejectUnauthorized") == Some(&false.into()) => {
                        SslPreference::Require
                    }
                    None => SslPreference::VerifyFull,
                }
            }
            _ => SslPreference::VerifyFull,
        }
    }

    /// A certificate authority file or PEM text to trust, when the record names one.
    pub fn ssl_ca(&self) -> Option<&str> {
        self.ssl
            .as_ref()
            .and_then(|ssl| ssl.get("ca"))
            .and_then(serde_json::Value::as_str)
            .filter(|ca| !ca.trim().is_empty())
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

/// PostgreSQL TLS modes, named and behaving as libpq's `sslmode`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SslPreference {
    Disable,
    Prefer,
    Require,
    VerifyCa,
    VerifyFull,
}

impl SslPreference {
    pub const ALL: [Self; 5] = [
        Self::Disable,
        Self::Prefer,
        Self::Require,
        Self::VerifyCa,
        Self::VerifyFull,
    ];

    /// The mode's libpq name, such as `verify-full`.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Disable => "disable",
            Self::Prefer => "prefer",
            Self::Require => "require",
            Self::VerifyCa => "verify-ca",
            Self::VerifyFull => "verify-full",
        }
    }

    /// A libpq `sslmode` name. `allow` is read as `prefer`, the nearest mode Tide has.
    pub fn parse(mode: &str) -> Option<Self> {
        match mode.trim().to_ascii_lowercase().as_str() {
            "disable" => Some(Self::Disable),
            "allow" | "prefer" => Some(Self::Prefer),
            "require" => Some(Self::Require),
            "verify-ca" => Some(Self::VerifyCa),
            "verify-full" => Some(Self::VerifyFull),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoredConnections {
    #[serde(default)]
    pub connections: Vec<ConnectionConfig>,
    /// Passwords for project connections. Read only from the user's own file.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub credentials: Vec<SavedCredential>,
}

/// The password you saved for a destination that a project file names. It lives in your
/// own connections file, with the password itself in the secret service, so a project
/// file can be shared without one, and it is only ever sent to that exact destination.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SavedCredential {
    pub host: String,
    pub port: u16,
    pub database: String,
    pub username: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ssl: Option<serde_json::Value>,
    pub password_secret: String,
}

impl SavedCredential {
    fn destination(connection: &ConnectionConfig) -> ConnectionConfig {
        ConnectionConfig {
            host: connection.host.clone(),
            port: connection.port,
            database: connection.database.clone(),
            username: connection.username.clone(),
            ssl: connection.ssl.clone(),
            ..ConnectionConfig::default()
        }
    }

    fn is_for(&self, connection: &ConnectionConfig) -> bool {
        SavedCredential::destination(connection).same_destination(&ConnectionConfig {
            host: self.host.clone(),
            port: self.port,
            database: self.database.clone(),
            username: self.username.clone(),
            ssl: self.ssl.clone(),
            ..ConnectionConfig::default()
        })
    }
}

#[derive(Debug, Clone, Default)]
pub struct ConnectionStore {
    global_file: PathBuf,
    project_file: Option<PathBuf>,
    connections: Vec<ConnectionConfig>,
    credentials: Vec<SavedCredential>,
}

impl ConnectionStore {
    pub fn new(global_file: impl Into<PathBuf>, project_file: Option<PathBuf>) -> Self {
        Self {
            global_file: global_file.into(),
            project_file,
            connections: Vec::new(),
            credentials: Vec::new(),
        }
    }

    pub fn load(&mut self) -> Result<(), ConnectionError> {
        self.connections.clear();
        self.credentials = load_file(
            &self.global_file,
            ConnectionScope::Global,
            &mut self.connections,
        )?;
        if let Some(project_file) = &self.project_file {
            // A project file cannot bring its own credentials.
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
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|error| ConnectionError::Io(error.to_string()))?;
        }
        let credentials = if scope == ConnectionScope::Global {
            self.credentials.clone()
        } else {
            Vec::new()
        };
        if connections.is_empty() && credentials.is_empty() && !write_when_empty && !path.exists() {
            return Ok(());
        }
        let text = serde_json::to_string_pretty(&StoredConnections {
            connections,
            credentials,
        })
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

    /// Project files describe destinations, never authority to select user secrets: a
    /// project connection uses a password you saved for its exact destination, either on
    /// one of your own connections or as a saved project credential.
    pub fn credential_reference<'a>(&'a self, connection: &ConnectionConfig) -> Option<&'a str> {
        let own = self
            .connections
            .iter()
            .find(|trusted| {
                trusted.scope == ConnectionScope::Global
                    && trusted.same_destination(connection)
                    && (connection.scope == ConnectionScope::Project || trusted.id == connection.id)
            })
            .and_then(|trusted| trusted.password_secret.as_deref());
        own.or_else(|| {
            (connection.scope == ConnectionScope::Project)
                .then(|| self.project_credential(connection))
                .flatten()
        })
    }

    /// The secret key of the password saved for a project connection's destination.
    pub fn project_credential(&self, connection: &ConnectionConfig) -> Option<&str> {
        self.credentials
            .iter()
            .find(|credential| credential.is_for(connection))
            .map(|credential| credential.password_secret.as_str())
    }

    /// Remembers `secret` as the password for `connection`'s destination, in the user's own
    /// file. Returns the secret key it replaces, which the caller may delete.
    pub fn save_project_credential(
        &mut self,
        connection: &ConnectionConfig,
        secret: &str,
    ) -> Result<Option<String>, ConnectionError> {
        let before = self.credentials.clone();
        let replaced = self
            .credentials
            .iter()
            .position(|credential| credential.is_for(connection))
            .map(|index| self.credentials.remove(index).password_secret);
        let destination = SavedCredential::destination(connection);
        self.credentials.push(SavedCredential {
            host: destination.host,
            port: destination.port,
            database: destination.database,
            username: destination.username,
            ssl: destination.ssl,
            password_secret: secret.to_owned(),
        });
        if let Err(error) = self.save() {
            self.credentials = before;
            return Err(error);
        }
        Ok(replaced.filter(|old| old != secret))
    }

    pub fn connections(&self) -> &[ConnectionConfig] {
        &self.connections
    }
}

/// Adds the connections in `path` and returns the credentials it holds.
fn load_file(
    path: &Path,
    scope: ConnectionScope,
    connections: &mut Vec<ConnectionConfig>,
) -> Result<Vec<SavedCredential>, ConnectionError> {
    let text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
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
    Ok(stored.credentials)
}

/// The password file libpq reads: `PGPASSFILE`, or `~/.pgpass`.
pub fn pgpass_path() -> Option<PathBuf> {
    std::env::var_os("PGPASSFILE")
        .filter(|path| !path.is_empty())
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".pgpass")))
}

/// The password for `connection` in a libpq password file at `path`.
///
/// Lines are `hostname:port:database:username:password`, `*` matches anything, `\` escapes
/// `:` and `\`, and the first matching line wins. As with libpq, the file is ignored unless
/// only its owner can read it. A connection from a project file only uses a line that names
/// its host, so a project cannot send a `*` password to a server of its choosing.
pub fn pgpass_password(path: &Path, connection: &ConnectionConfig) -> Option<String> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        let metadata = std::fs::metadata(path).ok()?;
        if !metadata.is_file() || metadata.permissions().mode() & 0o077 != 0 {
            if metadata.is_file() {
                tracing::warn!(
                    path = %path.display(),
                    "password file ignored: it must not be readable by others (chmod 600)"
                );
            }
            return None;
        }
    }
    let text = std::fs::read_to_string(path).ok()?;
    // libpq matches a Unix socket directory, or no host, as `localhost`.
    let host = if connection.host.is_empty() || connection.host.starts_with('/') {
        "localhost"
    } else {
        connection.host.as_str()
    };
    let port = connection.port.to_string();
    let wanted = [
        host,
        port.as_str(),
        &connection.database,
        &connection.username,
    ];
    text.lines()
        .filter(|line| !line.trim_start().starts_with('#'))
        .filter_map(pgpass_fields)
        .find(|fields| {
            fields
                .iter()
                .zip(wanted)
                .enumerate()
                .all(|(index, (field, wanted))| {
                    let any = field == "*"
                        && !(index == 0 && connection.scope == ConnectionScope::Project);
                    any || field == wanted
                })
        })
        .map(|mut fields| fields.remove(4))
        .filter(|password| !password.is_empty())
}

/// The five fields of a password file line, unescaped; None for a malformed line.
fn pgpass_fields(line: &str) -> Option<Vec<String>> {
    let mut fields = vec![String::new()];
    let mut chars = line.chars();
    while let Some(ch) = chars.next() {
        match ch {
            '\\' => fields.last_mut()?.push(chars.next().unwrap_or('\\')),
            ':' if fields.len() < 5 => fields.push(String::new()),
            ch => fields.last_mut()?.push(ch),
        }
    }
    (fields.len() == 5).then_some(fields)
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
    let mode = url
        .query_pairs()
        .find(|(key, _)| key == "sslmode")
        .map(|(_, value)| {
            SslPreference::parse(&value)
                .ok_or_else(|| ConnectionError::InvalidUrl(format!("unknown sslmode {value}")))
        })
        .transpose()?;
    let root = url
        .query_pairs()
        .find(|(key, _)| key == "sslrootcert")
        .map(|(_, value)| value.into_owned())
        .filter(|path| !path.is_empty());
    let ssl = match (mode, root) {
        (Some(SslPreference::Disable), _) => Some(serde_json::Value::Bool(false)),
        (mode, Some(ca)) => Some(serde_json::json!({
            "mode": mode.unwrap_or(SslPreference::VerifyFull).as_str(),
            "ca": ca,
        })),
        (Some(mode), None) => Some(serde_json::Value::String(mode.as_str().to_owned())),
        (None, None) => None,
    };
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
    fn project_credentials_stay_in_the_user_file_and_match_only_their_destination() {
        let dir = tempfile::tempdir().unwrap();
        let global = dir.path().join("global.json");
        let project_file = dir.path().join("project.json");
        let mut store = ConnectionStore::new(&global, Some(project_file.clone()));
        let mut project = config("app");
        project.id = "project".into();
        project.scope = ConnectionScope::Project;
        project.password_secret = None;
        store.add(project.clone()).unwrap();
        assert_eq!(store.credential_reference(&project), None);
        assert_eq!(
            store
                .save_project_credential(&project, "db.project.1.password")
                .unwrap(),
            None
        );
        assert_eq!(
            store
                .save_project_credential(&project, "db.project.2.password")
                .unwrap(),
            Some("db.project.1.password".into()),
            "the replaced secret is handed back"
        );
        assert!(
            !std::fs::read_to_string(&project_file)
                .unwrap()
                .contains("password")
        );
        assert!(
            std::fs::read_to_string(&global)
                .unwrap()
                .contains("db.project.2.password")
        );

        let mut reopened = ConnectionStore::new(&global, Some(project_file.clone()));
        reopened.load().unwrap();
        assert_eq!(
            reopened.credential_reference(&project),
            Some("db.project.2.password")
        );
        let mut elsewhere = project.clone();
        elsewhere.host = "other.example".into();
        assert_eq!(reopened.credential_reference(&elsewhere), None);
        let mut own = project.clone();
        own.scope = ConnectionScope::Global;
        assert_eq!(
            reopened.credential_reference(&own),
            None,
            "project credentials are only for project connections"
        );

        // Credentials written into a project file are ignored.
        std::fs::write(
            &project_file,
            r#"{"connections":[],"credentials":[{"host":"evil.example","port":5432,"database":"app","username":"tide","passwordSecret":"OPENAI_API_KEY"}]}"#,
        )
        .unwrap();
        let mut hostile = ConnectionStore::new(dir.path().join("none.json"), Some(project_file));
        hostile.load().unwrap();
        let mut evil = project;
        evil.host = "evil.example".into();
        assert_eq!(hostile.credential_reference(&evil), None);
    }

    #[cfg(unix)]
    #[test]
    fn password_files_match_like_libpq_but_projects_need_a_named_host() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("pgpass");
        std::fs::write(
            &path,
            "# comment\n\
             db.example:5432:app:tide:exact\n\
             db.example:*:*:other:any\\:port\n\
             localhost:5432:*:tide:socket\n\
             *:*:*:tide:wildcard\n",
        )
        .unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
        let mut connection = config("app");
        assert_eq!(
            pgpass_password(&path, &connection).as_deref(),
            Some("exact")
        );
        connection.username = "other".into();
        connection.port = 6543;
        assert_eq!(
            pgpass_password(&path, &connection).as_deref(),
            Some("any:port")
        );
        connection.username = "tide".into();
        connection.host = "/var/run/postgresql".into();
        connection.port = 5432;
        assert_eq!(
            pgpass_password(&path, &connection).as_deref(),
            Some("socket")
        );
        connection.host = "elsewhere.example".into();
        assert_eq!(
            pgpass_password(&path, &connection).as_deref(),
            Some("wildcard")
        );
        connection.scope = ConnectionScope::Project;
        assert_eq!(pgpass_password(&path, &connection), None);
        connection.host = "db.example".into();
        assert_eq!(
            pgpass_password(&path, &connection).as_deref(),
            Some("exact")
        );
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
        assert_eq!(
            pgpass_password(&path, &connection),
            None,
            "readable by others"
        );
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
    fn ssl_modes_have_libpq_names_and_older_records_stay_verified() {
        for mode in SslPreference::ALL {
            let (parsed, _) = parse_postgres_url(&format!(
                "postgres://u@db.example/app?sslmode={mode}",
                mode = mode.as_str()
            ))
            .unwrap();
            assert_eq!(parsed.ssl_mode(), mode);
            assert_eq!(SslPreference::parse(mode.as_str()), Some(mode));
        }
        assert!(parse_postgres_url("postgres://u@db.example/app?sslmode=sometimes").is_err());
        let (rooted, _) = parse_postgres_url(
            "postgres://u@db.example/app?sslmode=verify-ca&sslrootcert=/etc/ca.pem",
        )
        .unwrap();
        assert_eq!(rooted.ssl_mode(), SslPreference::VerifyCa);
        assert_eq!(rooted.ssl_ca(), Some("/etc/ca.pem"));
        let mut record = config("app");
        for (ssl, mode) in [
            (serde_json::json!(true), SslPreference::VerifyFull),
            (
                serde_json::json!({ "rejectUnauthorized": true }),
                SslPreference::VerifyFull,
            ),
            (
                serde_json::json!({ "ca": "/ca.pem" }),
                SslPreference::VerifyFull,
            ),
            (
                serde_json::json!({ "rejectUnauthorized": false }),
                SslPreference::Require,
            ),
            (
                serde_json::json!({ "mode": "require", "ca": "/ca.pem" }),
                SslPreference::Require,
            ),
            (serde_json::json!("require"), SslPreference::Require),
            (serde_json::json!("no-such-mode"), SslPreference::VerifyFull),
        ] {
            record.ssl = Some(ssl.clone());
            assert_eq!(record.ssl_mode(), mode, "{ssl}");
        }
    }

    #[test]
    fn unset_ssl_requires_tls_except_on_loopback() {
        let mut remote = config("app");
        assert_eq!(remote.ssl_mode(), SslPreference::VerifyFull);
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
            assert_eq!(remote.ssl_mode(), SslPreference::VerifyFull, "{host}");
        }
        remote.ssl = Some(serde_json::Value::Bool(false));
        assert_eq!(remote.ssl_mode(), SslPreference::Disable);
        remote.ssl = Some(serde_json::json!("prefer"));
        assert_eq!(remote.ssl_mode(), SslPreference::Prefer);
        let (parsed, _) = parse_postgres_url("postgres://u@db.example/app").unwrap();
        assert_eq!(parsed.ssl_mode(), SslPreference::VerifyFull);
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
