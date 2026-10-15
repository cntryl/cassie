//! Residual PostgreSQL 18 temporal function edges tracked by #815.

use cassie::types::Value;

use crate::support_sql_fixture::sql_fixture;

#[test]
fn should_bin_timestamp_before_origin_toward_negative_infinity() {
    // Arrange
    let fixture = sql_fixture("issue-815-date-bin-negative-timestamp", &[]);

    // Act
    let result = fixture.execute(
        "SELECT DATE_BIN(INTERVAL '1 hour', TIMESTAMP '1999-12-31 23:30:00', TIMESTAMP '2000-01-01 00:00:00') = TIMESTAMP '1999-12-31 23:00:00'",
    );

    // Assert
    assert_eq!(
        result.expect("timestamp before date_bin origin").rows(),
        &[vec![Value::Bool(true)]]
    );
}

#[test]
fn should_bin_timestamptz_before_origin_toward_negative_infinity() {
    // Arrange
    let fixture = sql_fixture(
        "issue-815-date-bin-negative-timestamptz",
        &["SET TIME ZONE 'UTC'"],
    );

    // Act
    let result = fixture.execute(
        "SELECT DATE_BIN(INTERVAL '1 hour', TIMESTAMPTZ '1999-12-31 23:30:00+00', TIMESTAMPTZ '2000-01-01 00:00:00+00') = TIMESTAMPTZ '1999-12-31 23:00:00+00'",
    );

    // Assert
    assert_eq!(
        result.expect("timestamptz before date_bin origin").rows(),
        &[vec![Value::Bool(true)]]
    );
}

#[test]
fn should_truncate_timestamptz_to_named_zone_midnight_across_dst() {
    // Arrange
    let fixture = sql_fixture(
        "issue-815-date-trunc-dst-day-length",
        &["SET TIME ZONE 'UTC'"],
    );

    // Act
    let result = fixture.execute(
        "SELECT DATE_TRUNC('day', TIMESTAMPTZ '2024-03-10 16:00:00+00', 'America/New_York') = TIMESTAMPTZ '2024-03-10 05:00:00+00' AND DATE_TRUNC('day', TIMESTAMPTZ '2024-03-11 16:00:00+00', 'America/New_York') = TIMESTAMPTZ '2024-03-11 04:00:00+00' AND EXTRACT(EPOCH FROM (DATE_TRUNC('day', TIMESTAMPTZ '2024-03-11 16:00:00+00', 'America/New_York') - DATE_TRUNC('day', TIMESTAMPTZ '2024-03-10 16:00:00+00', 'America/New_York'))) = 82800",
    );

    // Assert
    assert_eq!(
        result
            .expect("named-zone date_trunc across spring DST")
            .rows(),
        &[vec![Value::Bool(true)]]
    );
}
