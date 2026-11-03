//! Live PostgreSQL tests. Run with:
//! `TIDE_TEST_DATABASE_URL=postgres://user@127.0.0.1:5432/db cargo test -p tide-db --test live_postgres`
//!
//! Database tests are `#[ignore]`d by default; run them with `-- --include-ignored`.
//! TLS also needs `TIDE_TEST_DATABASE_TLS_URL` and `TIDE_TEST_DATABASE_CA`.

#[path = "support/sql_corpus.rs"]
mod sql_corpus;

use serde_json::Value;
use tide_db::{
    ConnectionConfig, DbError, PostgresSession, QueryHistory, QueryStatus, classify_sql_risk,
    parse_postgres_url,
};

// These tests create/drop objects in the catalog enumerated by the schema
// browser. Keep fixture DDL isolated so another test cannot remove an OID
// while pg_get_indexdef/pg_get_triggerdef is resolving it.
static LIVE_FIXTURES: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

fn live_config(read_only: bool, query_timeout: Option<u64>) -> (ConnectionConfig, Option<String>) {
    let url = std::env::var("TIDE_TEST_DATABASE_URL").expect("TIDE_TEST_DATABASE_URL");
    let (mut config, password) = parse_postgres_url(&url).expect("valid TIDE_TEST_DATABASE_URL");
    config.read_only = read_only;
    config.query_timeout = query_timeout;
    config.name = "live-test".to_owned();
    (config, password)
}

#[tokio::test]
#[ignore = "needs TIDE_TEST_DATABASE_URL (live PostgreSQL)"]
async fn live_queries_are_typed_and_read_only_is_enforced() {
    let _fixture = LIVE_FIXTURES.lock().await;
    let (config, password) = live_config(true, None);
    let session = PostgresSession::connect(&config, password.as_deref())
        .await
        .unwrap();
    let result = session
        .execute("SELECT 1 AS n, true AS ok, 'hi' AS label", false)
        .await
        .unwrap();
    assert_eq!(result.row_count, 1);
    assert_eq!(result.rows[0]["n"], Value::from(1));
    assert_eq!(result.rows[0]["ok"], Value::Bool(true));
    assert_eq!(result.rows[0]["label"], Value::String("hi".to_owned()));
    assert!(result.risk.as_ref().unwrap().risk == tide_db::SqlRiskLevel::Safe);

    assert!(matches!(
        session
            .execute("CREATE TEMP TABLE nope (id int)", false)
            .await,
        Err(DbError::ReadOnly(_))
    ));
    assert!(matches!(
        session
            .execute("SET default_transaction_read_only = off", false)
            .await,
        Err(DbError::ReadOnly(_)) | Err(DbError::ConfirmationRequired(_))
    ));
}

#[tokio::test]
#[ignore = "needs TIDE_TEST_DATABASE_URL (live PostgreSQL)"]
async fn live_mutations_require_confirmation_and_apply() {
    let _fixture = LIVE_FIXTURES.lock().await;
    let (config, password) = live_config(false, None);
    let session = PostgresSession::connect(&config, password.as_deref())
        .await
        .unwrap();
    session
        .execute(
            "DROP TABLE IF EXISTS tide_live_test; CREATE TABLE tide_live_test (id serial primary key, name text, owner text)",
            true,
        )
        .await
        .unwrap();
    // Plain inserts are mutations but do not require confirmation; broad
    // updates/deletes and destructive DDL do.
    assert!(matches!(
        session
            .execute("UPDATE tide_live_test SET name = 'b'", false)
            .await,
        Err(DbError::ConfirmationRequired(_))
    ));
    let inserted = session
        .execute(
            "INSERT INTO tide_live_test (name, owner) VALUES ('a', 'tide') RETURNING id",
            true,
        )
        .await
        .unwrap();
    assert_eq!(inserted.row_count, 1);
    let updated = session
        .execute(
            "UPDATE tide_live_test SET name = 'b' WHERE owner = 'tide'",
            true,
        )
        .await
        .unwrap();
    assert_eq!(updated.affected_rows, Some(1));
    let deleted = session
        .execute("DELETE FROM tide_live_test WHERE owner = 'tide'", true)
        .await
        .unwrap();
    assert_eq!(deleted.affected_rows, Some(1));
    session
        .execute("DROP TABLE tide_live_test", true)
        .await
        .unwrap();
}

#[tokio::test]
#[ignore = "needs TIDE_TEST_DATABASE_URL (live PostgreSQL)"]
async fn live_timeouts_and_cancellation_are_reported() {
    let _fixture = LIVE_FIXTURES.lock().await;
    let (config, password) = live_config(true, Some(1_000));
    let session = PostgresSession::connect(&config, password.as_deref())
        .await
        .unwrap();
    let started = std::time::Instant::now();
    assert!(matches!(
        session.execute("SELECT pg_sleep(5)", false).await,
        Err(DbError::Timeout(_))
    ));
    assert!(started.elapsed() < std::time::Duration::from_secs(4));
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    let result = session.execute("SELECT 2 AS n", false).await.unwrap();
    assert_eq!(result.rows[0]["n"], Value::from(2));
}

#[tokio::test]
#[ignore = "needs TIDE_TEST_DATABASE_URL (live PostgreSQL)"]
async fn live_schema_introspection_covers_the_browser() {
    let _fixture = LIVE_FIXTURES.lock().await;
    let (config, password) = live_config(false, None);
    let session = PostgresSession::connect(&config, password.as_deref())
        .await
        .unwrap();
    session
        .execute("DROP SCHEMA IF EXISTS tide_schema_test CASCADE", true)
        .await
        .unwrap();
    session
        .execute(
            "CREATE SCHEMA tide_schema_test; \
             CREATE TABLE tide_schema_test.users (id serial primary key, email text not null, team_id int); \
             CREATE INDEX users_email_idx ON tide_schema_test.users (email); \
             CREATE VIEW tide_schema_test.user_emails AS SELECT email FROM tide_schema_test.users",
            true,
        )
        .await
        .unwrap();

    let schemas = session.schemas().await.unwrap();
    assert!(
        schemas
            .iter()
            .any(|schema| schema.name == "tide_schema_test" && !schema.is_system)
    );
    let tables = session.tables("tide_schema_test").await.unwrap();
    assert!(tables.iter().any(|table| table.name == "users"));
    assert!(tables.iter().any(|table| table.name == "user_emails"));

    let columns = session.columns("tide_schema_test", "users").await.unwrap();
    assert!(
        columns
            .iter()
            .any(|column| column.name == "id" && column.is_primary_key)
    );
    assert!(
        columns
            .iter()
            .any(|column| column.name == "email" && !column.nullable)
    );
    let primary = session
        .primary_key("tide_schema_test", "users")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(primary.columns, vec!["id"]);
    let indexes = session.indexes("tide_schema_test", "users").await.unwrap();
    assert!(
        indexes
            .iter()
            .any(|index| index.name == "users_email_idx" && index.columns == vec!["email"])
    );
    assert!(indexes.iter().any(|index| index.is_primary));
    let ddl = session.ddl("tide_schema_test", "users").await.unwrap();
    assert!(ddl.contains("CREATE TABLE"));
    assert!(ddl.contains("PRIMARY KEY"));
    let view_ddl = session
        .ddl("tide_schema_test", "user_emails")
        .await
        .unwrap();
    assert!(view_ddl.contains("CREATE VIEW"));

    session.execute("CREATE FUNCTION tide_schema_test.touch_row() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN RETURN NEW; END $$; CREATE TRIGGER fixture_touch BEFORE UPDATE ON tide_schema_test.users FOR EACH ROW EXECUTE FUNCTION tide_schema_test.touch_row(); CREATE POLICY fixture_policy ON tide_schema_test.users USING (true)",true).await.unwrap();
    let catalog = session.catalog().await.unwrap();
    for kind in [
        "schema", "table", "view", "function", "index", "trigger", "policy",
    ] {
        assert!(
            catalog
                .iter()
                .any(|i| i.schema == "tide_schema_test" && i.kind == kind),
            "missing {kind}"
        );
    }
    let mut read_config = config.clone();
    read_config.read_only = true;
    let readonly = PostgresSession::connect(&read_config, password.as_deref())
        .await
        .unwrap();
    assert!(!readonly.catalog().await.unwrap().is_empty());

    session
        .execute("DROP SCHEMA tide_schema_test CASCADE", true)
        .await
        .unwrap();
}

#[test]
fn live_risk_classification_matches_the_safety_contract() {
    assert!(classify_sql_risk("DROP TABLE users").requires_confirmation);
    assert_eq!(
        classify_sql_risk("SELECT 1").risk,
        tide_db::SqlRiskLevel::Safe
    );
}

const DIFFERENTIAL_SETUP: &str = r#"
DROP SCHEMA IF EXISTS tide_diff CASCADE;
DROP SCHEMA IF EXISTS tide_diff_meta CASCADE;
CREATE SCHEMA tide_diff;
CREATE SCHEMA tide_diff_meta;
CREATE TABLE tide_diff.items (id int PRIMARY KEY, name text);
INSERT INTO tide_diff.items VALUES (1, 'a'), (2, 'b_c');
CREATE SEQUENCE tide_diff.counter;
CREATE FUNCTION tide_diff_meta.fingerprint() RETURNS text LANGUAGE plpgsql AS $fn$
DECLARE
    item_rows text := 'missing';
    counter text := 'missing';
BEGIN
    IF to_regclass('tide_diff.items') IS NOT NULL THEN
        EXECUTE $q$SELECT count(*) || ':' || coalesce(string_agg(id || '=' || coalesce(name, ''), ',' ORDER BY id), '') FROM tide_diff.items$q$ INTO item_rows;
    END IF;
    IF to_regclass('tide_diff.counter') IS NOT NULL THEN
        EXECUTE $q$SELECT last_value || ':' || is_called FROM tide_diff.counter$q$ INTO counter;
    END IF;
    RETURN concat_ws('|',
        'rows=' || item_rows,
        'relations=' || coalesce((SELECT string_agg(relname, ',' ORDER BY relname) FROM pg_class WHERE relnamespace = 'tide_diff'::regnamespace), ''),
        'counter=' || counter,
        'probe=' || coalesce(current_setting('tide.diff_probe', true), ''),
        'locks=' || (SELECT count(*) FROM pg_locks WHERE locktype = 'advisory' AND pid = pg_backend_pid()));
END
$fn$;
"#;

struct Observation {
    statements: usize,
    failed: bool,
    before: std::collections::BTreeMap<String, String>,
    after: std::collections::BTreeMap<String, String>,
}

async fn fingerprint(
    client: &tokio_postgres::Client,
) -> std::collections::BTreeMap<String, String> {
    let row = client
        .query_one("SELECT tide_diff_meta.fingerprint()", &[])
        .await
        .unwrap();
    row.get::<_, String>(0)
        .split('|')
        .filter_map(|part| part.split_once('='))
        .map(|(key, value)| (key.to_owned(), value.to_owned()))
        .collect()
}

/// Runs `sql` inside a transaction that is always rolled back, counting the
/// statements PostgreSQL completed and fingerprinting the sentinels. Failed
/// submissions are fingerprinted after rollback, which keeps only the
/// non-transactional effects that would survive the implicit rollback.
async fn observe(client: &tokio_postgres::Client, sql: &str) -> Observation {
    use futures_util::StreamExt as _;
    client.batch_execute("BEGIN").await.unwrap();
    let before = fingerprint(client).await;
    let mut statements = 0;
    let mut failed = false;
    match client.simple_query_raw(sql).await {
        Ok(stream) => {
            let mut stream = Box::pin(stream);
            while let Some(message) = stream.next().await {
                match message {
                    Ok(tokio_postgres::SimpleQueryMessage::CommandComplete(_)) => statements += 1,
                    Ok(_) => {}
                    Err(_) => {
                        failed = true;
                        break;
                    }
                }
            }
        }
        Err(_) => failed = true,
    }
    let after = if failed {
        None
    } else {
        Some(fingerprint(client).await)
    };
    client.batch_execute("ROLLBACK").await.unwrap();
    let after = match after {
        Some(after) => after,
        None => fingerprint(client).await,
    };
    client
        .batch_execute("SELECT pg_advisory_unlock_all()")
        .await
        .unwrap();
    Observation {
        statements,
        failed,
        before,
        after,
    }
}

fn assert_agrees(sql: &str, assessment: &tide_db::SqlRiskAssessment, observed: &Observation) {
    use tide_db::SqlOperation::{Create, Delete, DestructiveDdl, Function, Insert, Update};
    if !observed.failed {
        assert_eq!(
            assessment.statements.len(),
            observed.statements,
            "{sql:?}: classifier and PostgreSQL disagree on the statement count"
        );
    }
    let changed = |key: &str| observed.before.get(key) != observed.after.get(key);
    let has = |operation| assessment.operations.contains(&operation);
    if !["rows", "relations", "counter", "probe", "locks"]
        .into_iter()
        .any(changed)
    {
        return;
    }
    assert!(
        assessment.is_mutation,
        "{sql:?}: PostgreSQL mutated sentinels but the classifier saw no mutation"
    );
    assert!(
        assessment.requires_confirmation
            || assessment
                .operations
                .iter()
                .all(|operation| matches!(operation, Insert | Update | Delete)),
        "{sql:?}: PostgreSQL mutated sentinels without confirmation"
    );
    let relations = |fingerprint: &std::collections::BTreeMap<String, String>| {
        fingerprint["relations"]
            .split(',')
            .map(str::to_owned)
            .collect::<std::collections::BTreeSet<_>>()
    };
    let (before, after) = (relations(&observed.before), relations(&observed.after));
    if before.difference(&after).next().is_some() {
        assert!(has(DestructiveDdl), "{sql:?}: dropped a relation");
    }
    if after.difference(&before).next().is_some() {
        assert!(has(Create), "{sql:?}: created a relation");
    }
    if changed("rows") && after.contains("items") {
        let count = |fingerprint: &std::collections::BTreeMap<String, String>| {
            fingerprint["rows"]
                .split(':')
                .next()
                .unwrap()
                .parse::<u64>()
                .unwrap()
        };
        let (rows_before, rows_after) = (count(&observed.before), count(&observed.after));
        let explained = if rows_after < rows_before {
            has(Delete) || has(DestructiveDdl)
        } else if rows_after > rows_before {
            has(Insert)
        } else {
            has(Update)
        };
        assert!(explained, "{sql:?}: row changes are not named");
    }
    if ["counter", "probe", "locks"].into_iter().any(changed) {
        assert!(
            has(Function),
            "{sql:?}: function side effects are not named"
        );
    }
}

#[tokio::test]
#[ignore = "needs TIDE_TEST_DATABASE_URL (live PostgreSQL)"]
async fn live_classifier_agrees_with_postgres_on_the_corpus() {
    let _fixture = LIVE_FIXTURES.lock().await;
    let (config, password) = live_config(false, None);
    let mut pg = tokio_postgres::Config::new();
    pg.host(&config.host)
        .port(config.port)
        .dbname(&config.database)
        .user(&config.username);
    if let Some(password) = &password {
        pg.password(password);
    }
    let (client, connection) = pg
        .connect(tokio_postgres::NoTls)
        .await
        .expect("the differential test needs a plaintext TIDE_TEST_DATABASE_URL");
    tokio::spawn(connection);
    client.batch_execute(DIFFERENTIAL_SETUP).await.unwrap();
    for case in sql_corpus::ADVERSARIAL {
        let assessment = sql_corpus::assert_classified(case);
        assert_agrees(case.sql, &assessment, &observe(&client, case.sql).await);
    }
    for sql in sql_corpus::BENIGN {
        let assessment = sql_corpus::assert_benign(sql);
        let observed = observe(&client, sql).await;
        assert!(!observed.failed, "{sql:?} should run cleanly");
        assert_eq!(observed.before, observed.after, "{sql:?} mutated sentinels");
        assert_agrees(sql, &assessment, &observed);
    }
    client
        .batch_execute("DROP SCHEMA tide_diff CASCADE; DROP SCHEMA tide_diff_meta CASCADE")
        .await
        .unwrap();
}

#[test]
fn live_history_never_stores_passwords() {
    let dir = tempfile::tempdir().unwrap();
    let mut history = QueryHistory::new(dir.path());
    history
        .add_entry(
            "live",
            "Live",
            "postgres://user:secret@host/db",
            1,
            0,
            QueryStatus::Error,
            Some("password=hunter2"),
        )
        .unwrap();
    let entry = &history.entries().unwrap()[0];
    assert!(!entry.sql.contains("secret"));
    assert!(!entry.error.as_deref().unwrap().contains("hunter2"));
}

#[tokio::test]
#[ignore = "needs TIDE_TEST_DATABASE_TLS_URL and TIDE_TEST_DATABASE_CA (live PostgreSQL)"]
async fn live_tls_connections_accept_a_custom_ca() {
    let _fixture = LIVE_FIXTURES.lock().await;
    let url = std::env::var("TIDE_TEST_DATABASE_TLS_URL").expect("TIDE_TEST_DATABASE_TLS_URL");
    let (mut config, password) = parse_postgres_url(&url).unwrap();
    let ca = std::env::var("TIDE_TEST_DATABASE_CA").expect("TIDE_TEST_DATABASE_CA");
    config.ssl = Some(serde_json::json!({ "ca": ca, "rejectUnauthorized": true }));
    config.read_only = true;
    let session = PostgresSession::connect(&config, password.as_deref())
        .await
        .unwrap();
    let result = session.execute("SELECT 3 AS n", false).await.unwrap();
    assert_eq!(result.rows[0]["n"], Value::from(3));
}

#[cfg(unix)]
#[tokio::test]
#[ignore = "needs TIDE_TEST_DATABASE_URL (live PostgreSQL)"]
async fn live_project_connections_can_take_their_password_from_a_password_file() {
    use std::os::unix::fs::PermissionsExt;
    let (mut config, password) = live_config(true, None);
    let Some(password) = password else {
        return; // The server needs no password, so there is nothing to look up.
    };
    config.scope = tide_db::ConnectionScope::Project;
    let dir = tempfile::tempdir().unwrap();
    let pgpass = dir.path().join("pgpass");
    let escaped = password.replace('\\', "\\\\").replace(':', "\\:");
    std::fs::write(
        &pgpass,
        format!(
            "{}:{}:*:{}:{escaped}\n",
            config.host, config.port, config.username
        ),
    )
    .unwrap();
    std::fs::set_permissions(&pgpass, std::fs::Permissions::from_mode(0o600)).unwrap();
    assert!(
        PostgresSession::connect(&config, None).await.is_err(),
        "the server wants a password"
    );
    let found = tide_db::pgpass_password(&pgpass, &config);
    assert_eq!(found.as_deref(), Some(password.as_str()));
    let session = PostgresSession::connect(&config, found.as_deref())
        .await
        .unwrap();
    let result = session.execute("SELECT 4 AS n", false).await.unwrap();
    assert_eq!(result.rows[0]["n"], Value::from(4));
}

#[tokio::test]
#[ignore = "needs TIDE_TEST_DATABASE_TLS_URL and TIDE_TEST_DATABASE_CA (live PostgreSQL)"]
async fn live_tls_modes_check_certificates_as_libpq_does() {
    let _fixture = LIVE_FIXTURES.lock().await;
    let url = std::env::var("TIDE_TEST_DATABASE_TLS_URL").expect("TIDE_TEST_DATABASE_TLS_URL");
    let ca = std::env::var("TIDE_TEST_DATABASE_CA").expect("TIDE_TEST_DATABASE_CA");
    let (base, password) = parse_postgres_url(&url).unwrap();
    let connect = |ssl: Value| {
        let mut config = base.clone();
        config.ssl = Some(ssl);
        let password = password.clone();
        async move { PostgresSession::connect(&config, password.as_deref()).await }
    };
    // The server's certificate comes from a private CA that the system does not trust.
    for mode in ["prefer", "require"] {
        let session = connect(serde_json::json!(mode))
            .await
            .unwrap_or_else(|error| panic!("{mode}: {error}"));
        let result = session
            .execute(
                "SELECT ssl FROM pg_stat_ssl WHERE pid = pg_backend_pid()",
                false,
            )
            .await
            .unwrap();
        assert_eq!(result.rows[0]["ssl"], Value::Bool(true), "{mode} encrypts");
    }
    for mode in ["verify-ca", "verify-full"] {
        let error = connect(serde_json::json!(mode)).await.unwrap_err();
        // The failure names the cause and the way out.
        assert!(
            error.to_string().contains("UnknownIssuer")
                && error.to_string().contains("sslmode=require"),
            "{mode}: {error}"
        );
        connect(serde_json::json!({ "mode": mode, "ca": ca }))
            .await
            .unwrap_or_else(|error| panic!("{mode} with the CA: {error}"));
    }
}

#[tokio::test]
#[ignore = "needs TIDE_TEST_DATABASE_URL (live PostgreSQL)"]
async fn live_read_only_survives_session_functions_and_transaction_escapes() {
    let _fixture = LIVE_FIXTURES.lock().await;
    let (config, password) = live_config(true, None);
    let session = PostgresSession::connect(&config, password.as_deref())
        .await
        .unwrap();
    // set_config is a known side effect, so the classifier refuses it before
    // the server sees it.
    assert!(matches!(
        session
            .execute(
                "SELECT set_config('default_transaction_read_only', 'off', false)",
                false,
            )
            .await,
        Err(DbError::ReadOnly(_))
    ));
    assert_eq!(
        session
            .execute("SHOW transaction_read_only", false)
            .await
            .unwrap()
            .rows[0]["transaction_read_only"],
        Value::from("on")
    );
    for sql in [
        "COMMIT; SELECT 1",
        "BEGIN READ WRITE",
        "SELECT 1; COMMIT",
        "SELECT nextval('tide_readonly_probe')",
    ] {
        assert!(session.execute(sql, true).await.is_err(), "{sql}");
    }
    assert_eq!(
        session.execute("SELECT 4 AS n", false).await.unwrap().rows[0]["n"],
        Value::from(4)
    );
}

#[tokio::test]
#[ignore = "needs TIDE_TEST_DATABASE_URL (live PostgreSQL)"]
async fn live_restricted_role_cannot_mutate_through_safe_looking_functions() {
    let _fixture = LIVE_FIXTURES.lock().await;
    let (mut config, password) = live_config(false, None);
    let admin = PostgresSession::connect(&config, password.as_deref())
        .await
        .unwrap();
    admin.execute("DROP SCHEMA IF EXISTS tide_ro_review CASCADE; CREATE SCHEMA tide_ro_review; CREATE TABLE tide_ro_review.items (id int); CREATE SEQUENCE tide_ro_review.counter; DO $$ BEGIN IF NOT EXISTS (SELECT FROM pg_roles WHERE rolname='tide_ro_review') THEN CREATE ROLE tide_ro_review LOGIN; END IF; END $$; ALTER ROLE tide_ro_review PASSWORD 'tide-disposable-test'; GRANT USAGE ON SCHEMA tide_ro_review TO tide_ro_review; GRANT SELECT ON tide_ro_review.items TO tide_ro_review; GRANT USAGE ON SEQUENCE tide_ro_review.counter TO tide_ro_review; CREATE FUNCTION tide_ro_review.mutate() RETURNS int LANGUAGE plpgsql SECURITY DEFINER AS $$ BEGIN INSERT INTO tide_ro_review.items VALUES (1); RETURN 1; END $$; GRANT EXECUTE ON FUNCTION tide_ro_review.mutate() TO tide_ro_review",true).await.unwrap();
    config.username = "tide_ro_review".into();
    config.read_only = true;
    let session = PostgresSession::connect(&config, Some("tide-disposable-test"))
        .await
        .unwrap();
    assert!(session.matches_config(&config));
    let mut changed = config.clone();
    changed.database.push_str("_other");
    assert!(!session.matches_config(&changed));
    assert!(matches!(
        session
            .execute(
                "SELECT set_config('default_transaction_read_only','off',false)",
                false,
            )
            .await,
        Err(DbError::ReadOnly(_))
    ));
    assert!(
        session
            .execute("SELECT nextval('tide_ro_review.counter')", false)
            .await
            .is_err()
    );
    assert!(
        session
            .execute("SELECT tide_ro_review.mutate()", false)
            .await
            .is_err()
    );
    assert_eq!(
        session
            .execute("SELECT count(*) AS n FROM tide_ro_review.items", false)
            .await
            .unwrap()
            .rows[0]["n"],
        Value::from(0)
    );
    admin
        .execute(
            "DROP SCHEMA tide_ro_review CASCADE; DROP ROLE tide_ro_review",
            true,
        )
        .await
        .unwrap();
}

#[tokio::test]
#[ignore = "needs TIDE_TEST_DATABASE_URL (live PostgreSQL)"]
async fn live_row_edits_use_primary_keys_parameters_and_atomic_stale_guards() {
    let _fixture = LIVE_FIXTURES.lock().await;
    let (mut config, password) = live_config(false, None);
    config.id = "row-fixture".into();
    let session = PostgresSession::connect(&config, password.as_deref())
        .await
        .unwrap();
    let table = "\"tide row\"\"fixture\"";
    session.execute(&format!("DROP TABLE IF EXISTS {table};CREATE TABLE {table} (id bigint PRIMARY KEY, \"hostile\"\"name\" text, payload jsonb, amount numeric);INSERT INTO {table} VALUES (9007199254740993,'original','{{\"a\":1}}',1.20)"),true).await.unwrap();
    let result = session
        .execute(&format!("SELECT * FROM {table}"), false)
        .await
        .unwrap();
    let target = result
        .row_target
        .clone()
        .expect("native table ownership and primary key");
    assert_eq!(target.primary_key, vec!["id"]);
    assert_eq!(target.table, "tide row\"fixture");
    let hostile = "'; DROP TABLE public.anything; -- 👩‍💻";
    let request = tide_db::RowMutation {
        connection_id: config.id.clone(),
        query_id: result.query_id,
        target: target.clone(),
        original: result.rows[0].clone(),
        changes: std::collections::BTreeMap::from([
            ("hostile\"name".into(), Value::String(hostile.into())),
            ("payload".into(), Value::Null),
        ]),
        delete: false,
    };
    let updated = session.mutate_row(&request).await.unwrap().unwrap();
    assert_eq!(updated["hostile\"name"], hostile);
    assert_eq!(updated["payload"], Value::Null);
    assert!(
        session
            .mutate_row(&request)
            .await
            .unwrap_err()
            .to_string()
            .contains("Row changed")
    );
    let current = session
        .execute(&format!("SELECT * FROM {table}"), false)
        .await
        .unwrap();
    assert_eq!(current.row_count, 1);
    assert_eq!(current.rows[0]["hostile\"name"], hostile);
    assert!(
        session
            .execute(&format!("SELECT \"hostile\"\"name\" FROM {table}"), false)
            .await
            .unwrap()
            .row_target
            .is_none()
    );
    assert!(
        session
            .execute(&format!("SELECT id AS wrong_name FROM {table}"), false)
            .await
            .unwrap()
            .row_target
            .is_none()
    );
    let mut invalid = request.clone();
    invalid.original = current.rows[0].clone();
    invalid.changes = std::collections::BTreeMap::from([(
        "amount".into(),
        Value::String("invalid numeric".into()),
    )]);
    assert!(session.mutate_row(&invalid).await.is_err());
    assert_eq!(
        session
            .execute(&format!("SELECT amount FROM {table}"), false)
            .await
            .unwrap()
            .rows[0]["amount"],
        "1.20"
    );
    let mut deletion = request.clone();
    deletion.original = current.rows[0].clone();
    deletion.delete = true;
    deletion.changes.clear();
    assert!(session.mutate_row(&deletion).await.unwrap().is_none());
    assert!(session.mutate_row(&deletion).await.is_err());
    config.read_only = true;
    let readonly = PostgresSession::connect(&config, password.as_deref())
        .await
        .unwrap();
    assert!(matches!(
        readonly.mutate_row(&deletion).await,
        Err(DbError::ReadOnly(_))
    ));
    session
        .execute(&format!("DROP TABLE {table}"), true)
        .await
        .unwrap();
}

#[tokio::test]
#[ignore = "needs TIDE_TEST_DATABASE_URL (live PostgreSQL)"]
async fn add_row_preserves_defaults_nulls_generated_columns_and_quoted_values() {
    let _fixture = LIVE_FIXTURES.lock().await;
    let (mut config, password) = live_config(false, None);
    config.id = "insert-fixture".into();
    let session = PostgresSession::connect(&config, password.as_deref())
        .await
        .unwrap();
    session.execute("DROP TABLE IF EXISTS public.tide_insert_fixture; CREATE TABLE public.tide_insert_fixture (id bigint GENERATED ALWAYS AS IDENTITY PRIMARY KEY, name text NOT NULL UNIQUE, note text DEFAULT 'server default', amount numeric DEFAULT 7, derived text GENERATED ALWAYS AS (name || '!') STORED)", true).await.unwrap();
    let result = session
        .execute("SELECT * FROM public.tide_insert_fixture", false)
        .await
        .unwrap();
    let target = session
        .insert_target(result.row_source.as_ref().unwrap())
        .await
        .unwrap();
    assert!(
        target
            .columns
            .iter()
            .find(|c| c.name == "id")
            .unwrap()
            .generated
    );
    let hostile = "O'Reilly; DROP TABLE tide_insert_fixture; -- 界";
    let mut request = tide_db::RowInsert {
        connection_id: config.id.clone(),
        query_id: result.query_id,
        target,
        values: std::collections::BTreeMap::from([
            ("name".into(), Value::String(hostile.into())),
            ("amount".into(), Value::Null),
        ]),
    };
    let row = session.insert_row(&request).await.unwrap();
    assert_eq!(row["name"], hostile);
    assert_eq!(row["note"], "server default");
    assert_eq!(row["amount"], Value::Null);
    assert_eq!(row["derived"], format!("{hostile}!"));
    assert!(session.insert_row(&request).await.is_err());
    assert_eq!(
        session
            .execute("SELECT * FROM public.tide_insert_fixture", false)
            .await
            .unwrap()
            .row_count,
        1
    );
    request.values.insert("id".into(), 9.into());
    assert!(session.insert_row(&request).await.is_err());
    request.values.remove("id");
    request.values.insert("name".into(), "second".into());
    session
        .execute(
            "ALTER TABLE public.tide_insert_fixture ADD COLUMN changed int",
            true,
        )
        .await
        .unwrap();
    assert!(
        session
            .insert_row(&request)
            .await
            .unwrap_err()
            .to_string()
            .contains("definition changed")
    );
    config.read_only = true;
    let readonly = PostgresSession::connect(&config, password.as_deref())
        .await
        .unwrap();
    assert!(matches!(
        readonly.insert_row(&request).await,
        Err(DbError::ReadOnly(_))
    ));
    session
        .execute(
            "CREATE TEMP TABLE no_primary_key (value text DEFAULT 'default row', note text)",
            true,
        )
        .await
        .unwrap();
    let empty = session
        .execute("SELECT * FROM no_primary_key", false)
        .await
        .unwrap();
    assert!(empty.row_target.is_none());
    let target = session
        .insert_target(empty.row_source.as_ref().unwrap())
        .await
        .unwrap();
    let row = session
        .insert_row(&tide_db::RowInsert {
            connection_id: config.id.clone(),
            query_id: empty.query_id,
            target,
            values: Default::default(),
        })
        .await
        .unwrap();
    assert_eq!(row["value"], "default row");
    assert_eq!(row["note"], Value::Null);
    session
        .execute("DROP TABLE public.tide_insert_fixture", true)
        .await
        .unwrap();
}
