//! Native BYTEA integer values including empty and high-bit inputs.
use crate::support_sql_fixture::sql_fixture;
use cassie::types::Value;

#[test]
fn should_decode_0_empty_given_int8_bytea_when_the_width_is_not_exceeded() {
    // Arrange
    let fixture = sql_fixture("byteacasts-decode-sql-int8_0_empty", &[]);
    // Act
    let result = fixture.execute("SELECT '\\x'::BYTEA::INT8 AS output");
    // Assert
    assert_eq!(
        result.expect("native BYTEA integer decode").rows(),
        &[vec![Value::Int64(0)]]
    );
}

#[test]
fn should_decode_1_zero_given_int8_bytea_when_the_width_is_not_exceeded() {
    // Arrange
    let fixture = sql_fixture("byteacasts-decode-sql-int8_1_zero", &[]);
    // Act
    let result = fixture.execute("SELECT '\\x00'::BYTEA::INT8 AS output");
    // Assert
    assert_eq!(
        result.expect("native BYTEA integer decode").rows(),
        &[vec![Value::Int64(0)]]
    );
}

#[test]
fn should_decode_1_ones_given_int8_bytea_when_the_width_is_not_exceeded() {
    // Arrange
    let fixture = sql_fixture("byteacasts-decode-sql-int8_1_ones", &[]);
    // Act
    let result = fixture.execute("SELECT '\\xff'::BYTEA::INT8 AS output");
    // Assert
    assert_eq!(
        result.expect("native BYTEA integer decode").rows(),
        &[vec![Value::Int64(255)]]
    );
}

#[test]
fn should_decode_1_high_given_int8_bytea_when_the_width_is_not_exceeded() {
    // Arrange
    let fixture = sql_fixture("byteacasts-decode-sql-int8_1_high", &[]);
    // Act
    let result = fixture.execute("SELECT '\\x80'::BYTEA::INT8 AS output");
    // Assert
    assert_eq!(
        result.expect("native BYTEA integer decode").rows(),
        &[vec![Value::Int64(128)]]
    );
}

#[test]
fn should_decode_2_zero_given_int8_bytea_when_the_width_is_not_exceeded() {
    // Arrange
    let fixture = sql_fixture("byteacasts-decode-sql-int8_2_zero", &[]);
    // Act
    let result = fixture.execute("SELECT '\\x0000'::BYTEA::INT8 AS output");
    // Assert
    assert_eq!(
        result.expect("native BYTEA integer decode").rows(),
        &[vec![Value::Int64(0)]]
    );
}

#[test]
fn should_decode_2_ones_given_int8_bytea_when_the_width_is_not_exceeded() {
    // Arrange
    let fixture = sql_fixture("byteacasts-decode-sql-int8_2_ones", &[]);
    // Act
    let result = fixture.execute("SELECT '\\xffff'::BYTEA::INT8 AS output");
    // Assert
    assert_eq!(
        result.expect("native BYTEA integer decode").rows(),
        &[vec![Value::Int64(65_535)]]
    );
}

#[test]
fn should_decode_2_high_given_int8_bytea_when_the_width_is_not_exceeded() {
    // Arrange
    let fixture = sql_fixture("byteacasts-decode-sql-int8_2_high", &[]);
    // Act
    let result = fixture.execute("SELECT '\\x8000'::BYTEA::INT8 AS output");
    // Assert
    assert_eq!(
        result.expect("native BYTEA integer decode").rows(),
        &[vec![Value::Int64(32_768)]]
    );
}

#[test]
fn should_decode_3_zero_given_int8_bytea_when_the_width_is_not_exceeded() {
    // Arrange
    let fixture = sql_fixture("byteacasts-decode-sql-int8_3_zero", &[]);
    // Act
    let result = fixture.execute("SELECT '\\x000000'::BYTEA::INT8 AS output");
    // Assert
    assert_eq!(
        result.expect("native BYTEA integer decode").rows(),
        &[vec![Value::Int64(0)]]
    );
}

#[test]
fn should_decode_3_ones_given_int8_bytea_when_the_width_is_not_exceeded() {
    // Arrange
    let fixture = sql_fixture("byteacasts-decode-sql-int8_3_ones", &[]);
    // Act
    let result = fixture.execute("SELECT '\\xffffff'::BYTEA::INT8 AS output");
    // Assert
    assert_eq!(
        result.expect("native BYTEA integer decode").rows(),
        &[vec![Value::Int64(16_777_215)]]
    );
}

#[test]
fn should_decode_3_high_given_int8_bytea_when_the_width_is_not_exceeded() {
    // Arrange
    let fixture = sql_fixture("byteacasts-decode-sql-int8_3_high", &[]);
    // Act
    let result = fixture.execute("SELECT '\\x800000'::BYTEA::INT8 AS output");
    // Assert
    assert_eq!(
        result.expect("native BYTEA integer decode").rows(),
        &[vec![Value::Int64(8_388_608)]]
    );
}

#[test]
fn should_decode_4_zero_given_int8_bytea_when_the_width_is_not_exceeded() {
    // Arrange
    let fixture = sql_fixture("byteacasts-decode-sql-int8_4_zero", &[]);
    // Act
    let result = fixture.execute("SELECT '\\x00000000'::BYTEA::INT8 AS output");
    // Assert
    assert_eq!(
        result.expect("native BYTEA integer decode").rows(),
        &[vec![Value::Int64(0)]]
    );
}

#[test]
fn should_decode_4_ones_given_int8_bytea_when_the_width_is_not_exceeded() {
    // Arrange
    let fixture = sql_fixture("byteacasts-decode-sql-int8_4_ones", &[]);
    // Act
    let result = fixture.execute("SELECT '\\xffffffff'::BYTEA::INT8 AS output");
    // Assert
    assert_eq!(
        result.expect("native BYTEA integer decode").rows(),
        &[vec![Value::Int64(4_294_967_295)]]
    );
}

#[test]
fn should_decode_4_high_given_int8_bytea_when_the_width_is_not_exceeded() {
    // Arrange
    let fixture = sql_fixture("byteacasts-decode-sql-int8_4_high", &[]);
    // Act
    let result = fixture.execute("SELECT '\\x80000000'::BYTEA::INT8 AS output");
    // Assert
    assert_eq!(
        result.expect("native BYTEA integer decode").rows(),
        &[vec![Value::Int64(2_147_483_648)]]
    );
}

#[test]
fn should_decode_5_zero_given_int8_bytea_when_the_width_is_not_exceeded() {
    // Arrange
    let fixture = sql_fixture("byteacasts-decode-sql-int8_5_zero", &[]);
    // Act
    let result = fixture.execute("SELECT '\\x0000000000'::BYTEA::INT8 AS output");
    // Assert
    assert_eq!(
        result.expect("native BYTEA integer decode").rows(),
        &[vec![Value::Int64(0)]]
    );
}

#[test]
fn should_decode_5_ones_given_int8_bytea_when_the_width_is_not_exceeded() {
    // Arrange
    let fixture = sql_fixture("byteacasts-decode-sql-int8_5_ones", &[]);
    // Act
    let result = fixture.execute("SELECT '\\xffffffffff'::BYTEA::INT8 AS output");
    // Assert
    assert_eq!(
        result.expect("native BYTEA integer decode").rows(),
        &[vec![Value::Int64(1_099_511_627_775)]]
    );
}

#[test]
fn should_decode_5_high_given_int8_bytea_when_the_width_is_not_exceeded() {
    // Arrange
    let fixture = sql_fixture("byteacasts-decode-sql-int8_5_high", &[]);
    // Act
    let result = fixture.execute("SELECT '\\x8000000000'::BYTEA::INT8 AS output");
    // Assert
    assert_eq!(
        result.expect("native BYTEA integer decode").rows(),
        &[vec![Value::Int64(549_755_813_888)]]
    );
}

#[test]
fn should_decode_6_zero_given_int8_bytea_when_the_width_is_not_exceeded() {
    // Arrange
    let fixture = sql_fixture("byteacasts-decode-sql-int8_6_zero", &[]);
    // Act
    let result = fixture.execute("SELECT '\\x000000000000'::BYTEA::INT8 AS output");
    // Assert
    assert_eq!(
        result.expect("native BYTEA integer decode").rows(),
        &[vec![Value::Int64(0)]]
    );
}

#[test]
fn should_decode_6_ones_given_int8_bytea_when_the_width_is_not_exceeded() {
    // Arrange
    let fixture = sql_fixture("byteacasts-decode-sql-int8_6_ones", &[]);
    // Act
    let result = fixture.execute("SELECT '\\xffffffffffff'::BYTEA::INT8 AS output");
    // Assert
    assert_eq!(
        result.expect("native BYTEA integer decode").rows(),
        &[vec![Value::Int64(281_474_976_710_655)]]
    );
}

#[test]
fn should_decode_6_high_given_int8_bytea_when_the_width_is_not_exceeded() {
    // Arrange
    let fixture = sql_fixture("byteacasts-decode-sql-int8_6_high", &[]);
    // Act
    let result = fixture.execute("SELECT '\\x800000000000'::BYTEA::INT8 AS output");
    // Assert
    assert_eq!(
        result.expect("native BYTEA integer decode").rows(),
        &[vec![Value::Int64(140_737_488_355_328)]]
    );
}

#[test]
fn should_decode_7_zero_given_int8_bytea_when_the_width_is_not_exceeded() {
    // Arrange
    let fixture = sql_fixture("byteacasts-decode-sql-int8_7_zero", &[]);
    // Act
    let result = fixture.execute("SELECT '\\x00000000000000'::BYTEA::INT8 AS output");
    // Assert
    assert_eq!(
        result.expect("native BYTEA integer decode").rows(),
        &[vec![Value::Int64(0)]]
    );
}

#[test]
fn should_decode_7_ones_given_int8_bytea_when_the_width_is_not_exceeded() {
    // Arrange
    let fixture = sql_fixture("byteacasts-decode-sql-int8_7_ones", &[]);
    // Act
    let result = fixture.execute("SELECT '\\xffffffffffffff'::BYTEA::INT8 AS output");
    // Assert
    assert_eq!(
        result.expect("native BYTEA integer decode").rows(),
        &[vec![Value::Int64(72_057_594_037_927_935)]]
    );
}

#[test]
fn should_decode_7_high_given_int8_bytea_when_the_width_is_not_exceeded() {
    // Arrange
    let fixture = sql_fixture("byteacasts-decode-sql-int8_7_high", &[]);
    // Act
    let result = fixture.execute("SELECT '\\x80000000000000'::BYTEA::INT8 AS output");
    // Assert
    assert_eq!(
        result.expect("native BYTEA integer decode").rows(),
        &[vec![Value::Int64(36_028_797_018_963_968)]]
    );
}

#[test]
fn should_decode_8_zero_given_int8_bytea_when_the_width_is_not_exceeded() {
    // Arrange
    let fixture = sql_fixture("byteacasts-decode-sql-int8_8_zero", &[]);
    // Act
    let result = fixture.execute("SELECT '\\x0000000000000000'::BYTEA::INT8 AS output");
    // Assert
    assert_eq!(
        result.expect("native BYTEA integer decode").rows(),
        &[vec![Value::Int64(0)]]
    );
}

#[test]
fn should_decode_8_ones_given_int8_bytea_when_the_width_is_not_exceeded() {
    // Arrange
    let fixture = sql_fixture("byteacasts-decode-sql-int8_8_ones", &[]);
    // Act
    let result = fixture.execute("SELECT '\\xffffffffffffffff'::BYTEA::INT8 AS output");
    // Assert
    assert_eq!(
        result.expect("native BYTEA integer decode").rows(),
        &[vec![Value::Int64(-1)]]
    );
}

#[test]
fn should_decode_8_high_given_int8_bytea_when_the_width_is_not_exceeded() {
    // Arrange
    let fixture = sql_fixture("byteacasts-decode-sql-int8_8_high", &[]);
    // Act
    let result = fixture.execute("SELECT '\\x8000000000000000'::BYTEA::INT8 AS output");
    // Assert
    assert_eq!(
        result.expect("native BYTEA integer decode").rows(),
        &[vec![Value::Int64(-9_223_372_036_854_775_808)]]
    );
}
