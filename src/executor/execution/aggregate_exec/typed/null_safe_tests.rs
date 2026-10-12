use super::*;
use std::time::Instant;

const CASES: [(&str, i64); 4] = [
    (
        "SELECT COUNT(*) AS counted FROM null_safe_typed WHERE n IS NOT DISTINCT FROM NULL",
        2,
    ),
    (
        "SELECT COUNT(*) AS counted FROM null_safe_typed WHERE n IS DISTINCT FROM 7",
        4,
    ),
    (
        "SELECT SUM(n) AS total FROM null_safe_typed WHERE f IS DISTINCT FROM NULL",
        7,
    ),
    (
        "SELECT COUNT(*) AS counted FROM null_safe_typed WHERE f IS NOT DISTINCT FROM 0.0 OR n IS NOT DISTINCT FROM 3",
        2,
    ),
];

struct Observed {
    sql: &'static str,
    typed: Option<Value>,
    scalar: Vec<Vec<Value>>,
    retained_bytes: usize,
}

/// Runs every case through the typed aggregate path and the public SQL path.
fn observe_null_safe_aggregates(index: Option<&str>) -> Vec<Observed> {
    let _scan_control_guard = crate::midge::adapter::query_scan_control_test_guard();
    let path =
        std::env::temp_dir().join(format!("cassie-typed-null-safe-{}", uuid::Uuid::new_v4()));
    let cassie = Cassie::new_with_data_dir(path.to_str().expect("path")).expect("Cassie");
    let session = cassie.create_session("tester", None);
    for sql in [
        "CREATE TABLE null_safe_typed (n BIGINT, f FLOAT)",
        "INSERT INTO null_safe_typed (n,f) VALUES (NULL,NULL),(7,7.0),(NULL,2.5),(0,-0.0),(3,NULL)",
    ]
    .into_iter()
    .chain(index)
    {
        cassie.execute_sql(&session, sql, vec![]).expect("setup");
    }
    let controls = QueryExecutionControls::from_limits(&cassie.runtime.limits(), Instant::now());
    let mut observed = Vec::new();
    for (sql, _) in CASES {
        let statement = crate::sql::parse_statement(sql).expect("statement");
        let plan =
            super::super::super::build_logical_plan_in_session(&cassie, Some(&session), &statement)
                .expect("plan");
        let typed = try_execute(&cassie, Some(&session), &plan, &controls)
            .expect("typed execution")
            .map(|rows| rows[0].entries()[0].1.clone());
        let scalar = cassie
            .execute_sql(&session, sql, vec![])
            .expect("ordinary execution")
            .rows()
            .to_vec();
        observed.push(Observed {
            sql,
            typed,
            scalar,
            retained_bytes: controls.current_query_memory_bytes(),
        });
    }
    drop((session, cassie));
    std::fs::remove_dir_all(path).expect("cleanup");
    observed
}

fn assert_typed_matches(observed: &[Observed]) {
    for (case, (_, expected)) in observed.iter().zip(CASES) {
        let sql = case.sql;
        assert_eq!(case.typed, Some(Value::Int64(expected)), "{sql}");
        assert_eq!(case.scalar, vec![vec![Value::Int64(expected)]], "{sql}");
        assert_eq!(case.retained_bytes, 0, "{sql}");
    }
}

#[test]
fn should_aggregate_null_safe_numeric_predicates_through_the_typed_path() {
    // Arrange
    let index = None;
    // Act
    let observed = observe_null_safe_aggregates(index);
    // Assert
    assert_typed_matches(&observed);
}

#[test]
fn should_aggregate_null_safe_predicates_over_encoded_column_batches() {
    // Arrange
    let index = Some(
        "CREATE INDEX null_safe_typed_idx ON null_safe_typed USING column (n,f) WITH (segment_size = 2)",
    );
    // Act
    let observed = observe_null_safe_aggregates(index);
    // Assert
    assert_typed_matches(&observed);
}
