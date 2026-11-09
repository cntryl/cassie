//! Residual PostgreSQL 18 temporal function edges tracked by #815.

use cassie::types::Value;

use crate::support_sql_fixture::sql_fixture;

#[test]
fn should_format_interval_day_of_year_and_week_from_approximate_span() {
    // Arrange
    let fixture = sql_fixture("issue-815-interval-day-of-year-week", &[]);
    let cases = [
        ("0", "000|0|000th|01|1|01st"),
        ("1 day", "001|1|001st|01|1|01st"),
        ("1 month", "030|30|030th|05|5|05th"),
        ("1 year 2 months 3 days", "423|423|423rd|61|61|61st"),
        ("-1 day", "-01|-1|-01st|01|1|01st"),
        ("-1 month -3 days", "-33|-33|-33rd|-3|-3|-3rd"),
        ("1 month -30 days", "000|0|000th|01|1|01st"),
        ("1 month -31 days", "-01|-1|-01st|01|1|01st"),
        ("7 days", "007|7|007th|01|1|01st"),
        ("8 days", "008|8|008th|02|2|02nd"),
    ];

    for (interval, expected) in cases {
        // Act
        let result = fixture.execute(&format!(
            "SELECT to_char(INTERVAL '{interval}', 'DDD|FMDDD|DDDth|WW|FMWW|WWth')"
        ));

        // Assert
        assert_eq!(
            result.expect("interval span formatting").rows(),
            &[vec![Value::String(expected.to_owned())]],
            "PostgreSQL 18.6 interval span: {interval}"
        );
    }

    // Act
    let extremes = fixture.execute("SELECT to_char(INTERVAL '2147483647 months', 'DDD|WW'), to_char(INTERVAL '-2147483648 months', 'DDD|WW'), to_char(INTERVAL '2147483647 days', 'DDD|WW'), to_char(INTERVAL '-2147483648 days', 'DDD|WW'), to_char(INTERVAL '2147483647 months 2147483647 days', 'DDD|WW')");
    let date = fixture.execute("SELECT to_char(TIMESTAMP '2024-12-31', 'DDD|WW')");

    // Assert
    assert_eq!(
        extremes.expect("interval span extremes").rows(),
        &[vec![
            Value::String("-30|-3".to_owned()),
            Value::String("000|01".to_owned()),
            Value::String("2147483647|306783379".to_owned()),
            Value::String("-2147483648|306783379".to_owned()),
            Value::String("2147483617|306783374".to_owned()),
        ]]
    );
    assert_eq!(
        date.expect("timestamp day of year and week").rows(),
        &[vec![Value::String("366|53".to_owned())]]
    );
}

#[test]
fn should_return_null_for_an_empty_temporal_to_char_template() {
    // Arrange
    let fixture = sql_fixture("issue-815-empty-temporal-to-char", &[]);

    // Act
    let nulls = fixture.execute(
        "SELECT to_char(TIMESTAMP '2000-01-01', '') IS NULL, to_char(TIMESTAMPTZ '2000-01-01 00:00:00+00', '') IS NULL, to_char(DATE '2000-01-01', '') IS NULL, to_char(INTERVAL '1 day', '') IS NULL, to_char(TIMESTAMP 'infinity', '') IS NULL",
    );
    let quoted_empty = fixture.execute("SELECT to_char(INTERVAL '1 day', '\"\"')");
    let ordinary = fixture.execute("SELECT to_char(TIMESTAMP '2000-01-01', 'YYYY')");

    // Assert
    assert_eq!(
        nulls.expect("empty temporal formats").rows(),
        &[vec![Value::Bool(true); 5]]
    );
    assert_eq!(
        quoted_empty.expect("quoted empty literal").rows(),
        &[vec![Value::String(String::new())]]
    );
    assert_eq!(
        ordinary.expect("nonempty format").rows(),
        &[vec![Value::String("2000".to_owned())]]
    );
}

#[test]
fn should_format_interval_centuries_by_truncating_signed_years() {
    // Arrange
    let fixture = sql_fixture("issue-815-interval-century", &[]);
    let cases = [
        (-300, "-03|-3|-03rd|-03RD"),
        (-201, "-02|-2|-02nd|-02ND"),
        (-200, "-02|-2|-02nd|-02ND"),
        (-199, "-01|-1|-01st|-01ST"),
        (-100, "-01|-1|-01st|-01ST"),
        (-99, "00|0|00th|00TH"),
        (-1, "00|0|00th|00TH"),
        (0, "00|0|00th|00TH"),
        (1, "00|0|00th|00TH"),
        (99, "00|0|00th|00TH"),
        (100, "01|1|01st|01ST"),
        (101, "01|1|01st|01ST"),
        (200, "02|2|02nd|02ND"),
        (201, "02|2|02nd|02ND"),
    ];

    for (years, expected) in cases {
        // Act
        let result = fixture.execute(&format!(
            "SELECT to_char(INTERVAL '{years} years', 'CC|FMCC|CCth|CCTH')"
        ));

        // Assert
        assert_eq!(
            result.expect("interval century formatting").rows(),
            &[vec![Value::String(expected.to_owned())]],
            "PostgreSQL 18.6 century for {years} years"
        );
    }

    // Act
    let dates = fixture.execute("SELECT to_char(TIMESTAMP '0001-01-01', 'CC'), to_char(TIMESTAMP '0001-01-01 BC', 'CC'), to_char(TIMESTAMP '2000-01-01', 'CC')");

    // Assert
    assert_eq!(
        dates.expect("timestamp century formatting").rows(),
        &[vec![
            Value::String("01".to_owned()),
            Value::String("-01".to_owned()),
            Value::String("20".to_owned()),
        ]]
    );
}

#[test]
fn should_count_the_sign_in_negative_interval_day_padding() {
    // Arrange
    let fixture = sql_fixture("issue-815-negative-interval-day-padding", &[]);
    let cases = [
        (-99, "-99|-99|-99th|-99TH"),
        (-21, "-21|-21|-21st|-21ST"),
        (-11, "-11|-11|-11th|-11TH"),
        (-10, "-10|-10|-10th|-10TH"),
        (-9, "-9|-9|-9th|-9TH"),
        (-3, "-3|-3|-3rd|-3RD"),
        (-2, "-2|-2|-2nd|-2ND"),
        (-1, "-1|-1|-1st|-1ST"),
        (0, "00|0|00th|00TH"),
        (1, "01|1|01st|01ST"),
        (9, "09|9|09th|09TH"),
        (10, "10|10|10th|10TH"),
        (99, "99|99|99th|99TH"),
    ];

    for (days, expected) in cases {
        // Act
        let result = fixture.execute(&format!(
            "SELECT to_char(INTERVAL '{days} days', 'DD|FMDD|DDth|DDTH')"
        ));

        // Assert
        assert_eq!(
            result.expect("interval day formatting").rows(),
            &[vec![Value::String(expected.to_owned())]],
            "PostgreSQL 18.6 day formatting for {days} days"
        );
    }
}

#[test]
fn should_render_negative_interval_ordinal_suffixes_from_the_magnitude() {
    // Arrange
    let fixture = sql_fixture("issue-815-negative-interval-ordinal", &[]);
    let cases = [
        (-1, "-01st|-01ST|-1st"),
        (-2, "-02nd|-02ND|-2nd"),
        (-3, "-03rd|-03RD|-3rd"),
        (-11, "-11th|-11TH|-11th"),
        (-12, "-12th|-12TH|-12th"),
        (-13, "-13th|-13TH|-13th"),
        (-21, "-21st|-21ST|-21st"),
        (-22, "-22nd|-22ND|-22nd"),
        (-23, "-23rd|-23RD|-23rd"),
        (0, "00th|00TH|0th"),
        (1, "01st|01ST|1st"),
    ];

    for (hours, expected) in cases {
        // Act
        let result = fixture.execute(&format!(
            "SELECT to_char(INTERVAL '{hours} hours', 'HH24th|HH24TH|FMHH24th')"
        ));

        // Assert
        assert_eq!(
            result.expect("interval ordinal formatting").rows(),
            &[vec![Value::String(expected.to_owned())]],
            "PostgreSQL 18.6 ordinal for {hours} hours"
        );
    }
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
fn should_reject_unrecognized_truncation_units_for_finite_and_infinite_temporal_values() {
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
        // Act
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
fn should_keep_null_and_explicit_timezone_priority_for_truncation() {
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
fn should_preserve_null_parts_for_infinite_interval_weeks_and_timestamp_timezone() {
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
fn should_reject_out_of_range_make_interval_seconds_and_keep_session_usable() {
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
