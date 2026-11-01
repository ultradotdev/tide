//! CSV export for query results.

use crate::postgres::QueryResult;

pub fn result_to_csv(result: &QueryResult) -> String {
    let mut output = String::new();
    let headers = result
        .fields
        .iter()
        .map(|field| escape_csv(&field.name))
        .collect::<Vec<_>>();
    if !headers.is_empty() {
        output.push_str(&headers.join(","));
        output.push('\n');
    }
    for row in &result.rows {
        let cells = result
            .fields
            .iter()
            .map(|field| {
                row.get(&field.name)
                    .map_or_else(String::new, |value| match value {
                        serde_json::Value::Null => String::new(),
                        serde_json::Value::String(text) => escape_csv(text),
                        other => escape_csv(&other.to_string()),
                    })
            })
            .collect::<Vec<_>>();
        output.push_str(&cells.join(","));
        output.push('\n');
    }
    output
}

fn escape_csv(value: &str) -> String {
    if value.contains([',', '"', '\n', '\r']) {
        format!("\"{}\"", value.replace('"', "\"\""))
    } else {
        value.to_owned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::postgres::FieldInfo;
    use std::collections::BTreeMap;

    #[test]
    fn exports_csv_with_escaping() {
        let result = QueryResult {
            row_target: None,
            row_source: None,
            query_id: "q".to_owned(),
            connection_id: "c".to_owned(),
            sql: "SELECT".to_owned(),
            rows: vec![BTreeMap::from([
                ("name".to_owned(), serde_json::json!("a,b")),
                ("id".to_owned(), serde_json::json!(1)),
            ])],
            fields: vec![
                FieldInfo {
                    name: "id".to_owned(),
                    data_type: "int4".to_owned(),
                    data_type_id: 23,
                    nullable: true,
                },
                FieldInfo {
                    name: "name".to_owned(),
                    data_type: "text".to_owned(),
                    data_type_id: 25,
                    nullable: true,
                },
            ],
            row_count: 1,
            started_at: String::new(),
            completed_at: String::new(),
            duration_ms: 1,
            affected_rows: None,
            notices: Vec::new(),
            risk: None,
            truncated: false,
        };
        let csv = result_to_csv(&result);
        assert_eq!(csv, "id,name\n1,\"a,b\"\n");
    }
}
