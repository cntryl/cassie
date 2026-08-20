use std::sync::Arc;
use std::time::Instant;

use cassie::app::{Cassie, CassieError};
use cassie::catalog::{canonical_relation_name, RetentionPolicyState};
use cassie::config::CassieRuntimeConfig;
use cassie::executor::projection::set_projection_build_failure_point;
use cassie::executor::{self, QueryError};
use cassie::runtime::QueryExecutionControls;

#[path = "support/sql.rs"]
mod support;
use support::{data_dir, use_local_storage};

fn configured_cassie(label: &str, memory_budget: usize) -> (Cassie, String) {
    use_local_storage();
    let path = data_dir(label);
    let mut config = CassieRuntimeConfig::from_env().expect("runtime config");
    config.limits.query_memory_budget_bytes = memory_budget;
    config.limits.parallel_scan_workers = 1;
    let cassie = Cassie::new_with_data_dir_and_config(&path, config).expect("configured Cassie");
    cassie.startup().expect("startup");
    (cassie, path)
}

fn execute_with_memory_budget(
    cassie: &Cassie,
    sql: &str,
    memory_budget: usize,
) -> Result<cassie::executor::QueryResult, QueryError> {
    let mut limits = CassieRuntimeConfig::default().limits;
    limits.query_memory_budget_bytes = memory_budget;
    let controls = QueryExecutionControls::from_limits(&limits, Instant::now());
    let plan = cassie
        .compile_sql_physical_plan_for_diagnostics(sql)
        .expect("compile physical plan");
    executor::run_with_controls(cassie, &Arc::clone(&plan), vec![], &controls)
}

fn expanding_sum() -> String {
    format!(
        "SUM(COALESCE({}))",
        std::iter::repeat_n("amount", 32)
            .collect::<Vec<_>>()
            .join(", ")
    )
}

#[test]
fn should_bound_rollup_projection_paths_before_output_build() {
    // Arrange
    let (cassie, path) = configured_cassie("rollup-projection-budget", 16 * 1_024 * 1_024);
    let session = cassie.create_session("tester", None);
    cassie
        .execute_sql(
            &session,
            "CREATE TABLE controlled_rollup_memory (tenant TEXT, event_at TEXT, amount INT)",
            vec![],
        )
        .expect("create source table");
    cassie
        .midge
        .put_fresh_documents(
            "controlled_rollup_memory",
            vec![(
                Some("event-1".to_string()),
                serde_json::json!({
                    "tenant": "acme",
                    "event_at": "2026-01-01T00:05:00Z",
                    "amount": 7,
                }),
            )],
        )
        .expect("seed source table");
    let aggregate = expanding_sum();
    cassie
        .execute_sql(
            &session,
            &format!(
                "CREATE ROLLUP controlled_rollup_memory_hourly ON controlled_rollup_memory USING time_bucket('1 hour', event_at) GROUP BY tenant AGGREGATES {aggregate} AS amount_sum"
            ),
            vec![],
        )
        .expect("create rollup");
    let query = format!(
        "SELECT time_bucket('1 hour', event_at) AS bucket, tenant, {aggregate} AS amount_sum FROM controlled_rollup_memory GROUP BY time_bucket('1 hour', event_at), tenant"
    );
    let explain = cassie
        .execute_sql(&session, &format!("EXPLAIN {query}"), vec![])
        .expect("explain rollup query");
    assert!(explain.rows.iter().flatten().any(|value| {
        value
            .as_str()
            .is_some_and(|plan| plan.contains("rollup_rewrite="))
    }));

    // Act
    set_projection_build_failure_point(true);
    let query_error = execute_with_memory_budget(&cassie, &query, 1_024)
        .expect_err("rollup query projection should exceed memory budget");
    set_projection_build_failure_point(true);
    let refresh_error = execute_with_memory_budget(
        &cassie,
        "REFRESH ROLLUP controlled_rollup_memory_hourly",
        1_024,
    )
    .expect_err("rollup refresh projection should exceed memory budget");
    set_projection_build_failure_point(false);

    // Assert
    for error in [query_error, refresh_error] {
        assert!(
            matches!(error, QueryError::Cassie(CassieError::ResourceLimit(_))),
            "unexpected rollup memory error: {error:?}"
        );
        assert!(error.to_string().contains("query memory budget exceeded"));
    }

    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_record_retention_error_when_source_scan_exceeds_memory() {
    // Arrange
    let (cassie, path) = configured_cassie("retention-scan-budget", 512);
    let session = cassie.create_session("tester", None);
    cassie
        .execute_sql(
            &session,
            "CREATE TABLE controlled_retention_memory (event_at TEXT, payload TEXT)",
            vec![],
        )
        .expect("create retention table");
    let documents = (0..16)
        .map(|index| {
            (
                Some(format!("event-{index:02}")),
                serde_json::json!({
                    "event_at": "2026-01-03T00:00:00Z",
                    "payload": "x".repeat(256),
                }),
            )
        })
        .collect();
    cassie
        .midge
        .put_fresh_documents("controlled_retention_memory", documents)
        .expect("seed retention table");
    cassie
        .execute_sql(
            &session,
            "CREATE RETENTION POLICY controlled_retention_memory_policy ON controlled_retention_memory USING event_at RETAIN FOR '1 day'",
            vec![],
        )
        .expect("create retention policy");

    // Act
    let error = cassie
        .execute_sql(
            &session,
            "ENFORCE RETENTION POLICY controlled_retention_memory_policy AT '2026-01-03T00:00:00Z'",
            vec![],
        )
        .expect_err("retention source scan should exceed memory budget");
    let policy = cassie
        .catalog
        .get_retention_policy(&canonical_relation_name(
            "postgres",
            "public",
            "controlled_retention_memory_policy",
        ))
        .expect("retention policy metadata");

    // Assert
    assert!(matches!(error, CassieError::ResourceLimit(_)));
    assert!(error.to_string().contains("query memory budget exceeded"));
    assert_eq!(policy.state, RetentionPolicyState::Error);
    assert!(policy
        .last_error
        .as_deref()
        .is_some_and(|message| message.contains("query memory budget exceeded")));

    let _ = std::fs::remove_dir_all(path);
}
