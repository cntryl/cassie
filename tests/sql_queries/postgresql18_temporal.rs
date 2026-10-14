//! PostgreSQL 18 date, time, timezone, and interval behavior tracked by #814.

use cassie::app::Cassie;
use cassie::types::Value;

use crate::support_sql_fixture::{sql_fixture, SqlFixture};

fn assert_true(fixture: &SqlFixture, sql: &str) {
    let result = fixture.execute(sql);
    assert_eq!(
        result.expect("temporal SQL expression").rows(),
        &[vec![Value::Bool(true)]],
        "expected PostgreSQL temporal invariant: {sql}"
    );
}

#[test]
fn should_compare_timestamptz_by_instant_across_fixed_offsets() {
    // Arrange
    let fixture = sql_fixture("issue-814-timestamptz-fixed-offset", &[]);

    // Act
    let sql = "SELECT TIMESTAMPTZ '2024-01-01 00:00:00+00' = TIMESTAMPTZ '2023-12-31 19:00:00-05'";

    // Assert
    assert_true(&fixture, sql);
}

#[test]
fn should_ignore_offset_text_for_timestamp_without_time_zone() {
    // Arrange
    let fixture = sql_fixture("issue-814-timestamp-ignores-offset", &[]);

    // Act
    let sql = "SELECT TIMESTAMP '2001-02-16 20:38:40-05' = TIMESTAMP '2001-02-16 20:38:40+00'";

    // Assert
    assert_true(&fixture, sql);
}

#[test]
fn should_apply_session_timezone_when_timestamptz_input_has_no_zone() {
    // Arrange
    let fixture = sql_fixture(
        "issue-814-session-timezone-input",
        &["SET TIME ZONE 'America/New_York'"],
    );

    // Act
    let sql = "SELECT TIMESTAMPTZ '2024-07-01 12:00:00' = TIMESTAMPTZ '2024-07-01 16:00:00+00'";

    // Assert
    assert_true(&fixture, sql);
}

#[test]
fn should_change_local_timestamp_interpretation_with_session_timezone() {
    // Arrange
    let fixture = sql_fixture("issue-814-session-timezone-changes", &[]);

    // Act
    let first = fixture.execute("SET TIME ZONE 'UTC'");
    let second = fixture
        .execute("SELECT TIMESTAMPTZ '2024-01-01 00:00:00' = TIMESTAMPTZ '2024-01-01 00:00:00+00'");
    let third = fixture.execute("SET TIME ZONE 'America/New_York'");
    let fourth = fixture
        .execute("SELECT TIMESTAMPTZ '2024-01-01 00:00:00' = TIMESTAMPTZ '2024-01-01 05:00:00+00'");

    // Assert
    first.expect("set UTC");
    assert_eq!(
        second.expect("UTC interpretation").rows(),
        &[vec![Value::Bool(true)]]
    );
    third.expect("set named zone");
    assert_eq!(
        fourth.expect("New York interpretation").rows(),
        &[vec![Value::Bool(true)]]
    );
}

#[test]
fn should_convert_timestamp_without_zone_to_instant_at_named_zone() {
    // Arrange
    let fixture = sql_fixture("issue-814-at-time-zone-to-instant", &[]);

    // Act
    let sql = "SELECT TIMESTAMP '2001-02-16 20:38:40' AT TIME ZONE 'America/Denver' = TIMESTAMPTZ '2001-02-17 03:38:40+00'";

    // Assert
    assert_true(&fixture, sql);
}

#[test]
fn should_convert_timestamptz_to_local_timestamp_at_named_zone() {
    // Arrange
    let fixture = sql_fixture("issue-814-at-time-zone-to-local", &[]);

    // Act
    let sql = "SELECT TIMESTAMPTZ '2001-02-17 03:38:40+00' AT TIME ZONE 'America/Denver' = TIMESTAMP '2001-02-16 20:38:40'";

    // Assert
    assert_true(&fixture, sql);
}

#[test]
fn should_apply_interval_valued_at_time_zone_offset() {
    // Arrange
    let fixture = sql_fixture("issue-814-at-time-zone-interval", &[]);

    // Act
    let sql = "SELECT (TIMESTAMP '2001-02-16 20:38:40' AT TIME ZONE INTERVAL '-08:00') = TIMESTAMPTZ '2001-02-17 04:38:40+00'";

    // Assert
    assert_true(&fixture, sql);
}

#[test]
fn should_apply_at_local_using_the_session_timezone() {
    // Arrange
    let fixture = sql_fixture(
        "issue-814-at-local-session-zone",
        &["SET TIME ZONE 'America/Denver'"],
    );

    // Act
    let sql =
        "SELECT TIMESTAMP '2001-02-16 20:38:40' AT LOCAL = TIMESTAMPTZ '2001-02-17 03:38:40+00'";

    // Assert
    assert_true(&fixture, sql);
}

#[test]
fn should_resolve_daylight_saving_overlap_using_standard_time() {
    // Arrange
    let fixture = sql_fixture(
        "issue-814-dst-overlap",
        &["SET TIME ZONE 'America/New_York'"],
    );

    // Act
    let sql = "SELECT TIMESTAMPTZ '2024-11-03 01:30:00 America/New_York' = TIMESTAMPTZ '2024-11-03 06:30:00+00'";

    // Assert
    assert_true(&fixture, sql);
}

#[test]
fn should_resolve_daylight_saving_gap_using_standard_time() {
    // Arrange
    let fixture = sql_fixture("issue-814-dst-gap", &["SET TIME ZONE 'America/New_York'"]);

    // Act
    let sql = "SELECT TIMESTAMPTZ '2024-03-10 02:30:00 America/New_York' = TIMESTAMPTZ '2024-03-10 07:30:00+00'";

    // Assert
    assert_true(&fixture, sql);
}

#[test]
fn should_add_calendar_day_across_daylight_saving_transition() {
    // Arrange
    let fixture = sql_fixture(
        "issue-814-calendar-day-dst",
        &["SET TIME ZONE 'America/New_York'"],
    );

    // Act
    let sql = "SELECT TIMESTAMPTZ '2024-03-09 12:00:00 America/New_York' + INTERVAL '1 day' = TIMESTAMPTZ '2024-03-10 16:00:00+00'";

    // Assert
    assert_true(&fixture, sql);
}

#[test]
fn should_distinguish_calendar_day_from_twenty_four_hours_across_dst() {
    // Arrange
    let fixture = sql_fixture(
        "issue-814-calendar-day-vs-hours",
        &["SET TIME ZONE 'America/New_York'"],
    );

    // Act
    let sql = "SELECT TIMESTAMPTZ '2024-03-09 12:00:00 America/New_York' + INTERVAL '1 day' <> TIMESTAMPTZ '2024-03-09 12:00:00 America/New_York' + INTERVAL '24 hours'";

    // Assert
    assert_true(&fixture, sql);
}

#[test]
fn should_clamp_month_addition_at_non_leap_month_end() {
    // Arrange
    let fixture = sql_fixture("issue-814-month-end-common-year", &[]);

    // Act
    let sql = "SELECT DATE '2001-01-31' + INTERVAL '1 month' = TIMESTAMP '2001-02-28 00:00:00'";

    // Assert
    assert_true(&fixture, sql);
}

#[test]
fn should_clamp_month_addition_at_leap_month_end() {
    // Arrange
    let fixture = sql_fixture("issue-814-month-end-leap-year", &[]);

    // Act
    let sql = "SELECT DATE '2000-01-31' + INTERVAL '1 month' = TIMESTAMP '2000-02-29 00:00:00'";

    // Assert
    assert_true(&fixture, sql);
}

#[test]
fn should_keep_months_distinct_from_thirty_day_intervals() {
    // Arrange
    let fixture = sql_fixture("issue-814-calendar-month-vs-days", &[]);

    // Act
    let sql =
        "SELECT DATE '2001-03-01' - INTERVAL '1 month' <> DATE '2001-03-01' - INTERVAL '30 days'";

    // Assert
    assert_true(&fixture, sql);
}

#[test]
fn should_apply_integer_day_offsets_to_date() {
    // Arrange
    let fixture = sql_fixture("issue-814-date-integer-arithmetic", &[]);

    // Act
    let sql = "SELECT DATE '2001-09-28' + 7 = DATE '2001-10-05' AND DATE '2001-09-28' - 7 = DATE '2001-09-21'";

    // Assert
    assert_true(&fixture, sql);
}

#[test]
fn should_subtract_dates_to_an_integer_day_count() {
    // Arrange
    let fixture = sql_fixture("issue-814-date-difference", &[]);

    // Act
    let sql = "SELECT DATE '2001-10-05' - DATE '2001-09-28' = 7";

    // Assert
    assert_true(&fixture, sql);
}

#[test]
fn should_convert_date_plus_time_to_timestamp() {
    // Arrange
    let fixture = sql_fixture("issue-814-date-plus-time", &[]);

    // Act
    let sql = "SELECT DATE '2001-09-28' + TIME '03:00:00' = TIMESTAMP '2001-09-28 03:00:00'";

    // Assert
    assert_true(&fixture, sql);
}

#[test]
fn should_apply_calendar_offsets_to_date() {
    // Arrange
    let fixture = sql_fixture("issue-814-date-interval-arithmetic", &[]);

    // Act
    let sql = "SELECT DATE '2001-01-01' + INTERVAL '2 months' = TIMESTAMP '2001-03-01' AND DATE '2001-03-01' - INTERVAL '2 months' = TIMESTAMP '2001-01-01'";

    // Assert
    assert_true(&fixture, sql);
}

#[test]
fn should_wrap_time_interval_arithmetic_at_midnight() {
    // Arrange
    let fixture = sql_fixture("issue-814-time-interval-wrap", &[]);

    // Act
    let sql = "SELECT TIME '23:00:00' + INTERVAL '2 hours' = TIME '01:00:00' AND TIME '01:00:00' - INTERVAL '2 hours' = TIME '23:00:00'";

    // Assert
    assert_true(&fixture, sql);
}

#[test]
fn should_subtract_times_to_an_interval() {
    // Arrange
    let fixture = sql_fixture("issue-814-time-difference", &[]);

    // Act
    let sql = "SELECT TIME '05:00:00' - TIME '03:00:00' = INTERVAL '2 hours'";

    // Assert
    assert_true(&fixture, sql);
}

#[test]
fn should_subtract_timestamps_to_a_calendar_interval() {
    // Arrange
    let fixture = sql_fixture("issue-814-timestamp-difference", &[]);

    // Act
    let sql = "SELECT TIMESTAMP '2001-09-29 03:00:00' - TIMESTAMP '2001-09-28 01:00:00' = INTERVAL '1 day 2 hours'";

    // Assert
    assert_true(&fixture, sql);
}

#[test]
fn should_scale_intervals_by_fractional_values() {
    // Arrange
    let fixture = sql_fixture("issue-814-interval-scaling", &[]);

    // Act
    let sql = "SELECT INTERVAL '1 day' * 2.5 = INTERVAL '2 days 12 hours' AND INTERVAL '3 hours' / 1.5 = INTERVAL '2 hours'";

    // Assert
    assert_true(&fixture, sql);
}

#[test]
fn should_negate_each_interval_component() {
    // Arrange
    let fixture = sql_fixture("issue-814-interval-negation", &[]);

    // Act
    let sql = "SELECT -INTERVAL '1 year 2 months 3 days 4 hours' = INTERVAL '-1 year -2 months -3 days -4 hours'";

    // Assert
    assert_true(&fixture, sql);
}

#[test]
fn should_accept_timetz_literals_with_distinct_zone_offsets() {
    // Arrange
    let fixture = sql_fixture("issue-814-timetz-comparison", &[]);

    // Act
    let sql = "SELECT TIMETZ '05:34:17-05' = TIMETZ '10:34:17+00', TIMETZ '05:34:17-05' <> TIMETZ '10:34:17+00', TIMETZ '05:34:17-05' = TIMETZ '05:34:17-04', TIMETZ '05:34:17-05' <> TIMETZ '05:34:17-04'";

    // Assert
    assert_eq!(
        fixture.execute(sql).expect("timetz literal values").rows(),
        &[vec![
            Value::Bool(false),
            Value::Bool(true),
            Value::Bool(false),
            Value::Bool(true),
        ]]
    );
}

#[test]
fn should_preserve_twenty_four_hour_time_as_a_distinct_endpoint() {
    // Arrange
    let fixture = sql_fixture("issue-814-time-endpoint", &[]);

    // Act
    let sql =
        "SELECT TIME '24:00:00' > TIME '23:59:59.999999' AND TIME '24:00:00' <> TIME '00:00:00'";

    // Assert
    assert_true(&fixture, sql);
}

#[test]
fn should_parse_verbose_ago_intervals_as_negative_components() {
    // Arrange
    let fixture = sql_fixture("issue-814-interval-ago", &[]);

    // Act
    let sql = "SELECT INTERVAL '@ 1 year 2 mons ago' = INTERVAL '-1 year -2 months'";

    // Assert
    assert_true(&fixture, sql);
}

#[test]
fn should_normalize_interval_unit_forms() {
    // Arrange
    let fixture = sql_fixture("issue-814-interval-unit-inputs", &[]);

    // Act
    let sql = "SELECT INTERVAL '1.5 weeks' = INTERVAL '10 days 12 hours' AND INTERVAL '1500 milliseconds' = INTERVAL '1.5 seconds' AND INTERVAL '2 decades' = INTERVAL '20 years'";

    // Assert
    assert_true(&fixture, sql);
}

#[test]
fn should_parse_both_iso_interval_notations() {
    // Arrange
    let fixture = sql_fixture("issue-814-interval-iso-inputs", &[]);

    // Act
    let sql = "SELECT INTERVAL 'P1Y2M3DT4H5M6S' = INTERVAL 'P0001-02-03T04:05:06'";

    // Assert
    assert_true(&fixture, sql);
}

#[test]
fn should_parse_sql_standard_interval_forms() {
    // Arrange
    let fixture = sql_fixture("issue-814-interval-sql-standard-inputs", &[]);

    // Act
    let sql = "SELECT INTERVAL '1-2' = INTERVAL '1 year 2 months' AND INTERVAL '3 4:05:06' = INTERVAL '3 days 4 hours 5 minutes 6 seconds'";

    // Assert
    assert_true(&fixture, sql);
}

#[test]
fn should_normalize_fractional_interval_fields_without_losing_microseconds() {
    // Arrange
    let fixture = sql_fixture("issue-814-interval-fractional-microseconds", &[]);

    // Act
    let sql = "SELECT INTERVAL '1.123456 seconds' = INTERVAL '1123456 microseconds' AND INTERVAL '1.5 years' = INTERVAL '1 year 6 months'";

    // Assert
    assert_true(&fixture, sql);
}

#[test]
fn should_follow_datestyle_for_ambiguous_date_input() {
    // Arrange
    let fixture = sql_fixture("issue-814-datestyle-input-order", &[]);

    // Act
    let mdy = fixture.execute("SET DateStyle = 'ISO, MDY'");
    let mdy_result = fixture.execute("SELECT DATE '1/8/1999' = DATE '1999-01-08'");
    let dmy = fixture.execute("SET DateStyle = 'ISO, DMY'");
    let dmy_result = fixture.execute("SELECT DATE '1/8/1999' = DATE '1999-08-01'");

    // Assert
    mdy.expect("set MDY input order");
    assert_eq!(
        mdy_result.expect("MDY date input").rows(),
        &[vec![Value::Bool(true)]]
    );
    dmy.expect("set DMY input order");
    assert_eq!(
        dmy_result.expect("DMY date input").rows(),
        &[vec![Value::Bool(true)]]
    );
}

#[test]
fn should_parse_documented_date_input_spellings() {
    // Arrange
    let fixture = sql_fixture("issue-814-date-input-forms", &[]);

    // Act
    let sql = "SELECT DATE 'January 8, 1999' = DATE '1999-01-08' AND DATE '1999-Jan-08' = DATE '1999-01-08' AND DATE '08-Jan-99' = DATE '1999-01-08' AND DATE '19990108' = DATE '1999-01-08' AND DATE '990108' = DATE '1999-01-08' AND DATE '1999.008' = DATE '1999-01-08' AND DATE 'J2451187' = DATE '1999-01-08'";

    // Assert
    assert_true(&fixture, sql);
}

#[test]
fn should_order_era_dates_without_year_zero() {
    // Arrange
    let fixture = sql_fixture("issue-814-date-era-order", &[]);

    // Act
    let sql = "SELECT DATE '0001-01-01 BC' < DATE '0001-01-01 AD' AND DATE '0001-01-01 AD' = DATE '0001-01-01'";

    // Assert
    assert_true(&fixture, sql);
}

#[test]
fn should_accept_postgresql_temporal_range_endpoints() {
    // Arrange
    let fixture = sql_fixture("issue-814-temporal-range-endpoints", &[]);

    // Act
    let sql = "SELECT DATE '4713-01-01 BC' < DATE '0001-01-01 BC' AND DATE '5874897-12-31' > DATE '294276-12-31' AND TIMESTAMP '4713-01-01 BC' < TIMESTAMP '0001-01-01 BC' AND TIMESTAMP '294276-12-31 23:59:59.999999' > TIMESTAMP '294276-01-01'";

    // Assert
    assert_true(&fixture, sql);
}

#[test]
fn should_follow_ymd_datestyle_for_ambiguous_date_input() {
    // Arrange
    let fixture = sql_fixture("issue-814-datestyle-ymd", &["SET DateStyle = 'ISO, YMD'"]);

    // Act
    let sql = "SELECT DATE '01/02/03' = DATE '2001-02-03' AND DATE '99-Jan-08' = DATE '1999-01-08'";

    // Assert
    assert_true(&fixture, sql);
}

#[test]
fn should_parse_compact_ampm_iso_time_forms() {
    // Arrange
    let fixture = sql_fixture("issue-814-time-input-forms", &[]);

    // Act
    let sql = "SELECT TIME '040506' = TIME '04:05:06' AND TIME '04:05 PM' = TIME '16:05:00' AND TIMESTAMP '1999-01-08T04:05:06' = TIMESTAMP '1999-01-08 04:05:06'";

    // Assert
    assert_true(&fixture, sql);
}

#[test]
fn should_parse_timezone_offset_spellings() {
    // Arrange
    let fixture = sql_fixture("issue-814-offset-input-forms", &[]);

    // Act
    let sql = "SELECT TIMESTAMPTZ '2001-02-16 20:38:40-8' = TIMESTAMPTZ '2001-02-17 04:38:40+00' AND TIMESTAMPTZ '2001-02-16 20:38:40-08:00' = TIMESTAMPTZ '2001-02-16 20:38:40-0800' AND TIMESTAMPTZ '2001-02-16 20:38:40+0730' = TIMESTAMPTZ '2001-02-16 20:38:40+07:30:00' AND TIMESTAMPTZ '2001-02-16 20:38:40zulu' = TIMESTAMPTZ '2001-02-16 20:38:40Z'";

    // Assert
    assert_true(&fixture, sql);
}

#[test]
fn should_parse_timezone_abbreviation_full_name_posix_forms() {
    // Arrange
    let fixture = sql_fixture("issue-814-zone-name-inputs", &[]);

    // Act
    let sql = "SELECT TIMESTAMPTZ '2001-02-16 20:38:40 PST' = TIMESTAMPTZ '2001-02-16 20:38:40-08' AND TIMESTAMPTZ '2024-07-01 12:00:00 America/New_York' = TIMESTAMPTZ '2024-07-01 16:00:00+00' AND TIMESTAMPTZ '2001-02-16 20:38:40 PST8PDT' = TIMESTAMPTZ '2001-02-16 20:38:40-08'";

    // Assert
    assert_true(&fixture, sql);
}

#[test]
fn should_prefer_session_timezone_abbreviation_before_abbreviation_set() {
    // Arrange
    let fixture = sql_fixture(
        "issue-814-timezone-abbreviation-precedence",
        &[
            "SET TIME ZONE 'America/Los_Angeles'",
            "SET timezone_abbreviations = 'Australia'",
        ],
    );

    // Act
    let sql = "SELECT TIMESTAMPTZ '2024-01-01 12:00:00 PST' = TIMESTAMPTZ '2024-01-01 20:00:00+00'";

    // Assert
    assert_true(&fixture, sql);
}

#[test]
fn should_recognize_date_time_special_inputs() {
    // Arrange
    let fixture = sql_fixture("issue-814-date-time-special-inputs", &[]);

    // Act
    let sql = "SELECT TIMESTAMPTZ 'epoch' = TIMESTAMPTZ '1970-01-01 00:00:00+00' AND DATE 'epoch' = DATE '1970-01-01' AND DATE 'today' = CURRENT_DATE AND DATE 'tomorrow' = CURRENT_DATE + 1 AND DATE 'yesterday' = CURRENT_DATE - 1 AND TIME 'allballs' = TIME '00:00:00'";

    // Assert
    assert_true(&fixture, sql);
}

struct TemporalDirectory(String);

impl Drop for TemporalDirectory {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn should_preserve_timestamptz_instants_across_restart() {
    // Arrange
    crate::support_sql::use_local_storage();
    let path = crate::support_sql::data_dir("issue-814-timestamptz-restart");
    let _directory = TemporalDirectory(path.clone());
    {
        let cassie = Cassie::new_with_data_dir(&path).expect("create temporal fixture");
        cassie.startup().expect("start temporal fixture");
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE issue_814_restart_values (value TIMESTAMPTZ)",
                vec![],
            )
            .expect("create timestamptz column");
        cassie
            .execute_sql(
                &session,
                "INSERT INTO issue_814_restart_values VALUES ('2024-01-01 00:00:00+00'), ('2023-12-31 19:00:00-05')",
                vec![],
            )
            .expect("store equal instants with distinct offsets");
    }
    let restarted = Cassie::new_with_data_dir(&path).expect("reopen temporal fixture");
    restarted.startup().expect("restart temporal fixture");
    let session = restarted.create_session("tester", None);

    // Act
    let result = restarted.execute_sql(
        &session,
        "SELECT COUNT(*) = 2 FROM issue_814_restart_values WHERE value = TIMESTAMPTZ '2024-01-01 00:00:00+00'",
        vec![],
    );

    // Assert
    assert_eq!(
        result
            .expect("stored timestamptz instants survive restart")
            .rows(),
        &[vec![Value::Bool(true)]]
    );
    drop(session);
    restarted.shutdown();
    drop(restarted);
}

#[test]
fn should_treat_now_as_transaction_start_time() {
    // Arrange
    let fixture = sql_fixture("issue-814-now-special-input", &[]);

    // Act
    let sql = "SELECT TIMESTAMPTZ 'now' = CURRENT_TIMESTAMP";

    // Assert
    assert_true(&fixture, sql);
}

#[test]
fn should_order_infinity_values_during_arithmetic() {
    // Arrange
    let fixture = sql_fixture("issue-814-infinity-arithmetic", &[]);

    // Act
    let sql = "SELECT DATE '-infinity' < DATE '0001-01-01 BC' AND TIMESTAMPTZ 'infinity' > TIMESTAMPTZ '294276-01-01' AND TIMESTAMPTZ 'infinity' + INTERVAL '1 day' = TIMESTAMPTZ 'infinity' AND TIMESTAMPTZ '-infinity' - INTERVAL '1 day' = TIMESTAMPTZ '-infinity'";

    // Assert
    assert_true(&fixture, sql);
}

#[test]
fn should_aggregate_interval_values() {
    // Arrange
    let fixture = sql_fixture(
        "issue-814-interval-aggregates",
        &[
            "CREATE TABLE issue_814_intervals (value INTERVAL)",
            "INSERT INTO issue_814_intervals VALUES (INTERVAL '1 day'), (INTERVAL '2 days')",
        ],
    );

    // Act
    let sql = "SELECT SUM(value) = INTERVAL '3 days', AVG(value) = INTERVAL '1 day 12 hours' FROM issue_814_intervals";
    let result = fixture.execute(sql);

    // Assert
    assert_eq!(
        result.expect("interval aggregates").rows(),
        &[vec![Value::Bool(true), Value::Bool(true)]]
    );
}

#[test]
fn should_convert_timezone_function_timestamp_overloads() {
    // Arrange
    let fixture = sql_fixture(
        "issue-814-timezone-function-overloads",
        &["SET TIME ZONE 'UTC'"],
    );

    // Act
    let sql = "SELECT timezone('America/Denver', TIMESTAMP '2001-02-16 20:38:40') = TIMESTAMPTZ '2001-02-17 03:38:40+00' AND timezone('UTC', TIMESTAMPTZ '2001-02-17 03:38:40+00') = TIMESTAMP '2001-02-17 03:38:40'";

    // Assert
    assert_true(&fixture, sql);
}

#[test]
fn should_convert_timezone_function_time_overloads() {
    // Arrange
    let fixture = sql_fixture("issue-814-timezone-function-time", &[]);

    // Act
    let sql = "SELECT timezone('UTC', TIMETZ '05:34:17-05') = TIMETZ '10:34:17+00'";

    // Assert
    assert_true(&fixture, sql);
}

#[test]
fn should_apply_at_local_to_time_with_time_zone() {
    // Arrange
    let fixture = sql_fixture("issue-814-at-local-timetz", &["SET TIME ZONE 'UTC'"]);

    // Act
    let sql = "SELECT TIME WITH TIME ZONE '20:38:40-05' AT LOCAL = TIMETZ '01:38:40+00'";

    // Assert
    assert_true(&fixture, sql);
}

#[test]
fn should_truncate_interval_fields_to_declared_qualifier() {
    // Arrange
    let fixture = sql_fixture("issue-814-interval-field-qualifier", &[]);

    // Act
    let sql = "SELECT INTERVAL '1 day 2:03:04' HOUR TO MINUTE = INTERVAL '1 day 2 hours 3 minutes'";

    // Assert
    assert_true(&fixture, sql);
}

#[test]
fn should_round_temporal_fractional_precision_at_typmod_boundary() {
    // Arrange
    let fixture = sql_fixture("issue-814-temporal-precision-rounding", &[]);

    // Act
    let sql = "SELECT TIMESTAMP(3) '2001-02-16 20:38:40.1235' = TIMESTAMP '2001-02-16 20:38:40.124' AND INTERVAL(3) '1.2345 seconds' = INTERVAL '1.235 seconds'";

    // Assert
    assert_true(&fixture, sql);
}
