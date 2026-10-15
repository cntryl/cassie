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

struct TemporalDirectory(String);

impl Drop for TemporalDirectory {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
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

include!("../../types/temporal/temporal.rs");
