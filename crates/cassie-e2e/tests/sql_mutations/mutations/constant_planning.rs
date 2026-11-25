//! Planning-time constant errors in data-changing statements: PostgreSQL folds
//! immutable constant SET, WHERE and RETURNING expressions before any row is
//! changed, so a dead constant branch or a zero-row command still fails.
use super::support_sql_fixture::{sql_fixture, SqlFixture};
use cassie::app::CassieError;
use cassie::executor::QueryResult;
use cassie::types::Value;

fn returning_fixture(label: &str) -> SqlFixture {
    sql_fixture(
        label,
        &[
            "CREATE TABLE rt (id BIGINT)",
            "INSERT INTO rt VALUES (1), (2)",
        ],
    )
}

fn ids(fixture: &SqlFixture) -> Vec<Vec<Value>> {
    fixture.rows("SELECT id FROM rt ORDER BY id")
}

fn is_division_by_zero(result: &Result<QueryResult, CassieError>) -> bool {
    matches!(result, Err(CassieError::Execution(message)) if message == "division by zero")
}

#[test]
fn should_reject_constant_returning_errors_before_changing_rows() {
    // Arrange
    let fixture = returning_fixture("constant_planning_returning_errors");
    let cases = [
        "UPDATE rt SET id=id RETURNING id, CASE WHEN id<0 THEN 1/0 ELSE 1 END AS selected",
        "UPDATE rt SET id=id+10 WHERE id<0 RETURNING 1/0 AS selected",
        "UPDATE rt SET id=CASE WHEN id<0 THEN 1/0 ELSE id+10 END",
        "UPDATE rt SET id=id+10 WHERE id<0 AND 1/0=1",
        "DELETE FROM rt WHERE id<0 RETURNING 1/0 AS selected",
        "INSERT INTO rt VALUES (3) RETURNING CASE WHEN id<0 THEN 1/0 ELSE id END",
        "INSERT INTO rt SELECT 1/0 FROM rt WHERE id < 0",
        "UPDATE rt SET id = id WHERE false RETURNING CASE WHEN id < 0 THEN 1/0 END",
    ];

    // Act
    let results = cases.map(|sql| fixture.execute(sql));

    // Assert
    for (sql, result) in cases.iter().zip(&results) {
        assert!(is_division_by_zero(result), "planning-time 22012: {sql}");
    }
    assert_eq!(
        ids(&fixture),
        vec![vec![Value::Int64(1)], vec![Value::Int64(2)]]
    );
}

#[test]
fn should_reject_constant_conflict_assignments_without_a_conflict() {
    // Arrange
    let fixture = sql_fixture(
        "constant_planning_conflict",
        &["CREATE TABLE ck (id INT PRIMARY KEY, v INT)"],
    );

    // Act
    let result =
        fixture.execute("INSERT INTO ck VALUES (1, 1) ON CONFLICT (id) DO UPDATE SET v = 1/0");

    // Assert
    assert!(is_division_by_zero(&result), "planning-time 22012");
    assert_eq!(fixture.rows("SELECT id FROM ck"), Vec::<Vec<Value>>::new());
}

#[test]
fn should_keep_row_dependent_returning_expressions_lazy() {
    // Arrange
    let fixture = returning_fixture("constant_planning_returning_lazy");

    // Act
    let dead_branch = fixture.execute(
        "UPDATE rt SET id=id RETURNING id, CASE WHEN id<0 THEN rt.id/0 ELSE 1 END AS selected",
    );
    let zero_rows =
        fixture.execute("UPDATE rt SET id=id+10 WHERE id<0 RETURNING rt.id/0 AS selected");
    let pruned = fixture.execute(
        "UPDATE rt SET id=id+10 WHERE id=1 RETURNING CASE WHEN false THEN 1/0 ELSE id END AS selected",
    );

    // Assert
    let Ok(dead_branch) = dead_branch else {
        panic!("row-dependent dead branch must stay lazy");
    };
    let mut dead_rows = dead_branch.rows().to_vec();
    dead_rows.sort_by_key(|row| match row[0] {
        Value::Int64(value) => value,
        _ => i64::MAX,
    });
    assert_eq!(
        dead_rows,
        vec![
            vec![Value::Int64(1), Value::Int64(1)],
            vec![Value::Int64(2), Value::Int64(1)],
        ]
    );
    assert_eq!(
        zero_rows.map(|result| result.rows().len()).ok(),
        Some(0),
        "zero-row row-dependent RETURNING"
    );
    assert_eq!(
        pruned.map(|result| result.rows().to_vec()).ok(),
        Some(vec![vec![Value::Int64(11)]])
    );
    assert_eq!(
        ids(&fixture),
        vec![vec![Value::Int64(2)], vec![Value::Int64(11)]]
    );
}
