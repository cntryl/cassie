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

#[test]
fn should_preserve_exact_microseconds_when_extracting_large_interval_epochs() {
    // Arrange
    let fixture = sql_fixture("issue-815-exact-large-interval-epoch", &[]);
    let cases = [
        ("100000000 days 00:00:00.000001", "8640000000000.000001"),
        ("-100000000 days -00:00:00.000001", "-8640000000000.000001"),
        (
            "1 year 1 month 100000000 days 00:00:00.000001",
            "8640034149600.000001",
        ),
        (
            "-1 year -1 month -100000000 days -00:00:00.000001",
            "-8640034149600.000001",
        ),
    ];

    for (input, expected) in cases {
        // Act
        let result = fixture.execute(&format!(
            "SELECT EXTRACT(EPOCH FROM INTERVAL '{input}')::text"
        ));

        // Assert
        assert_eq!(
            result.expect("exact interval epoch").rows(),
            &[vec![Value::String(expected.to_owned())]],
            "PostgreSQL 18.6 interval epoch: {input}"
        );
    }
}

#[test]
fn should_reject_dates_outside_timestamp_range_when_formatting() {
    // Arrange
    let fixture = sql_fixture("issue-815-to-char-date-range", &[]);

    // Act
    let boundary = fixture.execute("SELECT TO_CHAR(DATE '294276-12-31', 'YYYY-MM-DD')");
    let null_value = fixture.execute("SELECT TO_CHAR(NULL::date, 'YYYY-MM-DD')");
    let rejected = ["294277-01-01", "5874897-12-31"].map(|input| {
        let result = fixture.execute(&format!("SELECT TO_CHAR(DATE '{input}', 'YYYY-MM-DD')"));
        let recovery = fixture.execute("SELECT 1");
        (input, result, recovery)
    });

    // Assert
    assert_eq!(
        boundary.expect("last representable timestamp date").rows(),
        &[vec![Value::String("294276-12-31".to_owned())]]
    );
    assert_eq!(
        null_value.expect("strict NULL date formatting").rows(),
        &[vec![Value::Null]]
    );

    for (input, result, recovery) in rejected {
        let error = result.expect_err("DATE exceeds timestamp formatting range");
        assert!(
            matches!(&error, cassie::app::CassieError::TypeInput(input_error) if input_error.sqlstate == "22008"),
            "PostgreSQL 18.6 range error for {input}: {error:?}"
        );
        assert_eq!(
            recovery.expect("same-session recovery").rows(),
            &[vec![Value::Int64(1)]]
        );
    }
}

#[test]
fn should_return_null_when_formatting_infinite_temporal_values() {
    // Arrange
    let fixture = sql_fixture("issue-815-to-char-nonfinite", &[]);
    let inputs = [
        "DATE 'infinity'",
        "DATE '-infinity'",
        "TIMESTAMP 'infinity'",
        "TIMESTAMP '-infinity'",
        "TIMESTAMPTZ 'infinity'",
        "TIMESTAMPTZ '-infinity'",
        "INTERVAL 'infinity'",
        "INTERVAL '-infinity'",
    ];

    for input in inputs {
        // Act
        let result = fixture.execute(&format!("SELECT TO_CHAR({input}, 'YYYY-MM-DD')"));

        // Assert
        assert_eq!(
            result.expect("nonfinite temporal formatting").rows(),
            &[vec![Value::Null]],
            "PostgreSQL 18.6 nonfinite formatting: {input}"
        );
    }
}

#[test]
fn should_match_postgresql_interval_quarters_at_signed_month_boundaries() {
    // Arrange
    let fixture = sql_fixture("issue-815-signed-interval-quarter", &[]);
    let cases = [
        ("-1 month", "-1"),
        ("-3 months", "-2"),
        ("-12 months", "-1"),
        ("-13 months", "-1"),
        ("0 months", "1"),
        ("3 months", "2"),
        ("-2147483648 months", "1"),
        ("2147483647 months", "3"),
    ];

    for (input, expected) in cases {
        // Act
        let result = fixture.execute(&format!(
            "SELECT EXTRACT(QUARTER FROM INTERVAL '{input}')::text, DATE_PART('quarter', INTERVAL '{input}')::text"
        ));

        // Assert
        assert_eq!(
            result.expect("signed interval quarter").rows(),
            &[vec![
                Value::String(expected.to_owned()),
                Value::String(expected.to_owned()),
            ]],
            "PostgreSQL 18.6 interval quarter: {input}"
        );
    }
}
