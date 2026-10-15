use crate::support_sql_fixture::sql_fixture;

#[test]
fn should_bind_exists_subquery_relations_to_enclosing_ctes() {
    // Arrange
    let fixture = sql_fixture(
        "exists_cte_scope",
        &[
            "CREATE TABLE o (k INT)",
            "CREATE TABLE t (a INT)",
            "CREATE TABLE t2 (a INT)",
            "INSERT INTO o (k) VALUES (1)",
            "INSERT INTO t (a) VALUES (5)",
        ],
    );

    // Act
    let shadowing_empty = fixture.rows(
        "WITH t AS (SELECT a FROM t WHERE a = 999) \
             SELECT k FROM o WHERE EXISTS (SELECT 1 FROM t)",
    );
    let shadowing_full = fixture
        .rows("WITH t2 AS (SELECT k AS a FROM o) SELECT k FROM o WHERE EXISTS (SELECT 1 FROM t2)");
    let cte_only = fixture
        .rows("WITH c AS (SELECT k FROM o) SELECT k FROM o WHERE NOT EXISTS (SELECT 1 FROM c)");

    // Assert
    assert_eq!(
        (shadowing_empty.len(), shadowing_full.len(), cte_only.len()),
        (0, 1, 0)
    );
}
