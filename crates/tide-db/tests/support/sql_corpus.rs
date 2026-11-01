//! SQL classifier corpus shared by the offline expectations and the live
//! PostgreSQL differential test. Statements target the `tide_diff` sentinel
//! schema so the live run can observe what PostgreSQL actually did. Entries
//! must not contain transaction control: the live run wraps each in
//! `BEGIN … ROLLBACK`.

use tide_db::SqlOperation::{Create, Delete, DestructiveDdl, Function, Insert, Read, Update};
use tide_db::{SqlOperation, SqlRiskAssessment, SqlRiskLevel, classify_sql_risk};

pub struct Case {
    pub sql: &'static str,
    pub operations: &'static [SqlOperation],
    pub confirm: bool,
}

const fn case(sql: &'static str, operations: &'static [SqlOperation], confirm: bool) -> Case {
    Case {
        sql,
        operations,
        confirm,
    }
}

pub const ADVERSARIAL: &[Case] = &[
    case(
        "SELECT E'\\''; DROP TABLE tide_diff.items; --'",
        &[Read, DestructiveDdl],
        true,
    ),
    case(
        "SELECT e'\\\\'; DELETE FROM tide_diff.items",
        &[Read, Delete],
        true,
    ),
    case(
        "SELECT 1; --\rDELETE FROM tide_diff.items",
        &[Read, Delete],
        true,
    ),
    case(
        "SELECT 1 /* a /* nested */ b */; UPDATE tide_diff.items SET name = 'x'",
        &[Read, Update],
        true,
    ),
    case(
        "SELECT $é$'$é$; TRUNCATE tide_diff.items; --'",
        &[Read, DestructiveDdl],
        true,
    ),
    case(
        "SELECT $a$ $b$ $a$; INSERT INTO tide_diff.items VALUES (9, 'z')",
        &[Read, Insert],
        true,
    ),
    case(
        "SELECT 'a\\'; DELETE FROM tide_diff.items; --'",
        &[Read, Delete],
        true,
    ),
    case(
        "SELECT U&'\\0027'; DELETE FROM tide_diff.items",
        &[Read, Delete],
        true,
    ),
    case(
        "SELECT \"a\"\"b\" FROM (SELECT 1 AS \"a\"\"b\") s; DELETE FROM tide_diff.items",
        &[Read, Delete],
        true,
    ),
    case(
        "SELECT éE'\\'; DROP TABLE tide_diff.items; --'",
        &[Read, DestructiveDdl],
        true,
    ),
    case(
        "WITH gone AS (DELETE FROM tide_diff.items RETURNING *) SELECT count(*) FROM gone",
        &[Delete],
        true,
    ),
    case(
        "SELECT * INTO tide_diff.copy FROM tide_diff.items",
        &[Create],
        true,
    ),
    case(
        "SELECT pg_catalog.nextval('tide_diff.counter')",
        &[Function],
        true,
    ),
    case("SELECT \"nextval\"('tide_diff.counter')", &[Function], true),
    case("SELECT setval('tide_diff.counter', 42)", &[Function], true),
    case(
        "SELECT set_config('tide.diff_probe', 'changed', false)",
        &[Function],
        true,
    ),
    case("SELECT pg_advisory_lock(4242)", &[Function], true),
    case("SELECT 'unterminated", &[Read], true),
    case("SELECT 1 /* unterminated", &[Read], true),
];

pub const BENIGN: &[&str] = &[
    "SELECT 1",
    "SELECT * FROM tide_diff.items WHERE id = 1",
    "SELECT count(*) FROM tide_diff.items WHERE name = 'it''s'",
    "SELECT E'line\\nbreak', E'it\\'s', e'\\\\'",
    "SELECT name FROM tide_diff.items WHERE name LIKE '%\\_%'",
    "SELECT regexp_replace(name, '\\s+', ' ', 'g') FROM tide_diff.items",
    "SELECT 'C:\\path\\file'",
    "SELECT U&'\\0041', x'1F', b'0101', N'text'",
    "SELECT \"name\" FROM tide_diff.items",
    "SELECT $$ a ' b ; DROP TABLE tide_diff.items $$, $fn$ x $fn$",
    "/* nested /* comment */ */ SELECT 1 -- trailing\r\n",
    "WITH recent AS (SELECT * FROM tide_diff.items) SELECT * FROM recent",
    "SELECT now(), lower(name), current_setting('search_path') FROM tide_diff.items",
    "SELECT last_value FROM tide_diff.counter",
    "SELECT 1 AS into",
    "EXPLAIN SELECT * FROM tide_diff.items",
    "SHOW search_path",
    "VALUES (1, 'a'), (2, 'b')",
    "TABLE tide_diff.items",
    "SELECT 'café' AS menü",
    "SELECT 1; SELECT 2; -- done",
];

pub fn assert_classified(case: &Case) -> SqlRiskAssessment {
    let assessment = classify_sql_risk(case.sql);
    assert_eq!(assessment.operations, case.operations, "{:?}", case.sql);
    assert_eq!(
        assessment.requires_confirmation, case.confirm,
        "{:?}",
        case.sql
    );
    assessment
}

pub fn assert_benign(sql: &str) -> SqlRiskAssessment {
    let assessment = classify_sql_risk(sql);
    assert_eq!(assessment.risk, SqlRiskLevel::Safe, "{sql:?}");
    assert!(!assessment.requires_confirmation, "{sql:?}");
    assessment
}
