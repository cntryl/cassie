//! EXISTS depends only on whether its subquery yields a row. PostgreSQL drops
//! the subquery's target list, GROUP BY, DISTINCT, ORDER BY and a positive
//! constant LIMIT before planning it, unless aggregates, HAVING, window
//! functions, set operations or OFFSET make those clauses observable.
use super::support_sql_fixture::{sql_fixture, SqlFixture};
use cassie::app::CassieError;
use cassie::types::Value;

fn zero_fixture(label: &str) -> SqlFixture {
    sql_fixture(
        label,
        &[
            "CREATE TABLE ri (n BIGINT)",
            "INSERT INTO ri VALUES (0)",
            "CREATE TABLE ri_empty (n BIGINT)",
            "CREATE TABLE ro (k BIGINT)",
            "INSERT INTO ro VALUES (0), (1)",
        ],
    )
}

fn single_bool(fixture: &SqlFixture, sql: &str) -> Value {
    let rows = fixture.rows(sql);
    assert_eq!(rows.len(), 1, "one row: {sql}");
    rows[0][0].clone()
}

fn is_division_by_zero(result: &Result<cassie::executor::QueryResult, CassieError>) -> bool {
    matches!(result, Err(CassieError::Execution(message)) if message == "division by zero")
}

#[test]
fn should_return_true_without_evaluating_exists_target_expressions() {
    // Arrange
    let fixture = zero_fixture("exists_target_elision_select");

    // Act
    let selected = single_bool(
        &fixture,
        "SELECT EXISTS(SELECT e.n/e.n FROM ri e) AS selected",
    );
    let negated = single_bool(
        &fixture,
        "SELECT NOT EXISTS(SELECT e.n/e.n FROM ri e) AS selected",
    );
    let empty = single_bool(
        &fixture,
        "SELECT EXISTS(SELECT e.n/e.n FROM ri_empty e) AS selected",
    );
    let constant = single_bool(&fixture, "SELECT EXISTS(SELECT 1/0 FROM ri) AS selected");

    // Assert
    assert_eq!(
        (selected, negated, empty, constant),
        (
            Value::Bool(true),
            Value::Bool(false),
            Value::Bool(false),
            Value::Bool(true)
        )
    );
    assert_eq!(
        fixture.rows("SELECT n FROM ri"),
        vec![vec![Value::Int64(0)]]
    );
}

#[test]
fn should_drop_exists_clauses_that_cannot_change_row_existence() {
    // Arrange
    let fixture = zero_fixture("exists_target_elision_clauses");
    let cases = [
        "SELECT EXISTS(SELECT DISTINCT e.n/e.n FROM ri e) AS selected",
        "SELECT EXISTS(SELECT 1 FROM ri e ORDER BY e.n/e.n) AS selected",
        "SELECT EXISTS(SELECT e.n/e.n FROM ri e GROUP BY e.n/e.n) AS selected",
        "SELECT EXISTS(SELECT e.n/e.n FROM ri e LIMIT 1) AS selected",
        "SELECT EXISTS(SELECT 1 FROM ri GROUP BY 1/0) AS selected",
    ];

    // Act
    let values: Vec<Value> = cases.iter().map(|sql| single_bool(&fixture, sql)).collect();

    // Assert
    assert_eq!(values, vec![Value::Bool(true); cases.len()]);
}

#[test]
fn should_keep_exists_target_expressions_where_they_change_rows() {
    // Arrange
    let fixture = zero_fixture("exists_target_elision_kept");
    let cases = [
        "SELECT EXISTS(SELECT sum(e.n/e.n) FROM ri e) AS selected",
        "SELECT EXISTS(SELECT 1 FROM ri e GROUP BY e.n HAVING sum(e.n/e.n) > 0) AS selected",
        "SELECT EXISTS(SELECT e.n/e.n FROM ri e OFFSET 0) AS selected",
        "SELECT EXISTS(SELECT e.n/e.n FROM ri e UNION ALL SELECT 1) AS selected",
        "SELECT EXISTS(SELECT 1 FROM ri e WHERE e.n/e.n = 1) AS selected",
        "SELECT EXISTS(SELECT 1/0 FROM ri LIMIT 0) AS selected",
    ];

    // Act
    let results = cases.map(|sql| fixture.execute(sql));

    // Assert
    for (sql, result) in cases.iter().zip(&results) {
        assert!(is_division_by_zero(result), "reached error: {sql}");
    }
}

#[test]
fn should_elide_correlated_exists_targets_across_statements() {
    // Arrange
    let fixture = zero_fixture("exists_target_elision_correlated");

    // Act
    let filtered = fixture.rows(
        "SELECT k FROM ro WHERE EXISTS(SELECT e.n/e.n FROM ri e WHERE e.n = ro.k) ORDER BY k",
    );
    let not_filtered = fixture.rows(
        "SELECT k FROM ro WHERE NOT EXISTS(SELECT e.n/e.n FROM ri e WHERE e.n = ro.k) ORDER BY k",
    );
    let returned = fixture.rows(
        "UPDATE ro SET k = k RETURNING k, EXISTS(SELECT e.n/e.n FROM ri e WHERE e.n = ro.k) AS selected",
    );
    let deleted = fixture.rows("DELETE FROM ro WHERE EXISTS(SELECT e.n/e.n FROM ri e) RETURNING k");

    // Assert
    assert_eq!(filtered, vec![vec![Value::Int64(0)]]);
    assert_eq!(not_filtered, vec![vec![Value::Int64(1)]]);
    let mut returned = returned;
    returned.sort_by_key(|row| match row[0] {
        Value::Int64(value) => value,
        _ => i64::MAX,
    });
    assert_eq!(
        returned,
        vec![
            vec![Value::Int64(0), Value::Bool(true)],
            vec![Value::Int64(1), Value::Bool(false)],
        ]
    );
    assert_eq!(deleted.len(), 2);
    assert_eq!(fixture.rows("SELECT k FROM ro"), Vec::<Vec<Value>>::new());
}
