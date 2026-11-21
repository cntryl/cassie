//! Cancellation inside a parallel wave of the column vector path (#1358, S5).
use cassie::app::{Cassie, CassieError, CassieSession};
use cassie::config::{CassieRuntimeConfig, ExecutionResultCacheEnabled};
use cassie::midge::adapter::{
    query_scan_control_test_guard, set_query_scan_cancellation_after_entries,
};

const ROWS: usize = 2_000;

fn fixture() -> (Cassie, CassieSession) {
    let mut config = CassieRuntimeConfig::from_env().expect("configuration");
    config.limits.execution_result_cache_enabled = ExecutionResultCacheEnabled::disabled();
    config.limits.parallel_aggregation_workers = 4;
    config.limits.parallel_scan_workers = 4;
    let cassie = Cassie::new_in_memory_with_config(config).expect("engine");
    cassie.startup().expect("startup");
    let session = cassie.create_session("tester", None);
    cassie
        .execute_sql(
            &session,
            "CREATE TABLE waves (g INT, v BIGINT) WITH (storage = column_store)",
            vec![],
        )
        .expect("table");
    for start in (0..ROWS).step_by(500) {
        let values = (start..start + 500)
            .map(|row| format!("({}, {row})", row % 7))
            .collect::<Vec<_>>()
            .join(",");
        cassie
            .execute_sql(
                &session,
                &format!("INSERT INTO waves VALUES {values}"),
                vec![],
            )
            .expect("rows");
    }
    cassie
        .midge
        .rebuild_column_store_summaries_for_diagnostics("waves")
        .expect("summaries");
    (cassie, session)
}

fn assert_released(cassie: &Cassie) {
    let metrics = cassie.metrics();
    assert_eq!(metrics["runtime"]["running_queries"].as_u64(), Some(0));
    assert_eq!(
        metrics["runtime"]["active_operator_workers"].as_u64(),
        Some(0)
    );
    assert_eq!(
        metrics["query"]["current_accounted_memory_bytes"].as_u64(),
        Some(0)
    );
}

fn canceled_inside_a_wave(sql: &str) {
    // Arrange
    let _guard = query_scan_control_test_guard();
    let (cassie, session) = fixture();
    let before = cassie.midge.column_vector_reads_for_diagnostics();
    set_query_scan_cancellation_after_entries(Some(700));

    // Act
    let result = cassie.execute_sql(&session, sql, vec![]);
    set_query_scan_cancellation_after_entries(None);
    let delta = cassie
        .midge
        .column_vector_reads_for_diagnostics()
        .since(before);

    // Assert
    assert!(matches!(result, Err(CassieError::QueryCanceled)));
    assert!(delta.partitions >= 4, "the wave had started");
    assert_released(&cassie);
}

#[test]
fn should_cancel_an_ungrouped_aggregate_inside_a_wave_without_output() {
    canceled_inside_a_wave("SELECT COUNT(*), SUM(v) FROM waves");
}

#[test]
fn should_cancel_a_grouped_aggregate_inside_a_wave_without_output() {
    canceled_inside_a_wave("SELECT g, SUM(v) FROM waves GROUP BY g");
}

#[test]
fn should_cancel_a_projection_inside_a_wave_without_output() {
    canceled_inside_a_wave("SELECT g, v FROM waves WHERE v >= 0");
}
