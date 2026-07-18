use cassie::app::{Cassie, CassieError};
use cassie::config::{CassieRuntimeConfig, ExecutionResultCacheEnabled};
use cassie::midge::adapter::{
    query_scan_control_test_guard, set_query_scan_cancellation_after_entries,
};
use cassie::types::Value;
use uuid::Uuid;

fn configured_cassie(label: &str, memory_budget: usize) -> (Cassie, String) {
    std::env::set_var("CASSIE_MIDGE_ALLOW_FALLBACK", "1");
    let path = std::env::temp_dir()
        .join(format!("cassie-scalar-controls-{label}-{}", Uuid::new_v4()))
        .to_string_lossy()
        .into_owned();
    let mut config = CassieRuntimeConfig::from_env().expect("runtime config");
    config.limits.query_memory_budget_bytes = memory_budget;
    config.limits.execution_result_cache_enabled = ExecutionResultCacheEnabled::disabled();
    config.limits.parallel_scan_workers = 1;
    let cassie = Cassie::new_with_data_dir_and_config(&path, config).expect("configured cassie");
    cassie.startup().expect("startup");
    (cassie, path)
}

fn seed_indexed_rows(cassie: &Cassie, session: &cassie::app::CassieSession, table: &str) {
    cassie
        .execute_sql(
            session,
            &format!("CREATE TABLE {table} (score BIGINT, label TEXT)"),
            vec![],
        )
        .expect("create table");
    for score in 0..32_i64 {
        cassie
            .midge
            .put_document(
                table,
                Some(format!("row-{score:04}")),
                serde_json::json!({"score": score, "label": format!("label-{score:04}")}),
            )
            .expect("seed indexed row");
    }
    cassie
        .execute_sql(
            session,
            &format!("CREATE INDEX {table}_score_idx ON {table} USING btree (score)"),
            vec![],
        )
        .expect("create scalar index");
}

#[test]
fn should_apply_limit_while_iterating_scalar_index_entries() {
    // Arrange
    let _hook_guard = query_scan_control_test_guard();
    let (cassie, path) = configured_cassie("bounded", 64 * 1_024);
    let session = cassie.create_session("tester", None);
    seed_indexed_rows(&cassie, &session, "controlled_scalar_limit");
    let before = cassie.midge.query_scan_entries_for_diagnostics();

    // Act
    let result = cassie
        .execute_sql(
            &session,
            "SELECT score FROM controlled_scalar_limit WHERE score >= 0 ORDER BY score LIMIT 2",
            vec![],
        )
        .expect("bounded scalar index read");
    let visited = cassie
        .midge
        .query_scan_entries_for_diagnostics()
        .saturating_sub(before);

    // Assert
    assert_eq!(
        result.rows,
        vec![vec![Value::Int64(0)], vec![Value::Int64(1)]]
    );
    assert_eq!(visited, 2, "the native index scan must stop at LIMIT 2");
    assert_eq!(
        cassie.metrics()["query"]["current_accounted_memory_bytes"].as_u64(),
        Some(0)
    );

    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_reject_scalar_index_hit_before_retaining_it_given_low_memory() {
    // Arrange
    let _hook_guard = query_scan_control_test_guard();
    let (cassie, path) = configured_cassie("low-memory", 64);
    let session = cassie.create_session("tester", None);
    seed_indexed_rows(&cassie, &session, "controlled_scalar_memory");

    // Act
    let error = cassie
        .execute_sql(
            &session,
            "SELECT score FROM controlled_scalar_memory WHERE score >= 0 ORDER BY score LIMIT 1",
            vec![],
        )
        .expect_err("one retained scalar hit should exceed the query budget");

    // Assert
    assert!(matches!(error, CassieError::ResourceLimit(_)));
    assert_eq!(
        cassie.metrics()["query"]["current_accounted_memory_bytes"].as_u64(),
        Some(0)
    );

    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_cancel_at_a_deterministic_scalar_index_entry_without_leaking_reservations() {
    // Arrange
    let _hook_guard = query_scan_control_test_guard();
    let (cassie, path) = configured_cassie("cancellation", 64 * 1_024);
    let session = cassie.create_session("tester", None);
    seed_indexed_rows(&cassie, &session, "controlled_scalar_cancel");
    let before = cassie.midge.query_scan_entries_for_diagnostics();
    set_query_scan_cancellation_after_entries(Some(3));

    // Act
    let error = cassie
        .execute_sql(
            &session,
            "SELECT score FROM controlled_scalar_cancel WHERE score >= 0 ORDER BY score LIMIT 16",
            vec![],
        )
        .expect_err("the scalar index hook should cancel the query");
    set_query_scan_cancellation_after_entries(None);
    let visited = cassie
        .midge
        .query_scan_entries_for_diagnostics()
        .saturating_sub(before);

    // Assert
    assert!(matches!(error, CassieError::QueryCancelled));
    assert_eq!(visited, 3);
    assert_eq!(
        cassie.metrics()["query"]["current_accounted_memory_bytes"].as_u64(),
        Some(0)
    );

    let _ = std::fs::remove_dir_all(path);
}
