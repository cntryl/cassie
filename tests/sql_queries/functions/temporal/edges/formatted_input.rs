//! PostgreSQL 18.6 formatted-input boundaries; typed TIME endpoints are separate.

use cassie::app::CassieError;
use cassie::types::Value;

use crate::support_sql_fixture::sql_fixture;

#[test]
fn should_preserve_negative_zero_hour_for_formatted_zone_minutes() {
    // Arrange
    let fixture = sql_fixture("formatted-zone-negative-zero", &["SET TIME ZONE 'UTC'"]);
    let cases = [
        ("-00:30", "TZH:TZM", "2000-01-01 12:30:00+00"),
        ("+00:30", "TZH:TZM", "2000-01-01 11:30:00+00"),
        ("-01:30", "TZH:TZM", "2000-01-01 13:30:00+00"),
        ("-00:30", "OF", "2000-01-01 12:30:00+00"),
        ("30 -00", "TZM TZH", "2000-01-01 12:30:00+00"),
        ("30 -01", "TZM TZH", "2000-01-01 13:30:00+00"),
        ("+15:59", "TZH:TZM", "1999-12-31 20:01:00+00"),
        ("-15:59", "OF", "2000-01-02 03:59:00+00"),
    ];

    for (offset, zone_template, expected) in cases {
        // Act
        let result = fixture.execute(&format!(
            "SELECT to_timestamp('2000-01-01 12:00:00 {offset}', 'YYYY-MM-DD HH24:MI:SS {zone_template}') = TIMESTAMPTZ '{expected}'"
        ));

        // Assert
        assert_eq!(
            result.expect("formatted offset").rows(),
            &[vec![Value::Bool(true)]]
        );
    }
}

#[test]
fn should_reject_out_of_range_formatted_timezone_fields() {
    // Arrange
    let fixture = sql_fixture("formatted-zone-field-range", &["SET TIME ZONE 'UTC'"]);
    let cases = [
        ("+16:00", "TZH:TZM"),
        ("-16:00", "TZH:TZM"),
        ("+01:60", "TZH:TZM"),
        ("-00:60", "TZH:TZM"),
        ("+00:-30", "TZH:TZM"),
        ("+16:00", "OF"),
        ("+01:60", "OF"),
    ];

    for function in ["to_date", "to_timestamp"] {
        for (offset, template) in cases {
            // Act
            let result = fixture.execute(&format!(
                "SELECT {function}('2000-01-01 {offset}', 'YYYY-MM-DD {template}')"
            ));

            // Assert
            assert!(
                matches!(result, Err(CassieError::TypeInput(error)) if error.sqlstate == "22009")
            );
        }
    }
}

#[test]
fn should_reject_conflicting_formatted_timezone_fields() {
    // Arrange
    let fixture = sql_fixture("formatted-zone-field-conflict", &["SET TIME ZONE 'UTC'"]);
    let cases = [("-01 +02 30", "TZH TZH TZM"), ("-01 30 15", "TZH TZM TZM")];

    for (offset, template) in cases {
        // Act
        let result = fixture.execute(&format!(
            "SELECT to_timestamp('2000-01-01 {offset}', 'YYYY-MM-DD {template}')"
        ));

        // Assert
        assert!(matches!(result, Err(CassieError::TypeInput(error)) if error.sqlstate == "22007"));
    }
}

#[test]
fn should_reject_invalid_formatted_clock_fields() {
    // Arrange
    let fixture = sql_fixture("formatted-clock-input-range", &["SET TIME ZONE 'UTC'"]);
    let inputs = [
        "2000-01-01 24:00:00.000000",
        "2000-01-01 24:01:00.000000",
        "2000-01-01 23:59:60.000000",
        "2000-01-01 12:00:00.9999999",
    ];

    for function in ["to_timestamp", "to_date"] {
        for input in inputs {
            // Act
            let result = fixture.execute(&format!(
                "SELECT {function}('{input}', 'YYYY-MM-DD HH24:MI:SS.US')"
            ));

            // Assert
            assert!(matches!(
                result,
                Err(CassieError::TypeInput(error)) if error.sqlstate == "22008"
            ));
        }
    }
}

#[test]
fn should_reject_formatted_instants_outside_timestamp_range_after_zone_conversion() {
    // Arrange
    let fixture = sql_fixture("formatted-zone-input-range", &["SET TIME ZONE 'UTC'"]);
    let inputs = [
        ("294276-12-31 23:59:59 -01", "YYYY-MM-DD HH24:MI:SS TZH"),
        ("294276-12-31 23:59:59 -15", "YYYY-MM-DD HH24:MI:SS TZH"),
        ("4714-11-24 BC 00:00:00 +01", "YYYY-MM-DD BC HH24:MI:SS TZH"),
    ];

    for (input, template) in inputs {
        // Act
        let result = fixture.execute(&format!("SELECT to_timestamp('{input}', '{template}')"));

        // Assert
        assert!(matches!(
            result,
            Err(CassieError::TypeInput(error)) if error.sqlstate == "22008"
        ));
    }
}

#[test]
fn should_reject_formatted_instants_outside_timestamp_range_in_session_timezone() {
    // Arrange
    let fixture = sql_fixture(
        "formatted-session-zone-input-range",
        &["SET TIME ZONE '-01'"],
    );

    // Act
    let result =
        fixture.execute("SELECT to_timestamp('294276-12-31 23:59:59', 'YYYY-MM-DD HH24:MI:SS')");

    // Assert
    assert!(matches!(
        result,
        Err(CassieError::TypeInput(error)) if error.sqlstate == "22008"
    ));
}

#[test]
fn should_accept_formatted_wall_times_moved_inside_timestamp_range_by_offset() {
    // Arrange
    let fixture = sql_fixture("formatted-zone-restores-range", &["SET TIME ZONE 'UTC'"]);
    let cases = [
        (
            "294277-01-01 00:30:00 +01",
            "YYYY-MM-DD HH24:MI:SS TZH",
            "294276-12-31 23:30:00+00",
        ),
        (
            "4714-11-23 BC 23:30:00 -01",
            "YYYY-MM-DD BC HH24:MI:SS TZH",
            "4714-11-24 00:30:00+00 BC",
        ),
    ];

    for (input, template, expected) in cases {
        // Act
        let result = fixture.execute(&format!(
            "SELECT to_timestamp('{input}', '{template}') = TIMESTAMPTZ '{expected}'"
        ));

        // Assert
        assert_eq!(
            result
                .expect("offset restores valid timestamp range")
                .rows(),
            &[vec![Value::Bool(true)]]
        );
    }
}

#[test]
fn should_reject_excess_delimited_microseconds_before_timezone_hour() {
    // Arrange
    let fixture = sql_fixture("formatted-fraction-before-zone", &["SET TIME ZONE 'UTC'"]);

    // Act
    let result = fixture.execute(
        "SELECT to_timestamp('2000-01-01 12:00:00.12345611', 'YYYY-MM-DD HH24:MI:SS.USTZH')",
    );

    // Assert
    assert!(matches!(result, Err(CassieError::TypeInput(error)) if error.sqlstate == "22008"));
}

#[test]
fn should_preserve_valid_typed_midnight_endpoint_beside_formatted_input_rules() {
    // Arrange
    let fixture = sql_fixture("typed-midnight-endpoint", &[]);

    // Act
    let result = fixture.execute("SELECT make_time(24,0,0) = TIME '24:00:00'");

    // Assert
    assert_eq!(
        result.expect("typed midnight remains valid").rows(),
        &[vec![Value::Bool(true)]]
    );
}

#[test]
fn should_parse_final_formatted_microsecond_within_timestamp_range() {
    // Arrange
    let fixture = sql_fixture("formatted-final-microsecond", &["SET TIME ZONE 'UTC'"]);

    // Act
    let result = fixture.execute(
        "SELECT to_timestamp('2000-01-01 23:59:59.999999', 'YYYY-MM-DD HH24:MI:SS.US') = TIMESTAMPTZ '2000-01-01 23:59:59.999999+00'",
    );

    // Assert
    assert_eq!(
        result.expect("valid formatted microsecond").rows(),
        &[vec![Value::Bool(true)]]
    );
}

#[test]
fn should_scale_short_formatted_fractions_by_input_width() {
    // Arrange
    let fixture = sql_fixture("formatted-short-fraction", &["SET TIME ZONE 'UTC'"]);
    let cases = [("1", "100000"), ("12", "120000"), ("123", "123000")];

    for field in ["MS", "US", "FF3"] {
        for (input, expected) in cases {
            // Act
            let result = fixture.execute(&format!(
                "SELECT to_timestamp('2000-01-01 12:00:00.{input}', 'YYYY-MM-DD HH24:MI:SS.{field}') = TIMESTAMPTZ '2000-01-01 12:00:00.{expected}+00'"
            ));

            // Assert
            assert_eq!(
                result.expect("short formatted fraction").rows(),
                &[vec![Value::Bool(true)]]
            );
        }
    }
}

#[test]
fn should_reject_delimited_fraction_values_outside_microsecond_range() {
    // Arrange
    let fixture = sql_fixture("formatted-fraction-input-range", &[]);
    let cases = [
        ("MS", "1234"),
        ("US", "1234567"),
        ("MS", "-1"),
        ("US", "-1"),
        ("MS", "9223372036854775807"),
    ];

    for (field, input) in cases {
        // Act
        let result = fixture.execute(&format!(
            "SELECT to_timestamp('2000-01-01 12:00:00.{input}', 'YYYY-MM-DD HH24:MI:SS.{field}')"
        ));

        // Assert
        assert!(matches!(result, Err(CassieError::TypeInput(error)) if error.sqlstate == "22008"));
    }
}

#[test]
fn should_parse_complete_delimited_fraction_fields_with_leading_zeroes() {
    // Arrange
    let fixture = sql_fixture("formatted-leading-zero-fraction", &["SET TIME ZONE 'UTC'"]);
    let cases = [("MS", "001000"), ("US", "000001")];

    for (field, expected) in cases {
        // Act
        let result = fixture.execute(&format!(
            "SELECT to_timestamp('2000-01-01 12:00:00.0000000001', 'YYYY-MM-DD HH24:MI:SS.{field}') = TIMESTAMPTZ '2000-01-01 12:00:00.{expected}+00'"
        ));

        // Assert
        assert_eq!(
            result.expect("complete delimited fraction").rows(),
            &[vec![Value::Bool(true)]]
        );
    }
}
