//! ORDER BY names an output column first: an alias that reuses another source column's name
//! orders by the aliased expression, on every read path.
use super::support_sql_fixture::{sql_fixture, SqlFixture};
use cassie::types::Value;

fn swapped_fixture(label: &str, extra: &[&str]) -> SqlFixture {
    let mut setup = vec![
        "CREATE TABLE swapped (a BIGINT, b BIGINT)",
        "INSERT INTO swapped (a, b) VALUES (1, 30), (2, 10), (3, 20)",
    ];
    setup.extend_from_slice(extra);
    sql_fixture(label, &setup)
}

fn pair(a: i64, b: i64) -> Vec<Value> {
    vec![Value::Int64(a), Value::Int64(b)]
}

#[test]
fn should_order_a_limited_read_by_the_output_alias_that_shadows_a_source_column() {
    // Arrange
    let fixture = swapped_fixture("swapped-alias-top-k", &[]);

    // Act
    let rows = fixture.rows("SELECT b AS a, a AS b FROM swapped ORDER BY a LIMIT 2");

    // Assert
    assert_eq!(rows, vec![pair(10, 2), pair(20, 3)]);
}

#[test]
fn should_order_a_descending_limited_read_by_the_shadowing_output_alias() {
    // Arrange
    let fixture = swapped_fixture("swapped-alias-top-k-desc", &[]);

    // Act
    let rows = fixture.rows("SELECT b AS a, a AS b FROM swapped ORDER BY a DESC LIMIT 1");

    // Assert
    assert_eq!(rows, vec![pair(30, 1)]);
}

#[test]
fn should_order_an_unlimited_read_by_the_shadowing_output_alias() {
    // Arrange
    let fixture = swapped_fixture("swapped-alias-full", &[]);

    // Act
    let rows = fixture.rows("SELECT b AS a, a AS b FROM swapped ORDER BY a");

    // Assert
    assert_eq!(rows, vec![pair(10, 2), pair(20, 3), pair(30, 1)]);
}

#[test]
fn should_order_by_the_shadowing_output_alias_when_the_source_column_is_indexed() {
    // Arrange
    let fixture = swapped_fixture(
        "swapped-alias-indexed",
        &["CREATE INDEX swapped_a_idx ON swapped USING btree (a)"],
    );

    // Act
    let rows = fixture.rows("SELECT b AS a, a AS b FROM swapped ORDER BY a LIMIT 2");

    // Assert
    assert_eq!(rows, vec![pair(10, 2), pair(20, 3)]);
}

#[test]
fn should_order_a_filtered_read_by_the_shadowing_output_alias() {
    // Arrange
    let fixture = swapped_fixture(
        "swapped-alias-filtered",
        &["CREATE INDEX swapped_a_idx ON swapped USING btree (a)"],
    );

    // Act
    let rows = fixture.rows("SELECT b AS a, a AS b FROM swapped WHERE a >= 1 ORDER BY a LIMIT 2");

    // Assert
    assert_eq!(rows, vec![pair(10, 2), pair(20, 3)]);
}

#[test]
fn should_order_by_the_shadowing_output_alias_with_an_offset() {
    // Arrange
    let fixture = swapped_fixture("swapped-alias-offset", &[]);

    // Act
    let rows = fixture.rows("SELECT b AS a, a AS b FROM swapped ORDER BY a LIMIT 1 OFFSET 1");

    // Assert
    assert_eq!(rows, vec![pair(20, 3)]);
}
