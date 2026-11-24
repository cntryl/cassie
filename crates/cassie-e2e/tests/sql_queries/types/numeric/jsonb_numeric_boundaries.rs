//! Numeric integer-weight boundaries shared by JSONB producers (#809).
use crate::support_sql_fixture::sql_fixture;
use cassie::types::Value;

#[test]
fn should_accept_exact_numeric_weight_with_fractional_coefficient_leading_zeroes() {
    // Arrange
    let fixture = sql_fixture("jsonb-numeric-significant-weight", &[]);
    // Act
    let result = fixture.execute(
        "SELECT length(('0.01e131073'::NUMERIC)::TEXT), \
        length(('0.01e131073'::JSONB)::TEXT),length(('-0.01e131073'::JSONB)::TEXT)",
    );
    // Assert
    assert_eq!(
        result.expect("exact format boundary").rows(),
        &[vec![
            Value::Int64(131_072),
            Value::Int64(131_072),
            Value::Int64(131_073)
        ]]
    );
}

#[test]
fn should_preserve_zero_jsonb_format_without_expanding_its_exponent() {
    // Arrange
    let fixture = sql_fixture("jsonb-zero-exponent-format", &[]);
    // Act
    let result = fixture.execute(
        "SELECT ('0e1073741823'::JSONB)::TEXT, \
        length(('0.00e-16381'::JSONB)::TEXT),('-0.00e1000000'::NUMERIC)::TEXT",
    );
    // Assert
    assert_eq!(
        result.expect("zero format boundary").rows(),
        &[vec![
            Value::String("0".into()),
            Value::Int64(16385),
            Value::String("0".into())
        ]]
    );
}
