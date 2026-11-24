//! PostgreSQL integer bitwise operators and shared operator precedence for #796.

use super::{sql_fixture, Value};

#[test]
fn should_evaluate_postgres_integer_bitwise_operators_with_native_precedence() {
    // Arrange
    let fixture = sql_fixture("issue-796-integer-bitwise-operator-precedence", &[]);

    // Act
    let result = fixture.execute(
        "SELECT 5 | 2 & 1 AS other_operator_chain, \
                1 + 2 << 1 AS arithmetic_before_shift, \
                5 # 3 AS bitwise_xor, \
                ~1 & 7 AS masked_complement, \
                1 << 3 AS left_shift, \
                32 >> 2 AS right_shift",
    );

    // Assert
    let result = result.expect("PostgreSQL integer bitwise operators and precedence");
    assert_eq!(
        result.rows(),
        &[vec![
            Value::Int64(1),
            Value::Int64(6),
            Value::Int64(6),
            Value::Int64(6),
            Value::Int64(8),
            Value::Int64(8),
        ]]
    );
    assert_eq!(
        result
            .columns()
            .iter()
            .map(|column| column.type_oid)
            .collect::<Vec<_>>(),
        vec![23; 6]
    );
}
