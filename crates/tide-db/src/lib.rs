//! PostgreSQL workspace: connection records, read-only safety, queries, schema
//! introspection, and bounded query history.

mod completion;
pub mod connections;
pub mod export;
pub mod history;
mod persistence;
pub mod postgres;
pub mod redact;
pub mod risk;
pub mod schema;

pub use completion::{CompletionSuggestion, complete_sql};
pub use connections::{
    ConnectionConfig, ConnectionError, ConnectionScope, ConnectionStore, SavedCredential,
    SslPreference, generate_id, parse_postgres_url, pgpass_password, pgpass_path,
};
pub use export::result_to_csv;
pub use history::{QueryHistory, QueryHistoryEntry, QueryStatus};
pub use postgres::row::{InsertColumn, InsertTarget, RowInsert, RowMutation, RowSource, RowTarget};
pub use postgres::{
    ConnectionTest, DbError, FieldInfo, PostgresSession, QueryResult, test_connection,
};
pub use redact::redact_database_sensitive_text;
pub use risk::{
    SqlOperation, SqlRiskAssessment, SqlRiskLevel, SqlStatementRisk, attempts_read_only_override,
    classify_sql_risk,
};
pub use schema::{
    CatalogItem, ColumnInfo, ForeignKeyInfo, FunctionInfo, IndexInfo, PolicyInfo, PrimaryKeyInfo,
    SchemaInfo, TableInfo, TableType, TriggerInfo,
};
