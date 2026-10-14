//! PostgreSQL 18 temporal type and expression grammar tracked by #814.

use cassie::types::Value;

use crate::support_sql_fixture::{sql_fixture, SqlFixture};

fn text_row(fixture: &SqlFixture, sql: &str) -> Vec<Value> {
    let result = fixture.execute(sql).unwrap_or_else(|error| {
        panic!("temporal expression must execute: {sql}: {error}");
    });
    assert_eq!(result.rows().len(), 1, "{sql}");
    result.rows()[0].clone()
}

#[test]
fn should_round_timestamp_with_time_zone_precision() {
    // Arrange
    let fixture = sql_fixture("issue-814-timestamptz-precision", &["SET TIME ZONE 'UTC'"]);

    // Act
    let actual = text_row(
        &fixture,
        "SELECT CAST('2024-01-02 03:04:05.123456+00' AS TIMESTAMPTZ(3))::text, \
         CAST('2024-01-02 03:04:05+00' AS TIMESTAMP(3) WITH TIME ZONE)::text",
    );

    // Assert
    assert_eq!(
        actual,
        vec![
            Value::String("2024-01-02 03:04:05.123+00".to_string()),
            Value::String("2024-01-02 03:04:05+00".to_string()),
        ]
    );
}

#[test]
fn should_round_time_with_time_zone_precision() {
    // Arrange
    let fixture = sql_fixture("issue-814-timetz-precision", &[]);

    // Act
    let actual = text_row(
        &fixture,
        "SELECT CAST('04:05:06.789-08:00' AS TIMETZ(2))::text, \
         CAST('04:05:06.789-08:00' AS TIME(2) WITH TIME ZONE)::text",
    );

    // Assert
    assert_eq!(
        actual,
        vec![
            Value::String("04:05:06.79-08".to_string()),
            Value::String("04:05:06.79-08".to_string()),
        ]
    );
}

#[test]
fn should_preserve_local_timestamp_fractional_precision() {
    // Arrange
    let fixture = sql_fixture("issue-814-local-time-precision", &[]);

    // Act
    let actual = text_row(
        &fixture,
        "SELECT CAST('2001-02-16 20:38:40.123456' AS TIMESTAMP(6) WITHOUT TIME ZONE)::text, \
         CAST('04:05:06.123456' AS TIME(6) WITHOUT TIME ZONE)::text",
    );

    // Assert
    assert_eq!(
        actual,
        vec![
            Value::String("2001-02-16 20:38:40.123456".to_string()),
            Value::String("04:05:06.123456".to_string()),
        ]
    );
}

#[test]
fn should_format_interval_qualifiers_with_sql_standard_style() {
    // Arrange
    let fixture = sql_fixture(
        "issue-814-interval-qualifier-matrix",
        &["SET intervalstyle TO sql_standard"],
    );

    // Act
    let actual = text_row(
        &fixture,
        "SELECT CAST('1 year 2 months' AS INTERVAL YEAR TO MONTH)::text, \
         CAST('1' AS INTERVAL YEAR)::text, CAST('1' AS INTERVAL MONTH)::text, \
         CAST('1' AS INTERVAL DAY)::text, CAST('1' AS INTERVAL HOUR)::text, \
         CAST('1' AS INTERVAL MINUTE)::text, CAST('1.25' AS INTERVAL SECOND(2))::text, \
         CAST('1 2:03:04' AS INTERVAL DAY TO HOUR)::text, \
         CAST('1 2:03:04' AS INTERVAL DAY TO MINUTE)::text, \
         CAST('3 days 4:05:06.789' AS INTERVAL DAY TO SECOND(3))::text, \
         CAST('4:05:06' AS INTERVAL HOUR TO MINUTE)::text, \
         CAST('4:05:06.789' AS INTERVAL HOUR TO SECOND(3))::text, \
         CAST('5:06.789' AS INTERVAL MINUTE TO SECOND(3))::text, \
         CAST('1 second' AS INTERVAL(3))::text",
    );

    // Assert
    assert_eq!(
        actual,
        vec![
            Value::String("1-2".to_string()),
            Value::String("1-0".to_string()),
            Value::String("0-1".to_string()),
            Value::String("1 0:00:00".to_string()),
            Value::String("1:00:00".to_string()),
            Value::String("0:01:00".to_string()),
            Value::String("0:00:01.25".to_string()),
            Value::String("1 2:00:00".to_string()),
            Value::String("1 2:03:00".to_string()),
            Value::String("3 4:05:06.789".to_string()),
            Value::String("4:05:00".to_string()),
            Value::String("4:05:06.789".to_string()),
            Value::String("0:05:06.789".to_string()),
            Value::String("0:00:01".to_string()),
        ]
    );
}

#[test]
fn should_format_mixed_interval_signs_with_sql_standard_style() {
    // Arrange
    let fixture = sql_fixture(
        "issue-814-interval-sql-standard-output",
        &["SET intervalstyle TO sql_standard"],
    );

    // Act
    let actual = text_row(
        &fixture,
        "SELECT INTERVAL '1 year 2 months 3 days 4 hours 5 minutes 6.007 seconds'::text",
    );

    // Assert
    assert_eq!(
        actual,
        vec![Value::String("+1-2 +3 +4:05:06.007".to_string())]
    );
}

#[test]
fn should_evaluate_timestamp_at_time_zone_directions() {
    // Arrange
    let fixture = sql_fixture(
        "issue-814-at-time-zone-directions",
        &["SET TIME ZONE 'UTC'"],
    );

    // Act
    let actual = text_row(
        &fixture,
        "SELECT (TIMESTAMP '2001-02-16 20:38:40' AT TIME ZONE 'America/Denver')::text, \
         (TIMESTAMPTZ '2001-02-16 20:38:40-05' AT TIME ZONE 'America/Denver')::text, \
         (TIMESTAMP '2001-02-16 20:38:40' AT TIME ZONE INTERVAL '-08:00')::text",
    );

    // Assert
    assert_eq!(
        actual,
        vec![
            Value::String("2001-02-17 03:38:40+00".to_string()),
            Value::String("2001-02-16 18:38:40".to_string()),
            Value::String("2001-02-17 04:38:40+00".to_string()),
        ]
    );
}

#[test]
fn should_evaluate_at_local_for_timestamp_values() {
    // Arrange
    let fixture = sql_fixture("issue-814-at-local-values", &["SET TIME ZONE 'UTC'"]);

    // Act
    let actual = text_row(
        &fixture,
        "SELECT (TIMESTAMP '2001-02-16 20:38:40' AT LOCAL)::text, \
         (TIMESTAMPTZ '2001-02-16 20:38:40-05' AT LOCAL)::text, \
         (TIME WITH TIME ZONE '20:38:40-05' AT LOCAL)::text",
    );

    // Assert
    assert_eq!(
        actual,
        vec![
            Value::String("2001-02-16 20:38:40+00".to_string()),
            Value::String("2001-02-17 01:38:40".to_string()),
            Value::String("01:38:40+00".to_string()),
        ]
    );
}

#[test]
fn should_evaluate_temporal_arithmetic_with_postgres_outputs() {
    // Arrange
    let fixture = sql_fixture(
        "issue-814-temporal-arithmetic",
        &["SET intervalstyle TO sql_standard"],
    );

    // Act
    let actual = text_row(
        &fixture,
        "SELECT (DATE '2001-09-28' + 7)::text, (DATE '2001-09-28' - 7)::text, \
         (DATE '2001-09-28' - DATE '2001-09-27')::text, \
         (DATE '2001-09-28' + TIME '03:00')::text, \
         (DATE '2001-09-28' + INTERVAL '1 month')::text, \
         (DATE '2001-09-28' - INTERVAL '1 day')::text, \
         (TIME '05:00' + INTERVAL '1 hour')::text, \
         (TIME '05:00' - INTERVAL '1 hour')::text, \
         (TIME '05:00' - TIME '03:00')::text, \
         (TIMESTAMP '2001-09-28 23:00' - TIMESTAMP '2001-09-28 01:00')::text, \
         (INTERVAL '1 day' * 2.5)::text, (INTERVAL '1 hour' / 1.5)::text, \
         (- INTERVAL '23 hours')::text",
    );

    // Assert
    assert_eq!(
        actual,
        vec![
            Value::String("2001-10-05".to_string()),
            Value::String("2001-09-21".to_string()),
            Value::String("1".to_string()),
            Value::String("2001-09-28 03:00:00".to_string()),
            Value::String("2001-10-28 00:00:00".to_string()),
            Value::String("2001-09-27 00:00:00".to_string()),
            Value::String("06:00:00".to_string()),
            Value::String("04:00:00".to_string()),
            Value::String("2:00:00".to_string()),
            Value::String("22:00:00".to_string()),
            Value::String("2 12:00:00".to_string()),
            Value::String("0:40:00".to_string()),
            Value::String("-23:00:00".to_string()),
        ]
    );
}

#[test]
fn should_evaluate_timezone_function_overloads() {
    // Arrange
    let fixture = sql_fixture("issue-814-timezone-overloads", &["SET TIME ZONE 'UTC'"]);

    // Act
    let actual = text_row(
        &fixture,
        "SELECT timezone('America/Denver', TIMESTAMP '2001-02-16 20:38:40')::text, \
         timezone('UTC', TIME WITH TIME ZONE '05:34:17-05')::text, \
         timezone(TIMESTAMP '2001-02-16 20:38:40')::text, \
         timezone(TIME WITH TIME ZONE '05:34:17-05')::text",
    );

    // Assert
    assert_eq!(
        actual,
        vec![
            Value::String("2001-02-17 03:38:40+00".to_string()),
            Value::String("10:34:17+00".to_string()),
            Value::String("2001-02-16 20:38:40+00".to_string()),
            Value::String("10:34:17+00".to_string()),
        ]
    );
}
