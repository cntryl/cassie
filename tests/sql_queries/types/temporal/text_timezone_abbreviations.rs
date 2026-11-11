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
