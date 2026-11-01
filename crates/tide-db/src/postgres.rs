//! PostgreSQL backend with read-only enforcement, timeouts, and cancellation.

#[path = "row.rs"]
pub mod row;
use std::collections::BTreeMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use futures_util::StreamExt as _;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio_postgres::config::SslMode;
use tokio_postgres::tls::{MakeTlsConnect, TlsConnect};
use tokio_postgres::{Client, Config as PgConfig, NoTls, SimpleQueryMessage, SimpleQueryRow};
use tokio_postgres_rustls::MakeRustlsConnect;

use crate::connections::{ConnectionConfig, SslPreference};
use crate::risk::{SqlRiskAssessment, classify_sql_risk};

#[derive(Debug, thiserror::Error)]
pub enum DbError {
    #[error("connection failed: {0}")]
    Connect(String),
    #[error("query failed: {0}")]
    Query(String),
    #[error("query timed out after {0}ms")]
    Timeout(u64),
    #[error("connection is read-only: {0}")]
    ReadOnly(String),
    #[error("confirmation is required: {0}")]
    ConfirmationRequired(String),
    #[error("TLS setup failed: {0}")]
    Tls(String),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FieldInfo {
    pub name: String,
    pub data_type: String,
    pub data_type_id: u32,
    #[serde(default)]
    pub nullable: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QueryResult {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub row_target: Option<row::RowTarget>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub row_source: Option<row::RowSource>,
    pub query_id: String,
    pub connection_id: String,
    pub sql: String,
    pub rows: Vec<BTreeMap<String, Value>>,
    pub fields: Vec<FieldInfo>,
    pub row_count: usize,
    pub started_at: String,
    pub completed_at: String,
    pub duration_ms: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub affected_rows: Option<u64>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub notices: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub risk: Option<SqlRiskAssessment>,
    #[serde(default)]
    pub truncated: bool,
}

pub const MAX_RESULT_ROWS: usize = 10_000;
pub const MAX_RESULT_BYTES: usize = 16 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConnectionTest {
    pub success: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    pub latency_ms: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub server_version: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub warning: Option<String>,
}

enum TlsMode {
    None,
    Rustls(Arc<rustls::ClientConfig>),
}

pub struct PostgresSession {
    client: Arc<Client>,
    connection_id: String,
    read_only: bool,
    query_timeout: Duration,
    tls: TlsMode,
    tls_warning: Option<String>,
    config: ConnectionConfig,
    query_lock: tokio::sync::Mutex<()>,
}

impl std::fmt::Debug for PostgresSession {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("PostgresSession")
            .field("connection_id", &self.connection_id)
            .field("read_only", &self.read_only)
            .field("query_timeout", &self.query_timeout)
            .finish_non_exhaustive()
    }
}

impl PostgresSession {
    /// The error for a query that ran past `query_timeout`. The timeout is
    /// clamped to at most 600 000 ms, so its milliseconds always fit `u64`.
    pub(crate) fn timeout_error(&self) -> DbError {
        DbError::Timeout(u64::try_from(self.query_timeout.as_millis()).unwrap_or(u64::MAX))
    }

    pub async fn connect(
        connection: &ConnectionConfig,
        password: Option<&str>,
    ) -> Result<Self, DbError> {
        let mut config = PgConfig::new();
        config
            .host(&connection.host)
            .port(connection.port)
            .dbname(&connection.database)
            .user(&connection.username)
            .application_name("tide");
        if let Some(password) = password.filter(|value| !value.is_empty()) {
            config.password(password);
        }
        config.connect_timeout(Duration::from_millis(
            connection
                .connection_timeout
                .unwrap_or(10_000)
                .clamp(1_000, 120_000),
        ));
        match connection.ssl_mode() {
            SslPreference::Disable => config.ssl_mode(SslMode::Disable),
            SslPreference::Prefer => config.ssl_mode(SslMode::Prefer),
            SslPreference::Require => config.ssl_mode(SslMode::Require),
        };

        let tls_used = Arc::new(AtomicBool::new(false));
        let (client, tls) = match connection.ssl_mode() {
            SslPreference::Disable => {
                let (client, connection_task) = config
                    .connect(NoTls)
                    .await
                    .map_err(|error| DbError::Connect(error.to_string()))?;
                tokio::spawn(async move {
                    if let Err(error) = connection_task.await {
                        tracing::debug!(%error, "PostgreSQL connection closed");
                    }
                });
                (client, TlsMode::None)
            }
            _ => {
                let tls_config = Arc::new(build_rustls(connection)?);
                let (client, connection_task) = config
                    .connect(TrackTls {
                        inner: MakeRustlsConnect::new(tls_config.as_ref().clone()),
                        used: tls_used.clone(),
                    })
                    .await
                    .map_err(|error| DbError::Connect(error.to_string()))?;
                tokio::spawn(async move {
                    if let Err(error) = connection_task.await {
                        tracing::debug!(%error, "PostgreSQL connection closed");
                    }
                });
                (client, TlsMode::Rustls(tls_config))
            }
        };
        let tls_warning = (connection.ssl_mode() == SslPreference::Prefer
            && !tls_used.load(Ordering::SeqCst))
        .then(|| {
            format!(
                "{} is connected without TLS: the server declined it and sslmode=prefer fell back to plaintext.",
                connection.summary()
            )
        });
        if let Some(warning) = &tls_warning {
            tracing::warn!("{warning}");
        }

        if connection.read_only {
            client
                .batch_execute("SET default_transaction_read_only = on")
                .await
                .map_err(|error| DbError::Connect(error.to_string()))?;
        }

        Ok(Self {
            client: Arc::new(client),
            connection_id: connection.id.clone(),
            read_only: connection.read_only,
            query_timeout: Duration::from_millis(
                connection
                    .query_timeout
                    .unwrap_or(30_000)
                    .clamp(1_000, 600_000),
            ),
            tls,
            tls_warning,
            config: connection.clone(),
            query_lock: tokio::sync::Mutex::new(()),
        })
    }

    pub fn matches_config(&self, connection: &ConnectionConfig) -> bool {
        &self.config == connection
    }

    /// Set when `sslmode=prefer` fell back to an unencrypted connection.
    pub fn tls_warning(&self) -> Option<&str> {
        self.tls_warning.as_deref()
    }

    pub fn is_read_only(&self) -> bool {
        self.read_only
    }

    pub fn connection_id(&self) -> &str {
        &self.connection_id
    }

    /// Execute SQL with classification, read-only enforcement, and timeout.
    pub async fn execute(
        &self,
        sql: &str,
        allow_destructive: bool,
    ) -> Result<QueryResult, DbError> {
        let _guard = self.query_lock.lock().await;
        let result = tokio::time::timeout(
            self.query_timeout,
            self.execute_inner(sql, allow_destructive),
        )
        .await;
        let result = match result {
            Ok(result) => result,
            Err(_) => {
                let _ = tokio::time::timeout(Duration::from_secs(2), self.cancel()).await;
                Err(self.timeout_error())
            }
        };
        // Read-only submissions never commit session configuration changes.
        if self.read_only {
            tokio::time::timeout(
                Duration::from_secs(2),
                self.client.batch_execute("ROLLBACK"),
            )
            .await
            .map_err(|_| DbError::Query("read-only rollback timed out; reconnect required".into()))?
            .map_err(|error| DbError::Query(error.to_string()))?;
        }
        result
    }

    async fn execute_inner(
        &self,
        sql: &str,
        allow_destructive: bool,
    ) -> Result<QueryResult, DbError> {
        let risk = classify_sql_risk(sql);
        if self.read_only
            && (risk.statements.len() != 1
                || risk
                    .operations
                    .iter()
                    .any(|op| *op != crate::risk::SqlOperation::Read)
                || risk.is_mutation
                || risk
                    .statements
                    .iter()
                    .any(|statement| statement.attempts_read_only_override))
        {
            return Err(DbError::ReadOnly(
                "this connection is read-only; disable read-only mode to run mutations".to_owned(),
            ));
        }
        if risk.requires_confirmation && !allow_destructive {
            return Err(DbError::ConfirmationRequired(
                risk.statements
                    .iter()
                    .filter_map(|statement| statement.reason.clone())
                    .collect::<Vec<_>>()
                    .join(" "),
            ));
        }
        let started = Instant::now();
        let started_at = chrono::Utc::now().to_rfc3339();
        if self.read_only {
            self.client
                .batch_execute("BEGIN READ ONLY")
                .await
                .map_err(|e| DbError::Query(e.to_string()))?;
            // PostgreSQL's parser enforces a single statement, independent of our UX classifier.
            self.client
                .prepare(sql)
                .await
                .map_err(|e| DbError::ReadOnly(e.to_string()))?;
        }
        // `simple_query` returns text values only, but preparing the statement
        // first gives us the real column types without changing execution.
        let statement = self.client.prepare(sql).await.ok();
        let row_target = if risk.statements.len() == 1 && !risk.is_mutation {
            if let Some(statement) = &statement {
                self.row_target(statement).await
            } else {
                None
            }
        } else {
            None
        };
        let row_source = if risk.statements.len() == 1 && !risk.is_mutation {
            if let Some(statement) = &statement {
                self.row_source(statement).await
            } else {
                None
            }
        } else {
            None
        };
        let prepared = statement.map(|statement| {
            statement
                .columns()
                .iter()
                .map(|column| FieldInfo {
                    name: column.name().to_owned(),
                    data_type: column.type_().name().to_owned(),
                    data_type_id: column.type_().oid(),
                    nullable: true,
                })
                .collect::<Vec<_>>()
        });
        let mut stream = match self.client.simple_query_raw(sql).await {
            Ok(stream) => Box::pin(stream),
            Err(error) => return Err(DbError::Query(error.to_string())),
        };
        let mut rows = Vec::new();
        let mut fields: Vec<FieldInfo> = Vec::new();
        let mut affected_rows = None;
        let mut notices = self.tls_warning.iter().cloned().collect::<Vec<_>>();
        let mut truncated = false;
        let mut bytes = 0usize;
        let result = async {
            while let Some(message) = stream.next().await {
                let message = message.map_err(|error| DbError::Query(error.to_string()))?;
                match message {
                    SimpleQueryMessage::Row(row) => {
                        if fields.is_empty() {
                            fields = prepared.clone().unwrap_or_else(|| row_fields(&row));
                        }
                        let row = row_to_json(&row, &fields);
                        bytes += row
                            .values()
                            .map(|value| value.to_string().len())
                            .sum::<usize>();
                        rows.push(row);
                        if rows.len() >= MAX_RESULT_ROWS || bytes >= MAX_RESULT_BYTES {
                            truncated = true;
                            break;
                        }
                    }
                    SimpleQueryMessage::CommandComplete(affected) => {
                        affected_rows = Some(affected_rows.unwrap_or(0) + affected);
                    }
                    SimpleQueryMessage::RowDescription(columns)
                        if fields.is_empty() && prepared.is_none() =>
                    {
                        fields = columns
                            .iter()
                            .map(|column| FieldInfo {
                                name: column.name().to_owned(),
                                data_type: "text".to_owned(),
                                data_type_id: 0,
                                nullable: true,
                            })
                            .collect();
                    }
                    _ => {}
                }
            }
            Ok::<(), DbError>(())
        }
        .await;
        result?;
        if truncated {
            notices.push(format!(
                "result truncated to {MAX_RESULT_ROWS} rows / {} MiB",
                MAX_RESULT_BYTES / (1024 * 1024)
            ));
        }
        drop(stream);
        if truncated {
            let _ = tokio::time::timeout(Duration::from_secs(2), self.cancel()).await;
        }
        let completed_at = chrono::Utc::now().to_rfc3339();
        let duration_ms = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX);
        Ok(QueryResult {
            row_target,
            row_source,
            query_id: generate_query_id(),
            connection_id: self.connection_id.clone(),
            sql: sql.to_owned(),
            row_count: rows.len(),
            rows,
            fields,
            started_at,
            completed_at,
            duration_ms,
            affected_rows,
            notices,
            risk: Some(risk),
            truncated,
        })
    }

    /// Simple rows as text, used by schema introspection.
    pub async fn rows_text(
        &self,
        sql: &str,
    ) -> Result<Vec<BTreeMap<String, Option<String>>>, DbError> {
        let result = self.execute(sql, false).await?;
        Ok(result
            .rows
            .into_iter()
            .map(|row| {
                row.into_iter()
                    .map(|(key, value)| {
                        let text = match value {
                            Value::Null => None,
                            Value::String(text) => Some(text),
                            other => Some(other.to_string()),
                        };
                        (key, text)
                    })
                    .collect()
            })
            .collect())
    }

    /// Cancel the currently running query, best effort.
    pub async fn cancel(&self) {
        let token = self.client.cancel_token();
        match &self.tls {
            TlsMode::None => {
                let _ = token.cancel_query(NoTls).await;
            }
            TlsMode::Rustls(config) => {
                let _ = token
                    .cancel_query(MakeRustlsConnect::new(config.as_ref().clone()))
                    .await;
            }
        }
    }

    /// Lightweight health check used before reusing a cached connection.
    pub async fn is_healthy(&self) -> bool {
        tokio::time::timeout(Duration::from_secs(2), self.client.simple_query("SELECT 1"))
            .await
            .is_ok_and(|result| result.is_ok())
    }

    pub async fn server_version(&self) -> Result<String, DbError> {
        let rows = self.rows_text("SHOW server_version").await?;
        Ok(rows
            .first()
            .and_then(|row| row.values().next().cloned().flatten())
            .unwrap_or_else(|| "unknown".to_owned()))
    }
}

/// Records whether the server accepted the TLS upgrade, so `prefer` can report
/// a plaintext fallback.
struct TrackTls<T> {
    inner: T,
    used: Arc<AtomicBool>,
}

impl<S, T: MakeTlsConnect<S>> MakeTlsConnect<S> for TrackTls<T> {
    type Stream = T::Stream;
    type TlsConnect = TrackTls<T::TlsConnect>;
    type Error = T::Error;

    fn make_tls_connect(&mut self, domain: &str) -> Result<Self::TlsConnect, Self::Error> {
        Ok(TrackTls {
            inner: self.inner.make_tls_connect(domain)?,
            used: self.used.clone(),
        })
    }
}

impl<S, T: TlsConnect<S>> TlsConnect<S> for TrackTls<T> {
    type Stream = T::Stream;
    type Error = T::Error;
    type Future = T::Future;

    fn connect(self, stream: S) -> Self::Future {
        self.used.store(true, Ordering::SeqCst);
        self.inner.connect(stream)
    }
}

/// One unparsable system certificate must not disable every TLS connection.
fn trusted_roots(certs: Vec<rustls_pki_types::CertificateDer<'static>>) -> rustls::RootCertStore {
    let mut roots = rustls::RootCertStore::empty();
    let (_, ignored) = roots.add_parsable_certificates(certs);
    if ignored > 0 {
        tracing::debug!(ignored, "skipped unparsable system root certificates");
    }
    roots
}

fn build_rustls(connection: &ConnectionConfig) -> Result<rustls::ClientConfig, DbError> {
    let _ = rustls::crypto::ring::default_provider().install_default();
    let mut roots = trusted_roots(rustls_native_certs::load_native_certs().certs);
    if let Some(serde_json::Value::Object(object)) = &connection.ssl
        && let Some(ca) = object.get("ca").and_then(serde_json::Value::as_str)
    {
        let pem = if ca.contains("-----BEGIN") {
            ca.as_bytes().to_vec()
        } else {
            std::fs::read(ca)
                .map_err(|error| DbError::Tls(format!("cannot read CA file {ca}: {error}")))?
        };
        use rustls_pki_types::pem::PemObject;
        let mut added = 0;
        for cert in rustls_pki_types::CertificateDer::pem_slice_iter(&pem) {
            let cert = cert.map_err(|error| DbError::Tls(error.to_string()))?;
            roots
                .add(cert)
                .map_err(|error| DbError::Tls(error.to_string()))?;
            added += 1;
        }
        if added == 0 {
            return Err(DbError::Tls(
                "the CA contains no PEM certificates".to_owned(),
            ));
        }
    }
    Ok(rustls::ClientConfig::builder()
        .with_root_certificates(roots)
        .with_no_client_auth())
}

/// Test a connection without retaining it.
pub async fn test_connection(
    connection: &ConnectionConfig,
    password: Option<&str>,
) -> ConnectionTest {
    let started = Instant::now();
    match PostgresSession::connect(connection, password).await {
        Ok(session) => {
            let server_version = session.server_version().await.ok();
            ConnectionTest {
                success: true,
                error: None,
                latency_ms: u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX),
                server_version,
                warning: session.tls_warning().map(str::to_owned),
            }
        }
        Err(error) => ConnectionTest {
            success: false,
            error: Some(error.to_string()),
            latency_ms: u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX),
            server_version: None,
            warning: None,
        },
    }
}

fn row_fields(row: &SimpleQueryRow) -> Vec<FieldInfo> {
    row.columns()
        .iter()
        .enumerate()
        .map(|(index, column)| FieldInfo {
            name: column.name().to_owned(),
            data_type: "text".to_owned(),
            // Postgres caps a result at 1664 columns, so the index always fits.
            data_type_id: u32::try_from(index).unwrap_or(u32::MAX),
            nullable: true,
        })
        .collect()
}

fn row_to_json(row: &SimpleQueryRow, fields: &[FieldInfo]) -> BTreeMap<String, Value> {
    let mut map = BTreeMap::new();
    for (index, column) in row.columns().iter().enumerate() {
        let data_type = fields
            .get(index)
            .map_or("text", |field| field.data_type.as_str());
        let value = row
            .get(index)
            .map_or(Value::Null, |text| convert_text(text, data_type));
        map.insert(column.name().to_owned(), value);
    }
    map
}

fn convert_text(text: &str, data_type: &str) -> Value {
    match data_type {
        "bool" => match text {
            "t" | "true" | "1" => Value::Bool(true),
            "f" | "false" | "0" => Value::Bool(false),
            _ => Value::String(text.to_owned()),
        },
        "int2" | "int4" | "int8" | "oid" => text.parse::<i64>().map_or_else(
            |_| Value::String(text.to_owned()),
            |value| Value::Number(value.into()),
        ),
        "float4" | "float8" => text
            .parse::<f64>()
            .ok()
            .and_then(serde_json::Number::from_f64)
            .map_or_else(|| Value::String(text.to_owned()), Value::Number),
        "json" | "jsonb" => {
            serde_json::from_str(text).unwrap_or_else(|_| Value::String(text.to_owned()))
        }
        _ => Value::String(text.to_owned()),
    }
}

fn generate_query_id() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_nanos());
    format!("query-{nanos}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read as _, Write as _};

    const TEST_CA: &str = "-----BEGIN CERTIFICATE-----
MIIBZDCCAQqgAwIBAgIUNkEXdqn/xfF7UrvkE1JETMsvKKQwCgYIKoZIzj0EAwIw
FzEVMBMGA1UEAwwMdGlkZS10ZXN0LWNhMCAXDTI2MTAzMTAzMDYyMloYDzIxMjYx
MDA3MDMwNjIyWjAXMRUwEwYDVQQDDAx0aWRlLXRlc3QtY2EwWTATBgcqhkjOPQIB
BggqhkjOPQMBBwNCAARkPuvIqO87PXg4qUEYZ0IeBGuGenc4ROHKqxbzplRQOCoA
mKP2XckhMmdBn8WpalDiVSorxPRDhhrVuJByBLJCozIwMDAdBgNVHQ4EFgQUmaVr
h/ygML2QsbT+vvnFdGNQg9cwDwYDVR0TAQH/BAUwAwEB/zAKBggqhkjOPQQDAgNI
ADBFAiEAsGi7EUC+1CmbZcErYJvFeYSXFVq+ypsSLBgmFKp+cv4CIBMlRHkOaxZS
IQ12asrsAAJ7PpGM7eSMYaBx+p5InRx+
-----END CERTIFICATE-----
";

    fn with_ca(ca: &str) -> ConnectionConfig {
        ConnectionConfig {
            ssl: Some(serde_json::json!({ "ca": ca, "rejectUnauthorized": true })),
            ..ConnectionConfig::default()
        }
    }

    fn test_ca_der() -> rustls_pki_types::CertificateDer<'static> {
        use rustls_pki_types::pem::PemObject;
        rustls_pki_types::CertificateDer::from_pem_slice(TEST_CA.as_bytes()).unwrap()
    }

    #[test]
    fn unreadable_ca_path_is_an_error_not_inline_pem() {
        let dir = tempfile::tempdir().unwrap();
        let missing = dir.path().join("missing-ca.pem");
        assert!(matches!(
            build_rustls(&with_ca(missing.to_str().unwrap())),
            Err(DbError::Tls(_))
        ));
        let empty = dir.path().join("empty.pem");
        std::fs::write(&empty, "not a certificate").unwrap();
        assert!(matches!(
            build_rustls(&with_ca(empty.to_str().unwrap())),
            Err(DbError::Tls(_))
        ));
        let file = dir.path().join("ca.pem");
        std::fs::write(&file, TEST_CA).unwrap();
        assert!(build_rustls(&with_ca(file.to_str().unwrap())).is_ok());
        assert!(build_rustls(&with_ca(TEST_CA)).is_ok());
    }

    #[test]
    fn one_unparsable_system_root_does_not_break_tls() {
        let roots = trusted_roots(vec![
            rustls_pki_types::CertificateDer::from(vec![0_u8; 8]),
            test_ca_der(),
        ]);
        assert_eq!(roots.len(), 1);
    }

    /// Answers the SSL request with `N`, then accepts startup and fails every query.
    fn plaintext_only_server() -> u16 {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(mut stream) = stream else { return };
                std::thread::spawn(move || {
                    let mut header = [0_u8; 8];
                    if stream.read_exact(&mut header).is_err() {
                        return;
                    }
                    if header[4..] == 80_877_103_u32.to_be_bytes() {
                        stream.write_all(b"N").unwrap();
                        stream.read_exact(&mut header).unwrap();
                    }
                    let length = u32::from_be_bytes(header[..4].try_into().unwrap()) as usize;
                    let mut startup = vec![0_u8; length - 8];
                    stream.read_exact(&mut startup).unwrap();
                    stream
                        .write_all(b"R\0\0\0\x08\0\0\0\0Z\0\0\0\x05I")
                        .unwrap();
                    let error = b"SERROR\0CXX000\0Mfake server\0\0";
                    loop {
                        let mut kind = [0_u8; 5];
                        if stream.read_exact(&mut kind).is_err() || kind[0] == b'X' {
                            return;
                        }
                        let length = u32::from_be_bytes(kind[1..].try_into().unwrap()) as usize;
                        let mut body = vec![0_u8; length - 4];
                        stream.read_exact(&mut body).unwrap();
                        if matches!(kind[0], b'Q' | b'S') {
                            let mut reply = vec![b'E'];
                            reply.extend_from_slice(
                                &(u32::try_from(error.len()).unwrap() + 4).to_be_bytes(),
                            );
                            reply.extend_from_slice(error);
                            reply.extend_from_slice(b"Z\0\0\0\x05I");
                            stream.write_all(&reply).unwrap();
                        }
                    }
                });
            }
        });
        port
    }

    #[tokio::test]
    async fn prefer_falling_back_to_plaintext_is_reported() {
        let port = plaintext_only_server();
        let mut config = ConnectionConfig {
            name: "fake".into(),
            host: "127.0.0.1".into(),
            port,
            database: "db".into(),
            username: "user".into(),
            read_only: false,
            ssl: Some(serde_json::json!("prefer")),
            ..ConnectionConfig::default()
        };
        let session = PostgresSession::connect(&config, None).await.unwrap();
        assert!(session.tls_warning().unwrap().contains("without TLS"));
        let test = test_connection(&config, None).await;
        assert!(test.success);
        assert!(test.warning.unwrap().contains("without TLS"));

        config.ssl = Some(serde_json::Value::Bool(false));
        let session = PostgresSession::connect(&config, None).await.unwrap();
        assert_eq!(session.tls_warning(), None);
        assert_eq!(test_connection(&config, None).await.warning, None);
    }

    #[test]
    fn converts_common_postgres_types() {
        assert_eq!(convert_text("42", "int4"), Value::Number(42.into()));
        assert_eq!(convert_text("true", "bool"), Value::Bool(true));
        assert_eq!(
            convert_text("{\"a\":1}", "jsonb"),
            serde_json::json!({"a": 1})
        );
        assert_eq!(
            convert_text("huge", "numeric"),
            Value::String("huge".to_owned())
        );
    }
}
