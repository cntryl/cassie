//! Literal family controls for the finite EXISTS RETURNING acceptance boundary.
use crate::support_relational_qualification::{fixture, Fixture};
use crate::support_returning_phases::column;
use cassie::executor::QueryResult;
use cassie::types::Value;

pub fn seeded() -> Fixture {
    let fixture = fixture();
    for statement in [
        "CREATE TABLE family_target(id BIGINT)",
        "CREATE TABLE family_control(id BIGINT)",
        "CREATE TABLE family_inner(id BIGINT)",
        "INSERT INTO family_inner VALUES(1)",
    ] {
        sql(&fixture, statement);
    }
    fixture
}

pub fn sql(fixture: &Fixture, statement: &str) -> QueryResult {
    fixture
        .cassie
        .execute_sql(&fixture.session, statement, vec![])
        .unwrap_or_else(|error| panic!("{statement}: {error}"))
}

pub fn assert_result(
    mut actual: QueryResult,
    command: &str,
    columns: &[(&str, bool)],
    expected: &[Vec<Value>],
) {
    actual.rows().sort_by_key(|row| match row.first() {
        Some(Value::Int64(id)) => *id,
        other => panic!("literal BIGINT id: {other:?}"),
    });
    assert_eq!(actual.command(), command);
    assert_eq!(
        actual.columns(),
        columns
            .iter()
            .map(|(name, boolean)| column(name, *boolean))
            .collect::<Vec<_>>(),
        "every literal ColumnMeta field"
    );
    assert_eq!(actual.rows().as_slice(), expected, "exact returned multiset");
}

pub fn assert_fresh_rows(
    fixture: &Fixture,
    table: &str,
    columns: &[(&str, bool)],
    expected: &[Vec<Value>],
) {
    let names = columns.iter().map(|(name, _)| *name).collect::<Vec<_>>();
    let observer = fixture.cassie.create_session("family-observer", None);
    let actual = fixture
        .cassie
        .execute_sql(
            &observer,
            &format!("SELECT {} FROM {table} ORDER BY id", names.join(",")),
            vec![],
        )
        .expect("independent committed family readback");
    assert_result(actual, "SELECT", columns, expected);
}

/// Literal typed IDs, independent of query output and descriptors.
pub fn ids(values: &[i64]) -> Vec<Vec<Value>> {
    values.iter().map(|id| vec![Value::Int64(*id)]).collect()
}

pub fn inserted_rows() -> Vec<Vec<Value>> {
    vec![
        vec![
            Value::Int64(1),
            Value::Bool(true),
            Value::Bool(true),
            Value::Bool(false),
            Value::Bool(true),
        ],
        vec![
            Value::Int64(2),
            Value::Bool(true),
            Value::Bool(false),
            Value::Bool(false),
            Value::Bool(true),
        ],
    ]
}

pub fn matched_rows(values: [i64; 2]) -> Vec<Vec<Value>> {
    vec![
        vec![
            Value::Int64(values[0]),
            Value::Bool(true),
            Value::Bool(true),
            Value::Bool(true),
        ],
        vec![
            Value::Int64(values[1]),
            Value::Bool(true),
            Value::Bool(false),
            Value::Bool(true),
        ],
    ]
}
