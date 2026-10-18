//! Native `real` (`float4`) storage, text output, ordering and casts.

use cassie::types::Value;

use crate::support_sql_fixture::sql_fixture;

fn text(value: &str) -> Value {
    Value::String(value.to_owned())
}

#[test]
fn should_return_stored_real_values_as_postgresql_text_in_float4_order() {
    // Arrange
    let fixture = sql_fixture(
        "real-storage-order",
        &[
            "CREATE TABLE real_values (id BIGINT, measure REAL)",
            "INSERT INTO real_values VALUES (1, 'NaN'), (2, 1.5), (3, 1000000), (4, 0.1), (5, '-Infinity')",
        ],
    );

    // Act
    let rows = fixture.rows("SELECT measure FROM real_values ORDER BY measure");

    // Assert
    assert_eq!(
        rows,
        vec![
            vec![text("-Infinity")],
            vec![text("0.1")],
            vec![text("1.5")],
            vec![text("1e+06")],
            vec![text("NaN")],
        ]
    );
}

#[test]
fn should_cast_text_and_numbers_to_real() {
    // Arrange
    let fixture = sql_fixture("real-casts", &[]);

    // Act
    let rows = fixture.rows(
        "SELECT CAST('3.25' AS real), CAST(2 AS real), CAST(CAST(0.1 AS real) AS text), CAST('  1e5 ' AS float4)",
    );

    // Assert
    assert_eq!(
        rows,
        vec![vec![text("3.25"), text("2"), text("0.1"), text("100000")]]
    );
}

#[test]
fn should_reject_real_input_outside_the_float4_range() {
    // Arrange
    let fixture = sql_fixture("real-input-range", &[]);

    // Act
    let messages = ["SELECT CAST('1e39' AS real)", "SELECT CAST('abc' AS real)"]
        .map(|sql| fixture.error(sql).to_string());

    // Assert
    assert_eq!(
        messages,
        [
            "\"1e39\" is out of range for type real".to_string(),
            "invalid input syntax for type real: \"abc\"".to_string(),
        ]
    );
}

#[test]
fn should_filter_real_rows_against_integer_and_float_bounds() {
    // Arrange
    let fixture = sql_fixture(
        "real-filters",
        &[
            "CREATE TABLE real_filters (id BIGINT, measure REAL)",
            "INSERT INTO real_filters VALUES (1, 0.5), (2, 1.5), (3, 2.5)",
        ],
    );

    // Act
    let rows = fixture
        .rows("SELECT id FROM real_filters WHERE measure > 1 AND measure < 2.75 ORDER BY id");

    // Assert
    assert_eq!(rows, vec![vec![Value::Int64(2)], vec![Value::Int64(3)]]);
}
