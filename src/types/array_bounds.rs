//! PostgreSQL's array dimension limits, shared by every array constructor.
//!
//! PostgreSQL rejects a dimension whose upper bound would reach `INT_MAX`
//! (`array upper bound is too large`) or whose length exceeds the maximum
//! allocatable array (`array size exceeds the maximum allowed`), both with
//! SQLSTATE 54000 (`program_limit_exceeded`).

use super::extended::TypeInputError;

/// Largest element count of one array dimension (PostgreSQL `MaxArraySize` for 8-byte datums).
pub const MAX_ARRAY_LENGTH: i64 = 134_217_727;

/// Reject a dimension PostgreSQL would refuse.
///
/// The upper bound is `lower_bound + length - 1`, computed without overflow.
///
/// # Errors
/// Returns a `54000` input error when the upper bound reaches `i32::MAX` or
/// the length exceeds [`MAX_ARRAY_LENGTH`].
pub(crate) fn check_dimension_bounds(lower_bound: i32, length: i64) -> Result<(), TypeInputError> {
    let upper_bound = i64::from(lower_bound)
        .saturating_add(length)
        .saturating_sub(1);
    if upper_bound >= i64::from(i32::MAX) {
        return Err(TypeInputError::new(
            "54000",
            format!("array upper bound is too large: {upper_bound}"),
        ));
    }
    if length > MAX_ARRAY_LENGTH {
        return Err(TypeInputError::new(
            "54000",
            format!("array size exceeds the maximum allowed ({MAX_ARRAY_LENGTH})"),
        ));
    }
    Ok(())
}

/// `lower_bound + length - 1` for a stored dimension, or `None` on overflow.
#[must_use]
pub fn checked_upper_bound(lower_bound: i32, length: i32) -> Option<i32> {
    lower_bound.checked_add(length.checked_sub(1)?)
}

#[cfg(test)]
mod tests {
    use super::{check_dimension_bounds, checked_upper_bound};

    #[test]
    fn should_reject_upper_bound_of_int_max() {
        // Arrange
        let lower = i32::MAX;

        // Act
        let error = check_dimension_bounds(lower, 1).unwrap_err();

        // Assert
        assert_eq!(error.sqlstate, "54000");
        assert_eq!(error.message, "array upper bound is too large: 2147483647");
    }

    #[test]
    fn should_reject_length_beyond_the_array_size_limit() {
        // Arrange
        let length = 134_217_728;

        // Act
        let error = check_dimension_bounds(1, length).unwrap_err();

        // Assert
        assert_eq!(error.sqlstate, "54000");
        assert!(error.message.contains("array size exceeds"));
    }

    #[test]
    fn should_accept_upper_bound_just_below_int_max() {
        // Arrange
        let lower = i32::MAX - 1;

        // Act
        let result = check_dimension_bounds(lower, 1);

        // Assert
        assert!(result.is_ok());
    }

    #[test]
    fn should_return_none_when_upper_bound_overflows() {
        // Arrange
        let (lower, length) = (i32::MAX, 2);

        // Act
        let upper = checked_upper_bound(lower, length);

        // Assert
        assert_eq!(upper, None);
    }

    #[test]
    fn should_compute_upper_bound_without_overflow_at_int_max() {
        // Arrange
        let (lower, length) = (i32::MAX, 1);

        // Act
        let upper = checked_upper_bound(lower, length);

        // Assert
        assert_eq!(upper, Some(i32::MAX));
    }
}
