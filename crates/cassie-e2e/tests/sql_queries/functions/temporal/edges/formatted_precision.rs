//! Formatted timestamp precision tracked by #815; native-oracle cases.

use cassie::app::CassieError;
use cassie::types::Value;

use crate::support_sql_fixture::sql_fixture;

#[test]
fn should_round_formatted_timestamp_to_ff_precision() {
    // Arrange
    let fixture = sql_fixture("formatted-ff-precision", &["SET TIME ZONE 'UTC'"]);
    let cases = [
        ("FF1", "123456", "1"),
        ("FF2", "123456", "12"),
        ("FF3", "1235", "124"),
        ("FF4", "123456", "1235"),
        ("FF5", "123456", "12346"),
        ("FF6", "123456", "123456"),
    ];

    for (field, input, expected) in cases {
        // Act
        let result = fixture.execute(&format!(
            "SELECT to_timestamp('2000-01-01 12:00:00.{input}', 'YYYY-MM-DD HH24:MI:SS.{field}') = TIMESTAMPTZ '2000-01-01 12:00:00.{expected}+00'"
        ));

        // Assert
        assert_eq!(
            result.expect("FF rounds the resulting instant").rows(),
            &[vec![Value::Bool(true)]]
        );
    }
}

#[test]
fn should_round_pre_epoch_formatted_midpoints_away_from_epoch() {
    // Arrange
    let fixture = sql_fixture("formatted-ff-pre-epoch", &["SET TIME ZONE 'UTC'"]);

    // Act
    let result = fixture.execute(
        "SELECT to_timestamp('1999-12-31 12:00:00.1235', 'YYYY-MM-DD HH24:MI:SS.FF3') = TIMESTAMPTZ '1999-12-31 12:00:00.123+00'",
    );

    // Assert
    assert_eq!(
        result
            .expect("negative instant midpoint rounds away from epoch")
            .rows(),
        &[vec![Value::Bool(true)]]
    );
}

#[test]
fn should_apply_ff_rounding_after_explicit_timezone_conversion() {
    // Arrange
    let fixture = sql_fixture("formatted-ff-explicit-zone", &["SET TIME ZONE 'UTC'"]);

    // Act
    let result = fixture.execute(
        "SELECT to_timestamp('2000-01-01 00:30:00.1235 +01', 'YYYY-MM-DD HH24:MI:SS.FF3 TZH') = TIMESTAMPTZ '1999-12-31 23:30:00.123+00'",
    );

    // Assert
    assert_eq!(
        result
            .expect("round the UTC instant rather than local fields")
            .rows(),
        &[vec![Value::Bool(true)]]
    );
}

#[test]
fn should_apply_ff_rounding_after_session_timezone_conversion() {
    // Arrange
    let fixture = sql_fixture("formatted-ff-session-zone", &["SET TIME ZONE '+01'"]);

    // Act
    let result = fixture.execute(
        "SELECT to_timestamp('2000-01-01 00:30:00.1235', 'YYYY-MM-DD HH24:MI:SS.FF3') = TIMESTAMPTZ '1999-12-31 23:30:00.123+00'",
    );

    // Assert
    assert_eq!(
        result
            .expect("session timezone precedes precision rounding")
            .rows(),
        &[vec![Value::Bool(true)]]
    );
}

#[test]
fn should_carry_ff_rounding_into_the_next_day() {
    // Arrange
    let fixture = sql_fixture("formatted-ff-day-carry", &["SET TIME ZONE 'UTC'"]);

    // Act
    let result = fixture.execute(
        "SELECT to_timestamp('2000-01-01 23:59:59.9995', 'YYYY-MM-DD HH24:MI:SS.FF3') = TIMESTAMPTZ '2000-01-02 00:00:00+00'",
    );

    // Assert
    assert_eq!(
        result
            .expect("fraction rounding carries across midnight")
            .rows(),
        &[vec![Value::Bool(true)]]
    );
}

#[test]
fn should_reject_ff_rounding_beyond_the_timestamp_limit() {
    // Arrange
    let fixture = sql_fixture("formatted-ff-range-carry", &["SET TIME ZONE 'UTC'"]);

    // Act
    let result = fixture.execute(
        "SELECT to_timestamp('294276-12-31 23:59:59.9995 +00', 'YYYY-MM-DD HH24:MI:SS.FF3 TZH')",
    );

    // Assert
    assert!(matches!(result, Err(CassieError::TypeInput(error)) if error.sqlstate == "22008"));
}

#[test]
fn should_keep_fixed_fraction_width_before_a_numeric_template_field() {
    // Arrange
    let fixture = sql_fixture("formatted-ff-numeric-adjacency", &["SET TIME ZONE 'UTC'"]);

    // Act
    let result = fixture.execute(
        "SELECT to_timestamp('2024-01-01 12:34:12356', 'YYYY-MM-DD HH24:MI:FF3SS') = TIMESTAMPTZ '2024-01-01 12:34:56.123+00'",
    );

    // Assert
    assert_eq!(
        result.expect("adjacent SS retains its own digits").rows(),
        &[vec![Value::Bool(true)]]
    );
}
