//! Late correlated phase reads retain the captured statement Data owner.
use super::{execute, Cassie, Value};
use crate::config::{CassieRuntimeConfig, ExecutionResultCacheEnabled};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

struct Observation {
    captured: Vec<Vec<Value>>,
    fresh: Vec<Vec<Value>>,
    committed: bool,
    charge: Option<u64>,
    workers: u64,
}

fn observe_phase_view(sql: &str) -> Observation {
    let path = std::env::temp_dir().join(format!("cassie-phase-view-{}", uuid::Uuid::new_v4()));
    let mut config = CassieRuntimeConfig::default();
    config.limits.execution_result_cache_enabled = ExecutionResultCacheEnabled::disabled();
    let cassie = Arc::new(Cassie::new_with_data_dir_and_config(&path, config).expect("Cassie"));
    cassie.startup().expect("startup");
    let reader = cassie.create_session("reader", None);
    for statement in [
        "CREATE TABLE phase_left (id BIGINT)",
        "CREATE TABLE phase_right (id BIGINT)",
        "CREATE TABLE phase_membership (id BIGINT)",
        "INSERT INTO phase_left VALUES (1),(2)",
        "INSERT INTO phase_right VALUES (1),(2)",
        "INSERT INTO phase_membership VALUES (1)",
    ] {
        execute(&cassie, &reader, statement);
    }
    let publisher = Arc::clone(&cassie);
    let writer = cassie.create_session("writer", None);
    let committed = Arc::new(AtomicBool::new(false));
    let observed = Arc::clone(&committed);
    let hook = crate::executor::JoinReadProbe::install(move || {
        execute(&publisher, &writer, "BEGIN");
        execute(
            &publisher,
            &writer,
            "DELETE FROM phase_membership WHERE id=1",
        );
        execute(
            &publisher,
            &writer,
            "INSERT INTO phase_membership VALUES (2)",
        );
        execute(&publisher, &writer, "COMMIT");
        observed.store(true, Ordering::SeqCst);
    });
    let captured = execute(&cassie, &reader, sql);
    drop(hook);
    let fresh = execute(&cassie, &reader, sql);
    let result = Observation {
        captured,
        fresh,
        committed: committed.load(Ordering::SeqCst),
        charge: cassie.metrics()["query"]["current_accounted_memory_bytes"].as_u64(),
        workers: cassie.runtime.snapshot().runtime.active_operator_workers,
    };
    drop(reader);
    cassie.shutdown();
    drop(cassie);
    std::fs::remove_dir_all(path).expect("strict phase fixture cleanup");
    result
}

fn assert_observation(result: &Observation, captured: &[Vec<Value>], fresh: &[Vec<Value>]) {
    assert!(result.committed, "commit followed outer-left acquisition");
    assert_eq!(result.captured, captured);
    assert_eq!(result.fresh, fresh);
    assert_eq!(result.charge, Some(0));
    assert_eq!(result.workers, 0);
}

#[test]
fn should_preserve_having_statement_view_after_source_commit() {
    // Arrange
    let sql = "SELECT l.id,COUNT(*) FROM phase_left l JOIN phase_right r ON l.id=r.id GROUP BY l.id HAVING EXISTS(SELECT 1 FROM phase_membership u WHERE u.id=l.id) ORDER BY l.id";
    let captured = vec![vec![Value::Int64(1), Value::Int64(1)]];
    let fresh = vec![vec![Value::Int64(2), Value::Int64(1)]];
    // Act
    let result = observe_phase_view(sql);
    // Assert
    assert_observation(&result, &captured, &fresh);
}

#[test]
fn should_preserve_order_statement_view_after_source_commit() {
    // Arrange
    let sql = "SELECT l.id FROM phase_left l JOIN phase_right r ON l.id=r.id ORDER BY EXISTS(SELECT 1 FROM phase_membership u WHERE u.id=l.id),l.id";
    let captured = vec![vec![Value::Int64(2)], vec![Value::Int64(1)]];
    let fresh = vec![vec![Value::Int64(1)], vec![Value::Int64(2)]];
    // Act
    let result = observe_phase_view(sql);
    // Assert
    assert_observation(&result, &captured, &fresh);
}

#[test]
fn should_preserve_join_statement_view_after_source_commit() {
    // Arrange
    let sql = "SELECT l.id FROM phase_left l JOIN phase_right r ON l.id=r.id AND EXISTS(SELECT 1 FROM phase_membership u WHERE u.id=l.id) ORDER BY l.id";
    let captured = vec![vec![Value::Int64(1)]];
    let fresh = vec![vec![Value::Int64(2)]];
    // Act
    let result = observe_phase_view(sql);
    // Assert
    assert_observation(&result, &captured, &fresh);
}

#[test]
fn should_preserve_window_statement_view_after_source_commit() {
    // Arrange
    let sql = "SELECT l.id,LAST_VALUE(EXISTS(SELECT 1 FROM phase_membership u WHERE u.id=l.id)) OVER (ORDER BY l.id) AS matched FROM phase_left l JOIN phase_right r ON l.id=r.id ORDER BY l.id";
    let captured = vec![
        vec![Value::Int64(1), Value::Bool(true)],
        vec![Value::Int64(2), Value::Bool(false)],
    ];
    let fresh = vec![
        vec![Value::Int64(1), Value::Bool(false)],
        vec![Value::Int64(2), Value::Bool(true)],
    ];
    // Act
    let result = observe_phase_view(sql);
    // Assert
    assert_observation(&result, &captured, &fresh);
}
