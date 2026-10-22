use cassie::app::{Cassie, CassieError};
use cassie::config::{CassieRuntimeConfig, ExecutionResultCacheEnabled};
use cassie::midge::adapter::{
    query_scan_control_test_guard, set_query_scan_cancellation_after_entries,
};
use cassie::types::Value;
fn configured_cassie(memory_budget: usize) -> Cassie {
    let mut config = CassieRuntimeConfig::from_env().expect("runtime config");
    config.limits.query_memory_budget_bytes = memory_budget;
    config.limits.execution_result_cache_enabled = ExecutionResultCacheEnabled::disabled();
    config.limits.parallel_scan_workers = 1;
    let cassie = Cassie::new_in_memory_with_config(config).expect("configured cassie");
    cassie.startup().expect("startup");
    cassie
}

fn seed_indexed_rows(cassie: &Cassie, session: &cassie::app::CassieSession, table: &str) {
    seed_indexed_rows_with_first_id_prefix(cassie, session, table, "");
}

fn seed_indexed_rows_with_first_id_prefix(
    cassie: &Cassie,
    session: &cassie::app::CassieSession,
    table: &str,
    first_id_prefix: &str,
) {
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
                Some(if score == 0 {
                    format!("{first_id_prefix}row-{score:04}")
                } else {
                    format!("row-{score:04}")
                }),
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
    let cassie = configured_cassie(64 * 1_024);
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
        result.rows(),
        vec![vec![Value::Int64(0)], vec![Value::Int64(1)]]
    );
    assert_eq!(visited, 2, "the native index scan must stop at LIMIT 2");
    drop(result);
    assert_eq!(
        cassie.metrics()["query"]["current_accounted_memory_bytes"].as_u64(),
        Some(0)
    );
}

#[test]
fn should_reject_scalar_index_hit_before_retaining_it_given_low_memory() {
    // Arrange
    let _hook_guard = query_scan_control_test_guard();
    // Act
    let observations = [512, 2 * 1_024].map(|memory_budget| {
        let cassie = configured_cassie(memory_budget);
        let session = cassie.create_session("tester", None);
        seed_indexed_rows_with_first_id_prefix(
            &cassie,
            &session,
            "controlled_scalar_memory",
            &"x".repeat(4_096),
        );
        let before_entries = cassie.midge.query_scan_entries_for_diagnostics();
        let before = cassie.metrics();
        let outcome = cassie.execute_sql(
            &session,
            "SELECT score FROM controlled_scalar_memory WHERE score >= 0 ORDER BY score LIMIT 1",
            vec![],
        );
        let visited = cassie
            .midge
            .query_scan_entries_for_diagnostics()
            .saturating_sub(before_entries);
        // Retire even an unexpected successful payload before fixture cleanup/assertions.
        let error = outcome.err();
        let after = cassie.metrics();
        drop(session);
        drop(cassie);
        (memory_budget, error, visited, before, after)
    });
    // Assert
    for ((memory_budget, error, visited, before, after), expected_entries) in
        observations.into_iter().zip([0, 1])
    {
        let error = error.expect("the selected scalar frontier must exceed its query budget");
        assert!(matches!(error, CassieError::ResourceLimit(_)));
        assert_eq!(
            visited, expected_entries,
            "budget{memory_budget} must reject at its source-backed frontier: {error}"
        );
        assert_eq!(after["query"]["current_accounted_memory_bytes"], 0);
        assert_eq!(after["runtime"]["running_queries"], 0);
        assert_eq!(after["runtime"]["active_operator_workers"], 0);
        assert_eq!(
            after["query"]["rows_returned_total"],
            before["query"]["rows_returned_total"]
        );
    }
}

#[test]
fn should_cancel_at_a_deterministic_scalar_index_entry_without_leaking_reservations() {
    // Arrange
    let _hook_guard = query_scan_control_test_guard();
    let cassie = configured_cassie(64 * 1_024);
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
}
