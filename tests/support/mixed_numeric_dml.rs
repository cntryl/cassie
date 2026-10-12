//! Production-entry mutation fixtures with literal PostgreSQL result oracles.
use crate::support_sql_fixture::sql_fixture_with_config;
use cassie::app::CassieError;
use cassie::config::{CassieRuntimeConfig, ExecutionResultCacheEnabled};
use cassie::executor::QueryResult;
use cassie::types::Value;

#[path = "mixed_numeric_dml_cases.rs"]
mod cases;
pub use cases::{
    ATOMICITY, DELETE, INSERT_SELECT, INSERT_SELECT_PROJECTION, ON_CONFLICT, ROLLBACK, UPDATE,
    UPDATE_SET,
};

pub struct Case {
    pub id: &'static str,
    pub setup: &'static [&'static str],
    pub before: &'static [&'static str],
    pub sql: &'static str,
    pub expected: Option<&'static ExpectedSnapshot>,
    pub posts: &'static [Post],
    pub recovery: &'static str,
}

pub struct Post {
    pub sql: &'static str,
    pub expected: &'static ExpectedSnapshot,
}

pub struct ExpectedSnapshot {
    pub columns: &'static [(&'static str, i64, i16, i32)],
    pub rows: &'static str,
    pub command: &'static str,
}

#[derive(Debug)]
pub struct Snapshot {
    columns: Vec<(String, i64, i16, i32)>,
    rows: Vec<Vec<Value>>,
    command: String,
}

impl Snapshot {
    fn from_result(result: QueryResult) -> Self {
        let mut rows = result.rows().to_vec();
        // SQL RETURNING has no ORDER BY. The id is a unique, exact INT carrier.
        rows.sort_by_key(|row| match row.first() {
            Some(Value::Int64(id)) => *id,
            _ => panic!("expected an integer row identity"),
        });
        let snapshot = Self {
            columns: result
                .columns()
                .iter()
                .map(|column| {
                    (
                        column.name.clone(),
                        column.type_oid,
                        column.typlen,
                        column.atttypmod,
                    )
                })
                .collect(),
            rows,
            command: result.command().to_string(),
        };
        drop(result);
        snapshot
    }
}

pub struct Observation {
    before: Vec<Result<Snapshot, CassieError>>,
    target: Result<Snapshot, CassieError>,
    posts: Vec<Result<Snapshot, CassieError>>,
    recovery: Result<Snapshot, CassieError>,
}

pub fn capture(cases: &[Case]) -> Vec<Observation> {
    cases.iter().map(capture_case).collect()
}

fn capture_case(case: &Case) -> Observation {
    let mut config = CassieRuntimeConfig::default();
    config.limits.query_timeout_ms = 0;
    config.limits.query_memory_budget_bytes = 64 * 1024 * 1024;
    config.limits.execution_result_cache_enabled = ExecutionResultCacheEnabled::disabled();
    config.limits.parallel_scan_workers = 1;
    let fixture = sql_fixture_with_config("mixed-numeric-dml", case.setup, config);
    let before = case
        .before
        .iter()
        .map(|sql| fixture.execute(sql).map(Snapshot::from_result))
        .collect();
    let target = fixture.execute(case.sql).map(Snapshot::from_result);
    // Observe all poststate/recovery steps even on unexpected target failure.
    // In the transaction control this includes ROLLBACK before retirement.
    let posts = case
        .posts
        .iter()
        .map(|post| fixture.execute(post.sql).map(Snapshot::from_result))
        .collect();
    let recovery = fixture.execute(case.recovery).map(Snapshot::from_result);
    drop(fixture);
    Observation {
        before,
        target,
        posts,
        recovery,
    }
}

pub fn assert_cases(cases: &[Case], observations: Vec<Observation>) {
    assert_eq!(cases.len(), observations.len());
    for (case, observed) in cases.iter().zip(observations) {
        for before in observed.before {
            let before = before.expect("BEGIN must succeed before the target");
            assert_eq!(before.command, "BEGIN", "{}", case.id);
            assert!(before.rows.is_empty(), "{}", case.id);
        }
        match case.expected {
            Some(expected) => assert_snapshot(
                &observed.target.expect("mutation should succeed"),
                expected,
                case.id,
            ),
            None => assert!(observed.target.is_err(), "{}", case.id),
        }
        assert_eq!(case.posts.len(), observed.posts.len(), "{}", case.id);
        for (post, actual) in case.posts.iter().zip(observed.posts) {
            assert_snapshot(&actual.expect("poststate query"), post.expected, case.id);
        }
        let recovery = observed.recovery.expect("SELECT 1 recovery");
        assert_eq!(recovery.rows, vec![vec![Value::Int64(1)]], "{}", case.id);
        assert_eq!(recovery.command, "SELECT", "{}", case.id);
    }
}

fn assert_snapshot(actual: &Snapshot, expected: &ExpectedSnapshot, id: &str) {
    let columns: Vec<_> = expected
        .columns
        .iter()
        .map(|(name, oid, length, modifier)| (name.to_string(), *oid, *length, *modifier))
        .collect();
    let rows: Vec<Vec<Value>> = serde_json::from_str(expected.rows).expect("literal PG rows");
    assert_eq!(actual.columns, columns, "column metadata: {id}");
    assert_eq!(
        actual.rows, rows,
        "rows and original integer carriers: {id}"
    );
    // QueryResult exposes SELECT; pgwire adds its separately qualified row count.
    let command = if expected.command.starts_with("SELECT ") {
        "SELECT"
    } else {
        expected.command
    };
    assert_eq!(actual.command, command, "command tag: {id}");
}
