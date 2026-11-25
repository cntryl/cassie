//! An inner FROM alias that redefines an outer alias hides the outer table, as in PostgreSQL.
//! The outer-only column `w` is used because a table without a declared `id` still answers a
//! qualified `id` with its row identity.
use cassie::types::Value;

use crate::support_sql_fixture::{sql_fixture, SqlFixture};

fn fixture(label: &str) -> SqlFixture {
    sql_fixture(
        label,
        &[
            "CREATE TABLE st (id INT, v INT, w INT)",
            "INSERT INTO st VALUES (1, 10, 100), (2, 20, 200)",
            "CREATE TABLE su (v INT)",
            "INSERT INTO su VALUES (5)",
        ],
    )
}

#[test]
fn should_reject_outer_column_hidden_by_inner_alias_in_exists() {
    // Arrange
    let fixture = fixture("alias-shadow-exists");

    // Act
    let error =
        fixture.error("SELECT * FROM st AS a WHERE EXISTS (SELECT 1 FROM su AS a WHERE a.w = 1)");

    // Assert
    assert_eq!(error.sql_state(), "42703");
}

#[test]
fn should_reject_outer_column_hidden_by_inner_alias_in_in_subquery() {
    // Arrange
    let fixture = fixture("alias-shadow-in");

    // Act
    let error = fixture.error("SELECT * FROM st AS a WHERE a.v IN (SELECT a.w FROM su AS a)");

    // Assert
    assert_eq!(error.sql_state(), "42703");
}

#[test]
fn should_reject_outer_column_hidden_by_inner_alias_in_scalar_subquery() {
    // Arrange
    let fixture = fixture("alias-shadow-scalar");

    // Act
    let error = fixture.error("SELECT (SELECT a.w FROM su AS a) FROM st AS a");

    // Assert
    assert_eq!(error.sql_state(), "42703");
}

#[test]
fn should_reject_outer_column_hidden_by_inner_alias_in_lateral_subquery() {
    // Arrange
    let fixture = fixture("alias-shadow-lateral");

    // Act
    let error = fixture.error("SELECT * FROM st AS a, LATERAL (SELECT a.w FROM su AS a) AS d");

    // Assert
    assert_eq!(error.sql_state(), "42703");
}

#[test]
fn should_keep_correlation_to_an_outer_alias_the_inner_scope_does_not_redefine() {
    // Arrange
    let fixture = fixture("alias-shadow-correlation");

    // Act
    let rows =
        fixture.rows("SELECT id FROM st AS a WHERE EXISTS (SELECT 1 FROM su AS b WHERE b.v < a.v)");

    // Assert
    assert_eq!(rows, vec![vec![Value::Int64(1)], vec![Value::Int64(2)]]);
}

#[test]
fn should_resolve_inner_alias_columns_when_the_alias_shadows_an_outer_alias() {
    // Arrange
    let fixture = fixture("alias-shadow-inner-use");

    // Act
    let rows =
        fixture.rows("SELECT id FROM st AS a WHERE EXISTS (SELECT 1 FROM su AS a WHERE a.v = 5)");

    // Assert
    assert_eq!(rows, vec![vec![Value::Int64(1)], vec![Value::Int64(2)]]);
}

#[test]
fn should_reject_outer_column_hidden_by_inner_alias_when_the_outer_table_is_empty() {
    // Arrange
    let fixture = sql_fixture(
        "alias-shadow-empty-outer",
        &[
            "CREATE TABLE st (id INT, v INT, w INT)",
            "CREATE TABLE su (v INT)",
        ],
    );

    // Act
    let error =
        fixture.error("SELECT * FROM st AS a WHERE EXISTS (SELECT 1 FROM su AS a WHERE a.w = 1)");

    // Assert
    assert_eq!(error.sql_state(), "42703");
}
