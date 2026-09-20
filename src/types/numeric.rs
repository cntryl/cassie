//! Lint-clean integer-to-float conversions shared across the engine.
//!
//! `as` casts between integers and `f64` are lossy or saturating in ways that
//! hide bugs, so conversions go through these helpers instead.

const TWO_TO_32: f64 = 4_294_967_296.0;

/// Converts `value` to the nearest `f64` (round-half-to-even), exactly like a
/// correctly rounded cast.
#[must_use]
pub fn u64_to_f64(value: u64) -> f64 {
    let bytes = value.to_be_bytes();
    let high = u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
    let low = u32::from_be_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]);
    // `high * 2^32` is exact, so the fused add rounds only once.
    f64::from(high).mul_add(TWO_TO_32, f64::from(low))
}

/// Converts `value` to the nearest `f64`.
#[must_use]
pub fn i64_to_f64(value: i64) -> f64 {
    let converted = u64_to_f64(value.unsigned_abs());
    if value.is_negative() {
        -converted
    } else {
        converted
    }
}

/// Converts `value` to the nearest `f64`.
#[must_use]
pub fn usize_to_f64(value: usize) -> f64 {
    // `usize` never exceeds `u64` on any supported target; the saturating
    // fallback keeps the conversion total instead of panicking.
    u64_to_f64(u64::try_from(value).unwrap_or(u64::MAX))
}

/// Converts `value` to `f64` only when the conversion is exact.
///
/// An integer is exactly representable when its magnitude, with trailing
/// zero bits removed, fits in the 53-bit significand.
#[must_use]
pub fn exact_i64_to_f64(value: i64) -> Option<f64> {
    let magnitude = value.unsigned_abs();
    let significant = magnitude
        .checked_shr(magnitude.trailing_zeros())
        .unwrap_or(0);
    (significant < (1_u64 << 53)).then(|| i64_to_f64(value))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parsed(value: &impl ToString) -> f64 {
        value
            .to_string()
            .parse::<f64>()
            .expect("integers parse as f64")
    }

    #[test]
    fn should_convert_only_exactly_representable_integers() {
        // Arrange
        let exact = [0, 1, -1, 1 << 53, -(1 << 53), i64::MIN, 3 << 60];
        let inexact = [(1 << 53) + 1, i64::MAX, i64::MIN + 1];

        // Act
        let converted = exact.map(|value| exact_i64_to_f64(value).map(f64::to_bits));
        let rejected = inexact.map(exact_i64_to_f64);

        // Assert
        assert_eq!(converted, exact.map(|value| Some(parsed(&value).to_bits())));
        assert_eq!(rejected, [None, None, None]);
    }

    #[test]
    fn should_round_like_a_correctly_rounded_cast() {
        // Arrange
        let values = [
            i64::MAX,
            i64::MIN,
            (1 << 53) + 1,
            123_456_789_012_345_678,
            -7,
        ];

        // Act
        let converted = values.map(|value| i64_to_f64(value).to_bits());

        // Assert
        assert_eq!(converted, values.map(|value| parsed(&value).to_bits()));
        assert_eq!(
            usize_to_f64(usize::MAX).to_bits(),
            u64_to_f64(u64::MAX).to_bits()
        );
    }
}
