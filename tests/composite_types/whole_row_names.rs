//! Names that could be either a table's whole row or something else in scope:
//! output aliases, window specifications and derived tables.

use crate::support_composite_types::first_column_text;
use crate::support_sql_fixture::{sql_fixture, SqlFixture};
use cassie::types::Value;

fn fixture(label: &str) -> SqlFixture {
    let fixture = sql_fixture(
        label,
        &["CREATE TABLE w (id int primary key, n int, s text)"],
    );
    fixture
        .execute("INSERT INTO w VALUES (1, 30, 'a'), (2, 20, 'b'), (3, 10, NULL)")
        .unwrap();
    fixture
}

#[test]
fn should_order_by_an_output_alias_named_like_the_table() {
    // Arrange
    let fixture = fixture("whole_row_alias_order_by");

    // Act
    let rows = fixture.rows("SELECT n AS w FROM w ORDER BY w");

    // Assert
    assert_eq!(
        rows,
        vec![
            vec![Value::Int64(10)],
            vec![Value::Int64(20)],
            vec![Value::Int64(30)],
        ]
    );
}

#[test]
fn should_group_by_an_output_alias_named_like_the_table() {
    // Arrange
    let fixture = fixture("whole_row_alias_group_by");

    // Act
    let rows = fixture.rows("SELECT n AS w, count(*) FROM w GROUP BY w");

    // Assert
    assert_eq!(rows.len(), 3);
}

#[test]
fn should_still_order_by_the_whole_row_without_a_matching_alias() {
    // Arrange
    let fixture = fixture("whole_row_order_by_row");

    // Act
    let rows = fixture.rows("SELECT n FROM w ORDER BY w");

    // Assert
    assert_eq!(
        rows,
        vec![
            vec![Value::Int64(30)],
            vec![Value::Int64(20)],
            vec![Value::Int64(10)],
        ]
    );
}

#[test]
fn should_reject_a_whole_row_in_a_window_order_with_sqlstate_0a000() {
    // Arrange
    let fixture = fixture("whole_row_window_order");

    // Act
    let error = fixture.error("SELECT row_number() OVER (ORDER BY w) FROM w");

    // Assert
    assert_eq!(error.sql_state(), "0A000");
}

#[test]
fn should_return_the_record_of_a_derived_table_alias() {
    // Arrange
    let fixture = fixture("whole_row_derived_table");

    // Act
    let rows =
        first_column_text(&fixture.rows("SELECT x::text FROM (SELECT id, n FROM w) x ORDER BY id"));

    // Assert
    assert_eq!(
        rows,
        vec![
            Some("(1,30)".to_owned()),
            Some("(2,20)".to_owned()),
            Some("(3,10)".to_owned()),
        ]
    );
}

#[test]
fn should_return_the_record_of_a_derived_star_over_one_table() {
    // Arrange
    let fixture = fixture("whole_row_derived_star");

    // Act
    let rows =
        first_column_text(&fixture.rows("SELECT x::text FROM (SELECT * FROM w) x WHERE id = 3"));

    // Assert
    assert_eq!(rows, vec![Some("(3,10,)".to_owned())]);
}

#[test]
fn should_prefer_a_derived_column_named_like_the_alias() {
    // Arrange
    let fixture = fixture("whole_row_derived_column_wins");

    // Act
    let rows = fixture.rows("SELECT x FROM (SELECT n AS x FROM w WHERE id = 1) x");

    // Assert
    assert_eq!(rows, vec![vec![Value::Int64(30)]]);
}
