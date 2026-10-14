//! PostgreSQL 18 temporal clocks, calendar, and formatting functions tracked by #815.

use cassie::types::Value;
use std::time::{Duration, Instant};

use crate::support_sql_fixture::sql_fixture;

fn assert_true(label: &str, sql: &str) {
    let fixture = sql_fixture(label, &[]);
    let result = fixture.execute(sql);
    assert_eq!(
        result.expect("temporal function expression").rows(),
        &[vec![Value::Bool(true)]],
        "expected PostgreSQL temporal function invariant: {sql}"
    );
}

fn assert_millisecond_precision(label: &str, sql: &str) {
    let fixture = sql_fixture(label, &[]);
    let result = fixture.execute(sql).expect("temporal precision expression");
    let microseconds = result.rows()[0][0]
        .as_f64()
        .expect("EXTRACT returns a numeric microsecond field");
    assert_eq!(
        microseconds % 1_000.0,
        0.0,
        "fraction is millisecond-aligned"
    );
}

#[test]
fn should_bind_current_time_expressions_to_transaction_start() {
    // Arrange
    let sql = "SELECT NOW() = TRANSACTION_TIMESTAMP() AND CURRENT_TIMESTAMP = NOW()";

    // Act
    let fixture = sql_fixture("issue-815-current-time-aliases", &[]);
    let result = fixture.execute(sql);

    // Assert
    assert_eq!(
        result.expect("transaction-time expressions").rows(),
        &[vec![Value::Bool(true)]]
    );
}

#[test]
fn should_bind_current_date_to_session_local_timestamp_date() {
    // Arrange
    let fixture = sql_fixture(
        "issue-815-current-date-session-zone",
        &["SET TIME ZONE 'UTC'"],
    );

    // Act
    let result =
        fixture.execute("SELECT CURRENT_DATE = CAST(CURRENT_TIMESTAMP AT TIME ZONE 'UTC' AS DATE)");

    // Assert
    assert_eq!(
        result.expect("current date in session zone").rows(),
        &[vec![Value::Bool(true)]]
    );
}

#[test]
fn should_evaluate_current_timestamp_in_where_predicates() {
    // Arrange
    let sql = "SELECT TRUE WHERE CURRENT_TIMESTAMP > TIMESTAMPTZ '2000-01-01 00:00:00+00'";

    // Act
    // Assert
    assert_true("issue-815-current-timestamp-where", sql);
}

#[test]
fn should_evaluate_now_inside_temporal_scalar_functions() {
    // Arrange
    let sql = "SELECT DATE_TRUNC('day', NOW()) = DATE_TRUNC('day', TRANSACTION_TIMESTAMP())";

    // Act
    // Assert
    assert_true("issue-815-now-scalar-argument", sql);
}

#[test]
fn should_evaluate_current_date_in_values_rows() {
    // Arrange
    let sql = "VALUES (CURRENT_DATE IS NOT NULL)";

    // Act
    // Assert
    assert_true("issue-815-current-date-values", sql);
}

#[test]
fn should_round_current_timestamp_precision_to_milliseconds() {
    // Arrange
    let sql = "SELECT EXTRACT(MICROSECONDS FROM CURRENT_TIMESTAMP(3))";

    // Act
    // Assert
    assert_millisecond_precision("issue-815-current-timestamp-precision", sql);
}

#[test]
fn should_round_current_time_precision_to_milliseconds() {
    // Arrange
    let sql = "SELECT EXTRACT(MICROSECONDS FROM CURRENT_TIME(3))";

    // Act
    // Assert
    assert_millisecond_precision("issue-815-current-time-precision", sql);
}

#[test]
fn should_round_local_time_precision_to_milliseconds() {
    // Arrange
    let sql = "SELECT EXTRACT(MICROSECONDS FROM LOCALTIME(3))";

    // Act
    // Assert
    assert_millisecond_precision("issue-815-localtime-precision", sql);
}

#[test]
fn should_round_local_timestamp_precision_to_milliseconds() {
    // Arrange
    let sql = "SELECT EXTRACT(MICROSECONDS FROM LOCALTIMESTAMP(3))";

    // Act
    // Assert
    assert_millisecond_precision("issue-815-localtimestamp-precision", sql);
}

#[test]
fn should_hold_statement_timestamp_constant_within_a_statement() {
    // Arrange
    let sql = "SELECT STATEMENT_TIMESTAMP() = STATEMENT_TIMESTAMP()";

    // Act
    let fixture = sql_fixture("issue-815-statement-clock-stable", &[]);
    let result = fixture.execute(sql);

    // Assert
    assert_eq!(
        result.expect("statement timestamp stability").rows(),
        &[vec![Value::Bool(true)]]
    );
}

#[test]
fn should_refresh_statement_timestamp_for_each_statement() {
    // Arrange
    let fixture = sql_fixture("issue-815-statement-clock-lifetime", &[]);

    // Act
    let first = fixture
        .execute("SELECT STATEMENT_TIMESTAMP()")
        .expect("first statement clock")
        .rows()
        .to_vec();
    std::thread::sleep(Duration::from_millis(20));
    let second = fixture
        .execute("SELECT STATEMENT_TIMESTAMP()")
        .expect("second statement clock")
        .rows()
        .to_vec();

    // Assert
    assert_ne!(
        first, second,
        "statement time is refreshed for each command"
    );
}

#[test]
fn should_hold_transaction_timestamp_constant_across_statements() {
    // Arrange
    let fixture = sql_fixture("issue-815-transaction-clock-lifetime", &[]);
    fixture.execute("BEGIN").expect("begin transaction");

    // Act
    let first = fixture
        .execute("SELECT TRANSACTION_TIMESTAMP()")
        .expect("first transaction clock")
        .rows()
        .to_vec();
    std::thread::sleep(Duration::from_millis(20));
    let second = fixture
        .execute("SELECT NOW()")
        .expect("second transaction clock")
        .rows()
        .to_vec();
    fixture.execute("COMMIT").expect("commit transaction");

    // Assert
    assert_eq!(first, second, "transaction time remains fixed until commit");
}

#[test]
fn should_observe_wall_clock_changes_between_expressions() {
    // Arrange
    let fixture = sql_fixture("issue-815-wall-clock-lifetime", &[]);

    // Act
    let first = fixture
        .execute("SELECT CLOCK_TIMESTAMP()")
        .expect("first wall clock")
        .rows()
        .to_vec();
    std::thread::sleep(Duration::from_millis(20));
    let second = fixture
        .execute("SELECT CLOCK_TIMESTAMP()")
        .expect("second wall clock")
        .rows()
        .to_vec();

    // Assert
    assert_ne!(first, second, "wall clock follows elapsed time");
}

#[test]
fn should_return_wall_clock_text_from_timeofday() {
    // Arrange
    let sql = "SELECT TIMEOFDAY() <> ''";

    // Act
    let fixture = sql_fixture("issue-815-timeofday", &[]);
    let result = fixture.execute(sql);

    // Assert
    assert_eq!(
        result.expect("timeofday text").rows(),
        &[vec![Value::Bool(true)]]
    );
}

#[test]
fn should_truncate_timestamps_to_calendar_boundaries() {
    // Arrange
    let sql = "SELECT DATE_TRUNC('quarter', TIMESTAMP '2001-05-16 20:38:40') = TIMESTAMP '2001-04-01 00:00:00' AND DATE_TRUNC('week', TIMESTAMP '2001-02-16 20:38:40') = TIMESTAMP '2001-02-12 00:00:00'";

    // Act
    let fixture = sql_fixture("issue-815-date-trunc-calendar", &[]);
    let result = fixture.execute(sql);

    // Assert
    assert_eq!(
        result.expect("calendar truncation").rows(),
        &[vec![Value::Bool(true)]]
    );
}

#[test]
fn should_truncate_temporal_values_to_requested_fractional_units() {
    // Arrange
    let sql = "SELECT DATE_TRUNC('milliseconds', TIMESTAMP '2001-02-16 20:38:40.123456') = TIMESTAMP '2001-02-16 20:38:40.123000' AND DATE_TRUNC('microseconds', TIMESTAMP '2001-02-16 20:38:40.123456') = TIMESTAMP '2001-02-16 20:38:40.123456'";

    // Act
    let fixture = sql_fixture("issue-815-date-trunc-fraction", &[]);
    let result = fixture.execute(sql);

    // Assert
    assert_eq!(
        result.expect("fractional truncation").rows(),
        &[vec![Value::Bool(true)]]
    );
}

#[test]
fn should_truncate_timestamptz_in_an_explicit_zone() {
    // Arrange
    let sql = "SELECT DATE_TRUNC('day', TIMESTAMPTZ '2001-02-16 20:38:40+00', 'Australia/Sydney') = TIMESTAMPTZ '2001-02-16 13:00:00+00'";

    // Act
    let fixture = sql_fixture("issue-815-date-trunc-zone", &[]);
    let result = fixture.execute(sql);

    // Assert
    assert_eq!(
        result.expect("zone-aware day truncation").rows(),
        &[vec![Value::Bool(true)]]
    );
}

#[test]
fn should_truncate_intervals_to_requested_calendar_units() {
    // Arrange
    let sql = "SELECT DATE_TRUNC('hour', INTERVAL '2 days 3 hours 40 minutes') = INTERVAL '2 days 3 hours'";

    // Act
    let fixture = sql_fixture("issue-815-date-trunc-interval", &[]);
    let result = fixture.execute(sql);

    // Assert
    assert_eq!(
        result.expect("interval truncation").rows(),
        &[vec![Value::Bool(true)]]
    );
}

#[test]
fn should_extract_calendar_date_subfields() {
    // Arrange
    let sql = "SELECT EXTRACT(YEAR FROM DATE '2001-02-16') = 2001 AND EXTRACT(MONTH FROM DATE '2001-02-16') = 2 AND EXTRACT(DAY FROM DATE '2001-02-16') = 16";

    // Act
    let fixture = sql_fixture("issue-815-extract-subfields", &[]);
    let result = fixture.execute(sql);

    // Assert
    assert_eq!(
        result.expect("date subfields").rows(),
        &[vec![Value::Bool(true)]]
    );
}

#[test]
fn should_extract_time_microseconds() {
    // Arrange
    let sql = "SELECT EXTRACT(MICROSECONDS FROM TIME '20:38:40.123456') = 40123456";

    // Act
    let fixture = sql_fixture("issue-815-extract-time-microseconds", &[]);
    let result = fixture.execute(sql);

    // Assert
    assert_eq!(
        result.expect("time microseconds").rows(),
        &[vec![Value::Bool(true)]]
    );
}

#[test]
fn should_extract_interval_month_component() {
    // Arrange
    let sql = "SELECT EXTRACT(MONTH FROM INTERVAL '2 years 3 months') = 3";

    // Act
    let fixture = sql_fixture("issue-815-extract-interval-month", &[]);
    let result = fixture.execute(sql);

    // Assert
    assert_eq!(
        result.expect("interval month component").rows(),
        &[vec![Value::Bool(true)]]
    );
}

#[test]
fn should_extract_iso_week_year_fields_at_calendar_boundary() {
    // Arrange
    let sql = "SELECT EXTRACT(ISOYEAR FROM DATE '2006-01-01') = 2005 AND EXTRACT(WEEK FROM DATE '2006-01-01') = 52 AND EXTRACT(ISODOW FROM DATE '2006-01-01') = 7";

    // Act
    let fixture = sql_fixture("issue-815-extract-iso-calendar", &[]);
    let result = fixture.execute(sql);

    // Assert
    assert_eq!(
        result.expect("ISO calendar fields").rows(),
        &[vec![Value::Bool(true)]]
    );
}

#[test]
fn should_return_epoch_seconds_from_date_part() {
    // Arrange
    let sql = "SELECT DATE_PART('epoch', TIMESTAMPTZ '2000-01-01 00:00:00+00') = 946684800";

    // Act
    let fixture = sql_fixture("issue-815-date-part-epoch", &[]);
    let result = fixture.execute(sql);

    // Assert
    assert_eq!(
        result.expect("date_part epoch").rows(),
        &[vec![Value::Bool(true)]]
    );
}

#[test]
fn should_format_timestamp_fields_with_fill_mode() {
    // Arrange
    let sql = "SELECT TO_CHAR(TIMESTAMP '2002-04-20 17:31:12.66', 'HH12:MI:SS') = '05:31:12' AND TO_CHAR(DATE '2002-04-20', 'FMMonth DD, YYYY') = 'April 20, 2002'";

    // Act
    let fixture = sql_fixture("issue-815-to-char-calendar", &[]);
    let result = fixture.execute(sql);

    // Assert
    assert_eq!(
        result.expect("date and time formatting").rows(),
        &[vec![Value::Bool(true)]]
    );
}

#[test]
fn should_format_timestamp_microseconds() {
    // Arrange
    let sql = "SELECT TO_CHAR(TIMESTAMP '2002-04-20 17:31:12.660123', 'YYYY-MM-DD\"T\"HH24:MI:SS.US') = '2002-04-20T17:31:12.660123'";

    // Act
    let fixture = sql_fixture("issue-815-to-char-microseconds", &[]);
    let result = fixture.execute(sql);

    // Assert
    assert_eq!(
        result.expect("microsecond formatting").rows(),
        &[vec![Value::Bool(true)]]
    );
}

#[test]
fn should_format_iso_week_date_fields() {
    // Arrange
    let sql = "SELECT TO_CHAR(DATE '2006-01-01', 'IYYY-IW-ID') = '2005-52-7'";

    // Act
    let fixture = sql_fixture("issue-815-to-char-iso-week", &[]);
    let result = fixture.execute(sql);

    // Assert
    assert_eq!(
        result.expect("ISO week formatting").rows(),
        &[vec![Value::Bool(true)]]
    );
}

#[test]
fn should_format_timestamptz_with_session_zone_offset() {
    // Arrange
    let fixture = sql_fixture(
        "issue-815-to-char-zone-offset",
        &["SET TIME ZONE 'America/Denver'"],
    );

    // Act
    let result =
        fixture.execute("SELECT TO_CHAR(TIMESTAMPTZ '2001-02-16 20:38:40+00', 'OF') AS offset");

    // Assert
    let result = result.expect("timezone offset formatting");
    assert_eq!(result.rows(), &[vec![Value::String("-07".to_owned())]]);
    assert_eq!(result.columns()[0].type_oid, 25);
}

#[test]
fn should_format_both_occurrences_of_a_dst_fold_with_distinct_offsets() {
    // Arrange
    let fixture = sql_fixture(
        "issue-815-to-char-dst-fold",
        &["SET TIME ZONE 'America/New_York'"],
    );

    // Act
    let result = fixture.execute(
        "SELECT TO_CHAR(TIMESTAMPTZ '2024-11-03 05:30:00+00', 'HH24:MI OF') = '01:30 -04' AND TO_CHAR(TIMESTAMPTZ '2024-11-03 06:30:00+00', 'HH24:MI OF') = '01:30 -05'",
    );

    // Assert
    assert_eq!(
        result.expect("DST fold formatting").rows(),
        &[vec![Value::Bool(true)]]
    );
}

#[test]
fn should_format_interval_hours_beyond_one_day() {
    // Arrange
    let sql = "SELECT TO_CHAR(INTERVAL '36 hours', 'HH24:MI:SS') = '36:00:00'";

    // Act
    let fixture = sql_fixture("issue-815-to-char-interval", &[]);
    let result = fixture.execute(sql);

    // Assert
    assert_eq!(
        result.expect("interval formatting").rows(),
        &[vec![Value::Bool(true)]]
    );
}

#[test]
fn should_parse_formatted_calendar_dates() {
    // Arrange
    let sql = "SELECT TO_DATE('05 Dec 2000', 'DD Mon YYYY') = DATE '2000-12-05'";

    // Act
    let fixture = sql_fixture("issue-815-to-date-basic", &[]);
    let result = fixture.execute(sql);

    // Assert
    assert_eq!(
        result.expect("to_date formatted input").rows(),
        &[vec![Value::Bool(true)]]
    );
}

#[test]
fn should_parse_iso_week_dates_with_template_fields() {
    // Arrange
    let sql = "SELECT TO_DATE('2006-42-4', 'IYYY-IW-ID') = DATE '2006-10-19'";

    // Act
    let fixture = sql_fixture("issue-815-to-date-iso-week", &[]);
    let result = fixture.execute(sql);

    // Assert
    assert_eq!(
        result.expect("to_date ISO week input").rows(),
        &[vec![Value::Bool(true)]]
    );
}

#[test]
fn should_parse_case_insensitive_formatted_date_fields() {
    // Arrange
    let sql = "SELECT TO_DATE('05 dec 2000', 'DD MON YYYY') = DATE '2000-12-05'";

    // Act
    let fixture = sql_fixture("issue-815-to-date-case-folding", &[]);
    let result = fixture.execute(sql);

    // Assert
    assert_eq!(
        result.expect("case-insensitive to_date").rows(),
        &[vec![Value::Bool(true)]]
    );
}

#[test]
fn should_parse_formatted_timestamps_with_microseconds() {
    // Arrange
    let fixture = sql_fixture("issue-815-to-timestamp-fraction", &["SET TIME ZONE 'UTC'"]);

    // Act
    let result = fixture.execute(
        "SELECT TO_TIMESTAMP('05 Dec 2000 15:12:02.020001', 'DD Mon YYYY HH24:MI:SS.US') = TIMESTAMPTZ '2000-12-05 15:12:02.020001+00'",
    );

    // Assert
    assert_eq!(
        result.expect("to_timestamp microseconds").rows(),
        &[vec![Value::Bool(true)]]
    );
}

#[test]
fn should_convert_epoch_seconds_to_timestamptz() {
    // Arrange
    let fixture = sql_fixture("issue-815-to-timestamp-epoch", &["SET TIME ZONE 'UTC'"]);

    // Act
    let result =
        fixture.execute("SELECT TO_TIMESTAMP(946684800) = TIMESTAMPTZ '2000-01-01 00:00:00+00'");

    // Assert
    assert_eq!(
        result.expect("epoch seconds conversion").rows(),
        &[vec![Value::Bool(true)]]
    );
}

#[test]
fn should_parse_formatted_timestamps_with_explicit_offset() {
    // Arrange
    let fixture = sql_fixture("issue-815-to-timestamp-offset", &["SET TIME ZONE 'UTC'"]);

    // Act
    let result = fixture.execute(
        "SELECT TO_TIMESTAMP('2000-01-01 00:00:00+02', 'YYYY-MM-DD HH24:MI:SSOF') = TIMESTAMPTZ '1999-12-31 22:00:00+00'",
    );

    // Assert
    assert_eq!(
        result.expect("offset-aware timestamp input").rows(),
        &[vec![Value::Bool(true)]]
    );
}

#[test]
fn should_calculate_symbolic_age_at_month_end() {
    // Arrange
    let sql =
        "SELECT AGE(TIMESTAMP '2004-06-01', TIMESTAMP '2004-04-30') = INTERVAL '1 month 1 day'";

    // Act
    let fixture = sql_fixture("issue-815-age-month-end", &[]);
    let result = fixture.execute(sql);

    // Assert
    assert_eq!(
        result.expect("symbolic age month boundary").rows(),
        &[vec![Value::Bool(true)]]
    );
}

#[test]
fn should_calculate_timestamptz_age_by_local_calendar_days_across_dst() {
    // Arrange
    let fixture = sql_fixture(
        "issue-815-age-dst-calendar-day",
        &["SET TIME ZONE 'America/New_York'"],
    );

    // Act
    let result = fixture.execute(
        "SELECT AGE(TIMESTAMPTZ '2024-03-10 12:00:00-04', TIMESTAMPTZ '2024-03-09 12:00:00-05') = INTERVAL '1 day'",
    );

    // Assert
    assert_eq!(
        result.expect("timestamptz calendar age across DST").rows(),
        &[vec![Value::Bool(true)]]
    );
}

#[test]
fn should_return_negative_symbolic_age_for_reversed_inputs() {
    // Arrange
    let sql =
        "SELECT AGE(TIMESTAMP '2004-04-30', TIMESTAMP '2004-06-01') = INTERVAL '-1 month -1 day'";

    // Act
    let fixture = sql_fixture("issue-815-age-negative", &[]);
    let result = fixture.execute(sql);

    // Assert
    assert_eq!(
        result.expect("negative symbolic age").rows(),
        &[vec![Value::Bool(true)]]
    );
}

#[test]
fn should_return_zero_age_for_current_date_at_midnight() {
    // Arrange
    let sql = "SELECT AGE(CAST(CURRENT_DATE AS TIMESTAMP)) = INTERVAL '0'";

    // Act
    let fixture = sql_fixture("issue-815-age-current-date", &[]);
    let result = fixture.execute(sql);

    // Assert
    assert_eq!(
        result.expect("age relative to current date").rows(),
        &[vec![Value::Bool(true)]]
    );
}

#[test]
fn should_construct_date_from_calendar_fields() {
    // Arrange
    let sql = "SELECT MAKE_DATE(2013, 7, 15) = DATE '2013-07-15'";

    // Act
    let fixture = sql_fixture("issue-815-make-date", &[]);
    let result = fixture.execute(sql);

    // Assert
    assert_eq!(
        result.expect("make_date fields").rows(),
        &[vec![Value::Bool(true)]]
    );
}

#[test]
fn should_construct_time_with_fractional_seconds() {
    // Arrange
    let sql = "SELECT MAKE_TIME(8, 15, 23.5) = TIME '08:15:23.5'";

    // Act
    let fixture = sql_fixture("issue-815-make-time", &[]);
    let result = fixture.execute(sql);

    // Assert
    assert_eq!(
        result.expect("make_time fields").rows(),
        &[vec![Value::Bool(true)]]
    );
}

#[test]
fn should_construct_timestamp_with_fractional_seconds() {
    // Arrange
    let sql = "SELECT MAKE_TIMESTAMP(2013, 7, 15, 8, 15, 23.5) = TIMESTAMP '2013-07-15 08:15:23.5'";

    // Act
    let fixture = sql_fixture("issue-815-make-timestamp", &[]);
    let result = fixture.execute(sql);

    // Assert
    assert_eq!(
        result.expect("make_timestamp fields").rows(),
        &[vec![Value::Bool(true)]]
    );
}

#[test]
fn should_construct_timestamptz_with_named_zone_rules() {
    // Arrange
    let fixture = sql_fixture("issue-815-make-timestamptz", &["SET TIME ZONE 'UTC'"]);

    // Act
    let result = fixture.execute(
        "SELECT MAKE_TIMESTAMPTZ(2013, 7, 15, 8, 15, 23.5, 'America/New_York') = TIMESTAMPTZ '2013-07-15 12:15:23.5+00'",
    );

    // Assert
    assert_eq!(
        result.expect("make_timestamptz with named zone").rows(),
        &[vec![Value::Bool(true)]]
    );
}

#[test]
fn should_construct_interval_from_named_calendar_fields() {
    // Arrange
    let sql = "SELECT MAKE_INTERVAL(years => 1, months => 2, weeks => 3, days => 4, hours => 5, mins => 6, secs => 7.5) = INTERVAL '1 year 2 months 25 days 5 hours 6 minutes 7.5 seconds'";

    // Act
    let fixture = sql_fixture("issue-815-make-interval", &[]);
    let result = fixture.execute(sql);

    // Assert
    assert_eq!(
        result.expect("make_interval named fields").rows(),
        &[vec![Value::Bool(true)]]
    );
}

#[test]
fn should_bin_timestamps_by_arbitrary_stride() {
    // Arrange
    let sql = "SELECT DATE_BIN(INTERVAL '15 minutes', TIMESTAMP '2020-02-11 15:44:17', TIMESTAMP '2001-01-01') = TIMESTAMP '2020-02-11 15:30:00'";

    // Act
    let fixture = sql_fixture("issue-815-date-bin-origin", &[]);
    let result = fixture.execute(sql);

    // Assert
    assert_eq!(
        result.expect("date_bin arbitrary stride").rows(),
        &[vec![Value::Bool(true)]]
    );
}

#[test]
fn should_align_date_bin_timestamps_to_a_nondefault_origin() {
    // Arrange
    let sql = "SELECT DATE_BIN(INTERVAL '15 minutes', TIMESTAMP '2020-02-11 15:44:17', TIMESTAMP '2001-01-01 00:02:30') = TIMESTAMP '2020-02-11 15:32:30'";

    // Act
    let fixture = sql_fixture("issue-815-date-bin-nondefault-origin", &[]);
    let result = fixture.execute(sql);

    // Assert
    assert_eq!(
        result.expect("date_bin nondefault origin").rows(),
        &[vec![Value::Bool(true)]]
    );
}

#[test]
fn should_bin_timestamptz_by_fixed_elapsed_stride() {
    // Arrange
    let sql = "SELECT DATE_BIN(INTERVAL '24 hours', TIMESTAMPTZ '2024-03-10 16:30:00+00', TIMESTAMPTZ '2024-03-09 17:00:00+00') = TIMESTAMPTZ '2024-03-09 17:00:00+00'";

    // Act
    let fixture = sql_fixture("issue-815-date-bin-timestamptz", &[]);
    let result = fixture.execute(sql);

    // Assert
    assert_eq!(
        result.expect("timestamptz fixed-duration bin").rows(),
        &[vec![Value::Bool(true)]]
    );
}

#[test]
fn should_justify_day_components_as_calendar_months() {
    // Arrange
    let sql = "SELECT JUSTIFY_DAYS(INTERVAL '1 year 65 days') = INTERVAL '1 year 2 months 5 days'";

    // Act
    let fixture = sql_fixture("issue-815-justify-days", &[]);
    let result = fixture.execute(sql);

    // Assert
    assert_eq!(
        result.expect("justify_days").rows(),
        &[vec![Value::Bool(true)]]
    );
}

#[test]
fn should_justify_hour_components_as_calendar_days() {
    // Arrange
    let sql = "SELECT JUSTIFY_HOURS(INTERVAL '27 hours') = INTERVAL '1 day 3 hours'";

    // Act
    let fixture = sql_fixture("issue-815-justify-hours", &[]);
    let result = fixture.execute(sql);

    // Assert
    assert_eq!(
        result.expect("justify_hours").rows(),
        &[vec![Value::Bool(true)]]
    );
}

#[test]
fn should_justify_mixed_sign_interval_components() {
    // Arrange
    let sql = "SELECT JUSTIFY_INTERVAL(INTERVAL '1 month -1 hour') = INTERVAL '29 days 23 hours'";

    // Act
    let fixture = sql_fixture("issue-815-justify-interval", &[]);
    let result = fixture.execute(sql);

    // Assert
    assert_eq!(
        result.expect("justify_interval mixed signs").rows(),
        &[vec![Value::Bool(true)]]
    );
}

#[test]
fn should_test_finiteness_of_supported_temporal_values() {
    // Arrange
    let sql = "SELECT ISFINITE(DATE '2001-02-16') AND NOT ISFINITE(DATE 'infinity') AND NOT ISFINITE(TIMESTAMP 'infinity') AND ISFINITE(INTERVAL '4 hours')";

    // Act
    let fixture = sql_fixture("issue-815-isfinite", &[]);
    let result = fixture.execute(sql);

    // Assert
    assert_eq!(
        result.expect("temporal finiteness").rows(),
        &[vec![Value::Bool(true)]]
    );
}

#[test]
fn should_add_calendar_interval_with_explicit_zone_rules() {
    // Arrange
    let sql = "SELECT DATE_ADD(TIMESTAMPTZ '2024-03-09 12:00:00 America/New_York', INTERVAL '1 day', 'America/New_York') = TIMESTAMPTZ '2024-03-10 16:00:00+00'";

    // Act
    let fixture = sql_fixture("issue-815-date-add-zone", &[]);
    let result = fixture.execute(sql);

    // Assert
    assert_eq!(
        result.expect("date_add with DST zone").rows(),
        &[vec![Value::Bool(true)]]
    );
}

#[test]
fn should_subtract_calendar_interval_with_explicit_zone_rules() {
    // Arrange
    let sql = "SELECT DATE_SUBTRACT(TIMESTAMPTZ '2024-03-11 12:00:00 America/New_York', INTERVAL '1 day', 'America/New_York') = TIMESTAMPTZ '2024-03-10 16:00:00+00'";

    // Act
    let fixture = sql_fixture("issue-815-date-subtract-zone", &[]);
    let result = fixture.execute(sql);

    // Assert
    assert_eq!(
        result.expect("date_subtract with DST zone").rows(),
        &[vec![Value::Bool(true)]]
    );
}

#[test]
fn should_treat_overlaps_periods_as_half_open() {
    // Arrange
    let sql = "SELECT (TIMESTAMP '2001-02-16 00:00:00', INTERVAL '1 day') OVERLAPS (TIMESTAMP '2001-02-16 12:00:00', INTERVAL '1 day') AND NOT ((TIMESTAMP '2001-02-16 00:00:00', INTERVAL '1 day') OVERLAPS (TIMESTAMP '2001-02-17 00:00:00', INTERVAL '1 day'))";

    // Act
    let fixture = sql_fixture("issue-815-overlaps-half-open", &[]);
    let result = fixture.execute(sql);

    // Assert
    assert_eq!(
        result.expect("OVERLAPS endpoint semantics").rows(),
        &[vec![Value::Bool(true)]]
    );
}

#[test]
fn should_convert_roman_numerals_with_to_number() {
    // Arrange
    let sql = "SELECT TO_NUMBER('XII', 'RN') = 12";

    // Act
    let fixture = sql_fixture("issue-815-to-number-roman", &[]);
    let result = fixture.execute(sql);

    // Assert
    assert_eq!(
        result.expect("Roman numeral input").rows(),
        &[vec![Value::Bool(true)]]
    );
}

#[test]
fn should_preserve_null_results_for_temporal_functions() {
    // Arrange
    let sql = "SELECT DATE_TRUNC('day', NULL::TIMESTAMP) IS NULL AND TO_CHAR(NULL::DATE, 'YYYY') IS NULL AND TO_DATE(NULL::TEXT, 'YYYY-MM-DD') IS NULL AND AGE(NULL::TIMESTAMP, TIMESTAMP '2001-01-01') IS NULL";

    // Act
    let fixture = sql_fixture("issue-815-temporal-function-nulls", &[]);
    let result = fixture.execute(sql);

    // Assert
    assert_eq!(
        result.expect("NULL temporal function arguments").rows(),
        &[vec![Value::Bool(true)]]
    );
}

#[test]
fn should_sleep_for_at_least_the_requested_interval() {
    // Arrange
    let fixture = sql_fixture("issue-815-pg-sleep-for", &[]);
    let start = Instant::now();

    // Act
    let result = fixture.execute("SELECT PG_SLEEP_FOR(INTERVAL '50 milliseconds')");
    let elapsed = start.elapsed();

    // Assert
    result.expect("pg_sleep_for");
    assert!(
        elapsed >= Duration::from_millis(40),
        "sleep returned before its interval elapsed: {elapsed:?}"
    );
}

#[test]
fn should_sleep_until_the_requested_wall_clock_time() {
    // Arrange
    let fixture = sql_fixture("issue-815-pg-sleep-until", &[]);
    let start = Instant::now();

    // Act
    let result =
        fixture.execute("SELECT PG_SLEEP_UNTIL(CLOCK_TIMESTAMP() + INTERVAL '50 milliseconds')");
    let elapsed = start.elapsed();

    // Assert
    result.expect("pg_sleep_until");
    assert!(
        elapsed >= Duration::from_millis(40),
        "sleep returned before target time: {elapsed:?}"
    );
}
