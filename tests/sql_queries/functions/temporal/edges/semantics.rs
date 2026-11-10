//! Residual PostgreSQL 18 temporal function edges tracked by #815.

use cassie::types::Value;

use crate::support_sql_fixture::sql_fixture;

const WIDE_DATE_EXTRACT_FIELDS: [&str; 14] = [
    "year",
    "month",
    "day",
    "quarter",
    "week",
    "isoyear",
    "dow",
    "isodow",
    "doy",
    "decade",
    "century",
    "millennium",
    "julian",
    "epoch",
];

const WIDE_DATE_EXTRACT_CASES: [(&str, [&str; 14]); 7] = [
    (
        "294276-12-31",
        [
            "294276",
            "12",
            "31",
            "4",
            "52",
            "294276",
            "0",
            "7",
            "366",
            "29427",
            "2943",
            "295",
            "109203527",
            "9224317929600",
        ],
    ),
    (
        "294277-01-01",
        [
            "294277",
            "1",
            "1",
            "1",
            "1",
            "294277",
            "1",
            "1",
            "1",
            "29427",
            "2943",
            "295",
            "109203528",
            "9224318016000",
        ],
    ),
    (
        "294277-01-02",
        [
            "294277",
            "1",
            "2",
            "1",
            "1",
            "294277",
            "2",
            "2",
            "2",
            "29427",
            "2943",
            "295",
            "109203529",
            "9224318102400",
        ],
    ),
    (
        "5874897-12-30",
        [
            "5874897",
            "12",
            "30",
            "4",
            "1",
            "5874898",
            "1",
            "1",
            "364",
            "587489",
            "58749",
            "5875",
            "2147483492",
            "185331706905600",
        ],
    ),
    (
        "5874897-12-31",
        [
            "5874897",
            "12",
            "31",
            "4",
            "1",
            "5874898",
            "2",
            "2",
            "365",
            "587489",
            "58749",
            "5875",
            "2147483493",
            "185331706992000",
        ],
    ),
    (
        "4714-11-24 BC",
        [
            "-4714",
            "11",
            "24",
            "4",
            "48",
            "-4714",
            "1",
            "1",
            "328",
            "-472",
            "-48",
            "-5",
            "0",
            "-210866803200",
        ],
    ),
    (
        "2000-01-01",
        [
            "2000",
            "1",
            "1",
            "1",
            "52",
            "1999",
            "6",
            "6",
            "1",
            "200",
            "20",
            "2",
            "2451545",
            "946684800",
        ],
    ),
];

#[test]
fn should_extract_every_supported_field_from_dates_beyond_timestamp_range() {
    // Arrange
    let fixture = sql_fixture("issue-815-extract-wide-date", &[]);

    for (date, expected) in WIDE_DATE_EXTRACT_CASES {
        for (field, expected) in WIDE_DATE_EXTRACT_FIELDS.into_iter().zip(expected) {
            // Act
            let sql = format!("SELECT EXTRACT({field} FROM DATE '{date}')::text");
            let result = fixture.execute(&sql);

            // Assert
            assert_eq!(
                result.expect("date field").rows(),
                &[vec![Value::String(expected.to_owned())]],
                "PostgreSQL 18.6 {sql}"
            );
        }
    }

    let widened = fixture.execute("SELECT DATE_PART('year', DATE '294277-01-01')");
    let nulls = fixture.execute(
        "SELECT EXTRACT(year FROM NULL::date) IS NULL, EXTRACT(month FROM DATE 'infinity') IS NULL",
    );
    let numeric = fixture.execute(
        "SELECT EXTRACT(year FROM DATE '5874897-12-31'), EXTRACT(epoch FROM DATE '5874897-12-31')",
    );

    // Assert
    assert!(
        matches!(widened, Err(cassie::app::CassieError::TypeInput(error)) if error.sqlstate == "22008" && error.message == "date out of range for timestamp")
    );
    assert_eq!(
        nulls.expect("date nulls").rows(),
        &[vec![Value::Bool(true), Value::Bool(true)]]
    );
    let numeric = numeric.expect("date numeric type and scale");
    assert_eq!(
        numeric
            .columns()
            .iter()
            .map(|column| column.type_oid)
            .collect::<Vec<_>>(),
        [1700, 1700],
        "EXTRACT columns retain PostgreSQL NUMERIC descriptors"
    );
    assert_eq!(
        numeric.rows(),
        &[vec![
            Value::String("5874897".to_owned()),
            Value::String("185331706992000".to_owned()),
        ]]
    );
}

#[test]
fn should_reject_time_fields_in_extract_from_infinite_dates() {
    // Arrange
    let fixture = sql_fixture("issue-815-extract-date-unsupported-fields", &[]);
    let fields = [
        "hour",
        "minute",
        "second",
        "milliseconds",
        "microseconds",
        "timezone",
        "timezone_hour",
        "timezone_minute",
    ];

    for date in ["infinity", "-infinity", "2000-01-01", "5874897-12-31"] {
        for field in fields {
            // Act
            let result = fixture.execute(&format!("SELECT EXTRACT({field} FROM DATE '{date}')"));

            // Assert
            assert!(
                matches!(result, Err(cassie::app::CassieError::TypeInput(error))
                if error.sqlstate == "0A000"
                    && error.message == format!("unit \"{field}\" not supported for type date")),
                "PostgreSQL 18.6 EXTRACT({field} FROM DATE '{date}')"
            );
        }
    }

    let controls = fixture.execute("SELECT EXTRACT(hour FROM NULL::date) IS NULL, EXTRACT(timezone FROM NULL::date) IS NULL, EXTRACT(month FROM DATE 'infinity') IS NULL, DATE_PART('timezone', DATE 'infinity') IS NULL, DATE_PART('timezone', TIMESTAMP '-infinity') IS NULL");
    let timestamp = fixture.execute("SELECT EXTRACT(timezone FROM TIMESTAMP '2000-01-01')");

    // Assert
    assert_eq!(
        controls.expect("NULL and infinity controls").rows(),
        &[vec![Value::Bool(true); 5]]
    );
    assert!(
        matches!(timestamp, Err(cassie::app::CassieError::TypeInput(error)) if error.sqlstate == "0A000" && error.message == "unit \"timezone\" not supported for type timestamp without time zone")
    );
}

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

#[test]
fn should_reject_unrecognized_truncation_units() {
    // Arrange
    let fixture = sql_fixture(
        "issue-815-trunc-unrecognized-units",
        &["SET TIME ZONE 'UTC'"],
    );
    let values = [
        "TIMESTAMP '2000-01-01'",
        "TIMESTAMP 'infinity'",
        "TIMESTAMP '-infinity'",
        "TIMESTAMPTZ '2000-01-01 00:00:00+00'",
        "TIMESTAMPTZ 'infinity'",
        "TIMESTAMPTZ '-infinity'",
        "INTERVAL '1 day'",
        "INTERVAL 'infinity'",
        "INTERVAL '-infinity'",
    ];

    for value in values {
        for unit in ["dow", "doy", "isodow", "isoyear", "epoch", "julian"] {
            // Act
            let result = fixture.execute(&format!("SELECT date_trunc('{unit}', {value})"));

            // Assert
            assert!(
                matches!(result, Err(cassie::app::CassieError::TypeInput(error)) if error.sqlstate == "22023"),
                "{unit}, {value}"
            );
        }
    }
}

#[test]
fn should_reject_unsupported_truncation_units_before_infinity_shortcut() {
    // Arrange
    let fixture = sql_fixture(
        "issue-815-trunc-unsupported-units",
        &["SET TIME ZONE 'UTC'"],
    );
    let values = [
        "TIMESTAMP '2000-01-01'",
        "TIMESTAMP 'infinity'",
        "TIMESTAMP '-infinity'",
        "TIMESTAMPTZ '2000-01-01 00:00:00+00'",
        "TIMESTAMPTZ 'infinity'",
        "TIMESTAMPTZ '-infinity'",
        "INTERVAL '1 day'",
        "INTERVAL 'infinity'",
        "INTERVAL '-infinity'",
    ];

    for value in values {
        for unit in ["timezone", "timezone_hour", "timezone_minute"] {
            // Act
            let result = fixture.execute(&format!("SELECT date_trunc('{unit}', {value})"));

            // Assert
            assert!(
                matches!(result, Err(cassie::app::CassieError::TypeInput(error)) if error.sqlstate == "0A000"),
                "{unit}, {value}"
            );
        }
    }
    for value in [
        "INTERVAL '1 day'",
        "INTERVAL 'infinity'",
        "INTERVAL '-infinity'",
    ] {
        let result = fixture.execute(&format!("SELECT date_trunc('week', {value})"));

        // Assert
        assert!(
            matches!(result, Err(cassie::app::CassieError::TypeInput(error)) if error.sqlstate == "0A000"),
            "week, {value}"
        );
    }
}

#[test]
fn should_preserve_infinity_for_supported_truncation_units() {
    // Arrange
    let fixture = sql_fixture(
        "issue-815-trunc-supported-infinity",
        &["SET TIME ZONE 'UTC'"],
    );

    // Act
    let result = fixture.execute("SELECT date_trunc('day', TIMESTAMP 'infinity') = TIMESTAMP 'infinity' AND date_trunc('week', TIMESTAMPTZ '-infinity') = TIMESTAMPTZ '-infinity' AND date_trunc('year', INTERVAL 'infinity') = INTERVAL 'infinity'");

    // Assert
    assert_eq!(
        result.expect("supported infinite truncation").rows(),
        &[vec![Value::Bool(true)]]
    );
}

#[test]
fn should_preserve_truncation_timezone_priority() {
    // Arrange
    let fixture = sql_fixture(
        "issue-815-trunc-null-zone-priority",
        &["SET TIME ZONE 'UTC'"],
    );

    // Act
    let nulls = fixture.execute("SELECT date_trunc('dow', NULL::timestamp) IS NULL AND date_trunc(NULL::text, TIMESTAMPTZ 'infinity') IS NULL AND date_trunc('timezone', NULL::interval) IS NULL");
    let invalid_zone =
        fixture.execute("SELECT date_trunc('dow', TIMESTAMPTZ 'infinity', 'NoSuch/Zone')");

    // Assert
    assert_eq!(
        nulls.expect("null truncation").rows(),
        &[vec![Value::Bool(true)]]
    );
    assert!(
        matches!(invalid_zone, Err(cassie::app::CassieError::TypeInput(error)) if error.sqlstate == "22023" && error.message == "time zone \"NoSuch/Zone\" not recognized")
    );
}

#[test]
fn should_extract_interval_weeks_by_truncating_signed_days() {
    // Arrange
    let fixture = sql_fixture("issue-815-interval-week-parts", &[]);
    let cases = [
        ("6 days", 0),
        ("7 days", 1),
        ("8 days", 1),
        ("-6 days", 0),
        ("-7 days", -1),
        ("-8 days", -1),
        ("1 month 15 days 23:59:59", 2),
        ("-1 month -15 days -23:59:59", -2),
    ];

    for (input, expected) in cases {
        // Act
        let result = fixture.execute(&format!(
            "SELECT date_part('week', INTERVAL '{input}') = {expected} AND EXTRACT(WEEK FROM INTERVAL '{input}') = {expected}"
        ));

        // Assert
        assert_eq!(
            result.expect("interval week parts").rows(),
            &[vec![Value::Bool(true)]],
            "{input}"
        );
    }
}

#[test]
fn should_reject_unsupported_fields_on_infinite_intervals() {
    // Arrange
    let fixture = sql_fixture("issue-815-infinite-interval-parts", &[]);
    let units = [
        "timezone",
        "timezone_hour",
        "timezone_minute",
        "dow",
        "doy",
        "isodow",
        "isoyear",
        "julian",
    ];

    for value in ["infinity", "-infinity"] {
        for unit in units {
            for sql in [
                format!("SELECT date_part('{unit}', INTERVAL '{value}')"),
                format!("SELECT EXTRACT({unit} FROM INTERVAL '{value}')"),
            ] {
                // Act
                let result = fixture.execute(&sql);

                // Assert
                assert!(
                    matches!(result, Err(cassie::app::CassieError::TypeInput(error)) if error.sqlstate == "0A000"),
                    "{sql}"
                );
            }
        }
    }
}

#[test]
fn should_preserve_null_temporal_parts() {
    // Arrange
    let fixture = sql_fixture("issue-815-infinite-week-parts", &[]);

    // Act
    let result = fixture.execute("SELECT date_part('week', INTERVAL 'infinity') IS NULL AND EXTRACT(WEEK FROM INTERVAL '-infinity') IS NULL AND date_part('timezone', TIMESTAMP 'infinity') IS NULL AND EXTRACT(TIMEZONE FROM TIMESTAMP '-infinity') IS NULL");

    // Assert
    assert_eq!(
        result.expect("infinite parts").rows(),
        &[vec![Value::Bool(true)]]
    );
}

#[test]
fn should_extract_epoch_across_the_unix_microsecond_overflow_boundary() {
    // Arrange
    let fixture = sql_fixture(
        "issue-815-epoch-overflow-boundary",
        &["SET TIME ZONE 'UTC'"],
    );
    let cases = [
        ("775806", "9223372036854.775806"),
        ("775807", "9223372036854.775800"),
        ("775849", "9223372036854.775800"),
        ("775850", "9223372036854.775900"),
        ("999950", "9223372036855.000000"),
    ];

    for (micros, expected) in cases {
        // Act
        let sql =
            format!("SELECT EXTRACT(EPOCH FROM TIMESTAMP '294247-01-10 04:00:54.{micros}')::text");
        let result = fixture.execute(&sql);

        // Assert
        assert_eq!(
            result.expect("timestamp epoch").rows(),
            &[vec![Value::String(expected.to_owned())]],
            "{micros}"
        );
    }
}

#[test]
fn should_match_float_epoch_rounding_above_the_overflow_boundary() {
    // Arrange
    let fixture = sql_fixture(
        "issue-815-float-epoch-overflow-boundary",
        &["SET TIME ZONE 'UTC'"],
    );

    // Act
    let result = fixture.execute("SELECT DATE_PART('epoch', TIMESTAMP '294247-01-10 04:00:54.999950'), DATE_PART('epoch', TIMESTAMPTZ '294247-01-10 04:00:54.999950+00')");

    // Assert
    assert_eq!(
        result.expect("floating timestamp epochs").rows(),
        &[vec![
            Value::Float64(9_223_372_036_855.002),
            Value::Float64(9_223_372_036_855.002)
        ]]
    );
}

#[test]
fn should_extract_epoch_at_the_upper_bound_in_a_non_utc_session() {
    // Arrange
    let fixture = sql_fixture(
        "issue-815-epoch-upper-timezone",
        &["SET TIME ZONE 'Asia/Kolkata'"],
    );

    // Act
    let result = fixture.execute("SELECT EXTRACT(EPOCH FROM TIMESTAMP '294276-12-31 23:59:59.999999')::text, EXTRACT(EPOCH FROM TIMESTAMPTZ '294247-01-10 09:30:54.775850+05:30')::text, DATE_PART('epoch', TIMESTAMPTZ '294247-01-10 09:30:54.775850+05:30'), DATE_PART('epoch', TIMESTAMPTZ '294276-12-31 23:59:59.999999+00')");

    // Assert
    assert_eq!(
        result.expect("upper timestamp epochs").rows(),
        &[vec![
            Value::String("9224318016000.000000".to_owned()),
            Value::String("9223372036854.775900".to_owned()),
            Value::Float64(9_223_372_036_854.775),
            Value::Float64(9_224_318_016_000.0)
        ]]
    );
}

#[test]
fn should_preserve_signed_microseconds_for_ordinary_epoch_parts() {
    // Arrange
    let fixture = sql_fixture(
        "issue-815-epoch-signed-microseconds",
        &["SET TIME ZONE 'UTC'"],
    );

    // Act
    let result = fixture.execute("SELECT EXTRACT(EPOCH FROM TIMESTAMP '1970-01-01 00:00:00.000001')::text, EXTRACT(EPOCH FROM TIMESTAMP '1969-12-31 23:59:59.999999')::text");

    // Assert
    assert_eq!(
        result.expect("signed epoch microseconds").rows(),
        &[vec![
            Value::String("0.000001".to_owned()),
            Value::String("-0.000001".to_owned())
        ]]
    );
}

#[test]
fn should_reject_minimum_i32_year_in_temporal_constructors() {
    // Arrange
    let fixture = sql_fixture("issue-815-constructor-min-year", &["SET TIME ZONE 'UTC'"]);
    let cases = [
        "make_date(-2147483648, 1, 1)",
        "make_timestamp(-2147483648, 1, 1, 0, 0, 0)",
        "make_timestamptz(-2147483648, 1, 1, 0, 0, 0, 'UTC')",
    ];

    for expression in cases {
        // Act
        let result = fixture.execute(&format!("SELECT {expression}"));
        let recovered = fixture.execute("SELECT 1");

        // Assert
        assert!(
            matches!(result, Err(cassie::app::CassieError::TypeInput(error)) if error.sqlstate == "22008" && error.message == "date field value out of range: -2147483648-01-01"),
            "{expression}"
        );
        assert_eq!(
            recovered.expect("session recovery").rows(),
            &[vec![Value::Int64(1)]]
        );
    }
}

#[test]
fn should_reject_timestamp_constructor_multiplication_overflow_after_time_validation() {
    // Arrange
    let fixture = sql_fixture("issue-815-constructor-range", &["SET TIME ZONE 'UTC'"]);
    let cases = [
        (
            "make_timestamp(1000000, 1, 1, 0, 0, 0)",
            "timestamp out of range: 1000000-01-01 0:00:00",
        ),
        (
            "make_timestamptz(1000000, 1, 1, 0, 0, 0, 'UTC')",
            "timestamp out of range: 1000000-01-01 0:00:00",
        ),
        (
            "make_timestamp(1000000, 1, 1, 25, 0, 0)",
            "time field value out of range: 25:00:00",
        ),
    ];

    for (expression, expected) in cases {
        // Act
        let result = fixture.execute(&format!("SELECT {expression}"));

        // Assert
        assert!(
            matches!(result, Err(cassie::app::CassieError::TypeInput(error)) if error.sqlstate == "22008" && error.message == expected),
            "{expression}"
        );
    }
}

#[test]
fn should_reject_timestamptz_constructor_that_crosses_the_utc_upper_bound() {
    // Arrange
    let fixture = sql_fixture("issue-815-constructor-zone-range", &["SET TIME ZONE 'UTC'"]);

    // Act
    let valid =
        fixture.execute("SELECT make_timestamptz(294276, 12, 31, 23, 59, 59, 'UTC') IS NOT NULL");
    let invalid =
        fixture.execute("SELECT make_timestamptz(294276, 12, 31, 23, 59, 59, 'America/New_York')");

    // Assert
    assert_eq!(
        valid.expect("UTC upper bound").rows(),
        &[vec![Value::Bool(true)]]
    );
    assert!(
        matches!(invalid, Err(cassie::app::CassieError::TypeInput(error)) if error.sqlstate == "22008" && error.message == "timestamp out of range")
    );
}

#[test]
fn should_reject_timestamptz_constructor_that_crosses_the_utc_lower_bound() {
    // Arrange
    let fixture = sql_fixture("issue-815-constructor-zone-lower", &["SET TIME ZONE 'UTC'"]);

    // Act
    let valid = fixture.execute("SELECT make_timestamptz(-4714, 11, 24, 0, 0, 0, 'UTC') IS NOT NULL AND make_date(-1, 1, 1) = DATE '0001-01-01 BC'");
    let invalid =
        fixture.execute("SELECT make_timestamptz(-4714, 11, 24, 0, 0, 0, 'Asia/Kolkata')");

    // Assert
    assert_eq!(
        valid.expect("UTC lower bound and BCE").rows(),
        &[vec![Value::Bool(true)]]
    );
    assert!(
        matches!(invalid, Err(cassie::app::CassieError::TypeInput(error)) if error.sqlstate == "22008" && error.message == "timestamp out of range")
    );
}

#[test]
fn should_preserve_constructor_field_priority_over_zone_resolution() {
    // Arrange
    let fixture = sql_fixture(
        "issue-815-constructor-error-priority",
        &["SET TIME ZONE 'UTC'"],
    );

    // Act
    let invalid_time =
        fixture.execute("SELECT make_timestamptz(1000000, 1, 1, 25, 0, 0, 'NoSuch/Zone')");
    let invalid_zone =
        fixture.execute("SELECT make_timestamptz(294276, 12, 31, 23, 59, 59, 'NoSuch/Zone')");

    // Assert
    assert!(
        matches!(invalid_time, Err(cassie::app::CassieError::TypeInput(error)) if error.sqlstate == "22008" && error.message == "time field value out of range: 25:00:00")
    );
    assert!(
        matches!(invalid_zone, Err(cassie::app::CassieError::TypeInput(error)) if error.sqlstate == "22023" && error.message == "time zone \"NoSuch/Zone\" not recognized")
    );
}

#[test]
fn should_reject_out_of_range_make_interval_seconds() {
    // Arrange
    let fixture = sql_fixture("issue-815-make-interval-seconds-range", &[]);
    let cases = [
        ("1e300", "22008", "interval out of range"),
        ("-1e300", "22008", "interval out of range"),
        ("1e308", "22003", "value out of range: overflow"),
        ("-1e308", "22003", "value out of range: overflow"),
    ];

    for (seconds, state, message) in cases {
        // Act
        let result = fixture.execute(&format!(
            "SELECT make_interval(0, 0, 0, 0, 0, 0, {seconds})"
        ));
        let recovered = fixture.execute("SELECT 1");

        // Assert
        assert!(
            matches!(result, Err(cassie::app::CassieError::TypeInput(error)) if error.sqlstate == state && error.message == message),
            "{seconds}"
        );
        assert_eq!(
            recovered.expect("session recovery").rows(),
            &[vec![Value::Int64(1)]]
        );
    }
}

#[test]
fn should_round_constructor_seconds_before_checking_their_range() {
    // Arrange
    let fixture = sql_fixture(
        "issue-815-make-time-rounded-range",
        &["SET TIME ZONE 'UTC'"],
    );

    // Act
    let accepted = fixture.execute("SELECT make_time(1, 2, -0.0000005) = TIME '01:02:00' AND make_time(1, 2, 60.0000005) = TIME '01:03:00' AND make_time(24, 0, -0.0000005) = TIME '24:00:00' AND make_timestamp(2000, 1, 1, 1, 2, -0.0000005) = TIMESTAMP '2000-01-01 01:02:00' AND make_timestamptz(2000, 1, 1, 1, 2, 60.0000005, 'UTC') = TIMESTAMPTZ '2000-01-01 01:03:00+00'");
    let negative = fixture.execute("SELECT make_time(1, 2, -0.0000006)");
    let above = fixture.execute("SELECT make_time(1, 2, 60.0000006)");
    let end = fixture.execute("SELECT make_time(24, 0, 0.0000006)");

    // Assert
    assert_eq!(
        accepted.expect("rounded constructor seconds").rows(),
        &[vec![Value::Bool(true)]]
    );
    for result in [negative, above, end] {
        assert!(
            matches!(result, Err(cassie::app::CassieError::TypeInput(error)) if error.sqlstate == "22008")
        );
    }
}
