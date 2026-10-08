//! Latest authoritative graph verification shares the captured statement Data view.
use super::{execute, CaptureHook, Cassie, Value};
use crate::config::{CassieRuntimeConfig, ExecutionResultCacheEnabled};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

#[test]
fn should_keep_graph_neighborhood_membership_on_the_captured_committed_view() {
    // Arrange
    let path = std::env::temp_dir().join(format!("cassie-graph-view-{}", uuid::Uuid::new_v4()));
    let mut config = CassieRuntimeConfig::default();
    config.limits.execution_result_cache_enabled = ExecutionResultCacheEnabled::disabled();
    let cassie = Arc::new(Cassie::new_with_data_dir_and_config(&path, config).expect("Cassie"));
    cassie.startup().expect("startup");
    let reader = cassie.create_session("reader", None);
    let writer = cassie.create_session("writer", None);
    execute(
        &cassie,
        &writer,
        "CREATE GRAPH social (NODES (label TEXT), EDGES (source TEXT))",
    );
    execute(&cassie, &writer, "INSERT INTO social_nodes (node_type, node_id, label) VALUES ('person', 'alice', 'Alice'), ('person', 'bob', 'Bob'), ('person', 'carol', 'Carol')");
    execute(&cassie, &writer, "INSERT INTO social_edges (edge_id, source_type, source_id, target_type, target_id, edge_type, weight, source) VALUES ('e1', 'person', 'alice', 'person', 'bob', 'knows', 1, 'direct')");
    let sql = "SELECT node_id, cost FROM graph_neighbors('social', 'person', 'alice', 'out', 'knows', 10)";
    let original = execute(&cassie, &reader, sql);
    let publisher = Arc::clone(&cassie);
    let committed = Arc::new(AtomicBool::new(false));
    let observed = Arc::clone(&committed);
    let hook = CaptureHook::install(move || {
        execute(&publisher, &writer, "BEGIN");
        execute(
            &publisher,
            &writer,
            "UPDATE social_nodes SET label = 'Changed' WHERE node_id = 'bob'",
        );
        execute(
            &publisher,
            &writer,
            "UPDATE social_edges SET target_id = 'carol', weight = 9 WHERE edge_id = 'e1'",
        );
        execute(&publisher, &writer, "COMMIT");
        let warmed = execute(&publisher, &writer, sql);
        assert_eq!(
            warmed,
            vec![vec![
                Value::String("carol".to_string()),
                Value::Float64(9.0)
            ]]
        );
        observed.store(true, Ordering::SeqCst);
    });
    // Act
    let captured = execute(&cassie, &reader, sql);
    drop(hook);
    let fresh = execute(&cassie, &reader, sql);
    // Assert
    assert!(committed.load(Ordering::SeqCst));
    assert_eq!(
        original,
        vec![vec![Value::String("bob".to_string()), Value::Float64(1.0)]]
    );
    assert_eq!(captured, original);
    assert_eq!(
        fresh,
        vec![vec![
            Value::String("carol".to_string()),
            Value::Float64(9.0)
        ]]
    );
    eprintln!(
        "captured graph rows={captured:?} fresh={fresh:?} metrics={}",
        cassie.metrics()["graph"]
    );
    assert_eq!(
        cassie.metrics()["query"]["current_accounted_memory_bytes"].as_u64(),
        Some(0)
    );
    assert_eq!(cassie.runtime.snapshot().runtime.active_operator_workers, 0);
    drop(reader);
    drop(cassie);
    std::fs::remove_dir_all(path).expect("strict graph fixture cleanup");
}
