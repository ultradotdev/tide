//! Remove credentials and connection strings before persistence or logging.

use std::sync::OnceLock;

use regex::Regex;

fn postgres_uri() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r#"(?i)\b(?:jdbc:)?postgres(?:ql)?://[^\s'"<>]+"#).expect("valid regex")
    })
}

fn password_clause() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r#"(?i)(\bPASSWORD\s+)(?:'(?:''|[^'])*'|"(?:""|[^"])*"|[^\s;,=:][^\s;,]*)"#)
            .expect("valid regex")
    })
}

fn secret_assignment() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(
            r#"(?i)(\b(?:password|passwd|api[_-]?key|access[_-]?token|secret)\b\s*(?:=|:)\s*)(?:'(?:''|[^'])*'|"(?:""|[^"])*"|[^\s;,]+)"#,
        )
        .expect("valid regex")
    })
}

pub fn redact_database_sensitive_text(text: &str, known_secrets: &[&str]) -> String {
    let mut redacted = text.to_owned();
    for secret in known_secrets {
        if !secret.is_empty() {
            redacted = redacted.replace(secret, "[REDACTED]");
        }
    }
    let redacted = postgres_uri()
        .replace_all(&redacted, "[REDACTED_CONNECTION_STRING]")
        .into_owned();
    let redacted = password_clause()
        .replace_all(&redacted, "$1'[REDACTED]'")
        .into_owned();
    secret_assignment()
        .replace_all(&redacted, "$1'[REDACTED]'")
        .into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redacts_urls_and_assignments() {
        assert_eq!(
            redact_database_sensitive_text("postgres://user:pass@host/db", &[]),
            "[REDACTED_CONNECTION_STRING]"
        );
        assert_eq!(
            redact_database_sensitive_text("password=hunter2", &[]),
            "password='[REDACTED]'"
        );
        assert_eq!(
            redact_database_sensitive_text("ALTER USER x PASSWORD 'hunter2'", &[]),
            "ALTER USER x PASSWORD '[REDACTED]'"
        );
    }

    #[test]
    fn redacts_known_secret_values() {
        assert_eq!(
            redact_database_sensitive_text("token s3cret appears", &["s3cret"]),
            "token [REDACTED] appears"
        );
    }
}
