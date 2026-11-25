//! Signed BIGINT boundary inputs for PostgreSQL bitwise aggregates (#806).

use cassie::types::Value;

use crate::support_sql_fixture::sql_fixture;

#[test]
fn should_preserve_bigint_boundary_bits_in_aggregates() {
    // Arrange
    let fixture = sql_fixture(
        "issue-806-bigint-bitwise-boundaries",
        &[
            "CREATE TABLE issue_806_bigint_bitwise_boundaries (value BIGINT)",
            "INSERT INTO issue_806_bigint_bitwise_boundaries VALUES (-9223372036854775808), (9223372036854775807)",
        ],
    );

    // Act
    let result = fixture.execute(
        "SELECT bit_and(value), bit_or(value), bit_xor(value) \
         FROM issue_806_bigint_bitwise_boundaries",
    );

    // Assert
    let result = result.expect("bitwise aggregates retain signed BIGINT boundary bits");
    assert_eq!(
        result.rows(),
        &[vec![Value::Int64(0), Value::Int64(-1), Value::Int64(-1)]]
    );
    assert_eq!(
        result
            .columns()
            .iter()
            .map(|column| column.type_oid)
            .collect::<Vec<_>>(),
        vec![20, 20, 20]
    );
}
