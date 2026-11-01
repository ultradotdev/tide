//! Conservative PostgreSQL statement classification for confirmation UX.
//!
//! A small tokenizer splits a submission into statements, classifies each
//! operation and risk level, and flags mutations, broad or destructive changes
//! and read-only overrides that need confirmation.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum SqlOperation {
    Read,
    Insert,
    Update,
    Delete,
    DestructiveDdl,
    Create,
    Function,
    Write,
    Transaction,
    Session,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SqlRiskLevel {
    Safe,
    Mutation,
    Broad,
    Destructive,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SqlStatementRisk {
    pub sql: String,
    pub operation: SqlOperation,
    pub risk: SqlRiskLevel,
    pub requires_confirmation: bool,
    pub attempts_read_only_override: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SqlRiskAssessment {
    pub statements: Vec<SqlStatementRisk>,
    pub operations: Vec<SqlOperation>,
    pub risk: SqlRiskLevel,
    pub requires_confirmation: bool,
    pub is_mutation: bool,
    pub is_mixed_submission: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Token {
    value: String,
    quoted: Option<String>,
    depth: usize,
    start: usize,
    end: usize,
}

#[derive(Debug, Default)]
struct Lexed {
    tokens: Vec<Token>,
    uncertain: Vec<(usize, &'static str)>,
}

const UNTERMINATED: &str = "The SQL has an unterminated quote, comment, or dollar-quoted string.";
const BACKSLASH_QUOTE: &str = "A backslash in a quoted string makes statement boundaries depend on standard_conforming_strings.";
const UNICODE_IDENTIFIER: &str = "Unicode-escaped identifiers cannot be classified safely.";
const NESTED_SEMICOLON: &str = "A semicolon inside parentheses cannot be classified safely.";

const READS: &[&str] = &["SELECT", "SHOW", "EXPLAIN", "VALUES", "TABLE"];
const OTHER_WRITES: &[&str] = &[
    "CALL", "COPY", "GRANT", "REVOKE", "COMMENT", "SECURITY", "REINDEX", "CLUSTER", "REFRESH",
    "VACUUM", "ANALYZE", "DO", "MERGE",
];
const TRANSACTIONS: &[&str] = &[
    "BEGIN",
    "START",
    "COMMIT",
    "ROLLBACK",
    "SAVEPOINT",
    "RELEASE",
];
const SIDE_EFFECT_FUNCTIONS: &[&str] = &[
    "PG_TERMINATE_BACKEND",
    "PG_CANCEL_BACKEND",
    "PG_RELOAD_CONF",
    "PG_ROTATE_LOGFILE",
    "PG_LOG_BACKEND_MEMORY_CONTEXTS",
    "SET_CONFIG",
    "NEXTVAL",
    "SETVAL",
    "LOWRITE",
    "PG_NOTIFY",
    "PG_SWITCH_WAL",
    "PG_PROMOTE",
    "PG_WAL_REPLAY_PAUSE",
    "PG_WAL_REPLAY_RESUME",
    "PG_BACKUP_START",
    "PG_BACKUP_STOP",
    "PG_START_BACKUP",
    "PG_STOP_BACKUP",
    "PG_IMPORT_SYSTEM_COLLATIONS",
    "PG_DROP_REPLICATION_SLOT",
    "PG_COPY_PHYSICAL_REPLICATION_SLOT",
    "PG_COPY_LOGICAL_REPLICATION_SLOT",
    "PG_REPLICATION_SLOT_ADVANCE",
];
const SIDE_EFFECT_PREFIXES: &[&str] = &[
    "DBLINK",
    "LO_",
    "PG_ADVISORY",
    "PG_TRY_ADVISORY",
    "PG_CREATE_",
    "PG_LOGICAL_",
    "PG_REPLICATION_ORIGIN_",
    "PG_STAT_RESET",
    "PG_STAT_STATEMENTS_RESET",
    "PG_FILE_",
];
const MUTATIONS: &[SqlOperation] = &[
    SqlOperation::Insert,
    SqlOperation::Update,
    SqlOperation::Delete,
    SqlOperation::DestructiveDdl,
    SqlOperation::Create,
    SqlOperation::Function,
    SqlOperation::Write,
    SqlOperation::Session,
];

pub fn classify_sql_risk(sql: &str) -> SqlRiskAssessment {
    let lexed = tokenize(sql);
    let ranges = split_statements(sql, &lexed.tokens);
    let mut statements = ranges
        .iter()
        .map(|(start, end, tokens)| classify_statement(sql[*start..*end].trim(), tokens))
        .collect::<Vec<_>>();
    for (position, reason) in lexed.uncertain {
        let index = ranges
            .iter()
            .position(|(start, end, _)| (*start..*end).contains(&position))
            .unwrap_or(ranges.len().saturating_sub(1));
        if let Some(statement) = statements.get_mut(index) {
            statement.requires_confirmation = true;
            statement.risk = SqlRiskLevel::Unknown;
            statement.reason = Some(match statement.reason.take() {
                Some(existing) if existing.contains(reason) => existing,
                Some(existing) => format!("{existing} {reason}"),
                None => reason.to_owned(),
            });
        }
    }
    let operations = statements
        .iter()
        .map(|statement| statement.operation)
        .collect::<Vec<_>>();
    let is_mutation = operations
        .iter()
        .any(|operation| MUTATIONS.contains(operation));
    let is_mixed_submission = statements.len() > 1 && is_mutation;
    let risk = statements
        .iter()
        .map(|statement| statement.risk)
        .max()
        .unwrap_or(SqlRiskLevel::Unknown);
    SqlRiskAssessment {
        requires_confirmation: statements.is_empty()
            || is_mixed_submission
            || statements
                .iter()
                .any(|statement| statement.requires_confirmation),
        statements,
        operations,
        risk,
        is_mutation,
        is_mixed_submission,
    }
}

pub fn attempts_read_only_override(sql: &str) -> bool {
    classify_sql_risk(sql)
        .statements
        .iter()
        .any(|statement| statement.attempts_read_only_override)
}

/// Mirrors PostgreSQL's scanner for everything that can move a statement
/// boundary. Whatever it cannot resolve the way the server would is recorded
/// as uncertain so classification fails closed.
fn tokenize(sql: &str) -> Lexed {
    let bytes = sql.as_bytes();
    let mut lexed = Lexed::default();
    let mut index = 0;
    let mut depth = 0_usize;
    while index < bytes.len() {
        let start = index;
        let byte = bytes[index];
        let next = bytes.get(index + 1).copied();
        let after = bytes.get(index + 2).copied();
        if matches!(byte, b' ' | b'\t' | b'\n' | b'\r' | b'\x0b' | b'\x0c') {
            index += 1;
            continue;
        }
        if byte == b'-' && next == Some(b'-') {
            index = sql[index..]
                .find(['\n', '\r'])
                .map_or(bytes.len(), |found| index + found);
            continue;
        }
        if byte == b'/' && next == Some(b'*') {
            index = block_comment_end(bytes, index).unwrap_or_else(|| {
                lexed.uncertain.push((start, UNTERMINATED));
                bytes.len()
            });
            continue;
        }
        // (opening quote, backslash escapes, standard_conforming_strings dependent)
        let string = match (byte, next, after) {
            (b'\'', ..) => Some((index, false, true)),
            (b'E' | b'e', Some(b'\''), _) => Some((index + 1, true, false)),
            (b'U' | b'u', Some(b'&'), Some(b'\'')) => Some((index + 2, false, false)),
            _ => None,
        };
        if let Some((open, backslash, ambiguous)) = string {
            let end = quoted_end(bytes, open, backslash);
            if ambiguous && end.is_some() && end != quoted_end(bytes, open, true) {
                lexed.uncertain.push((start, BACKSLASH_QUOTE));
            }
            index = end.unwrap_or_else(|| {
                lexed.uncertain.push((start, UNTERMINATED));
                bytes.len()
            });
            continue;
        }
        let identifier = match (byte, next, after) {
            (b'"', ..) => Some(index),
            (b'U' | b'u', Some(b'&'), Some(b'"')) => Some(index + 2),
            _ => None,
        };
        if let Some(open) = identifier {
            let end = quoted_end(bytes, open, false);
            let quoted = if open != start {
                lexed.uncertain.push((start, UNICODE_IDENTIFIER));
                None
            } else {
                end.map(|end| sql[open + 1..end - 1].replace("\"\"", "\"").to_uppercase())
            };
            index = end.unwrap_or_else(|| {
                lexed.uncertain.push((start, UNTERMINATED));
                bytes.len()
            });
            lexed.tokens.push(Token {
                value: "IDENTIFIER".to_owned(),
                quoted,
                depth,
                start,
                end: index,
            });
            continue;
        }
        if let Some(tag) = dollar_tag_at(sql, index) {
            index += tag.len();
            index = sql[index..].find(tag).map_or_else(
                || {
                    lexed.uncertain.push((start, UNTERMINATED));
                    bytes.len()
                },
                |found| index + found + tag.len(),
            );
            continue;
        }
        if byte.is_ascii_alphabetic() || byte == b'_' || byte >= 0x80 {
            index += 1;
            while index < bytes.len() && is_identifier_byte(bytes[index]) {
                index += 1;
            }
            lexed.tokens.push(Token {
                value: sql[start..index].to_uppercase(),
                quoted: None,
                depth,
                start,
                end: index,
            });
            continue;
        }
        if byte == b')' {
            depth = depth.saturating_sub(1);
        }
        if byte == b';' && depth > 0 {
            lexed.uncertain.push((start, NESTED_SEMICOLON));
        }
        lexed.tokens.push(Token {
            value: char::from(byte).to_string(),
            quoted: None,
            depth,
            start,
            end: index + 1,
        });
        if byte == b'(' {
            depth += 1;
        }
        index += 1;
    }
    lexed
}

/// PostgreSQL identifiers continue with ASCII alphanumerics, `_`, `$`, and any
/// non-ASCII byte.
fn is_identifier_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'$' || byte >= 0x80
}

fn block_comment_end(bytes: &[u8], open: usize) -> Option<usize> {
    let mut index = open + 2;
    let mut nested = 1;
    while index + 1 < bytes.len() {
        match (bytes[index], bytes[index + 1]) {
            (b'/', b'*') => {
                nested += 1;
                index += 2;
            }
            (b'*', b'/') => {
                nested -= 1;
                index += 2;
                if nested == 0 {
                    return Some(index);
                }
            }
            _ => index += 1,
        }
    }
    None
}

/// Exclusive end of the quoted run opened at `open`, honoring doubled quotes
/// and, when `backslash` is set, backslash escapes.
fn quoted_end(bytes: &[u8], open: usize, backslash: bool) -> Option<usize> {
    let quote = bytes[open];
    let mut index = open + 1;
    while index < bytes.len() {
        if backslash && bytes[index] == b'\\' {
            index += 2;
        } else if bytes[index] != quote {
            index += 1;
        } else if bytes.get(index + 1) == Some(&quote) {
            index += 2;
        } else {
            return Some(index + 1);
        }
    }
    None
}

fn dollar_tag_at(sql: &str, index: usize) -> Option<&str> {
    let bytes = sql.as_bytes();
    if bytes.get(index) != Some(&b'$') || bytes.get(index + 1).is_some_and(u8::is_ascii_digit) {
        return None;
    }
    let mut end = index + 1;
    while end < bytes.len() && bytes[end] != b'$' && is_identifier_byte(bytes[end]) {
        end += 1;
    }
    (bytes.get(end) == Some(&b'$')).then(|| &sql[index..=end])
}

/// Byte ranges and tokens per statement. PostgreSQL ignores statements that
/// hold only comments, so they are dropped unless nothing else remains.
fn split_statements<'a>(sql: &str, tokens: &'a [Token]) -> Vec<(usize, usize, &'a [Token])> {
    let mut result = Vec::new();
    let mut char_start = 0;
    let mut token_start = 0;
    for (index, token) in tokens.iter().enumerate() {
        if token.value != ";" || token.depth != 0 {
            continue;
        }
        result.push((char_start, token.start, &tokens[token_start..index]));
        char_start = token.end;
        token_start = index + 1;
    }
    result.push((char_start, sql.len(), &tokens[token_start..]));
    let any_tokens = result.iter().any(|(.., tokens)| !tokens.is_empty());
    result.retain(|(start, end, tokens)| {
        if any_tokens {
            !tokens.is_empty()
        } else {
            !sql[*start..*end].trim().is_empty()
        }
    });
    result
}

fn side_effect_call(tokens: &[Token]) -> Option<String> {
    tokens.windows(2).find_map(|pair| {
        let name = pair[0].quoted.as_deref().unwrap_or(&pair[0].value);
        let known = SIDE_EFFECT_FUNCTIONS.contains(&name)
            || SIDE_EFFECT_PREFIXES
                .iter()
                .any(|prefix| name.starts_with(prefix));
        (known && pair[1].value == "(").then(|| name.to_lowercase())
    })
}

fn selects_into(tokens: &[Token]) -> bool {
    tokens.iter().enumerate().any(|(index, token)| {
        token.value == "INTO"
            && token.quoted.is_none()
            && (index == 0 || tokens[index - 1].value != "AS")
    })
}

fn primary_command(tokens: &[Token]) -> Option<&Token> {
    let first = tokens.iter().find(|token| {
        token.depth == 0 && token.value.starts_with(|c: char| c.is_ascii_uppercase())
    })?;
    if first.value != "WITH" {
        return Some(first);
    }
    tokens.iter().find(|token| {
        token.depth == 0
            && token.start > first.start
            && (READS.contains(&token.value.as_str())
                || ["INSERT", "UPDATE", "DELETE", "MERGE"].contains(&token.value.as_str()))
    })
}

fn classify_statement(sql: &str, tokens: &[Token]) -> SqlStatementRisk {
    let words = tokens
        .iter()
        .filter(|token| token.value.starts_with(|c: char| c.is_ascii_uppercase()))
        .collect::<Vec<_>>();
    let top_level = words
        .iter()
        .filter(|token| token.depth == 0)
        .map(|token| token.value.as_str())
        .collect::<Vec<_>>();
    let mentions_read_only = top_level.contains(&"DEFAULT_TRANSACTION_READ_ONLY");
    let read_write_first = top_level.first().is_some_and(|first| {
        ["SET", "BEGIN", "START"].contains(first)
            && top_level.contains(&"READ")
            && top_level.contains(&"WRITE")
    });
    let set_off = top_level.first() == Some(&"SET")
        && mentions_read_only
        && (top_level.contains(&"OFF") || top_level.contains(&"FALSE"));
    let reset =
        top_level.first() == Some(&"RESET") && (mentions_read_only || top_level.contains(&"ALL"));
    let attempts_read_only_override = read_write_first || set_off || reset;

    let make = |operation: SqlOperation,
                risk: SqlRiskLevel,
                requires_confirmation: bool,
                reason: Option<String>| SqlStatementRisk {
        sql: sql.to_owned(),
        operation,
        risk,
        requires_confirmation,
        attempts_read_only_override,
        reason,
    };

    let has_destructive_ddl = words.iter().any(|token| {
        token.value == "DROP"
            || token.value == "TRUNCATE"
            || (token.value == "ALTER"
                && words
                    .iter()
                    .position(|candidate| std::ptr::eq(*candidate, *token))
                    .and_then(|index| words.get(index + 1))
                    .is_some_and(|next| ["DROP", "RENAME", "ALTER"].contains(&next.value.as_str())))
            || (token.value == "CREATE"
                && words
                    .iter()
                    .any(|candidate| candidate.value == "DATABASE" || candidate.value == "SCHEMA"))
    });
    if has_destructive_ddl {
        return make(
            SqlOperation::DestructiveDdl,
            SqlRiskLevel::Destructive,
            true,
            Some("Destructive DDL changes database structure or removes data.".to_owned()),
        );
    }

    let embedded = words
        .iter()
        .find(|token| ["INSERT", "UPDATE", "DELETE", "MERGE"].contains(&token.value.as_str()));
    let Some(command) = primary_command(tokens).or(embedded.copied()) else {
        return make(
            SqlOperation::Unknown,
            SqlRiskLevel::Unknown,
            true,
            Some("The statement could not be classified safely.".to_owned()),
        );
    };
    match command.value.as_str() {
        "INSERT" => make(SqlOperation::Insert, SqlRiskLevel::Mutation, false, None),
        "UPDATE" | "DELETE" => {
            let operation = if command.value == "UPDATE" {
                SqlOperation::Update
            } else {
                SqlOperation::Delete
            };
            let broad = !tokens.iter().any(|token| {
                token.depth == command.depth
                    && token.start > command.start
                    && token.value == "WHERE"
            });
            make(
                operation,
                if broad {
                    SqlRiskLevel::Broad
                } else {
                    SqlRiskLevel::Mutation
                },
                broad,
                broad.then(|| {
                    if command.value == "UPDATE" {
                        "UPDATE has no top-level WHERE clause.".to_owned()
                    } else {
                        "DELETE has no top-level WHERE clause.".to_owned()
                    }
                }),
            )
        }
        "CREATE" => make(
            SqlOperation::Create,
            SqlRiskLevel::Destructive,
            true,
            Some("CREATE requires confirmation.".to_owned()),
        ),
        "MERGE" | "ALTER" => make(
            SqlOperation::Write,
            SqlRiskLevel::Destructive,
            true,
            Some(format!("{} requires confirmation.", command.value)),
        ),
        value if OTHER_WRITES.contains(&value) => make(
            SqlOperation::Write,
            SqlRiskLevel::Destructive,
            true,
            Some(format!("{value} requires confirmation.")),
        ),
        value if READS.contains(&value) => {
            if let Some(embedded) = embedded {
                return make(
                    match embedded.value.as_str() {
                        "INSERT" => SqlOperation::Insert,
                        "UPDATE" => SqlOperation::Update,
                        "DELETE" => SqlOperation::Delete,
                        _ => SqlOperation::Write,
                    },
                    SqlRiskLevel::Destructive,
                    true,
                    Some("A data-modifying CTE requires confirmation.".to_owned()),
                );
            }
            if selects_into(tokens) {
                return make(
                    SqlOperation::Create,
                    SqlRiskLevel::Destructive,
                    true,
                    Some("SELECT … INTO creates a table and requires confirmation.".to_owned()),
                );
            }
            if let Some(function) = side_effect_call(tokens) {
                return make(
                    SqlOperation::Function,
                    SqlRiskLevel::Mutation,
                    true,
                    Some(format!(
                        "{function}() has side effects and requires confirmation."
                    )),
                );
            }
            make(SqlOperation::Read, SqlRiskLevel::Safe, false, None)
        }
        "SET" | "RESET" => make(
            SqlOperation::Session,
            SqlRiskLevel::Destructive,
            true,
            Some(
                if attempts_read_only_override {
                    "This statement attempts to enable writes on a read-only connection."
                } else {
                    "Session settings require confirmation."
                }
                .to_owned(),
            ),
        ),
        value if TRANSACTIONS.contains(&value) => {
            if attempts_read_only_override {
                make(
                    SqlOperation::Session,
                    SqlRiskLevel::Destructive,
                    true,
                    Some(
                        "This transaction attempts to enable writes on a read-only connection."
                            .to_owned(),
                    ),
                )
            } else {
                make(SqlOperation::Transaction, SqlRiskLevel::Safe, false, None)
            }
        }
        value => make(
            SqlOperation::Unknown,
            SqlRiskLevel::Unknown,
            true,
            Some(format!("{value} is not a recognized safe statement.")),
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_reads_and_plain_mutations() {
        let assessment = classify_sql_risk("SELECT * FROM users; INSERT INTO logs VALUES (1);");
        assert_eq!(assessment.statements.len(), 2);
        assert_eq!(assessment.risk, SqlRiskLevel::Mutation);
        assert!(assessment.is_mutation);
        assert!(assessment.is_mixed_submission);
        assert!(assessment.requires_confirmation);
    }

    #[test]
    fn flags_broad_update_and_delete() {
        assert_eq!(
            classify_sql_risk("UPDATE users SET active = true").risk,
            SqlRiskLevel::Broad
        );
        assert!(classify_sql_risk("DELETE FROM users").requires_confirmation);
        assert_eq!(
            classify_sql_risk("DELETE FROM users WHERE id = 1").risk,
            SqlRiskLevel::Mutation
        );
    }

    #[test]
    fn flags_destructive_ddl_and_writes() {
        for sql in [
            "DROP TABLE users",
            "TRUNCATE users",
            "ALTER TABLE users DROP COLUMN email",
            "CREATE DATABASE prod",
            "CREATE TABLE t (id int)",
            "GRANT ALL ON t TO public",
        ] {
            assert!(
                classify_sql_risk(sql).requires_confirmation,
                "{sql} should require confirmation"
            );
        }
    }

    #[test]
    fn detects_read_only_overrides() {
        assert!(attempts_read_only_override(
            "SET default_transaction_read_only = off"
        ));
        assert!(attempts_read_only_override("BEGIN READ WRITE"));
        assert!(attempts_read_only_override("RESET ALL"));
        assert!(!attempts_read_only_override("SELECT 1"));
    }

    #[test]
    fn ignores_keywords_inside_literals_and_comments() {
        let assessment = classify_sql_risk(
            "SELECT 'DROP TABLE users' AS sample; -- DELETE FROM users\n/* UPDATE x */ SELECT 1;",
        );
        assert_eq!(assessment.risk, SqlRiskLevel::Safe);
        assert!(!assessment.requires_confirmation);
    }

    #[test]
    fn handles_ctes_and_dollar_quoting() {
        assert_eq!(
            classify_sql_risk("WITH x AS (SELECT 1) SELECT * FROM x").risk,
            SqlRiskLevel::Safe
        );
        assert_eq!(
            classify_sql_risk("WITH x AS (DELETE FROM users RETURNING *) SELECT * FROM x").risk,
            SqlRiskLevel::Destructive
        );
        assert_eq!(
            classify_sql_risk("SELECT $$ DROP TABLE users; $$").risk,
            SqlRiskLevel::Safe
        );
    }

    #[test]
    fn arbitrary_sql_never_panics_and_always_classifies() {
        let alphabet = b"SELECTDROPTABLE'\"$$();,--/*\n 0123456789WHEREUPDATE";
        let mut state = 0xdead_beef_u32;
        for _ in 0..500 {
            state = state.wrapping_mul(1103515245).wrapping_add(12345);
            let length = (state as usize % 120) + 1;
            let sql = (0..length)
                .map(|_| {
                    state = state.wrapping_mul(1103515245).wrapping_add(12345);
                    alphabet[(state as usize) % alphabet.len()] as char
                })
                .collect::<String>();
            let assessment = classify_sql_risk(&sql);
            if !sql.trim().is_empty() {
                assert!(!assessment.statements.is_empty(), "{sql:?}");
            }
            let _ = attempts_read_only_override(&sql);
        }
    }

    fn operations(sql: &str) -> Vec<SqlOperation> {
        classify_sql_risk(sql).operations
    }

    #[test]
    fn escape_strings_cannot_hide_following_statements() {
        for sql in [
            "SELECT E'\\''; DROP TABLE t; --'",
            "SELECT e'\\''; DROP TABLE t; --'",
            "SELECT E'\\\\'; DROP TABLE t",
        ] {
            let assessment = classify_sql_risk(sql);
            assert!(assessment.requires_confirmation, "{sql}");
            assert!(
                assessment
                    .operations
                    .contains(&SqlOperation::DestructiveDdl),
                "{sql}: {:?}",
                assessment.operations
            );
        }
        assert_eq!(
            operations("SELECT E'it\\'s'; SELECT 2"),
            [SqlOperation::Read, SqlOperation::Read]
        );
    }

    #[test]
    fn carriage_return_ends_line_comments() {
        let assessment = classify_sql_risk("SELECT 1; --\rDELETE FROM users");
        assert!(assessment.requires_confirmation);
        let last = assessment.statements.last().unwrap();
        assert_eq!(last.operation, SqlOperation::Delete);
        assert_eq!(last.risk, SqlRiskLevel::Broad);
        assert!(!assessment.operations.contains(&SqlOperation::Unknown));
    }

    #[test]
    fn trailing_comments_are_not_statements() {
        for sql in [
            "SELECT 1; -- note",
            "SELECT 1; /* note */",
            "SELECT 1;\n-- a\n-- b\n",
        ] {
            let assessment = classify_sql_risk(sql);
            assert_eq!(assessment.operations, [SqlOperation::Read], "{sql}");
            assert!(!assessment.requires_confirmation, "{sql}");
        }
    }

    #[test]
    fn lexer_uncertainty_fails_closed() {
        for sql in [
            "SELECT 'unterminated",
            "SELECT E'unterminated\\'",
            "SELECT \"unterminated",
            "SELECT $tag$ unterminated",
            "SELECT 1 /* unterminated",
            "SELECT 'C:\\'",
            "SELECT 'a\\'; DELETE FROM users; --'",
            "SELECT U&\"\\0064rop\"()",
            "SELECT (1; DROP TABLE t)",
        ] {
            assert!(
                classify_sql_risk(sql).requires_confirmation,
                "{sql} should require confirmation"
            );
        }
        let ambiguous = classify_sql_risk("SELECT 'a\\'; DELETE FROM users; --'");
        assert!(ambiguous.operations.contains(&SqlOperation::Delete));
        assert!(
            ambiguous
                .statements
                .iter()
                .any(|statement| statement.reason.is_some())
        );
    }

    #[test]
    fn dollar_quotes_follow_postgres_tag_rules() {
        let assessment = classify_sql_risk("SELECT $é$'$é$; DROP TABLE t; --'");
        assert!(assessment.requires_confirmation);
        assert!(
            assessment
                .operations
                .contains(&SqlOperation::DestructiveDdl)
        );
        let assessment = classify_sql_risk("SELECT $1$; DROP TABLE t; $1$");
        assert!(
            assessment
                .operations
                .contains(&SqlOperation::DestructiveDdl)
        );
    }

    #[test]
    fn unicode_strings_and_identifiers_lex_like_postgres() {
        assert_eq!(
            operations("SELECT U&'d\\0061t\\+000061'; DROP TABLE t"),
            [SqlOperation::Read, SqlOperation::DestructiveDdl]
        );
        let assessment = classify_sql_risk("SELECT éE'\\'; DROP TABLE t; --'");
        assert!(assessment.requires_confirmation);
        assert!(
            assessment
                .operations
                .contains(&SqlOperation::DestructiveDdl)
        );
    }

    #[test]
    fn benign_queries_stay_safe() {
        for sql in [
            "SELECT 1",
            "SELECT * FROM users WHERE id = 1",
            "select count(*) from orders where status = 'open'",
            "SELECT 'it''s' AS quote",
            "SELECT E'line\\nbreak', E'it\\'s', e'\\\\'",
            "SELECT * FROM t WHERE name LIKE '%\\_%'",
            "SELECT regexp_replace(name, '\\s+', ' ', 'g') FROM t",
            "SELECT 'C:\\path\\file'",
            "SELECT U&'\\0041', x'1F', b'0101', N'text'",
            "SELECT \"weird\"\"name\", \"order\" FROM \"Table\"",
            "SELECT $$ a ' b ; DROP TABLE t $$, $fn$ x $fn$",
            "/* nested /* comment */ */ SELECT 1 -- trailing\r\n",
            "WITH recent AS (SELECT * FROM events) SELECT * FROM recent",
            "SELECT now(), lower(name), lastval_report(), current_setting('search_path') FROM t",
            "SELECT pg_sleep(0.1), pg_size_pretty(pg_database_size(current_database()))",
            "SELECT setval_count, nextval_total FROM metrics",
            "SELECT 1 AS into",
            "EXPLAIN SELECT * FROM t",
            "SHOW search_path",
            "VALUES (1, 'a'), (2, 'b')",
            "TABLE users",
            "SELECT café FROM menü",
            "SELECT 1; SELECT 2;",
        ] {
            let assessment = classify_sql_risk(sql);
            assert_eq!(assessment.risk, SqlRiskLevel::Safe, "{sql}: {assessment:?}");
            assert!(!assessment.requires_confirmation, "{sql}");
        }
    }

    #[test]
    fn side_effecting_functions_require_confirmation() {
        for (sql, name) in [
            ("SELECT pg_terminate_backend(123)", "pg_terminate_backend"),
            ("SELECT pg_cancel_backend(123)", "pg_cancel_backend"),
            ("SELECT pg_reload_conf()", "pg_reload_conf"),
            ("SELECT pg_rotate_logfile()", "pg_rotate_logfile"),
            ("SELECT set_config('search_path', 'x', false)", "set_config"),
            ("SELECT nextval('s')", "nextval"),
            ("SELECT setval('s', 1)", "setval"),
            ("SELECT dblink_exec('db', 'DELETE FROM t')", "dblink_exec"),
            (
                "SELECT * FROM dblink('db', 'SELECT 1') AS t(n int)",
                "dblink",
            ),
            ("SELECT lo_unlink(1)", "lo_unlink"),
            ("SELECT lo_import('/etc/passwd')", "lo_import"),
            ("SELECT lo_export(1, '/tmp/x')", "lo_export"),
            ("SELECT lo_from_bytea(0, 'x')", "lo_from_bytea"),
            ("SELECT lowrite(0, 'x')", "lowrite"),
            ("SELECT pg_advisory_lock(1)", "pg_advisory_lock"),
            ("SELECT pg_advisory_xact_lock(1)", "pg_advisory_xact_lock"),
            ("SELECT pg_try_advisory_lock(1)", "pg_try_advisory_lock"),
            ("SELECT pg_advisory_unlock_all()", "pg_advisory_unlock_all"),
            ("SELECT pg_notify('c', 'x')", "pg_notify"),
            ("SELECT pg_switch_wal()", "pg_switch_wal"),
            (
                "SELECT pg_create_restore_point('x')",
                "pg_create_restore_point",
            ),
            (
                "SELECT pg_create_logical_replication_slot('s', 'p')",
                "pg_create_logical_replication_slot",
            ),
            (
                "SELECT pg_drop_replication_slot('s')",
                "pg_drop_replication_slot",
            ),
            (
                "SELECT * FROM pg_logical_slot_get_changes('s', NULL, NULL)",
                "pg_logical_slot_get_changes",
            ),
            ("SELECT pg_stat_reset()", "pg_stat_reset"),
            ("SELECT pg_promote()", "pg_promote"),
            ("SELECT pg_file_write('f', 'x', false)", "pg_file_write"),
            ("SELECT pg_catalog.nextval('s')", "nextval"),
            ("SELECT \"nextval\"('s')", "nextval"),
            ("SELECT NEXTVAL ( 's' )", "nextval"),
            (
                "SELECT id FROM t WHERE pg_terminate_backend(pid)",
                "pg_terminate_backend",
            ),
            (
                "WITH x AS (SELECT setval('s', 9)) SELECT * FROM x",
                "setval",
            ),
            ("VALUES (nextval('s'))", "nextval"),
        ] {
            let assessment = classify_sql_risk(sql);
            assert!(assessment.requires_confirmation, "{sql}");
            assert!(assessment.is_mutation, "{sql}");
            let statement = &assessment.statements[0];
            assert_eq!(statement.operation, SqlOperation::Function, "{sql}");
            assert_eq!(statement.risk, SqlRiskLevel::Mutation, "{sql}");
            assert!(
                statement.reason.as_deref().unwrap_or("").contains(name),
                "{sql}: {:?}",
                statement.reason
            );
        }
    }

    #[test]
    fn select_into_creates_a_table() {
        for sql in [
            "SELECT * INTO backup FROM users",
            "select id into temp t2 from t",
            "WITH x AS (SELECT 1 AS n) SELECT n INTO copy FROM x",
        ] {
            let assessment = classify_sql_risk(sql);
            let statement = &assessment.statements[0];
            assert!(assessment.requires_confirmation, "{sql}");
            assert!(assessment.is_mutation, "{sql}");
            assert_eq!(statement.operation, SqlOperation::Create, "{sql}");
            assert_eq!(statement.risk, SqlRiskLevel::Destructive, "{sql}");
        }
    }

    #[test]
    fn empty_sql_needs_confirmation() {
        let assessment = classify_sql_risk("   ");
        assert!(assessment.requires_confirmation);
        assert_eq!(assessment.risk, SqlRiskLevel::Unknown);
        assert!(assessment.statements.is_empty());
    }
}
