use super::{make_time, make_timestamp};
use crate::types::extended::temporal::calendar::USECS_PER_DAY;

#[test]
fn should_validate_seconds_after_ties_to_even_rounding() {
    // Arrange
    let cases = [
        (-0.000_000_1, 0),
        (-0.000_000_5, 0),
        (60.000_000_1, 60_000_000),
        (60.000_000_5, 60_000_000),
        (0.000_001_5, 2),
    ];

    for (seconds, expected) in cases {
        // Act
        let result = make_time(1, 2, seconds);

        // Assert
        assert_eq!(
            result.expect("rounded time"),
            3_720_000_000 + expected,
            "{seconds}"
        );
    }
}

#[test]
fn should_reject_rounded_seconds_outside_the_valid_range() {
    // Arrange
    let cases = [
        -0.000_000_6,
        60.000_000_6,
        -1e300,
        1e300,
        f64::NAN,
        f64::INFINITY,
        f64::NEG_INFINITY,
    ];

    for seconds in cases {
        // Act
        let result = make_time(1, 2, seconds);

        // Assert
        assert!(
            matches!(result, Err(error) if error.sqlstate == "22008"),
            "{seconds}"
        );
    }
}

#[test]
fn should_keep_end_of_day_with_timestamp_field_validation() {
    // Arrange
    let end = make_time(24, 0, -0.000_000_5);
    let over = make_time(24, 0, 0.000_000_6);
    let bad_minute = make_time(24, 1, -0.000_000_1);

    // Act
    let timestamp = make_timestamp((2000, 1, 1, 1, 2), -0.000_000_5);
    let bad_date = make_timestamp((2000, 13, 1, 1, 2), -0.000_000_5);

    // Assert
    assert_eq!(end.expect("end of day"), USECS_PER_DAY);
    assert!(matches!(over, Err(error) if error.sqlstate == "22008"));
    assert!(matches!(bad_minute, Err(error) if error.sqlstate == "22008"));
    assert!(timestamp.is_ok());
    assert!(
        matches!(bad_date, Err(error) if error.message.starts_with("date field value out of range:"))
    );
}
