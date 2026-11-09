use super::Numeric;
use num_traits::Zero;

#[test]
fn should_return_zero_for_extreme_negative_scales_without_changing_scale() {
    // Arrange
    let values = [
        Numeric::from_i64(1),
        Numeric::from_i64(-1),
        Numeric::from_i64(5),
        Numeric::from_i64(-5),
    ];

    for value in values {
        // Act
        let rounded = value.round_to(i32::MIN);
        let truncated = value.truncate_to(i32::MIN);

        // Assert
        assert!(matches!(rounded, Numeric::Finite { mantissa, scale: 0 } if mantissa.is_zero()));
        assert!(matches!(truncated, Numeric::Finite { mantissa, scale: 0 } if mantissa.is_zero()));
    }
}

#[test]
fn should_keep_zero_display_scale_when_rescaling_or_truncating() {
    // Arrange
    let value = Numeric::zero();

    // Act
    let rounded = value.round_to(1000);
    let truncated = value.truncate_to(1000);
    let downscaled = value.round_to(i32::MIN);

    // Assert
    assert!(matches!(rounded, Numeric::Finite { mantissa, scale: 1000 } if mantissa.is_zero()));
    assert!(matches!(truncated, Numeric::Finite { mantissa, scale: 1000 } if mantissa.is_zero()));
    assert!(matches!(downscaled, Numeric::Finite { mantissa, scale: 0 } if mantissa.is_zero()));
}

#[test]
fn should_preserve_half_away_rounding_and_truncation_at_the_boundary() {
    // Arrange
    let cases = [
        (4, 0, 0),
        (5, 10, 0),
        (-4, 0, 0),
        (-5, -10, 0),
        (15, 20, 10),
        (-15, -20, -10),
    ];

    for (input, rounded, truncated) in cases {
        // Act
        let value = Numeric::from_i64(input);
        let actual_round = value.round_to(-1);
        let actual_trunc = value.truncate_to(-1);

        // Assert
        assert_eq!(actual_round, Numeric::from_i64(rounded));
        assert_eq!(actual_trunc, Numeric::from_i64(truncated));
    }
}

#[test]
fn should_preserve_numeric_special_values_at_extreme_scales() {
    // Arrange
    let values = [Numeric::NaN, Numeric::Infinity, Numeric::NegInfinity];

    for value in values {
        // Act
        let rounded = value.round_to(i32::MIN);
        let truncated = value.truncate_to(i32::MAX);

        // Assert
        assert_eq!(rounded, value);
        assert_eq!(truncated, value);
    }
}
