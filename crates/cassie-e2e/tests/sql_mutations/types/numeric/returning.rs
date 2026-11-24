//! Exact high-precision numeric values returned from UPDATE (#812).

use cassie::types::Value;

use crate::support_sql_fixture::sql_fixture;

#[test]
fn should_preserve_exact_numeric_values_given_high_precision_rows_when_update_returning_executes() {
    // Arrange
    let initial_value = "1234567890123456789012345678901234567890.12345678901234567890";
    let updated_value = "1234567890123456789012345678901234567890.12345678901234567891";
    let fixture = sql_fixture(
        "numeric-high-precision-update-returning",
        &[
            "CREATE TABLE amount_rows (id INT4 PRIMARY KEY, amount NUMERIC)",
            &format!("INSERT INTO amount_rows VALUES (1, '{initial_value}')"),
        ],
    );
    let before_update = fixture.execute("SELECT amount::TEXT FROM amount_rows WHERE id = 1");
    let returned = fixture.execute(
        "UPDATE amount_rows \
         SET amount = amount + CAST('0.00000000000000000001' AS NUMERIC) \
         WHERE id = 1 \
         RETURNING id, amount::TEXT, amount = CAST( \
             '1234567890123456789012345678901234567890.12345678901234567891' \
             AS NUMERIC)",
    );

    // Act
    let before_update = before_update.expect("exact numeric storage control");
    let returned = returned.expect("high-precision UPDATE RETURNING");

    // Assert
    assert_eq!(
        before_update.rows(),
        &[vec![Value::String(initial_value.to_owned())]]
    );
    assert_eq!(
        returned
            .columns()
            .iter()
            .map(|column| column.type_oid)
            .collect::<Vec<_>>(),
        [23, 25, 16]
    );
    assert_eq!(
        returned.rows(),
        &[vec![
            Value::Int64(1),
            Value::String(updated_value.to_owned()),
            Value::Bool(true),
        ]]
    );
}
