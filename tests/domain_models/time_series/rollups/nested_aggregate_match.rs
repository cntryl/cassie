//! A rollup answers a query only when it holds every aggregate the query computes.

use super::support_sql_fixture::{sql_fixture, SqlFixture};
use cassie::types::Value;

fn rollup_fixture(label: &str) -> SqlFixture {
    let fixture = sql_fixture(label, &[]);
    fixture
        .cassie
        .startup()
        .expect("bootstrap canonical catalog");
    for sql in [
        "CREATE TABLE ev (tenant TEXT, event_at TIMESTAMP, amount BIGINT)",
        "INSERT INTO ev VALUES ('a', '2026-01-01T00:05:00Z', 5), ('a', '2026-01-01T00:15:00Z', 7)",
    ] {
        fixture.execute(sql).expect("rollup fixture setup");
    }
    fixture
}

fn create_count_rollup(fixture: &SqlFixture) {
    for sql in [
        "CREATE ROLLUP evr ON ev USING time_bucket('1 hour', event_at) GROUP BY tenant AGGREGATES COUNT(*) AS total",
        "REFRESH ROLLUP evr",
    ] {
        fixture.execute(sql).expect("create rollup");
    }
}

const NESTED_SUM: &str = "SELECT time_bucket('1 hour', event_at) AS bucket, tenant, COUNT(*) AS total, SUM(amount) + 1 AS shifted FROM ev GROUP BY time_bucket('1 hour', event_at), tenant";
const NESTED_COUNT: &str = "SELECT time_bucket('1 hour', event_at) AS bucket, tenant, COUNT(*) * 2 AS doubled FROM ev GROUP BY time_bucket('1 hour', event_at), tenant";
const ORDERED_BY_SUM: &str = "SELECT time_bucket('1 hour', event_at) AS bucket, tenant, COUNT(*) AS total FROM ev GROUP BY time_bucket('1 hour', event_at), tenant ORDER BY SUM(amount)";
const CASE_SUM: &str = "SELECT time_bucket('1 hour', event_at) AS bucket, tenant, COUNT(*) AS total, CASE WHEN SUM(amount) > 10 THEN 'big' ELSE 'small' END AS size FROM ev GROUP BY time_bucket('1 hour', event_at), tenant";

#[test]
fn should_return_nested_aggregate_row_from_table_without_rollup() {
    // Arrange
    let fixture = rollup_fixture("rollup-nested-baseline");

    // Act
    let rows = fixture.rows(NESTED_SUM);

    // Assert
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0][1], Value::String("a".into()));
    assert_eq!(rows[0][2], Value::Int64(2));
    assert_eq!(rows[0][3], Value::Int64(13));
}

#[test]
fn should_decline_rollup_when_aggregate_is_nested_in_projection_expression() {
    // Arrange
    let fixture = rollup_fixture("rollup-nested-sum");
    let expected = fixture.rows(NESTED_SUM);
    create_count_rollup(&fixture);

    // Act
    let rows = fixture.rows(NESTED_SUM);

    // Assert
    assert_eq!(rows, expected);
}

#[test]
fn should_return_table_rows_for_nested_aggregate_the_rollup_holds() {
    // Arrange
    let fixture = rollup_fixture("rollup-nested-count");
    let expected = fixture.rows(NESTED_COUNT);
    create_count_rollup(&fixture);

    // Act
    let rows = fixture.rows(NESTED_COUNT);

    // Assert
    assert_eq!(rows, expected);
    assert_eq!(rows[0][2], Value::Int64(4));
}

#[test]
fn should_decline_rollup_when_order_by_uses_aggregate_it_lacks() {
    // Arrange
    let fixture = rollup_fixture("rollup-order-by-sum");
    let expected = fixture.rows(ORDERED_BY_SUM);
    create_count_rollup(&fixture);

    // Act
    let rows = fixture.rows(ORDERED_BY_SUM);

    // Assert
    assert_eq!(rows, expected);
}

#[test]
fn should_decline_rollup_when_case_expression_nests_aggregate_it_lacks() {
    // Arrange
    let fixture = rollup_fixture("rollup-case-sum");
    let expected = fixture.rows(CASE_SUM);
    create_count_rollup(&fixture);

    // Act
    let rows = fixture.rows(CASE_SUM);

    // Assert
    assert_eq!(rows, expected);
    assert_eq!(rows[0][3], Value::String("big".into()));
}
