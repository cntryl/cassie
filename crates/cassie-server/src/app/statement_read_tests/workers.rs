//! Whole-statement worker dispatch after committed and staged views diverge.
use super::{execute, CaptureHook, Cassie, Value};
use crate::config::{CassieRuntimeConfig, ExecutionResultCacheEnabled};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

#[test]
fn should_dispatch_parallel_scan_workers_with_the_captured_statement_view() {
    // Arrange
    let path =
        std::env::temp_dir().join(format!("cassie-statement-workers-{}", uuid::Uuid::new_v4()));
    let mut config = CassieRuntimeConfig::default();
    config.limits.parallel_scan_workers = 4;
    config.limits.execution_result_cache_enabled = ExecutionResultCacheEnabled::disabled();
    let cassie = Arc::new(Cassie::new_with_data_dir_and_config(&path, config).expect("Cassie"));
    cassie.startup().expect("startup");
    let reader = cassie.create_session("reader", None);
    let writer = cassie.create_session("writer", None);
    execute(&cassie, &writer, "CREATE TABLE worker_rows (n INT)");
    cassie
        .midge
        .put_documents(
            "worker_rows",
            (0..1025)
                .map(|n| (Some(format!("doc-{n:04}")), serde_json::json!({"n":n})))
                .collect(),
        )
        .expect("seed multiple scan batches");
    execute(&cassie, &reader, "BEGIN");
    execute(
        &cassie,
        &reader,
        "INSERT INTO worker_rows (n) VALUES (1025)",
    );
    let same_session = reader.clone();
    let publisher = Arc::clone(&cassie);
    let changed = Arc::new(AtomicBool::new(false));
    let observed = Arc::clone(&changed);
    let hook = CaptureHook::install(move || {
        execute(
            &publisher,
            &writer,
            "INSERT INTO worker_rows (n) VALUES (2000)",
        );
        execute(&publisher, &same_session, "ROLLBACK");
        observed.store(true, Ordering::SeqCst);
    });
    let before = cassie.metrics();
    // Act
    let captured = execute(&cassie, &reader, "SELECT n FROM worker_rows ORDER BY n");
    drop(hook);
    let after = cassie.metrics();
    let fresh = execute(&cassie, &reader, "SELECT n FROM worker_rows ORDER BY n");
    // Assert
    assert!(changed.load(Ordering::SeqCst));
    assert_eq!(
        captured,
        (0..1026).map(|n| vec![Value::Int64(n)]).collect::<Vec<_>>()
    );
    let mut expected_fresh = (0..1025).map(|n| vec![Value::Int64(n)]).collect::<Vec<_>>();
    expected_fresh.push(vec![Value::Int64(2000)]);
    assert_eq!(fresh, expected_fresh);
    let metric = |metrics: &serde_json::Value, field: &str| {
        metrics["parallel_scans"][field]
            .as_u64()
            .unwrap_or_default()
    };
    let scans = metric(&after, "scans") - metric(&before, "scans");
    let workers = metric(&after, "workers") - metric(&before, "workers");
    let rows = metric(&after, "rows") - metric(&before, "rows");
    eprintln!("captured worker scans={scans} workers={workers} rows={rows}");
    assert_eq!(scans, 1);
    assert!(workers >= 2);
    assert_eq!(rows, 1026);
    assert_eq!(
        cassie.metrics()["query"]["current_accounted_memory_bytes"].as_u64(),
        Some(0)
    );
    assert_eq!(cassie.runtime.snapshot().runtime.active_operator_workers, 0);
    drop(reader);
    drop(cassie);
    std::fs::remove_dir_all(path).expect("strict fixture cleanup");
}
