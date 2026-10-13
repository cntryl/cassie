//! PostgreSQL 18 SQL-defined operator behavior tracked by #912.

use cassie::types::Value;

use crate::support_sql_fixture::sql_fixture;

#[test]
fn should_execute_sql_defined_operator_with_its_declared_function() {
    // Arrange
    let fixture = sql_fixture(
        "issue-912-sql-defined-operator",
        &[
            "CREATE OPERATOR ## (LEFTARG = INTEGER, RIGHTARG = INTEGER, FUNCTION = pg_catalog.int4pl)",
        ],
    );

    // Act
    let result = fixture.execute("SELECT 3 ## 4");

    // Assert
    assert_eq!(
        result.expect("SQL-defined operator").rows(),
        &[vec![Value::Int64(7)]]
    );
}
