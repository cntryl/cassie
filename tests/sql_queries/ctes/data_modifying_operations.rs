//! PostgreSQL 18.6 acceptance cases for #834 data-modifying CTEs.
//!
//! Cassie's current ordinary SELECT CTE tests also fail in the #801 foundation,
//! so these result assertions remain blocked by that prerequisite until SELECT
//! CTE bodies execute; they are not isolated #834 semantic proof yet.

use cassie::types::Value;

use crate::support_sql_fixture::sql_fixture;

#[test]
fn should_return_rows_from_an_insert_data_modifying_cte() {
    // Arrange
    let fixture = sql_fixture(
        "issue-834-insert-returning-cte",
        &["CREATE TABLE issue_834_insert_cte_target (id INT PRIMARY KEY, payload TEXT)"],
    );

    // Act
    let returned = fixture.execute(
        "WITH inserted AS (\
             INSERT INTO issue_834_insert_cte_target VALUES (2, 'new') \
             RETURNING id, payload\
         ) SELECT id, payload FROM inserted",
    );
    let stored = fixture.execute("SELECT id, payload FROM issue_834_insert_cte_target ORDER BY id");

    // Assert
    let expected = &[vec![Value::Int64(2), Value::String("new".to_owned())]];
    assert_eq!(
        returned.expect("INSERT RETURNING CTE output").rows(),
        expected
    );
    assert_eq!(
        stored.expect("row inserted by a data-modifying CTE").rows(),
        expected
    );
}

#[test]
fn should_return_rows_from_an_update_data_modifying_cte() {
    // Arrange
    let fixture = sql_fixture(
        "issue-834-update-returning-cte",
        &[
            "CREATE TABLE issue_834_update_cte_target (id INT PRIMARY KEY, payload TEXT)",
            "INSERT INTO issue_834_update_cte_target VALUES (1, 'old'), (2, 'keep')",
        ],
    );

    // Act
    let returned = fixture.execute(
        "WITH updated AS (\
             UPDATE issue_834_update_cte_target SET payload = 'changed' WHERE id = 1 \
             RETURNING id, payload\
         ) SELECT id, payload FROM updated",
    );
    let stored = fixture.execute("SELECT id, payload FROM issue_834_update_cte_target ORDER BY id");

    // Assert
    assert_eq!(
        returned.expect("UPDATE RETURNING CTE output").rows(),
        &[vec![Value::Int64(1), Value::String("changed".to_owned())]]
    );
    assert_eq!(
        stored.expect("rows after the data-modifying CTE").rows(),
        &[
            vec![Value::Int64(1), Value::String("changed".to_owned())],
            vec![Value::Int64(2), Value::String("keep".to_owned())],
        ]
    );
}
