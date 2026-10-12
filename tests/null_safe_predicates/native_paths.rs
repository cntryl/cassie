use crate::support_sql_fixture::sql_fixture;
use cassie::types::Value;

const SOURCE: &[&str] = &[
    "CREATE TABLE pruned (id BIGINT, n BIGINT, f FLOAT, payload TEXT)",
    "INSERT INTO pruned VALUES (1,NULL,NULL,'a'),(2,7,7.0,'b'),(3,NULL,2.5,'c'),(4,0,-0.0,'d'),(5,3,NULL,'e')",
];

fn ids(rows: &[Vec<Value>]) -> Vec<i64> {
    rows.iter()
        .map(|row| match row.first() {
            Some(Value::Int64(id)) => *id,
            _ => panic!("first column must be a BIGINT id"),
        })
        .collect()
}

fn pruned(fixture: &crate::support_sql_fixture::SqlFixture, sql: &str) -> bool {
    let plan = fixture
        .execute(&format!("EXPLAIN {sql}"))
        .expect("explain filter");
    crate::support_sql::explain_plan_text(&plan).contains("projection_pruning=true")
}

#[test]
fn should_prune_projected_scans_for_null_safe_filters() {
    // Arrange
    let fixture = sql_fixture("null_safe_projection_pruning", SOURCE);
    let cases = [
        (
            "SELECT id FROM pruned WHERE n IS NOT DISTINCT FROM NULL ORDER BY id",
            vec![1, 3],
        ),
        (
            "SELECT id FROM pruned WHERE n IS DISTINCT FROM 7 ORDER BY id",
            vec![1, 3, 4, 5],
        ),
        (
            "SELECT id FROM pruned WHERE f IS NOT DISTINCT FROM 0.0 OR n IS NOT DISTINCT FROM 3 ORDER BY id",
            vec![4, 5],
        ),
    ];
    for (sql, expected) in cases {
        // Act
        let pruned = pruned(&fixture, sql);
        let rows = fixture.rows(sql);
        // Assert
        assert!(pruned, "{sql} must read only its referenced fields");
        assert_eq!(ids(&rows), expected, "{sql}");
    }
}

#[test]
fn should_keep_mixed_null_safe_filters_on_the_equality_read_path() {
    // Arrange
    let fixture = sql_fixture("null_safe_mixed_projection", SOURCE);
    let cases = [
        (
            "SELECT id FROM pruned WHERE n IS NOT DISTINCT FROM f ORDER BY id",
            "SELECT id FROM pruned WHERE n = f ORDER BY id",
            vec![1, 2, 4],
        ),
        (
            "SELECT id FROM pruned WHERE f IS DISTINCT FROM n AND id > 1 ORDER BY id",
            "SELECT id FROM pruned WHERE f <> n AND id > 1 ORDER BY id",
            vec![3, 5],
        ),
    ];
    for (sql, equality, expected) in cases {
        // Act
        let null_safe_pruned = pruned(&fixture, sql);
        let equality_pruned = pruned(&fixture, equality);
        let rows = fixture.rows(sql);
        // Assert
        assert_eq!(null_safe_pruned, equality_pruned, "{sql}");
        assert_eq!(ids(&rows), expected, "{sql}");
    }
}

#[test]
fn should_aggregate_null_safe_filters_with_sql_results() {
    // Arrange
    let fixture = sql_fixture("null_safe_typed_aggregate_sql", SOURCE);
    // Act
    let counted =
        fixture.rows("SELECT COUNT(*) AS counted FROM pruned WHERE n IS NOT DISTINCT FROM NULL");
    let summed = fixture.rows("SELECT SUM(n) AS total FROM pruned WHERE f IS DISTINCT FROM NULL");
    // Assert
    assert_eq!(counted, vec![vec![Value::Int64(2)]]);
    assert_eq!(summed, vec![vec![Value::Int64(7)]]);
}

#[test]
fn should_distinguish_json_null_in_pruned_null_safe_filters() {
    // Arrange
    let fixture = sql_fixture(
        "null_safe_pruned_json",
        &[
            "CREATE TABLE pruned_json (id BIGINT, j JSON, payload TEXT)",
            "INSERT INTO pruned_json VALUES (1,NULL,'a'),(2,'null','b'),(3,'{\"n\":1}','c')",
        ],
    );
    let cases = [
        (
            "SELECT id FROM pruned_json WHERE j IS NOT DISTINCT FROM NULL ORDER BY id",
            vec![1],
        ),
        (
            "SELECT id FROM pruned_json WHERE j IS DISTINCT FROM NULL ORDER BY id",
            vec![2, 3],
        ),
        (
            "SELECT id FROM pruned_json WHERE j IS NULL ORDER BY id",
            vec![1],
        ),
    ];
    for (sql, expected) in cases {
        // Act
        let rows = fixture.rows(sql);
        // Assert
        assert_eq!(ids(&rows), expected, "{sql}");
    }
}
