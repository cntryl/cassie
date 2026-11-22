//! CHECK literal bounds are typed to their column when the constraint is
//! created, so comparison runs on typed values (not on text or JSON shape) and
//! a bound that cannot belong to the column is rejected at DDL time.

use super::support_sql_fixture::sql_fixture;

/// Runs `setup`, then `statement`, and returns `"ok"` or the SQLSTATE.
fn outcome(setup: &[&str], statement: &str) -> String {
    let fixture = sql_fixture("check-literal-types", setup);
    match fixture.execute(statement) {
        Ok(_) => "ok".to_string(),
        Err(error) => error.sql_state().to_string(),
    }
}

#[test]
fn should_accept_timestamp_below_bound_written_with_a_time() {
    // Arrange
    let setup = ["CREATE TABLE tb (ts TIMESTAMP CHECK (ts < '2024-01-01 12:00:00'))"];
    let statement = "INSERT INTO tb (ts) VALUES ('2024-01-01 06:00:00')";

    // Act
    let result = outcome(&setup, statement);

    // Assert
    assert_eq!(result, "ok");
}

#[test]
fn should_reject_timestamp_at_midnight_for_greater_than_date_bound() {
    // Arrange
    let setup = ["CREATE TABLE tb (ts TIMESTAMP CHECK (ts > '2024-01-01'))"];
    let statement = "INSERT INTO tb (ts) VALUES ('2024-01-01 00:00:00')";

    // Act
    let result = outcome(&setup, statement);

    // Assert
    assert_eq!(result, "23514");
}

#[test]
fn should_accept_timestamp_equal_to_bound_written_with_a_time() {
    // Arrange
    let setup = ["CREATE TABLE tb (ts TIMESTAMP CHECK (ts = '2024-01-01 00:00:00'))"];
    let statement = "INSERT INTO tb (ts) VALUES ('2024-01-01 00:00:00')";

    // Act
    let result = outcome(&setup, statement);

    // Assert
    assert_eq!(result, "ok");
}

#[test]
fn should_accept_timestamp_at_less_or_equal_bound() {
    // Arrange
    let setup = ["CREATE TABLE tb (ts TIMESTAMP CHECK (ts <= '2024-01-01 12:00:00'))"];
    let statement = "INSERT INTO tb (ts) VALUES ('2024-01-01 12:00:00')";

    // Act
    let result = outcome(&setup, statement);

    // Assert
    assert_eq!(result, "ok");
}

#[test]
fn should_accept_timestamp_at_greater_or_equal_bound() {
    // Arrange
    let setup = ["CREATE TABLE tb (ts TIMESTAMP CHECK (ts >= '2024-01-01'))"];
    let statement = "INSERT INTO tb (ts) VALUES ('2024-01-01 00:00:00')";

    // Act
    let result = outcome(&setup, statement);

    // Assert
    assert_eq!(result, "ok");
}

#[test]
fn should_reject_timestamp_equal_to_not_equal_bound() {
    // Arrange
    let setup = ["CREATE TABLE tb (ts TIMESTAMP CHECK (ts <> '2024-01-01'))"];
    let statement = "INSERT INTO tb (ts) VALUES ('2024-01-01 00:00:00')";

    // Act
    let result = outcome(&setup, statement);

    // Assert
    assert_eq!(result, "23514");
}

#[test]
fn should_accept_timestamp_with_fraction_below_bound() {
    // Arrange
    let setup = ["CREATE TABLE tb (ts TIMESTAMP CHECK (ts < '2024-01-01 12:00:00.5'))"];
    let statement = "INSERT INTO tb (ts) VALUES ('2024-01-01 12:00:00.25')";

    // Act
    let result = outcome(&setup, statement);

    // Assert
    assert_eq!(result, "ok");
}

#[test]
fn should_reject_timestamp_above_bound_with_shorter_text() {
    // Arrange
    let setup = ["CREATE TABLE tb (ts TIMESTAMP CHECK (ts < '2024-01-01 12:00'))"];
    let statement = "INSERT INTO tb (ts) VALUES ('2024-01-01 12:00:30')";

    // Act
    let result = outcome(&setup, statement);

    // Assert
    assert_eq!(result, "23514");
}

#[test]
fn should_reject_timestamp_bound_that_is_not_a_date() {
    // Arrange
    let setup = [];
    let statement = "CREATE TABLE tb (ts TIMESTAMP CHECK (ts < 'not a date'))";

    // Act
    let result = outcome(&setup, statement);

    // Assert
    assert_eq!(result, "22007");
}

#[test]
fn should_reject_existing_timestamp_row_for_added_constraint() {
    // Arrange
    let setup = [
        "CREATE TABLE tb (ts TIMESTAMP)",
        "INSERT INTO tb (ts) VALUES ('2024-01-01 00:00:00')",
    ];
    let statement = "ALTER TABLE tb ADD CONSTRAINT c CHECK (ts > '2024-01-01')";

    // Act
    let result = outcome(&setup, statement);

    // Assert
    assert_eq!(result, "23514");
}

#[test]
fn should_accept_existing_timestamp_row_for_added_constraint() {
    // Arrange
    let setup = [
        "CREATE TABLE tb (ts TIMESTAMP)",
        "INSERT INTO tb (ts) VALUES ('2024-01-01 06:00:00')",
    ];
    let statement = "ALTER TABLE tb ADD CONSTRAINT c CHECK (ts < '2024-01-01 12:00:00')";

    // Act
    let result = outcome(&setup, statement);

    // Assert
    assert_eq!(result, "ok");
}

#[test]
fn should_reject_added_timestamp_constraint_with_invalid_bound() {
    // Arrange
    let setup = ["CREATE TABLE tb (ts TIMESTAMP)"];
    let statement = "ALTER TABLE tb ADD CONSTRAINT c CHECK (ts < 'not a date')";

    // Act
    let result = outcome(&setup, statement);

    // Assert
    assert_eq!(result, "22007");
}

#[test]
fn should_accept_timestamptz_below_offsetless_bound_in_session_zone() {
    // Arrange
    let setup = [
        "SET TIME ZONE 'America/New_York'",
        "CREATE TABLE tb (ts TIMESTAMPTZ CHECK (ts < '2024-01-01 12:00:00'))",
    ];
    let statement = "INSERT INTO tb (ts) VALUES ('2024-01-01 16:00:00+00')";

    // Act
    let result = outcome(&setup, statement);

    // Assert
    assert_eq!(result, "ok");
}

#[test]
fn should_reject_timestamptz_above_offsetless_bound_in_session_zone() {
    // Arrange
    let setup = [
        "SET TIME ZONE 'America/New_York'",
        "CREATE TABLE tb (ts TIMESTAMPTZ CHECK (ts < '2024-01-01 12:00:00'))",
    ];
    let statement = "INSERT INTO tb (ts) VALUES ('2024-01-01 18:00:00+00')";

    // Act
    let result = outcome(&setup, statement);

    // Assert
    assert_eq!(result, "23514");
}

#[test]
fn should_accept_timestamptz_below_bound_with_offset() {
    // Arrange
    let setup = [
        "SET TIME ZONE 'America/New_York'",
        "CREATE TABLE tb (ts TIMESTAMPTZ CHECK (ts < '2024-01-01 12:00:00+00'))",
    ];
    let statement = "INSERT INTO tb (ts) VALUES ('2024-01-01 11:00:00+00')";

    // Act
    let result = outcome(&setup, statement);

    // Assert
    assert_eq!(result, "ok");
}

#[test]
fn should_reject_timestamptz_above_bound_with_offset() {
    // Arrange
    let setup = [
        "SET TIME ZONE 'America/New_York'",
        "CREATE TABLE tb (ts TIMESTAMPTZ CHECK (ts < '2024-01-01 12:00:00+00'))",
    ];
    let statement = "INSERT INTO tb (ts) VALUES ('2024-01-01 12:30:00+00')";

    // Act
    let result = outcome(&setup, statement);

    // Assert
    assert_eq!(result, "23514");
}

#[test]
fn should_resolve_offsetless_timestamptz_bound_at_constraint_creation() {
    // Arrange
    let setup = [
        "SET TIME ZONE 'America/New_York'",
        "CREATE TABLE tb (ts TIMESTAMPTZ CHECK (ts < '2024-01-01 12:00:00'))",
        "SET TIME ZONE 'UTC'",
    ];
    let statement = "INSERT INTO tb (ts) VALUES ('2024-01-01 17:30:00')";

    // Act
    let result = outcome(&setup, statement);

    // Assert
    assert_eq!(result, "23514");
}

#[test]
fn should_accept_timestamptz_equal_to_bound_in_other_zone() {
    // Arrange
    let setup = ["CREATE TABLE tb (ts TIMESTAMPTZ CHECK (ts = '2024-01-01 12:00:00+02'))"];
    let statement = "INSERT INTO tb (ts) VALUES ('2024-01-01 10:00:00+00')";

    // Act
    let result = outcome(&setup, statement);

    // Assert
    assert_eq!(result, "ok");
}

#[test]
fn should_reject_timestamptz_bound_that_is_not_a_date() {
    // Arrange
    let setup = [];
    let statement = "CREATE TABLE tb (ts TIMESTAMPTZ CHECK (ts < 'not a date'))";

    // Act
    let result = outcome(&setup, statement);

    // Assert
    assert_eq!(result, "22007");
}

#[test]
fn should_reject_existing_timestamptz_row_for_added_constraint() {
    // Arrange
    let setup = [
        "CREATE TABLE tb (ts TIMESTAMPTZ)",
        "INSERT INTO tb (ts) VALUES ('2024-01-01 18:00:00+00')",
        "SET TIME ZONE 'America/New_York'",
    ];
    let statement = "ALTER TABLE tb ADD CONSTRAINT c CHECK (ts < '2024-01-01 12:00:00')";

    // Act
    let result = outcome(&setup, statement);

    // Assert
    assert_eq!(result, "23514");
}

#[test]
fn should_accept_date_after_bound_written_without_padding() {
    // Arrange
    let setup = ["CREATE TABLE tb (d DATE CHECK (d > '2024-1-1'))"];
    let statement = "INSERT INTO tb (d) VALUES ('2024-01-02')";

    // Act
    let result = outcome(&setup, statement);

    // Assert
    assert_eq!(result, "ok");
}

#[test]
fn should_reject_date_equal_to_greater_than_bound() {
    // Arrange
    let setup = ["CREATE TABLE tb (d DATE CHECK (d > '2024-01-01'))"];
    let statement = "INSERT INTO tb (d) VALUES ('2024-01-01')";

    // Act
    let result = outcome(&setup, statement);

    // Assert
    assert_eq!(result, "23514");
}

#[test]
fn should_accept_date_equal_to_bound_in_other_notation() {
    // Arrange
    let setup = ["CREATE TABLE tb (d DATE CHECK (d = '2024-01-01'))"];
    let statement = "INSERT INTO tb (d) VALUES ('2024-1-1')";

    // Act
    let result = outcome(&setup, statement);

    // Assert
    assert_eq!(result, "ok");
}

#[test]
fn should_reject_date_bound_out_of_range() {
    // Arrange
    let setup = [];
    let statement = "CREATE TABLE tb (d DATE CHECK (d < '2024-13-45'))";

    // Act
    let result = outcome(&setup, statement);

    // Assert
    assert_eq!(result, "22008");
}

#[test]
fn should_reject_existing_date_row_for_added_constraint() {
    // Arrange
    let setup = [
        "CREATE TABLE tb (d DATE)",
        "INSERT INTO tb (d) VALUES ('2024-01-01')",
    ];
    let statement = "ALTER TABLE tb ADD CONSTRAINT c CHECK (d > '2024-01-01')";

    // Act
    let result = outcome(&setup, statement);

    // Assert
    assert_eq!(result, "23514");
}

#[test]
fn should_accept_time_below_bound_written_without_seconds() {
    // Arrange
    let setup = ["CREATE TABLE tb (t TIME CHECK (t < '12:00'))"];
    let statement = "INSERT INTO tb (t) VALUES ('11:59:59')";

    // Act
    let result = outcome(&setup, statement);

    // Assert
    assert_eq!(result, "ok");
}

#[test]
fn should_reject_time_equal_to_less_than_bound() {
    // Arrange
    let setup = ["CREATE TABLE tb (t TIME CHECK (t < '12:00'))"];
    let statement = "INSERT INTO tb (t) VALUES ('12:00:00')";

    // Act
    let result = outcome(&setup, statement);

    // Assert
    assert_eq!(result, "23514");
}

#[test]
fn should_accept_time_equal_to_bound() {
    // Arrange
    let setup = ["CREATE TABLE tb (t TIME CHECK (t = '12:00'))"];
    let statement = "INSERT INTO tb (t) VALUES ('12:00:00')";

    // Act
    let result = outcome(&setup, statement);

    // Assert
    assert_eq!(result, "ok");
}

#[test]
fn should_reject_time_bound_that_is_not_a_time() {
    // Arrange
    let setup = [];
    let statement = "CREATE TABLE tb (t TIME CHECK (t < 'noon'))";

    // Act
    let result = outcome(&setup, statement);

    // Assert
    assert_eq!(result, "22007");
}

#[test]
fn should_accept_interval_below_bound() {
    // Arrange
    let setup = ["CREATE TABLE tb (i INTERVAL CHECK (i < '1 day'))"];
    let statement = "INSERT INTO tb (i) VALUES ('2 hours')";

    // Act
    let result = outcome(&setup, statement);

    // Assert
    assert_eq!(result, "ok");
}

#[test]
fn should_reject_interval_above_bound() {
    // Arrange
    let setup = ["CREATE TABLE tb (i INTERVAL CHECK (i < '1 day'))"];
    let statement = "INSERT INTO tb (i) VALUES ('2 days')";

    // Act
    let result = outcome(&setup, statement);

    // Assert
    assert_eq!(result, "23514");
}

#[test]
fn should_accept_interval_equal_to_bound_in_other_units() {
    // Arrange
    let setup = ["CREATE TABLE tb (i INTERVAL CHECK (i = '1 day'))"];
    let statement = "INSERT INTO tb (i) VALUES ('24 hours')";

    // Act
    let result = outcome(&setup, statement);

    // Assert
    assert_eq!(result, "ok");
}

#[test]
fn should_reject_interval_bound_that_is_not_an_interval() {
    // Arrange
    let setup = [];
    let statement = "CREATE TABLE tb (i INTERVAL CHECK (i < 'soon'))";

    // Act
    let result = outcome(&setup, statement);

    // Assert
    assert_eq!(result, "22007");
}

#[test]
fn should_accept_uuid_equal_to_upper_case_bound() {
    // Arrange
    let setup = ["CREATE TABLE tb (u UUID CHECK (u = 'A0EEBC99-9C0B-4EF8-BB6D-6BB9BD380A11'))"];
    let statement = "INSERT INTO tb (u) VALUES ('a0eebc99-9c0b-4ef8-bb6d-6bb9bd380a11')";

    // Act
    let result = outcome(&setup, statement);

    // Assert
    assert_eq!(result, "ok");
}

#[test]
fn should_reject_uuid_equal_to_not_equal_bound_in_other_case() {
    // Arrange
    let setup = ["CREATE TABLE tb (u UUID CHECK (u <> 'A0EEBC99-9C0B-4EF8-BB6D-6BB9BD380A11'))"];
    let statement = "INSERT INTO tb (u) VALUES ('a0eebc99-9c0b-4ef8-bb6d-6bb9bd380a11')";

    // Act
    let result = outcome(&setup, statement);

    // Assert
    assert_eq!(result, "23514");
}

#[test]
fn should_reject_uuid_different_from_bound() {
    // Arrange
    let setup = ["CREATE TABLE tb (u UUID CHECK (u = 'A0EEBC99-9C0B-4EF8-BB6D-6BB9BD380A11'))"];
    let statement = "INSERT INTO tb (u) VALUES ('b0eebc99-9c0b-4ef8-bb6d-6bb9bd380a11')";

    // Act
    let result = outcome(&setup, statement);

    // Assert
    assert_eq!(result, "23514");
}

#[test]
fn should_reject_uuid_bound_that_is_not_a_uuid() {
    // Arrange
    let setup = [];
    let statement = "CREATE TABLE tb (u UUID CHECK (u = 'xyz'))";

    // Act
    let result = outcome(&setup, statement);

    // Assert
    assert_eq!(result, "22P02");
}

#[test]
fn should_accept_boolean_equal_to_yes_bound() {
    // Arrange
    let setup = ["CREATE TABLE tb (b BOOLEAN CHECK (b = 'yes'))"];
    let statement = "INSERT INTO tb (b) VALUES (true)";

    // Act
    let result = outcome(&setup, statement);

    // Assert
    assert_eq!(result, "ok");
}

#[test]
fn should_reject_boolean_different_from_yes_bound() {
    // Arrange
    let setup = ["CREATE TABLE tb (b BOOLEAN CHECK (b = 'yes'))"];
    let statement = "INSERT INTO tb (b) VALUES (false)";

    // Act
    let result = outcome(&setup, statement);

    // Assert
    assert_eq!(result, "23514");
}

#[test]
fn should_accept_boolean_equal_to_t_bound() {
    // Arrange
    let setup = ["CREATE TABLE tb (b BOOLEAN CHECK (b = 't'))"];
    let statement = "INSERT INTO tb (b) VALUES (true)";

    // Act
    let result = outcome(&setup, statement);

    // Assert
    assert_eq!(result, "ok");
}

#[test]
fn should_reject_boolean_bound_that_is_not_a_boolean() {
    // Arrange
    let setup = [];
    let statement = "CREATE TABLE tb (b BOOLEAN CHECK (b = 'maybe'))";

    // Act
    let result = outcome(&setup, statement);

    // Assert
    assert_eq!(result, "22P02");
}

#[test]
fn should_reject_boolean_compared_with_integer() {
    // Arrange
    let setup = [];
    let statement = "CREATE TABLE tb (b BOOLEAN CHECK (b = 1))";

    // Act
    let result = outcome(&setup, statement);

    // Assert
    assert_eq!(result, "42883");
}

#[test]
fn should_accept_numeric_equal_to_text_bound_with_scale() {
    // Arrange
    let setup = ["CREATE TABLE tb (n NUMERIC CHECK (n = '1.0'))"];
    let statement = "INSERT INTO tb (n) VALUES (1)";

    // Act
    let result = outcome(&setup, statement);

    // Assert
    assert_eq!(result, "ok");
}

#[test]
fn should_reject_numeric_different_from_text_bound() {
    // Arrange
    let setup = ["CREATE TABLE tb (n NUMERIC CHECK (n = '1.0'))"];
    let statement = "INSERT INTO tb (n) VALUES (2)";

    // Act
    let result = outcome(&setup, statement);

    // Assert
    assert_eq!(result, "23514");
}

#[test]
fn should_accept_numeric_below_text_bound() {
    // Arrange
    let setup = ["CREATE TABLE tb (n NUMERIC CHECK (n < '10.50'))"];
    let statement = "INSERT INTO tb (n) VALUES (10.4)";

    // Act
    let result = outcome(&setup, statement);

    // Assert
    assert_eq!(result, "ok");
}

#[test]
fn should_reject_numeric_bound_that_is_not_a_number() {
    // Arrange
    let setup = [];
    let statement = "CREATE TABLE tb (n NUMERIC CHECK (n < 'abc'))";

    // Act
    let result = outcome(&setup, statement);

    // Assert
    assert_eq!(result, "22P02");
}

#[test]
fn should_accept_integer_equal_to_text_bound() {
    // Arrange
    let setup = ["CREATE TABLE tb (i INT CHECK (i = '1'))"];
    let statement = "INSERT INTO tb (i) VALUES (1)";

    // Act
    let result = outcome(&setup, statement);

    // Assert
    assert_eq!(result, "ok");
}

#[test]
fn should_reject_integer_different_from_text_bound() {
    // Arrange
    let setup = ["CREATE TABLE tb (i INT CHECK (i = '1'))"];
    let statement = "INSERT INTO tb (i) VALUES (2)";

    // Act
    let result = outcome(&setup, statement);

    // Assert
    assert_eq!(result, "23514");
}

#[test]
fn should_reject_integer_bound_text_that_is_not_an_integer() {
    // Arrange
    let setup = [];
    let statement = "CREATE TABLE tb (i INT CHECK (i = 'abc'))";

    // Act
    let result = outcome(&setup, statement);

    // Assert
    assert_eq!(result, "22P02");
}

#[test]
fn should_accept_integer_below_numeric_bound() {
    // Arrange
    let setup = ["CREATE TABLE tb (i INT CHECK (i < 1.5))"];
    let statement = "INSERT INTO tb (i) VALUES (1)";

    // Act
    let result = outcome(&setup, statement);

    // Assert
    assert_eq!(result, "ok");
}

#[test]
fn should_reject_integer_above_numeric_bound() {
    // Arrange
    let setup = ["CREATE TABLE tb (i INT CHECK (i < 1.5))"];
    let statement = "INSERT INTO tb (i) VALUES (2)";

    // Act
    let result = outcome(&setup, statement);

    // Assert
    assert_eq!(result, "23514");
}
