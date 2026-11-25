//! Record comparison: typed field order and constructor membership.

use crate::support_sql_fixture::{sql_fixture, SqlFixture};
use cassie::types::Value;

fn fixture(label: &str) -> SqlFixture {
    let fixture = sql_fixture(
        label,
        &[
            "CREATE TYPE kd AS (d date, n numeric)",
            "CREATE TABLE kdt (id int primary key, v kd)",
        ],
    );
    fixture
        .execute(
            "INSERT INTO kdt VALUES \
             (1, ROW('2026-01-10', 1)), (5, ROW('2026-01-10', 10)), \
             (8, ROW('infinity', 0)), (3, ROW('2026-01-10', 100)), \
             (4, ROW('2026-01-10', 3)), (6, ROW('2026-01-10', -5)), \
             (7, ROW('2026-01-10', -50))",
        )
        .unwrap();
    fixture
}

fn ids(rows: &[Vec<Value>]) -> Vec<i64> {
    rows.iter()
        .map(|row| match row.first() {
            Some(Value::Int64(id)) => *id,
            _ => panic!("expected an integer id"),
        })
        .collect()
}

#[test]
fn should_filter_records_by_typed_field_order() {
    // Arrange
    let fixture = fixture("composite_typed_field_filter");

    // Act
    let rows = fixture.rows("SELECT id FROM kdt WHERE v < ROW('2026-01-10', 9)::kd ORDER BY id");

    // Assert
    assert_eq!(ids(&rows), vec![1, 4, 6, 7]);
}

#[test]
fn should_sort_records_by_typed_field_order() {
    // Arrange
    let fixture = fixture("composite_typed_field_sort");

    // Act
    let rows = fixture.rows("SELECT id FROM kdt ORDER BY v");

    // Assert
    assert_eq!(ids(&rows), vec![7, 6, 1, 4, 5, 3, 8]);
}

#[test]
fn should_return_null_for_a_row_in_list_decided_by_a_null_field() {
    // Arrange
    let fixture = fixture("composite_row_in_null");

    // Act
    let rows = fixture.rows("SELECT ROW(1, NULL) IN (ROW(1, 2))");

    // Assert
    assert_eq!(rows, vec![vec![Value::Null]]);
}

#[test]
fn should_return_false_for_a_row_in_list_with_a_differing_field() {
    // Arrange
    let fixture = fixture("composite_row_in_false");

    // Act
    let rows = fixture.rows("SELECT ROW(1, NULL) IN (ROW(2, 2))");

    // Assert
    assert_eq!(rows, vec![vec![Value::Bool(false)]]);
}

#[test]
fn should_return_true_for_a_row_in_list_with_an_equal_member() {
    // Arrange
    let fixture = fixture("composite_row_in_true");

    // Act
    let rows = fixture.rows("SELECT ROW(1, 2) IN (ROW(3, 4), ROW(1, 2))");

    // Assert
    assert_eq!(rows, vec![vec![Value::Bool(true)]]);
}
