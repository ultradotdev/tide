//! Minimal SQL completion over keywords, tables, and columns.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CompletionSuggestion {
    pub label: String,
    pub kind: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

const KEYWORDS: &[&str] = &[
    "SELECT",
    "FROM",
    "WHERE",
    "JOIN",
    "LEFT JOIN",
    "RIGHT JOIN",
    "INNER JOIN",
    "FULL JOIN",
    "ON",
    "GROUP BY",
    "ORDER BY",
    "HAVING",
    "LIMIT",
    "OFFSET",
    "INSERT INTO",
    "VALUES",
    "UPDATE",
    "SET",
    "DELETE FROM",
    "RETURNING",
    "WITH",
    "AS",
    "DISTINCT",
    "UNION",
    "ALL",
    "CASE",
    "WHEN",
    "THEN",
    "ELSE",
    "END",
    "AND",
    "OR",
    "NOT",
    "NULL",
    "IS",
    "IN",
    "EXISTS",
    "BETWEEN",
    "LIKE",
    "ILIKE",
    "ASC",
    "DESC",
    "CREATE TABLE",
    "ALTER TABLE",
    "DROP TABLE",
    "TRUNCATE",
    "BEGIN",
    "COMMIT",
    "ROLLBACK",
    "EXPLAIN",
    "ANALYZE",
    "VACUUM",
    "SHOW",
];

pub fn complete_sql(
    input: &str,
    tables: &[String],
    columns: &[String],
) -> Vec<CompletionSuggestion> {
    let token = input
        .rsplit(|ch: char| !(ch.is_ascii_alphanumeric() || ch == '_' || ch == '.'))
        .next()
        .unwrap_or_default();
    let needle = token.rsplit('.').next().unwrap_or_default().to_lowercase();
    let qualified = token.contains('.');
    let mut suggestions = Vec::new();
    if !qualified {
        for keyword in KEYWORDS {
            if needle.is_empty() || keyword.to_lowercase().starts_with(&needle) {
                suggestions.push(CompletionSuggestion {
                    label: (*keyword).to_owned(),
                    kind: "keyword".to_owned(),
                    detail: None,
                });
            }
        }
        for table in tables {
            if needle.is_empty() || table.to_lowercase().starts_with(&needle) {
                suggestions.push(CompletionSuggestion {
                    label: table.clone(),
                    kind: "table".to_owned(),
                    detail: Some("table".to_owned()),
                });
            }
        }
    }
    for column in columns {
        if needle.is_empty() || column.to_lowercase().starts_with(&needle) {
            suggestions.push(CompletionSuggestion {
                label: column.clone(),
                kind: "column".to_owned(),
                detail: Some("column".to_owned()),
            });
        }
    }
    suggestions.sort_by(|left, right| left.label.cmp(&right.label));
    suggestions.dedup_by(|left, right| left.label == right.label && left.kind == right.kind);
    suggestions.truncate(100);
    suggestions
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn completes_keywords_tables_and_columns() {
        let tables = vec!["users".to_owned(), "orders".to_owned()];
        let columns = vec!["user_id".to_owned(), "email".to_owned()];
        assert!(
            complete_sql("SEL", &tables, &columns)
                .iter()
                .any(|item| item.label == "SELECT")
        );
        assert!(
            complete_sql("us", &tables, &columns)
                .iter()
                .any(|item| item.label == "users")
        );
        assert!(
            complete_sql("em", &tables, &columns)
                .iter()
                .any(|item| item.label == "email" && item.kind == "column")
        );
        assert!(complete_sql("x", &tables, &columns).is_empty());
    }

    #[test]
    fn qualified_tokens_only_suggest_columns() {
        let tables = vec!["users".to_owned()];
        let columns = vec!["user_id".to_owned()];
        let suggestions = complete_sql("users.user", &tables, &columns);
        assert!(suggestions.iter().all(|item| item.kind == "column"));
        assert!(suggestions.iter().any(|item| item.label == "user_id"));
    }
}
