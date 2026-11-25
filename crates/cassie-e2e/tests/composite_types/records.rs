//! Record equality, ordering and sorting follow PostgreSQL's record comparison.

use crate::support_composite_types::{first_column_text, only_text};
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
fn should_order_null_fields_after_values_in_record_comparison() {
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
fn should_sort_records_with_null_fields_after_values_then_null_records() {
    // Arrange
    let fixture = fixture("composite_order_by");

    // Act
    let ordered =
        first_column_text(&fixture.rows("SELECT a::text AS t FROM places ORDER BY places.a"));

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

#[test]
fn should_reject_scalar_functions_over_records_with_sqlstate_42883() {
    // Arrange
    let fixture = fixture("composite_function_over_record");

    // Act
    let length = fixture.error("SELECT length(ROW(1, 2))");
    let abs = fixture.error("SELECT abs(ROW(1, 2))");

    // Assert
    assert_eq!(length.sql_state(), "42883");
    assert_eq!(abs.sql_state(), "42883");
}

#[test]
fn should_keep_record_equality_for_named_values_after_function_dispatch_changes() {
    // Arrange
    let fixture = fixture("composite_equality_after_dispatch");

    // Act
    let equal = single_bool(
        &fixture,
        "SELECT ROW('a', 1)::address = ROW('a', 1)::address",
    );

    // Assert
    assert!(equal);
}

#[test]
fn should_render_records_through_concat_with_pg_text_output() {
    // Arrange
    let fixture = fixture("composite_concat_record");

    // Act
    let text = only_text(&fixture.rows("SELECT concat(ROW(1, 2))"));

    // Assert
    assert_eq!(text, Some("(1,2)".to_owned()));
}

#[test]
fn should_render_records_through_concat_ws_with_pg_text_output() {
    // Arrange
    let fixture = fixture("composite_concat_ws_record");

    // Act
    let text = only_text(&fixture.rows("SELECT concat_ws('-', 'a', ROW(1, 2))"));

    // Assert
    assert_eq!(text, Some("a-(1,2)".to_owned()));
}

#[test]
fn should_render_records_through_format_with_pg_text_output() {
    // Arrange
    let fixture = fixture("composite_format_record");

    // Act
    let text = only_text(&fixture.rows("SELECT format('%s', ROW(1, 2))"));

    // Assert
    assert_eq!(text, Some("(1,2)".to_owned()));
}

#[test]
fn should_render_records_through_quote_nullable_as_a_literal() {
    // Arrange
    let fixture = fixture("composite_quote_nullable_record");

    // Act
    let text = only_text(&fixture.rows("SELECT quote_nullable(ROW(1, 2))"));

    // Assert
    assert_eq!(text, Some("'(1,2)'".to_owned()));
}
