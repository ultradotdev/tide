//! Schema browser queries and lightweight DDL reconstruction.

use serde::{Deserialize, Serialize};

use crate::postgres::{DbError, PostgresSession};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SchemaInfo {
    pub name: String,
    pub is_system: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub owner: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TableType {
    Table,
    View,
    MaterializedView,
    ForeignTable,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TableInfo {
    pub schema: String,
    pub name: String,
    #[serde(rename = "type")]
    pub kind: TableType,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub row_count: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub size_bytes: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub comment: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ForeignKeyRef {
    pub schema: String,
    pub table: String,
    pub column: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ColumnInfo {
    pub name: String,
    pub data_type: String,
    pub nullable: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default_value: Option<String>,
    pub is_primary_key: bool,
    pub is_foreign_key: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub references: Option<ForeignKeyRef>,
    pub position: i32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub comment: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PrimaryKeyInfo {
    pub name: String,
    pub columns: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ForeignKeyInfo {
    pub name: String,
    pub columns: Vec<String>,
    pub referenced_schema: String,
    pub referenced_table: String,
    pub referenced_columns: Vec<String>,
    pub on_update: String,
    pub on_delete: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IndexInfo {
    pub schema: String,
    pub table: String,
    pub name: String,
    pub columns: Vec<String>,
    pub is_unique: bool,
    pub is_primary: bool,
    pub method: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub size_bytes: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub definition: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FunctionInfo {
    pub schema: String,
    pub name: String,
    pub kind: String,
    pub return_type: String,
    pub arguments: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub language: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TriggerInfo {
    pub schema: String,
    pub table: String,
    pub name: String,
    pub definition: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PolicyInfo {
    pub schema: String,
    pub table: String,
    pub name: String,
    pub command: String,
    pub permissive: String,
    pub roles: Vec<String>,
}

fn value(row: &std::collections::BTreeMap<String, Option<String>>, key: &str) -> Option<String> {
    row.get(key).cloned().flatten()
}

fn value_i64(row: &std::collections::BTreeMap<String, Option<String>>, key: &str) -> Option<i64> {
    value(row, key).and_then(|value| value.parse().ok())
}

fn value_bool(row: &std::collections::BTreeMap<String, Option<String>>, key: &str) -> bool {
    matches!(value(row, key).as_deref(), Some("t" | "true" | "1"))
}

/// Quote a string literal for interpolation into a catalog query. Like
/// PostgreSQL's `quote_literal`, backslashes force an `E''` literal so the
/// result does not depend on `standard_conforming_strings`.
pub fn quote_literal(value: &str) -> String {
    if value.contains('\\') {
        format!("E'{}'", value.replace('\\', "\\\\").replace('\'', "''"))
    } else {
        format!("'{}'", value.replace('\'', "''"))
    }
}

pub fn quote_ident(value: &str) -> String {
    format!("\"{}\"", value.replace('"', "\"\""))
}

impl PostgresSession {
    pub async fn schemas(&self) -> Result<Vec<SchemaInfo>, DbError> {
        let rows = self
            .rows_text(
                "SELECT n.nspname AS name, pg_get_userbyid(n.nspowner) AS owner, \
                 (n.nspname LIKE 'pg\\_%' OR n.nspname = 'information_schema') AS is_system \
                 FROM pg_namespace n ORDER BY n.nspname",
            )
            .await?;
        Ok(rows
            .into_iter()
            .filter_map(|row| {
                Some(SchemaInfo {
                    name: value(&row, "name")?,
                    is_system: value_bool(&row, "is_system"),
                    owner: value(&row, "owner"),
                })
            })
            .collect())
    }

    /// All user tables across non-system schemas (bounded for the picker).
    pub async fn all_tables(&self) -> Result<Vec<TableInfo>, DbError> {
        let mut tables = Vec::new();
        for schema in self
            .schemas()
            .await?
            .into_iter()
            .filter(|schema| !schema.is_system)
        {
            tables.extend(self.tables(&schema.name).await?);
            if tables.len() >= 2000 {
                break;
            }
        }
        Ok(tables)
    }

    pub async fn tables(&self, schema: &str) -> Result<Vec<TableInfo>, DbError> {
        let sql = format!(
            "SELECT c.relname AS name, c.relkind::text AS kind, \
             (CASE WHEN c.relkind = 'r' THEN c.reltuples::bigint END) AS row_count, \
             pg_total_relation_size(c.oid) AS size_bytes, obj_description(c.oid, 'pg_class') AS comment \
             FROM pg_class c JOIN pg_namespace n ON n.oid = c.relnamespace \
             WHERE n.nspname = {} AND c.relkind IN ('r','v','m','f') ORDER BY c.relname",
            quote_literal(schema)
        );
        let rows = self.rows_text(&sql).await?;
        Ok(rows
            .into_iter()
            .filter_map(|row| {
                let kind = match value(&row, "kind").as_deref() {
                    Some("v") => TableType::View,
                    Some("m") => TableType::MaterializedView,
                    Some("f") => TableType::ForeignTable,
                    _ => TableType::Table,
                };
                Some(TableInfo {
                    schema: schema.to_owned(),
                    name: value(&row, "name")?,
                    kind,
                    row_count: value_i64(&row, "row_count"),
                    size_bytes: value_i64(&row, "size_bytes"),
                    comment: value(&row, "comment"),
                })
            })
            .collect())
    }

    pub async fn columns(&self, schema: &str, table: &str) -> Result<Vec<ColumnInfo>, DbError> {
        let relation = format!("{}.{}", quote_ident(schema), quote_ident(table));
        let sql = format!(
            "SELECT a.attname AS name, format_type(a.atttypid, a.atttypmod) AS data_type, \
             NOT a.attnotnull AS nullable, pg_get_expr(ad.adbin, ad.adrelid) AS default_value, \
             a.attnum AS position, col_description(a.attrelid, a.attnum) AS comment, \
             EXISTS (SELECT 1 FROM pg_constraint p WHERE p.conrelid = a.attrelid AND p.contype = 'p' AND a.attnum = ANY(p.conkey)) AS is_primary_key, \
             EXISTS (SELECT 1 FROM pg_constraint f WHERE f.conrelid = a.attrelid AND f.contype = 'f' AND a.attnum = ANY(f.conkey)) AS is_foreign_key \
             FROM pg_attribute a LEFT JOIN pg_attrdef ad ON ad.adrelid = a.attrelid AND ad.adnum = a.attnum \
             WHERE a.attrelid = {}::regclass AND a.attnum > 0 AND NOT a.attisdropped ORDER BY a.attnum",
            quote_literal(&relation)
        );
        let rows = self.rows_text(&sql).await?;
        Ok(rows
            .into_iter()
            .filter_map(|row| {
                Some(ColumnInfo {
                    name: value(&row, "name")?,
                    data_type: value(&row, "data_type").unwrap_or_default(),
                    nullable: value_bool(&row, "nullable"),
                    default_value: value(&row, "default_value"),
                    is_primary_key: value_bool(&row, "is_primary_key"),
                    is_foreign_key: value_bool(&row, "is_foreign_key"),
                    references: None,
                    // An out-of-range position is malformed; treat it like a missing one.
                    position: value_i64(&row, "position")
                        .and_then(|position| i32::try_from(position).ok())
                        .unwrap_or(0),
                    comment: value(&row, "comment"),
                })
            })
            .collect())
    }

    pub async fn primary_key(
        &self,
        schema: &str,
        table: &str,
    ) -> Result<Option<PrimaryKeyInfo>, DbError> {
        let relation = format!("{}.{}", quote_ident(schema), quote_ident(table));
        let sql = format!(
            "SELECT con.conname AS name, \
             (SELECT array_agg(a.attname ORDER BY k.ord)::text FROM unnest(con.conkey) WITH ORDINALITY k(attnum, ord) \
              JOIN pg_attribute a ON a.attrelid = con.conrelid AND a.attnum = k.attnum) AS columns \
             FROM pg_constraint con WHERE con.conrelid = {}::regclass AND con.contype = 'p' LIMIT 1",
            quote_literal(&relation)
        );
        let rows = self.rows_text(&sql).await?;
        Ok(rows.into_iter().next().map(|row| PrimaryKeyInfo {
            name: value(&row, "name").unwrap_or_default(),
            columns: parse_pg_array(&value(&row, "columns").unwrap_or_default()),
        }))
    }

    pub async fn indexes(&self, schema: &str, table: &str) -> Result<Vec<IndexInfo>, DbError> {
        let relation = format!("{}.{}", quote_ident(schema), quote_ident(table));
        let sql = format!(
            "SELECT i.relname AS name, ix.indisunique AS is_unique, ix.indisprimary AS is_primary, \
             am.amname AS method, pg_relation_size(i.oid) AS size_bytes, pg_get_indexdef(ix.indexrelid) AS definition, \
             (SELECT array_agg(a.attname ORDER BY k.ord)::text FROM unnest(ix.indkey) WITH ORDINALITY k(attnum, ord) \
              JOIN pg_attribute a ON a.attrelid = ix.indrelid AND a.attnum = k.attnum) AS columns \
             FROM pg_index ix JOIN pg_class i ON i.oid = ix.indexrelid JOIN pg_class t ON t.oid = ix.indrelid \
             JOIN pg_am am ON am.oid = i.relam WHERE t.oid = {}::regclass ORDER BY i.relname",
            quote_literal(&relation)
        );
        let rows = self.rows_text(&sql).await?;
        Ok(rows
            .into_iter()
            .filter_map(|row| {
                Some(IndexInfo {
                    schema: schema.to_owned(),
                    table: table.to_owned(),
                    name: value(&row, "name")?,
                    columns: parse_pg_array(&value(&row, "columns").unwrap_or_default()),
                    is_unique: value_bool(&row, "is_unique"),
                    is_primary: value_bool(&row, "is_primary"),
                    method: value(&row, "method").unwrap_or_else(|| "btree".to_owned()),
                    size_bytes: value_i64(&row, "size_bytes"),
                    definition: value(&row, "definition"),
                })
            })
            .collect())
    }

    pub async fn foreign_keys(
        &self,
        schema: &str,
        table: &str,
    ) -> Result<Vec<ForeignKeyInfo>, DbError> {
        let relation = format!("{}.{}", quote_ident(schema), quote_ident(table));
        let sql = format!(
            "SELECT con.conname AS name, \
             (SELECT array_agg(a.attname ORDER BY k.ord)::text FROM unnest(con.conkey) WITH ORDINALITY k(attnum, ord) \
              JOIN pg_attribute a ON a.attrelid = con.conrelid AND a.attnum = k.attnum) AS columns, \
             rn.nspname AS referenced_schema, rc.relname AS referenced_table, \
             (SELECT array_agg(a.attname ORDER BY k.ord)::text FROM unnest(con.confkey) WITH ORDINALITY k(attnum, ord) \
              JOIN pg_attribute a ON a.attrelid = con.confrelid AND a.attnum = k.attnum) AS referenced_columns, \
             con.confupdtype AS on_update, con.confdeltype AS on_delete \
             FROM pg_constraint con JOIN pg_class rc ON rc.oid = con.confrelid JOIN pg_namespace rn ON rn.oid = rc.relnamespace \
             WHERE con.conrelid = {}::regclass AND con.contype = 'f' ORDER BY con.conname",
            quote_literal(&relation)
        );
        let rows = self.rows_text(&sql).await?;
        Ok(rows
            .into_iter()
            .filter_map(|row| {
                Some(ForeignKeyInfo {
                    name: value(&row, "name")?,
                    columns: parse_pg_array(&value(&row, "columns").unwrap_or_default()),
                    referenced_schema: value(&row, "referenced_schema").unwrap_or_default(),
                    referenced_table: value(&row, "referenced_table").unwrap_or_default(),
                    referenced_columns: parse_pg_array(
                        &value(&row, "referenced_columns").unwrap_or_default(),
                    ),
                    on_update: fk_action(&value(&row, "on_update").unwrap_or_default()),
                    on_delete: fk_action(&value(&row, "on_delete").unwrap_or_default()),
                })
            })
            .collect())
    }

    pub async fn functions(&self, schema: &str) -> Result<Vec<FunctionInfo>, DbError> {
        let sql = format!(
            "SELECT p.proname AS name, p.prokind::text AS kind, pg_get_function_result(p.oid) AS return_type, \
             pg_get_function_arguments(p.oid) AS arguments, l.lanname AS language \
             FROM pg_proc p JOIN pg_namespace n ON n.oid = p.pronamespace JOIN pg_language l ON l.oid = p.prolang \
             WHERE n.nspname = {} ORDER BY p.proname",
            quote_literal(schema)
        );
        let rows = self.rows_text(&sql).await?;
        Ok(rows
            .into_iter()
            .filter_map(|row| {
                Some(FunctionInfo {
                    schema: schema.to_owned(),
                    name: value(&row, "name")?,
                    kind: match value(&row, "kind").as_deref() {
                        Some("p") => "procedure",
                        Some("a") => "aggregate",
                        Some("w") => "window",
                        _ => "function",
                    }
                    .to_owned(),
                    return_type: value(&row, "return_type").unwrap_or_default(),
                    arguments: value(&row, "arguments").unwrap_or_default(),
                    language: value(&row, "language"),
                })
            })
            .collect())
    }

    pub async fn triggers(&self, schema: &str, table: &str) -> Result<Vec<TriggerInfo>, DbError> {
        let relation = format!("{}.{}", quote_ident(schema), quote_ident(table));
        let sql = format!(
            "SELECT tg.tgname AS name, pg_get_triggerdef(tg.oid) AS definition \
             FROM pg_trigger tg WHERE tg.tgrelid = {}::regclass AND NOT tg.tgisinternal ORDER BY tg.tgname",
            quote_literal(&relation)
        );
        let rows = self.rows_text(&sql).await?;
        Ok(rows
            .into_iter()
            .filter_map(|row| {
                Some(TriggerInfo {
                    schema: schema.to_owned(),
                    table: table.to_owned(),
                    name: value(&row, "name")?,
                    definition: value(&row, "definition").unwrap_or_default(),
                })
            })
            .collect())
    }

    pub async fn policies(&self, schema: &str, table: &str) -> Result<Vec<PolicyInfo>, DbError> {
        let sql = format!(
            "SELECT policyname AS name, cmd AS command, permissive, roles::text AS roles \
             FROM pg_policies WHERE schemaname = {} AND tablename = {} ORDER BY policyname",
            quote_literal(schema),
            quote_literal(table)
        );
        let rows = self.rows_text(&sql).await?;
        Ok(rows
            .into_iter()
            .filter_map(|row| {
                Some(PolicyInfo {
                    schema: schema.to_owned(),
                    table: table.to_owned(),
                    name: value(&row, "name")?,
                    command: value(&row, "command").unwrap_or_default(),
                    permissive: value(&row, "permissive").unwrap_or_default(),
                    roles: parse_pg_array(&value(&row, "roles").unwrap_or_default()),
                })
            })
            .collect())
    }

    /// Lightweight DDL for display. Views use `pg_get_viewdef`; tables are
    /// reconstructed from columns, the primary key, and indexes.
    pub async fn ddl(&self, schema: &str, table: &str) -> Result<String, DbError> {
        let relation = format!("{}.{}", quote_ident(schema), quote_ident(table));
        let kind_sql = format!(
            "SELECT c.relkind::text AS kind FROM pg_class c JOIN pg_namespace n ON n.oid = c.relnamespace \
             WHERE n.nspname = {} AND c.relname = {} LIMIT 1",
            quote_literal(schema),
            quote_literal(table)
        );
        let kind = self
            .rows_text(&kind_sql)
            .await?
            .first()
            .and_then(|row| value(row, "kind"))
            .unwrap_or_default();
        if matches!(kind.as_str(), "v" | "m") {
            let sql = format!(
                "SELECT pg_get_viewdef({}::regclass, true) AS definition",
                quote_literal(&relation)
            );
            let rows = self.rows_text(&sql).await?;
            if let Some(definition) = rows.first().and_then(|row| value(row, "definition")) {
                let keyword = if kind == "m" {
                    "CREATE MATERIALIZED VIEW"
                } else {
                    "CREATE VIEW"
                };
                return Ok(format!(
                    "{keyword} {}.{} AS\n{}",
                    quote_ident(schema),
                    quote_ident(table),
                    definition.trim()
                ));
            }
        }
        let columns = self.columns(schema, table).await?;
        let primary = self.primary_key(schema, table).await?;
        let mut ddl = format!(
            "CREATE TABLE {}.{} (\n",
            quote_ident(schema),
            quote_ident(table)
        );
        let mut lines = columns
            .iter()
            .map(|column| {
                let mut line = format!("  {} {}", quote_ident(&column.name), column.data_type);
                if let Some(default) = &column.default_value {
                    line.push_str(&format!(" DEFAULT {default}"));
                }
                if !column.nullable {
                    line.push_str(" NOT NULL");
                }
                line
            })
            .collect::<Vec<_>>();
        if let Some(primary) = primary
            && !primary.columns.is_empty()
        {
            lines.push(format!(
                "  PRIMARY KEY ({})",
                primary
                    .columns
                    .iter()
                    .map(|column| quote_ident(column))
                    .collect::<Vec<_>>()
                    .join(", ")
            ));
        }
        ddl.push_str(&lines.join(",\n"));
        ddl.push_str("\n);");
        Ok(ddl)
    }
}

fn parse_pg_array(text: &str) -> Vec<String> {
    let trimmed = text.trim().trim_start_matches('{').trim_end_matches('}');
    if trimmed.is_empty() {
        return Vec::new();
    }
    let mut values = Vec::new();
    let mut current = String::new();
    let mut quoted = false;
    let mut chars = trimmed.chars().peekable();
    while let Some(ch) = chars.next() {
        match ch {
            '"' if quoted && chars.peek() == Some(&'"') => {
                current.push('"');
                chars.next();
            }
            '"' => quoted = !quoted,
            '\\' if quoted => {
                if let Some(next) = chars.next() {
                    current.push(next);
                }
            }
            ',' if !quoted => {
                values.push(std::mem::take(&mut current));
            }
            _ => current.push(ch),
        }
    }
    values.push(current);
    values
}

fn fk_action(code: &str) -> String {
    match code {
        "c" => "CASCADE",
        "n" => "SET NULL",
        "d" => "SET DEFAULT",
        "r" => "RESTRICT",
        _ => "NO ACTION",
    }
    .to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_postgres_arrays_and_quotes_identifiers() {
        assert_eq!(parse_pg_array("{id,name}"), vec!["id", "name"]);
        assert_eq!(parse_pg_array("{\"a,b\",c}"), vec!["a,b", "c"]);
        assert!(parse_pg_array("{}").is_empty());
        assert_eq!(quote_ident("a\"b"), "\"a\"\"b\"");
        assert_eq!(quote_literal("a'b"), "'a''b'");
    }

    #[test]
    fn backslash_names_quote_to_safe_unambiguous_literals() {
        for name in ["a\\", "a\\'; DROP TABLE t; --", "plain"] {
            let sql = format!(
                "SELECT relname FROM pg_class WHERE relname = {}",
                quote_literal(name)
            );
            let risk = crate::risk::classify_sql_risk(&sql);
            assert!(!risk.requires_confirmation, "{sql}");
            assert_eq!(risk.statements.len(), 1, "{sql}");
        }
        assert_eq!(quote_literal("a\\'b"), "E'a\\\\''b'");
    }

    #[test]
    fn maps_foreign_key_actions() {
        assert_eq!(fk_action("c"), "CASCADE");
        assert_eq!(fk_action("x"), "NO ACTION");
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CatalogItem {
    pub kind: String,
    pub schema: String,
    pub table: String,
    pub name: String,
    pub detail: String,
}
impl PostgresSession {
    pub async fn catalog(&self) -> Result<Vec<CatalogItem>, DbError> {
        let rows=self.rows_text(r#"
WITH user_schemas AS (SELECT oid,nspname FROM pg_catalog.pg_namespace WHERE nspname !~ '^pg_' AND nspname<>'information_schema')
SELECT kind,schema_name,table_name,object_name,detail FROM (
 SELECT 'schema'::text kind,n.nspname schema_name,''::text table_name,n.nspname object_name,''::text detail FROM user_schemas n
 UNION ALL SELECT CASE WHEN c.relkind IN ('v','m') THEN 'view' ELSE 'table' END,n.nspname,c.relname,c.relname,COALESCE(pg_catalog.obj_description(c.oid),'') FROM pg_catalog.pg_class c JOIN user_schemas n ON n.oid=c.relnamespace WHERE c.relkind IN ('r','p','v','m','f')
 UNION ALL SELECT 'function',n.nspname,'',p.proname||'('||pg_catalog.pg_get_function_arguments(p.oid)||')',pg_catalog.pg_get_function_result(p.oid) FROM pg_catalog.pg_proc p JOIN user_schemas n ON n.oid=p.pronamespace
 UNION ALL SELECT 'index',n.nspname,t.relname,c.relname,pg_catalog.pg_get_indexdef(c.oid) FROM pg_catalog.pg_index i JOIN pg_catalog.pg_class c ON c.oid=i.indexrelid JOIN pg_catalog.pg_class t ON t.oid=i.indrelid JOIN user_schemas n ON n.oid=t.relnamespace
 UNION ALL SELECT 'trigger',n.nspname,c.relname,t.tgname,pg_catalog.pg_get_triggerdef(t.oid) FROM pg_catalog.pg_trigger t JOIN pg_catalog.pg_class c ON c.oid=t.tgrelid JOIN user_schemas n ON n.oid=c.relnamespace WHERE NOT t.tgisinternal
 UNION ALL SELECT 'policy',n.nspname,c.relname,p.polname,COALESCE(pg_catalog.pg_get_expr(p.polqual,p.polrelid),'') FROM pg_catalog.pg_policy p JOIN pg_catalog.pg_class c ON c.oid=p.polrelid JOIN user_schemas n ON n.oid=c.relnamespace
) objects ORDER BY schema_name,kind,table_name,object_name LIMIT 5000
"#).await?;
        Ok(rows
            .into_iter()
            .map(|r| {
                let get = |key: &str| r.get(key).and_then(|v| v.clone()).unwrap_or_default();
                CatalogItem {
                    kind: get("kind"),
                    schema: get("schema_name"),
                    table: get("table_name"),
                    name: get("object_name"),
                    detail: get("detail"),
                }
            })
            .collect())
    }
}
