//! A filtered rollup must keep and answer exactly the rows its WHERE predicate selects.

use super::support_sql_fixture::{sql_fixture, SqlFixture};
use cassie::types::Value;

const SETUP: [&str; 2] = [
    "CREATE TABLE rsrc (tenant TEXT, ts TIMESTAMP, n INT, m INT, s TEXT)",
    "INSERT INTO rsrc VALUES \
     ('a', '2024-01-01 00:05:00', 0, 3, 'alpha'), \
     ('a', '2024-01-01 00:10:00', 5, 5, 'beta'), \
     ('a', '2024-01-02 00:10:00', 7, 2, 'alpine'), \
     ('b', '2024-01-01 01:00:00', 2, 9, NULL), \
     ('b', '2024-01-01 02:00:00', NULL, 1, 'gamma'), \
     ('b', '2024-01-02 03:00:00', 10, 10, 'alpha')",
];

const PREDICATES: [&str; 17] = [
    "n > 1",
    "n >= 5",
    "n < 5",
    "n <= 5",
    "n = 5",
    "n <> 5",
    "n > 1 AND m < 9",
    "n > 6 OR m = 1",
    "NOT n > 1",
    "n IN (0, 5, 10)",
    "n BETWEEN 2 AND 7",
    "s IS NULL",
    "s IS NOT NULL",
    "s LIKE 'alp%'",
    "s = 'alpha'",
    "n = m",
    "n > m",
];

fn fixture(label: &str) -> SqlFixture {
    sql_fixture(label, &SETUP)
}

fn query(predicate: &str) -> String {
    format!(
        "SELECT time_bucket('1 day', ts) AS bucket, tenant, COUNT(*) AS total FROM rsrc WHERE {predicate} \
         GROUP BY time_bucket('1 day', ts), tenant ORDER BY bucket, tenant"
    )
}

fn create_rollup(fixture: &SqlFixture, predicate: &str) {
    fixture
        .execute(&format!(
            "CREATE ROLLUP rr ON rsrc USING time_bucket('1 day', ts) GROUP BY tenant \
             AGGREGATES COUNT(*) AS total WHERE {predicate}"
        ))
        .expect("create rollup");
    fixture.execute("REFRESH ROLLUP rr").expect("refresh");
}

#[test]
fn should_answer_comparison_filtered_rollup_with_the_matching_rows() {
    // Arrange
    let fixture = fixture("rollup-filter-comparison");
    let expected = fixture.rows(&query("n > 1"));
    assert_eq!(expected.len(), 4);

    // Act
    create_rollup(&fixture, "n > 1");
    let rows = fixture.rows(&query("n > 1"));

    // Assert
    assert_eq!(rows, expected);
}

#[test]
fn should_match_table_for_every_filter_shape_after_refresh() {
    // Arrange
    let mut mismatches = Vec::new();

    // Act
    for predicate in PREDICATES {
        let fixture = fixture("rollup-filter-shapes");
        let expected = fixture.rows(&query(predicate));
        create_rollup(&fixture, predicate);
        let rows = fixture.rows(&query(predicate));
        if rows != expected {
            mismatches.push(predicate);
        }
    }

    // Assert
    assert!(
        mismatches.is_empty(),
        "mismatching predicates: {mismatches:?}"
    );
}

#[test]
fn should_match_table_for_every_filter_shape_after_refreshing_dml_changes() {
    // Arrange
    let mut mismatches = Vec::new();

    // Act
    for predicate in PREDICATES {
        let fixture = fixture("rollup-filter-dml");
        create_rollup(&fixture, predicate);
        for dml in [
            "INSERT INTO rsrc VALUES ('c', '2024-01-03 00:00:00', 6, 6, 'alpha')",
            "UPDATE rsrc SET n = 100, m = 0 WHERE n = 5",
            "UPDATE rsrc SET n = 5, m = 5, s = 'alpha' WHERE n = 0",
            "DELETE FROM rsrc WHERE tenant = 'b' AND n = 10",
        ] {
            fixture.execute(dml).expect("dml");
            let expected = fixture.rows(&query(predicate));
            fixture.execute("REFRESH ROLLUP rr").expect("refresh");
            if fixture.rows(&query(predicate)) != expected {
                mismatches.push(format!("{predicate} after {dml}"));
            }
        }
    }

    // Assert
    assert!(mismatches.is_empty(), "mismatches: {mismatches:?}");
}

#[test]
fn should_not_answer_a_differently_spelled_predicate_with_wrong_rows() {
    // Arrange
    let fixture = fixture("rollup-filter-respelled");
    let expected = fixture.rows(&query("1 < n"));
    create_rollup(&fixture, "n > 1");

    // Act
    let rows = fixture.rows(&query("1 < n"));

    // Assert
    assert_eq!(rows, expected);
}

#[test]
fn should_hold_the_documented_repro_row() {
    // Arrange
    let fixture = fixture("rollup-filter-repro");
    create_rollup(&fixture, "n > 1");

    // Act
    let rows = fixture.rows(&query("n > 1"));

    // Assert
    assert_eq!(rows.len(), 4);
    assert_eq!(rows[0][1], Value::String("a".into()));
}
