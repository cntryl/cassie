use super::{weight, QueryError};
use crate::types::extended::numeric::{parse_text, Numeric};
use crate::types::{typed::TypedValue, Value};

#[test]
fn should_preserve_finite_nonnegative_weights_exactly() {
    // Arrange
    let scaled_zero = parse_text("-0.000").expect("scaled zero");
    let large = parse_text("9223372036854775808.0000000000000000000000000001")
        .expect("exact numeric outside the integer range");
    let cases = [
        (Value::Int64(0), parse_text("0").expect("integer zero")),
        (
            Value::Int64(i64::MAX),
            parse_text("9223372036854775807").expect("maximum integer"),
        ),
        (
            Value::Typed(TypedValue::Numeric(scaled_zero.clone())),
            scaled_zero,
        ),
        (Value::Typed(TypedValue::Numeric(large.clone())), large),
    ];

    // Act
    let actual = cases.each_ref().map(|(value, _)| weight(Some(value)));

    // Assert
    for (observed, (_, expected)) in actual.into_iter().zip(cases) {
        assert_eq!(observed.expect("valid exact weight"), expected);
    }
}

#[test]
fn should_report_the_selected_edge_diagnostic_for_invalid_weights() {
    // Arrange
    let values = [
        Value::Null,
        Value::Int64(-1),
        Value::Int64(i64::MIN),
        Value::Typed(TypedValue::Numeric(
            parse_text("-0.00001").expect("negative numeric"),
        )),
        Value::Typed(TypedValue::Numeric(Numeric::NaN)),
        Value::Typed(TypedValue::Numeric(Numeric::Infinity)),
        Value::Typed(TypedValue::Numeric(Numeric::NegInfinity)),
        Value::Float64(1.0),
        Value::String("1".to_owned()),
    ];

    // Act
    let actual = std::iter::once(None)
        .chain(values.iter().map(Some))
        .map(weight)
        .collect::<Vec<_>>();

    // Assert
    for observed in actual {
        let error = observed.expect_err("invalid graph weight");
        let QueryError::Cassie(diagnostic) = error else {
            panic!("unexpected graph weight diagnostic: {error}");
        };
        assert_eq!(diagnostic.sql_state(), "22023");
        assert_eq!(
            diagnostic.to_string(),
            "a selected edge has a missing, negative or non-finite weight",
        );
    }
}
