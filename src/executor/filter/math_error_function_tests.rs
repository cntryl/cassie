use super::{erf, erfc};

#[test]
fn should_terminate_at_signed_zero_and_subnormal_inputs() {
    // Arrange
    let positive_zero = 0.0_f64;
    let negative_zero = -0.0_f64;
    let positive_subnormal = f64::from_bits(1);
    let negative_subnormal = f64::from_bits((1_u64 << 63) | 1);
    let larger_subnormal = f64::MIN_POSITIVE / 2.0;

    // Act
    let values = [
        erf(positive_zero),
        erf(negative_zero),
        erf(positive_subnormal),
        erf(negative_subnormal),
    ];
    let complementary = [
        erfc(positive_zero),
        erfc(negative_zero),
        erfc(positive_subnormal),
        erfc(negative_subnormal),
    ];
    let larger = [erf(larger_subnormal), erf(-larger_subnormal)];

    // Assert
    assert_eq!(
        values.map(f64::to_bits),
        [0, 1_u64 << 63, 1, (1_u64 << 63) | 1]
    );
    assert_eq!(complementary, [1.0; 4]);
    assert!(larger[0].is_finite() && larger[0] > 0.0);
    assert!(larger[1].is_finite() && larger[1] < 0.0);
}

#[test]
fn should_preserve_large_and_special_error_function_results() {
    // Arrange
    let inputs = [f64::MAX, -f64::MAX, f64::INFINITY, f64::NEG_INFINITY];

    // Act
    let ordinary = inputs.map(erf);
    let complementary = inputs.map(erfc);
    let nan = [erf(f64::NAN), erfc(f64::NAN)];

    // Assert
    assert_eq!(ordinary, [1.0, -1.0, 1.0, -1.0]);
    assert_eq!(complementary, [0.0, 2.0, 0.0, 2.0]);
    assert!(nan.into_iter().all(f64::is_nan));
}

#[test]
fn should_preserve_normal_error_function_convergence() {
    // Arrange
    let input = 1.0;

    // Act
    let ordinary = erf(input);
    let complementary = erfc(input);

    // Assert
    assert!((ordinary - 0.842_700_792_949_714_9).abs() < 1e-14);
    assert!((complementary - 0.157_299_207_050_285_13).abs() < 1e-14);
}
