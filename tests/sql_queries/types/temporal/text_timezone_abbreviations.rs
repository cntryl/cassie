use cassie::app::CassieError;
use cassie::types::Value;

use crate::support_sql_fixture::{sql_fixture, SqlFixture};

fn assert_true(fixture: &SqlFixture, sql: &str) {
    let result = fixture.execute(sql);
    assert_eq!(
        result.expect("temporal abbreviation expression").rows(),
        &[vec![Value::Bool(true)]],
        "expected PostgreSQL temporal input invariant: {sql}"
    );
}

#[test]
fn should_resolve_default_msk_at_the_complete_timestamp_date() {
    // Arrange
    let fixture = sql_fixture("issue-815-default-msk-date", &["SET TIME ZONE 'UTC'"]);

    // Act
    let sql = "SELECT TIMESTAMPTZ '2012-01-01 MSK 00:00:00' = TIMESTAMPTZ '2011-12-31 20:00:00+00' AND TIMESTAMPTZ '2012-01-01 00:00:00 MSK' = TIMESTAMPTZ '2011-12-31 20:00:00+00' AND TIMESTAMPTZ '2015-01-01 MSK 12:00:00' = TIMESTAMPTZ '2015-01-01 09:00:00+00'";

    // Assert
    assert_true(&fixture, sql);
}

#[test]
fn should_resolve_default_msk_at_the_moscow_gap() {
    // Arrange
    let fixture = sql_fixture("issue-815-default-msk-gap", &["SET TIME ZONE 'UTC'"]);

    // Act
    let sql = "SELECT TIMESTAMPTZ '2011-03-27 02:30:00 MSK' = TIMESTAMPTZ '2011-03-26 22:30:00+00' AND TIMESTAMPTZ '2011-03-27 MSK 02:30:00' = TIMESTAMPTZ '2011-03-26 22:30:00+00'";

    // Assert
    assert_true(&fixture, sql);
}

#[test]
fn should_resolve_default_msk_at_the_moscow_fold() {
    // Arrange
    let fixture = sql_fixture("issue-815-default-msk-fold", &["SET TIME ZONE 'UTC'"]);

    // Act
    let sql = "SELECT TIMESTAMPTZ '2014-10-26 01:30:00 MSK' = TIMESTAMPTZ '2014-10-25 22:30:00+00' AND TIMESTAMPTZ '2014-10-26 MSK 01:30:00' = TIMESTAMPTZ '2014-10-25 22:30:00+00'";

    // Assert
    assert_true(&fixture, sql);
}

#[test]
fn should_preserve_default_msk_input_diagnostic_order() {
    // Arrange
    let fixture = sql_fixture(
        "issue-815-default-msk-diagnostics",
        &["SET TIME ZONE 'UTC'"],
    );

    // Act
    let prefix = fixture.execute("SELECT TIMESTAMPTZ 'MSK 2012-01-01 00:00:00'");
    let invalid_date = fixture.execute("SELECT TIMESTAMPTZ '2012-02-30 MSK 00:00:00'");
    let unknown = fixture.execute("SELECT TIMESTAMPTZ '2012-01-01 NOT_A_ZONE 00:00:00'");

    // Assert
    assert!(matches!(
        prefix,
        Err(CassieError::TypeInput(error)) if error.sqlstate == "22007"
    ));
    assert!(matches!(
        invalid_date,
        Err(CassieError::TypeInput(error)) if error.sqlstate == "22008"
    ));
    assert!(matches!(
        unknown,
        Err(CassieError::TypeInput(error)) if error.sqlstate == "22007"
    ));
}

#[test]
fn should_resolve_default_clt_for_each_temporal_input_at_the_input_date() {
    // Arrange
    let fixture = sql_fixture(
        "default-clt-dynamic-at-input-date",
        &["SET TIME ZONE 'UTC'"],
    );

    // Act
    let result = fixture.execute(
        "SELECT DATE '2024-01-15 CLT' = DATE '2024-01-15', TIMESTAMP '2024-01-15 12:00:00 CLT' = TIMESTAMP '2024-01-15 12:00:00', (TIMESTAMPTZ '2024-01-15 12:00:00 CLT' AT TIME ZONE 'UTC') = TIMESTAMP '2024-01-15 15:00:00', (TIMESTAMPTZ '2024-07-15 12:00:00 CLT' AT TIME ZONE 'UTC') = TIMESTAMP '2024-07-15 16:00:00', (TIMESTAMPTZ '2024-01-15 12:00:00 cLt' AT TIME ZONE 'UTC') = TIMESTAMP '2024-01-15 15:00:00'",
    );

    // Assert
    assert_eq!(
        result
            .expect("Default CLT uses each input's final date")
            .rows(),
        &[vec![
            Value::Bool(true),
            Value::Bool(true),
            Value::Bool(true),
            Value::Bool(true),
            Value::Bool(true),
        ]]
    );
}

#[test]
fn should_prefer_a_session_zone_abbreviation_to_a_catalogued_dynamic_alias() {
    // Arrange
    let fixture = sql_fixture("session-zone-clt-precedence", &["SET TIME ZONE 'CLT0'"]);

    // Act
    let result = fixture.execute(
        "SELECT (TIMESTAMPTZ '2024-01-15 12:00:00 CLT' AT TIME ZONE 'UTC') = TIMESTAMP '2024-01-15 12:00:00'",
    );

    // Assert
    assert_eq!(
        result
            .expect("session-zone CLT takes precedence over the Default catalog")
            .rows(),
        &[vec![Value::Bool(true)]]
    );
}

#[test]
fn should_reject_default_clt_before_a_complete_date() {
    // Arrange
    let fixture = sql_fixture("default-clt-requires-complete-date", &[]);

    // Act
    let result = fixture.execute("SELECT TIMESTAMPTZ 'CLT 2024-01-15 12:00:00'");

    // Assert
    assert!(
        matches!(result, Err(cassie::app::CassieError::TypeInput(error)) if error.sqlstate == "22007"),
        "PostgreSQL rejects a dynamic abbreviation before the complete date"
    );
}

#[test]
fn should_validate_the_date_before_resolving_default_clt() {
    // Arrange
    let fixture = sql_fixture("default-clt-invalid-date-priority", &[]);

    // Act
    let result = fixture.execute("SELECT TIMESTAMPTZ '2024-02-30 CLT 12:00:00'");

    // Assert
    assert!(
        matches!(result, Err(cassie::app::CassieError::TypeInput(error)) if error.sqlstate == "22008"),
        "invalid date diagnostics precede dynamic zone resolution"
    );
}

#[test]
fn should_reject_a_catalogued_dynamic_abbreviation_prefix() {
    // Arrange
    let fixture = sql_fixture("default-clt-prefix-is-not-an-alias", &[]);

    // Act
    let result = fixture.execute("SELECT TIMESTAMPTZ '2024-01-15 12:00:00 CLTx'");

    // Assert
    assert!(
        matches!(result, Err(CassieError::TypeInput(error)) if error.sqlstate == "22007"),
        "catalogued dynamic aliases require an exact token match"
    );
}
