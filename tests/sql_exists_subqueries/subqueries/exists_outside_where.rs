use cassie::types::Value;

use crate::support_sql_fixture::{sql_fixture, SqlFixture};

fn clause_fixture(label: &str) -> SqlFixture {
    sql_fixture(
        label,
        &[
            "CREATE TABLE t (a INT, g TEXT)",
            "CREATE TABLE u (b INT)",
            "CREATE TABLE e (z INT)",
            "INSERT INTO t (a, g) VALUES (1, 'x')",
            "INSERT INTO t (a, g) VALUES (2, 'x')",
            "INSERT INTO u (b) VALUES (1)",
        ],
    )
}

#[test]
fn should_evaluate_exists_wherever_a_boolean_is_legal() {
    // Arrange
    let fixture = clause_fixture("exists_clauses");

    // Act
    let having = fixture.rows(
        "SELECT g, COUNT(*) AS c FROM t GROUP BY g \
             HAVING COUNT(*) > 0 AND EXISTS (SELECT 1 FROM u)",
    );
    let having_empty =
        fixture.rows("SELECT g, COUNT(*) AS c FROM t GROUP BY g HAVING EXISTS (SELECT 1 FROM e)");
    let join_on =
        fixture.rows("SELECT t.a FROM t JOIN u ON t.a = u.b AND EXISTS (SELECT 1 FROM t)");
    let projection = fixture.rows(
        "SELECT a, EXISTS (SELECT 1 FROM e) AS e_any, NOT EXISTS (SELECT 1 FROM e) AS e_none \
             FROM t ORDER BY a",
    );
    let function_arg = fixture
        .rows("SELECT a FROM t WHERE COALESCE(NOT EXISTS (SELECT 1 FROM e), false) ORDER BY a");

    // Assert
    assert_eq!(
        having,
        vec![vec![Value::String("x".to_string()), Value::Int64(2)]]
    );
    assert!(
        having_empty.is_empty(),
        "HAVING EXISTS over an empty table keeps no group"
    );
    assert_eq!(join_on, vec![vec![Value::Int64(1)]]);
    assert_eq!(
        projection,
        vec![
            vec![Value::Int64(1), Value::Bool(false), Value::Bool(true)],
            vec![Value::Int64(2), Value::Bool(false), Value::Bool(true)],
        ]
    );
    assert_eq!(
        function_arg,
        vec![vec![Value::Int64(1)], vec![Value::Int64(2)]]
    );
}
