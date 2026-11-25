//! PostgreSQL 18 temporal function syntax tracked by #815.

use cassie::types::Value;

use crate::support_sql_fixture::sql_fixture;

#[test]
fn should_evaluate_overlaps_period_operator() {
    // Arrange
    let fixture = sql_fixture("issue-815-overlaps", &[]);

    // Act
    let result = fixture.execute(
        "SELECT (TIMESTAMP '2001-02-16 00:00:00', INTERVAL '1 day') \
         OVERLAPS (TIMESTAMP '2001-02-16 12:00:00', INTERVAL '1 day')",
    );

    // Assert
    let result = result.expect("OVERLAPS period expression executes");
    assert_eq!(result.rows(), &[vec![Value::Bool(true)]]);
    assert_eq!(result.columns()[0].type_oid, 16);
}
