//! Subquery bounds, aliased mutation targets and array quantifiers (#762),
//! after the PostgreSQL 18.6 oracle.

use cassie::types::Value;

use crate::support_sql_fixture::sql_fixture;

#[test]
fn should_bound_rows_with_scalar_subquery_pagination() {
    // Arrange
    let fixture = sql_fixture(
        "issue-762-subquery-bounds",
        &[
            "CREATE TABLE sb_values (id INT)",
            "INSERT INTO sb_values VALUES (1), (2), (3), (4)",
        ],
    );

    // Act
    let bounded =
        fixture.rows("SELECT id FROM sb_values ORDER BY id LIMIT (SELECT 2) OFFSET (SELECT 1)");
    let unbounded = fixture.rows("SELECT id FROM sb_values ORDER BY id LIMIT (SELECT NULL::INT)");

    // Assert
    assert_eq!(bounded, vec![vec![Value::Int64(2)], vec![Value::Int64(3)]]);
    assert_eq!(unbounded.len(), 4);
}

#[test]
fn should_correlate_assignment_subqueries_with_an_aliased_update_target() {
    // Arrange
    let fixture = sql_fixture(
        "issue-762-aliased-update-subquery",
        &[
            "CREATE TABLE al_target (id INT PRIMARY KEY, value INT)",
            "CREATE TABLE al_source (id INT, value INT)",
            "INSERT INTO al_target VALUES (1, 10), (2, 20)",
            "INSERT INTO al_source VALUES (1, 100)",
        ],
    );

    // Act
    let updated = fixture.execute(
        "UPDATE al_target AS t SET value = \
         (SELECT s.value FROM al_source AS s WHERE s.id = t.id) RETURNING id, value",
    );
    let stored = fixture.rows("SELECT id, value FROM al_target ORDER BY id");

    // Assert
    let mut returned = updated
        .expect("correlated assignment through the target alias")
        .rows()
        .to_vec();
    returned.sort_by_key(|row| format!("{:?}", row[0]));
    let expected = vec![
        vec![Value::Int64(1), Value::Int64(100)],
        vec![Value::Int64(2), Value::Null],
    ];
    assert_eq!(returned, expected);
    assert_eq!(stored, expected);
}

#[test]
fn should_quantify_comparisons_over_untyped_array_literals() {
    // Arrange
    let fixture = sql_fixture("issue-762-array-quantifiers", &[]);

    // Act
    let result = fixture.rows(
        "SELECT 2 = ANY ('{1,2}'), 3 = ALL ('{3,NULL}'), 5 <> ALL ('{1,2}'), \
         4 = ANY ('{}'), 4 = ALL ('{}')",
    );

    // Assert
    assert_eq!(
        result,
        vec![vec![
            Value::Bool(true),
            Value::Null,
            Value::Bool(true),
            Value::Bool(false),
            Value::Bool(true),
        ]]
    );
}
