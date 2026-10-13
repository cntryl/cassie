//! PostgreSQL 18 SQL-defined aggregate behavior tracked by #910.

use cassie::types::Value;

use crate::support_sql_fixture::sql_fixture;

#[test]
fn should_execute_sql_defined_aggregate_over_rows_in_a_table_expression() {
    // Arrange
    let fixture = sql_fixture(
        "issue-910-sql-aggregate",
        &[
            "CREATE FUNCTION issue_910_sum_state(state BIGINT, value INTEGER) RETURNS BIGINT LANGUAGE SQL IMMUTABLE AS $$ SELECT state + value $$",
            "CREATE AGGREGATE issue_910_sum(INTEGER) (SFUNC = issue_910_sum_state, STYPE = BIGINT, INITCOND = '0')",
        ],
    );
    let sql = "SELECT issue_910_sum(value) FROM (VALUES (1), (2), (3)) AS values_to_sum(value)";

    // Act
    let result = fixture.execute(sql);

    // Assert
    assert_eq!(
        result.expect("SQL-defined aggregate").rows(),
        &[vec![Value::Int64(6)]]
    );
}
