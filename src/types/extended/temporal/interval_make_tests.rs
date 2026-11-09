use super::make_interval;

#[test]
fn should_reject_finite_seconds_outside_interval_microsecond_range() {
    // Arrange
    let inputs = [1e300, -1e300, 9_223_372_036_854.775, -9_223_372_036_854.777];

    for seconds in inputs {
        // Act
        let result = make_interval([0; 6], seconds);

        // Assert
        assert!(
            matches!(result, Err(error) if error.sqlstate == "22008" && error.message == "interval out of range"),
            "{seconds}"
        );
    }
}

#[test]
fn should_accept_representable_signed_microsecond_boundaries() {
    // Arrange
    let cases = [
        (9_223_372_036_854.773, 9_223_372_036_854_773_760_i64),
        (-9_223_372_036_854.775, i64::MIN),
        (-9_223_372_036_854.776, i64::MIN),
    ];

    for (seconds, expected) in cases {
        // Act
        let result = make_interval([0; 6], seconds);

        // Assert
        assert_eq!(
            result.expect("finite boundary").micros,
            expected,
            "{seconds}"
        );
    }
}

#[test]
fn should_preserve_ties_to_even_for_fractional_interval_seconds() {
    // Arrange
    let cases = [
        (0.000_000_5, 0),
        (0.000_001_5, 2),
        (-0.000_000_5, 0),
        (-0.000_001_5, -2),
    ];

    for (seconds, expected) in cases {
        // Act
        let result = make_interval([0; 6], seconds);

        // Assert
        assert_eq!(
            result.expect("rounded interval").micros,
            expected,
            "{seconds}"
        );
    }
}

#[test]
fn should_preserve_float_component_and_infinity_error_priority() {
    // Arrange
    let cases = [
        ([0; 6], 1e308, "22003", "value out of range: overflow"),
        ([0; 6], -1e308, "22003", "value out of range: overflow"),
        (
            [i32::MAX, 0, 0, 0, 0, 0],
            1e308,
            "22008",
            "interval out of range",
        ),
        (
            [0, 0, i32::MAX, 0, 0, 0],
            1e308,
            "22008",
            "interval out of range",
        ),
        ([0; 6], f64::INFINITY, "22008", "interval out of range"),
        ([0; 6], f64::NAN, "22008", "interval out of range"),
        (
            [0, i32::MIN, 0, i32::MIN, 0, 0],
            -9_223_372_036_854.775,
            "22008",
            "interval out of range",
        ),
    ];

    for (components, seconds, state, message) in cases {
        // Act
        let result = make_interval(components, seconds);

        // Assert
        assert!(
            matches!(result, Err(error) if error.sqlstate == state && error.message == message)
        );
    }
}
