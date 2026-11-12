//! Record equality, ordering and sorting follow PostgreSQL's record comparison.

use crate::support_composite_types::first_column_text;
use crate::support_sql_fixture::{sql_fixture, SqlFixture};
use cassie::types::Value;

fn fixture(label: &str) -> SqlFixture {
    sql_fixture(
        label,
        &[
            "CREATE TYPE address AS (street text, zip int)",
            "CREATE TABLE places (id int, a address)",
            "INSERT INTO places VALUES (1, '(x,2)'), (2, '(x,1)'), (3, '(x,)'), (4, '(a,9)'), (5, NULL)",
        ],
    )
}

fn single_bool(fixture: &SqlFixture, sql: &str) -> bool {
    match fixture.rows(sql).as_slice() {
        [row] => match row.as_slice() {
            [Value::Bool(flag)] => *flag,
            _ => panic!("expected a boolean cell"),
        },
        _ => panic!("expected one row"),
    }
}

#[test]
fn should_compare_records_field_wise() {
    // Arrange
    let fixture = fixture("composite_compare");

    // Act
    let less = single_bool(&fixture, "SELECT '(a,1)'::address < '(a,2)'::address");
    let greater = single_bool(&fixture, "SELECT '(b,0)'::address > '(a,9)'::address");
    let unequal = single_bool(&fixture, "SELECT '(x,1)'::address <> '(x,2)'::address");

    // Assert
    assert!(less);
    assert!(greater);
    assert!(unequal);
}

#[test]
fn should_treat_null_fields_as_equal_and_greater_than_values_in_record_comparison() {
    // Arrange
    let fixture = fixture("composite_compare_null_fields");

    // Act
    let equal = single_bool(&fixture, "SELECT '(a,)'::address = '(a,)'::address");
    let greater = single_bool(&fixture, "SELECT '(a,)'::address > '(a,5)'::address");

    // Assert
    assert!(equal, "record equality treats NULL fields as equal");
    assert!(greater, "a NULL field sorts after any value");
}

#[test]
fn should_sort_records_with_null_fields_after_values_and_null_records_last() {
    // Arrange
    let fixture = fixture("composite_order_by");

    // Act
    let ordered = first_column_text(&fixture.rows("SELECT a::text FROM places ORDER BY a"));

    // Assert
    assert_eq!(
        ordered,
        vec![
            Some("(a,9)".to_owned()),
            Some("(x,1)".to_owned()),
            Some("(x,2)".to_owned()),
            Some("(x,)".to_owned()),
            None,
        ]
    );
}

#[test]
fn should_filter_by_record_equality_against_a_typed_value() {
    // Arrange
    let fixture = fixture("composite_filter_equality");

    // Act
    let rows = fixture.rows("SELECT id FROM places WHERE a = '(x,1)'::address");

    // Assert
    assert_eq!(rows, vec![vec![Value::Int64(2)]]);
}

#[test]
fn should_reject_comparing_a_record_with_an_untyped_literal() {
    // Arrange
    let fixture = fixture("composite_literal_comparison");

    // Act
    let error = fixture.error("SELECT id FROM places WHERE a = '(x,1)'");

    // Assert
    assert_eq!(error.sql_state(), "0A000");
}
