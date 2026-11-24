use cassie::types::Value;

use crate::support_sql_fixture::sql_fixture;

#[test]
fn should_apply_cte_alias_lists_over_wildcard_bodies() {
    // Arrange
    let fixture = sql_fixture(
        "cte_alias_wildcard",
        &[
            "CREATE TABLE two (id INT, x INT, y INT)",
            "INSERT INTO two (id, x, y) VALUES (1, 2, 3)",
        ],
    );

    // Act
    let full = fixture.rows("WITH c(p, q, r) AS (SELECT * FROM two) SELECT r, p FROM c");
    let partial = fixture.execute("WITH c(p) AS (SELECT * FROM two) SELECT p, x, y FROM c");
    let too_many = fixture.execute("WITH c(p, q, r, s) AS (SELECT * FROM two) SELECT p FROM c");

    // Assert
    assert_eq!(full, vec![vec![Value::Int64(3), Value::Int64(1)]]);
    let Ok(partial) = partial else {
        panic!("a shorter alias list renames only the leading columns");
    };
    assert_eq!(
        partial.rows(),
        vec![vec![Value::Int64(1), Value::Int64(2), Value::Int64(3)]]
    );
    assert!(too_many.is_err(), "more aliases than columns is an error");
}
