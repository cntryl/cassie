//! Native BYTEA integer values including empty and high-bit inputs.
use crate::support_sql_fixture::sql_fixture;
use cassie::types::Value;

#[test]
fn should_decode_0_empty_given_int2_bytea_when_the_width_is_not_exceeded() {
    // Arrange
    let fixture = sql_fixture("byteacasts-decode-sql-int2_0_empty", &[]);
    // Act
    let result = fixture.execute("SELECT '\\x'::BYTEA::INT2 AS output");
    // Assert
    assert_eq!(
        result.expect("native BYTEA integer decode").rows(),
        &[vec![Value::Int64(0)]]
    );
}

#[test]
fn should_decode_1_zero_given_int2_bytea_when_the_width_is_not_exceeded() {
    // Arrange
    let fixture = sql_fixture("byteacasts-decode-sql-int2_1_zero", &[]);
    // Act
    let result = fixture.execute("SELECT '\\x00'::BYTEA::INT2 AS output");
    // Assert
    assert_eq!(
        result.expect("native BYTEA integer decode").rows(),
        &[vec![Value::Int64(0)]]
    );
}

#[test]
fn should_decode_1_ones_given_int2_bytea_when_the_width_is_not_exceeded() {
    // Arrange
    let fixture = sql_fixture("byteacasts-decode-sql-int2_1_ones", &[]);
    // Act
    let result = fixture.execute("SELECT '\\xff'::BYTEA::INT2 AS output");
    // Assert
    assert_eq!(
        result.expect("native BYTEA integer decode").rows(),
        &[vec![Value::Int64(255)]]
    );
}

#[test]
fn should_decode_1_high_given_int2_bytea_when_the_width_is_not_exceeded() {
    // Arrange
    let fixture = sql_fixture("byteacasts-decode-sql-int2_1_high", &[]);
    // Act
    let result = fixture.execute("SELECT '\\x80'::BYTEA::INT2 AS output");
    // Assert
    assert_eq!(
        result.expect("native BYTEA integer decode").rows(),
        &[vec![Value::Int64(128)]]
    );
}

#[test]
fn should_decode_2_zero_given_int2_bytea_when_the_width_is_not_exceeded() {
    // Arrange
    let fixture = sql_fixture("byteacasts-decode-sql-int2_2_zero", &[]);
    // Act
    let result = fixture.execute("SELECT '\\x0000'::BYTEA::INT2 AS output");
    // Assert
    assert_eq!(
        result.expect("native BYTEA integer decode").rows(),
        &[vec![Value::Int64(0)]]
    );
}

#[test]
fn should_decode_2_ones_given_int2_bytea_when_the_width_is_not_exceeded() {
    // Arrange
    let fixture = sql_fixture("byteacasts-decode-sql-int2_2_ones", &[]);
    // Act
    let result = fixture.execute("SELECT '\\xffff'::BYTEA::INT2 AS output");
    // Assert
    assert_eq!(
        result.expect("native BYTEA integer decode").rows(),
        &[vec![Value::Int64(-1)]]
    );
}

#[test]
fn should_decode_2_high_given_int2_bytea_when_the_width_is_not_exceeded() {
    // Arrange
    let fixture = sql_fixture("byteacasts-decode-sql-int2_2_high", &[]);
    // Act
    let result = fixture.execute("SELECT '\\x8000'::BYTEA::INT2 AS output");
    // Assert
    assert_eq!(
        result.expect("native BYTEA integer decode").rows(),
        &[vec![Value::Int64(-32_768)]]
    );
}
