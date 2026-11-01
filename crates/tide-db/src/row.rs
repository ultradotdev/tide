//! Typed row mutations: server-derived ownership, primary keys, atomic stale guards.
use super::{DbError, PostgresSession};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    collections::{BTreeMap, BTreeSet},
    time::Duration,
};
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RowTarget {
    pub oid: u32,
    pub schema: String,
    pub table: String,
    pub primary_key: Vec<String>,
    pub columns: Vec<String>,
}
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RowMutation {
    pub connection_id: String,
    pub query_id: String,
    pub target: RowTarget,
    pub original: BTreeMap<String, Value>,
    pub changes: BTreeMap<String, Value>,
    pub delete: bool,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RowSource {
    pub oid: u32,
    pub schema: String,
    pub table: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InsertColumn {
    pub name: String,
    pub data_type: String,
    pub nullable: bool,
    pub has_default: bool,
    pub generated: bool,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InsertTarget {
    pub oid: u32,
    pub schema: String,
    pub table: String,
    pub columns: Vec<InsertColumn>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RowInsert {
    pub connection_id: String,
    pub query_id: String,
    pub target: InsertTarget,
    /// Omitted columns use server defaults; explicit JSON null means SQL NULL.
    pub values: BTreeMap<String, Value>,
}

fn insert_error(error: tokio_postgres::Error) -> DbError {
    DbError::Query(
        error
            .as_db_error()
            .map_or_else(|| error.to_string(), |db| db.message().to_owned()),
    )
}
fn quote(s: &str) -> Result<String, DbError> {
    if s.is_empty() || s.len() > 4096 || s.contains('\0') {
        return Err(DbError::Query("Invalid row identifier".into()));
    }
    Ok(format!("\"{}\"", s.replace('"', "\"\"")))
}
impl PostgresSession {
    pub async fn insert_target(&self, source: &RowSource) -> Result<InsertTarget, DbError> {
        let _guard = self.query_lock.lock().await;
        tokio::time::timeout(self.query_timeout, self.describe_insert_target(source.oid))
            .await
            .map_err(|_| self.timeout_error())?
            .and_then(|target| {
                if target.schema == source.schema && target.table == source.table {
                    Ok(target)
                } else {
                    Err(DbError::Query("Table changed; refresh results".into()))
                }
            })
    }
    async fn describe_insert_target(&self, oid: u32) -> Result<InsertTarget, DbError> {
        let identity = self.client.query_opt("SELECT n.nspname,c.relname FROM pg_catalog.pg_class c JOIN pg_catalog.pg_namespace n ON n.oid=c.relnamespace WHERE c.oid=$1 AND c.relkind IN ('r','p')", &[&oid]).await.map_err(insert_error)?.ok_or_else(|| DbError::Query("Insert source is no longer a table".into()))?;
        let rows = self.client.query("SELECT a.attname,pg_catalog.format_type(a.atttypid,a.atttypmod),NOT a.attnotnull,(a.atthasdef OR a.attidentity<>''),(a.attgenerated<>'' OR a.attidentity='a') FROM pg_catalog.pg_attribute a WHERE a.attrelid=$1 AND a.attnum>0 AND NOT a.attisdropped ORDER BY a.attnum LIMIT 1025", &[&oid]).await.map_err(insert_error)?;
        if rows.is_empty() || rows.len() > 1024 {
            return Err(DbError::Query("Table column limit exceeded".into()));
        }
        Ok(InsertTarget {
            oid,
            schema: identity.get(0),
            table: identity.get(1),
            columns: rows
                .into_iter()
                .map(|r| InsertColumn {
                    name: r.get(0),
                    data_type: r.get(1),
                    nullable: r.get(2),
                    has_default: r.get(3),
                    generated: r.get(4),
                })
                .collect(),
        })
    }
    pub async fn insert_row(
        &self,
        request: &RowInsert,
    ) -> Result<BTreeMap<String, Value>, DbError> {
        if self.read_only {
            return Err(DbError::ReadOnly("Adding rows is disabled".into()));
        }
        if request.connection_id != self.connection_id
            || request.target.columns.is_empty()
            || request.target.columns.len() > 1024
            || request.values.keys().any(|key| {
                !request
                    .target
                    .columns
                    .iter()
                    .any(|c| &c.name == key && !c.generated)
            })
        {
            return Err(DbError::Query(
                "Invalid insert destination or columns".into(),
            ));
        }
        let payload =
            serde_json::to_string(&request.values).map_err(|e| DbError::Query(e.to_string()))?;
        if payload.len() > 1024 * 1024 {
            return Err(DbError::Query("New row exceeds 1 MiB".into()));
        }
        for column in &request.target.columns {
            if !column.nullable && request.values.get(&column.name).is_some_and(Value::is_null) {
                return Err(DbError::Query(format!("{} cannot be NULL", column.name)));
            }
            if !column.generated
                && !column.nullable
                && !column.has_default
                && !request.values.contains_key(&column.name)
            {
                return Err(DbError::Query(format!("{} needs a value", column.name)));
            }
        }
        let _guard = self.query_lock.lock().await;
        let result = match tokio::time::timeout(
            self.query_timeout,
            self.insert_row_inner(request, &payload),
        )
        .await
        {
            Ok(result) => result,
            Err(_) => {
                let _ = tokio::time::timeout(Duration::from_secs(2), self.cancel()).await;
                Err(self.timeout_error())
            }
        };
        if result.is_err() {
            tokio::time::timeout(
                Duration::from_secs(2),
                self.client.batch_execute("ROLLBACK"),
            )
            .await
            .map_err(|_| DbError::Query("Rollback timed out; reconnect required".into()))?
            .map_err(insert_error)?;
        }
        result
    }
    async fn insert_row_inner(
        &self,
        request: &RowInsert,
        payload: &str,
    ) -> Result<BTreeMap<String, Value>, DbError> {
        self.client
            .batch_execute("BEGIN")
            .await
            .map_err(insert_error)?;
        let table = format!(
            "{}.{}",
            quote(&request.target.schema)?,
            quote(&request.target.table)?
        );
        self.client
            .batch_execute(&format!("LOCK TABLE {table} IN ROW EXCLUSIVE MODE"))
            .await
            .map_err(insert_error)?;
        if self.describe_insert_target(request.target.oid).await? != request.target {
            return Err(DbError::Query(
                "Table definition changed; reopen Add Row".into(),
            ));
        }
        let rows = if request.values.is_empty() {
            self.client.query(&format!("INSERT INTO {table} AS inserted DEFAULT VALUES RETURNING to_jsonb(inserted)::text"), &[]).await
        } else {
            let names = request.values.keys().map(|key| quote(key)).collect::<Result<Vec<_>, _>>()?.join(",");
            self.client.query(&format!("INSERT INTO {table} AS inserted ({names}) SELECT {names} FROM jsonb_populate_record(NULL::{table},$1::text::jsonb) RETURNING to_jsonb(inserted)::text"), &[&payload]).await
        }.map_err(insert_error)?;
        if rows.len() != 1 {
            return Err(DbError::Query(
                "Insert did not return exactly one row".into(),
            ));
        }
        let row = serde_json::from_str(&rows[0].get::<_, String>(0))
            .map_err(|e| DbError::Query(e.to_string()))?;
        self.client
            .batch_execute("COMMIT")
            .await
            .map_err(insert_error)?;
        Ok(row)
    }

    /// Server provenance for insertion does not require a selected primary key.
    pub(super) async fn row_source(
        &self,
        statement: &tokio_postgres::Statement,
    ) -> Option<RowSource> {
        let columns = statement.columns();
        let oid = columns.first()?.table_oid()?;
        if columns.len() > 1024
            || columns
                .iter()
                .any(|c| c.table_oid() != Some(oid) || c.column_id().is_none())
        {
            return None;
        }
        let identity=self.client.query_opt("SELECT n.nspname,c.relname FROM pg_catalog.pg_class c JOIN pg_catalog.pg_namespace n ON n.oid=c.relnamespace WHERE c.oid=$1 AND c.relkind IN ('r','p')",&[&oid]).await.ok()??;
        Some(RowSource {
            oid,
            schema: identity.get(0),
            table: identity.get(1),
        })
    }
    pub(super) async fn row_target(
        &self,
        statement: &tokio_postgres::Statement,
    ) -> Option<RowTarget> {
        let columns = statement.columns();
        let oid = columns.first()?.table_oid()?;
        let mut seen = BTreeSet::new();
        if columns.len() > 1024
            || columns.iter().any(|c| {
                c.table_oid() != Some(oid) || c.column_id().is_none() || !seen.insert(c.name())
            })
        {
            return None;
        }
        let target = self
            .describe_row_target(oid, columns.iter().map(|c| c.name().to_owned()).collect())
            .await
            .ok()?;
        let attrs=self.client.query("SELECT attnum,attname FROM pg_catalog.pg_attribute WHERE attrelid=$1 AND attnum>0 AND NOT attisdropped",&[&oid]).await.ok()?;
        if columns.iter().any(|c| {
            !attrs.iter().any(|a| {
                a.get::<_, i16>(0) == c.column_id().unwrap() && a.get::<_, String>(1) == c.name()
            })
        }) {
            return None;
        }
        Some(target)
    }
    async fn describe_row_target(
        &self,
        oid: u32,
        columns: Vec<String>,
    ) -> Result<RowTarget, DbError> {
        let identity=self.client.query_opt("SELECT n.nspname,c.relname FROM pg_catalog.pg_class c JOIN pg_catalog.pg_namespace n ON n.oid=c.relnamespace WHERE c.oid=$1 AND c.relkind IN ('r','p')",&[&oid]).await.map_err(|e|DbError::Query(e.to_string()))?.ok_or_else(||DbError::Query("Row source is no longer a table".into()))?;
        let rows=self.client.query("SELECT a.attname FROM pg_catalog.pg_index i CROSS JOIN LATERAL unnest(i.indkey) WITH ORDINALITY k(attnum,position) JOIN pg_catalog.pg_attribute a ON a.attrelid=i.indrelid AND a.attnum=k.attnum WHERE i.indrelid=$1 AND i.indisprimary ORDER BY k.position",&[&oid]).await.map_err(|e|DbError::Query(e.to_string()))?;
        let primary_key = rows
            .iter()
            .map(|r| r.get::<_, String>(0))
            .collect::<Vec<_>>();
        if primary_key.is_empty() || primary_key.iter().any(|k| !columns.contains(k)) {
            return Err(DbError::Query(
                "Row editing requires every primary-key column".into(),
            ));
        }
        Ok(RowTarget {
            oid,
            schema: identity.get(0),
            table: identity.get(1),
            primary_key,
            columns,
        })
    }
    pub async fn mutate_row(
        &self,
        request: &RowMutation,
    ) -> Result<Option<BTreeMap<String, Value>>, DbError> {
        if self.read_only {
            return Err(DbError::ReadOnly("Row editing is disabled".into()));
        }
        if request.connection_id != self.connection_id
            || request.target.columns.is_empty()
            || request.target.columns.len() > 1024
            || request
                .original
                .keys()
                .any(|k| !request.target.columns.contains(k))
            || request
                .target
                .columns
                .iter()
                .any(|k| !request.original.contains_key(k))
            || request
                .changes
                .keys()
                .any(|k| !request.target.columns.contains(k))
        {
            return Err(DbError::Query("Invalid row identity or columns".into()));
        }
        if !request.delete && request.changes.is_empty() {
            return Err(DbError::Query("No changed fields".into()));
        }
        let _guard = self.query_lock.lock().await;
        let outcome =
            tokio::time::timeout(self.query_timeout, self.mutate_row_inner(request)).await;
        let result = match outcome {
            Ok(result) => result,
            Err(_) => {
                let _ = tokio::time::timeout(Duration::from_secs(2), self.cancel()).await;
                Err(self.timeout_error())
            }
        };
        if result.is_err() {
            tokio::time::timeout(
                Duration::from_secs(2),
                self.client.batch_execute("ROLLBACK"),
            )
            .await
            .map_err(|_| DbError::Query("Rollback timed out; reconnect required".into()))?
            .map_err(|e| DbError::Query(e.to_string()))?;
        }
        result
    }
    async fn mutate_row_inner(
        &self,
        r: &RowMutation,
    ) -> Result<Option<BTreeMap<String, Value>>, DbError> {
        self.client
            .batch_execute("BEGIN")
            .await
            .map_err(|e| DbError::Query(e.to_string()))?;
        let locked_table = format!("{}.{}", quote(&r.target.schema)?, quote(&r.target.table)?);
        self.client
            .batch_execute(&format!("LOCK TABLE {locked_table} IN ROW EXCLUSIVE MODE"))
            .await
            .map_err(|e| DbError::Query(e.to_string()))?;
        let current = self
            .describe_row_target(r.target.oid, r.target.columns.clone())
            .await?;
        if current != r.target {
            return Err(DbError::Query("Row source changed; refresh results".into()));
        }
        let table = format!("{}.{}", quote(&r.target.schema)?, quote(&r.target.table)?);
        let key = r
            .target
            .primary_key
            .iter()
            .map(|k| quote(k).map(|q| format!("t.{q} IS NOT DISTINCT FROM expected.{q}")))
            .collect::<Result<Vec<_>, _>>()?
            .join(" AND ");
        let stale = r
            .target
            .columns
            .iter()
            .map(|k| {
                quote(k)
                    .map(|q| format!("to_jsonb(t.{q}) IS NOT DISTINCT FROM to_jsonb(expected.{q})"))
            })
            .collect::<Result<Vec<_>, _>>()?
            .join(" AND ");
        let original =
            serde_json::to_string(&r.original).map_err(|e| DbError::Query(e.to_string()))?;
        let changes =
            serde_json::to_string(&r.changes).map_err(|e| DbError::Query(e.to_string()))?;
        let sql = if r.delete {
            format!(
                "DELETE FROM {table} AS t USING jsonb_populate_record(NULL::{table},$1::text::jsonb) expected WHERE {key} AND {stale} RETURNING to_jsonb(t)::text"
            )
        } else {
            let assignments = r
                .changes
                .keys()
                .map(|k| quote(k).map(|q| format!("{q}=updated.{q}")))
                .collect::<Result<Vec<_>, _>>()?
                .join(",");
            format!(
                "UPDATE {table} AS t SET {assignments} FROM jsonb_populate_record(NULL::{table},$1::text::jsonb) expected,jsonb_populate_record(NULL::{table},$2::text::jsonb) updated WHERE {key} AND {stale} RETURNING to_jsonb(t)::text"
            )
        };
        let values = if r.delete {
            self.client.query(&sql, &[&original]).await
        } else {
            self.client.query(&sql, &[&original, &changes]).await
        }
        .map_err(|e| DbError::Query(e.to_string()))?;
        if values.len() != 1 {
            return Err(DbError::Query(
                "Row changed or disappeared; refresh before retrying".into(),
            ));
        }
        let row: BTreeMap<String, Value> = serde_json::from_str(&values[0].get::<_, String>(0))
            .map_err(|e| DbError::Query(e.to_string()))?;
        self.client
            .batch_execute("COMMIT")
            .await
            .map_err(|e| DbError::Query(e.to_string()))?;
        Ok((!r.delete).then_some(row))
    }
}

impl std::fmt::Debug for RowMutation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RowMutation")
            .field("connection_id", &self.connection_id)
            .field("target", &self.target)
            .field("delete", &self.delete)
            .finish_non_exhaustive()
    }
}
