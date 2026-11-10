use super::width_bucket_float;
use crate::types::Value;

#[test]
fn should_bucket_finite_values_when_the_bound_span_overflows() {
    // Arrange
    let cases = [
        (0.0, -1e308, 1e308, 6),
        (5e307, -1e308, 1e308, 8),
        (0.0, 1e308, -1e308, 6),
        (-5e307, 1e308, -1e308, 8),
        (-1e308, -1e308, 1e308, 1),
        (1e308, -1e308, 1e308, 11),
    ];

    for (operand, low, high, expected) in cases {
        // Act
        let result = width_bucket_float(operand, low, high, 10);

        // Assert
        assert!(matches!(result, Ok(Value::Int64(value)) if value == expected));
    }
}

#[test]
fn should_reject_equal_positive_with_negative_zero_bounds() {
    // Arrange
    let bounds = [(0.0, -0.0), (-0.0, 0.0)];

    for (low, high) in bounds {
        // Act
        let result = width_bucket_float(0.0, low, high, 10);

        // Assert
        assert!(
            matches!(result, Err(crate::executor::QueryError::Cassie(crate::app::CassieError::TypeInput(error))) if error.sqlstate == "2201G" && error.message == "lower bound cannot equal upper bound")
        );
    }
}

#[test]
fn should_clamp_near_upper_bounds_without_overflowing_either_direction() {
    // Arrange
    let highest = f64::MAX;
    let just_below = f64::from_bits(highest.to_bits() - 1);
    let cases = [
        (just_below, -highest, highest, 10, 10),
        (-just_below, highest, -highest, 10, 10),
        (
            just_below,
            0.0,
            highest,
            i64::from(i32::MAX),
            i64::from(i32::MAX),
        ),
        (
            -just_below,
            0.0,
            -highest,
            i64::from(i32::MAX),
            i64::from(i32::MAX),
        ),
        (f64::INFINITY, -1e308, 1e308, 10, 11),
        (f64::NEG_INFINITY, -1e308, 1e308, 10, 0),
    ];

    for (operand, low, high, count, expected) in cases {
        // Act
        let result = width_bucket_float(operand, low, high, count);

        // Assert
        assert!(matches!(result, Ok(Value::Int64(value)) if value == expected));
    }
}
