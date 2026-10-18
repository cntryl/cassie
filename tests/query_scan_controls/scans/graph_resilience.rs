use cassie::app::{Cassie, CassieError, CassieSession};
use cassie::catalog::GraphMeta;
use cassie::config::{CassieRuntimeConfig, ExecutionResultCacheEnabled};
use cassie::midge::adapter::{set_query_scan_cancellation_after_entries, StorageFamily};
use cassie::types::Value;
use serde_json::json;

use super::support_graph_evidence::SeededGraphFixture;
use super::support_sql as support;
use support::{data_dir, use_local_storage};

fn current_thread_runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime")
}

fn execute(cassie: &Cassie, session: &CassieSession, sql: &str) {
    cassie
        .execute_sql(session, sql, vec![])
        .expect("execute graph statement");
}

#[test]
fn should_preserve_seeded_graph_results_across_path_permutations() {
    // Arrange
    let _suite_query_scan_guard = cassie::midge::adapter::query_scan_control_test_guard();
    let forward = SeededGraphFixture::from_seed(0x00CA_551E, false).compare();
    let reverse = SeededGraphFixture::from_seed(0x00CA_551E, true).compare();
    let other_seed = SeededGraphFixture::from_seed(0x000A_11CE, false).compare();

    // Act
    let forward_rows = forward.adjacency.clone();
    let reverse_rows = reverse.adjacency.clone();

    // Assert
    assert_eq!(forward.native_overlay, forward_rows);
    assert_eq!(reverse.native_overlay, reverse_rows);
    assert_eq!(forward_rows, reverse_rows);
    assert_ne!(forward_rows, other_seed.adjacency);
    assert_eq!(
        forward.disconnected_native_overlay,
        [] as [std::vec::Vec<cassie::types::Value>; 0]
    );
    assert_eq!(
        forward.disconnected_adjacency,
        [] as [std::vec::Vec<cassie::types::Value>; 0]
    );
    assert_eq!(forward.overlay_fallback_reason, "transaction-overlay");
}

fn configured_cassie(path: &str, memory_budget: usize) -> Cassie {
    let mut config = CassieRuntimeConfig::from_env().expect("runtime config");
    config.limits.query_memory_budget_bytes = memory_budget;
    config.limits.execution_result_cache_enabled = ExecutionResultCacheEnabled::disabled();
    config.limits.parallel_scan_workers = 1;
    let cassie = Cassie::new_with_data_dir_and_config(path, config).expect("configured cassie");
    cassie.startup().expect("startup");
    cassie
}

fn graph_edge_payload(
    edge_id: &str,
    source_id: &str,
    target_id: &str,
    edge_type: &str,
    weight: f64,
) -> serde_json::Value {
    json!({
        "edge_id": edge_id,
        "source_type": "person",
        "source_id": source_id,
        "target_type": "person",
        "target_id": target_id,
        "edge_type": edge_type,
        "weight": weight,
    })
}

#[test]
fn should_keep_colon_containing_node_identities_distinct_in_shortest_path() {
    // Arrange
    let _suite_query_scan_guard = cassie::midge::adapter::query_scan_control_test_guard();
    use_local_storage();
    let path = data_dir("graph_colon_identity");
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).expect("cassie");
        let session = cassie.create_session("tester", None);
        execute(&cassie, &session, "CREATE GRAPH social");
        execute(
            &cassie,
            &session,
            "INSERT INTO social_edges (edge_id, source_type, source_id, target_type, target_id, edge_type, weight) VALUES ('dead', 'root', 'start', 'a:b', 'c', 'knows', 1), ('route', 'root', 'start', 'a', 'b:c', 'knows', 2), ('finish', 'a', 'b:c', 'goal', 'finish', 'knows', 1)",
        );

        // Act
        let rows_result = cassie
            .execute_sql(
                &session,
                "SELECT node_type, node_id, cost, depth FROM graph_shortest_path('social', 'root', 'start', 'goal', 'finish', 3, 'out', 'knows', 1)",
                vec![],
            )
            .expect("shortest path");
        let rows = rows_result.rows();

        // Assert
        assert_eq!(
            rows,
            vec![vec![
                Value::String("goal".into()),
                Value::String("finish".into()),
                Value::Float64(3.0),
                Value::Int64(2),
            ]]
        );
        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_merge_both_directions_by_weight_then_edge_id_before_limit() {
    // Arrange
    let _suite_query_scan_guard = cassie::midge::adapter::query_scan_control_test_guard();
    use_local_storage();
    let path = data_dir("graph_weighted_limit");
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).expect("cassie");
        let session = cassie.create_session("tester", None);
        execute(&cassie, &session, "CREATE GRAPH social");
        execute(
            &cassie,
            &session,
            "INSERT INTO social_edges (edge_id, source_type, source_id, target_type, target_id, edge_type, weight) VALUES ('e3', 'person', 'alice', 'person', 'dana', 'knows', 1), ('e1', 'person', 'bob', 'person', 'alice', 'knows', 1), ('e2', 'person', 'alice', 'person', 'carol', 'knows', 1), ('e0', 'person', 'erin', 'person', 'alice', 'knows', 2), ('e4', 'person', 'alice', 'person', 'frank', 'knows', 0.5)",
        );

        // Act
        let rows_result = cassie
            .execute_sql(
                &session,
                "SELECT edge_id, cost, node_id FROM graph_neighbors('social', 'person', 'alice', 'both', 'knows', 4)",
                vec![],
            )
            .expect("weighted neighbors");
        let rows = rows_result.rows();

        // Assert
        assert_eq!(
            rows,
            vec![
                vec![
                    Value::String("e4".into()),
                    Value::Float64(0.5),
                    Value::String("frank".into()),
                ],
                vec![
                    Value::String("e1".into()),
                    Value::Float64(1.0),
                    Value::String("bob".into()),
                ],
                vec![
                    Value::String("e2".into()),
                    Value::Float64(1.0),
                    Value::String("carol".into()),
                ],
                vec![
                    Value::String("e3".into()),
                    Value::Float64(1.0),
                    Value::String("dana".into()),
                ],
            ]
        );
        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_return_a_self_loop_once_when_scanning_both_directions() {
    // Arrange
    let _suite_query_scan_guard = cassie::midge::adapter::query_scan_control_test_guard();
    use_local_storage();
    let path = data_dir("graph_self_loop_both");
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).expect("cassie");
        let session = cassie.create_session("tester", None);
        execute(&cassie, &session, "CREATE GRAPH social");
        execute(
            &cassie,
            &session,
            "INSERT INTO social_edges (edge_id, source_type, source_id, target_type, target_id, edge_type, weight) VALUES ('loop', 'person', 'alice', 'person', 'alice', 'knows', 1)",
        );

        // Act
        let rows_result = cassie
            .execute_sql(
                &session,
                "SELECT edge_id, node_id FROM graph_neighbors('social', 'person', 'alice', 'both', 'knows', 10)",
                vec![],
            )
            .expect("self-loop neighbors");
        let rows = rows_result.rows();

        // Assert
        assert_eq!(
            rows,
            vec![vec![
                Value::String("loop".into()),
                Value::String("alice".into()),
            ]]
        );
        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_fail_graph_traversal_atomically_given_low_query_memory() {
    // Arrange
    let _suite_query_scan_guard = cassie::midge::adapter::query_scan_control_test_guard();
    use_local_storage();
    let path = data_dir("graph_low_memory");
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let cassie = configured_cassie(&path, 64 * 1_024);
        let session = cassie.create_session("tester", None);
        execute(&cassie, &session, "CREATE GRAPH social");
        let edges = (0..8)
            .map(|index| {
                let edge_id = format!("edge-{index:02}");
                let target_id = format!("neighbor-with-a-retained-identity-{index:02}");
                (
                    Some(edge_id.clone()),
                    graph_edge_payload(&edge_id, "alice", &target_id, "knows", index.into()),
                )
            })
            .collect();
        cassie
            .midge
            .put_fresh_graph_documents("social_edges", edges)
            .expect("seed graph edges");
        // Bootstrap under an ample budget, then reopen the same durable graph at the target cap.
        drop(session);
        drop(cassie);
        let cassie = configured_cassie(&path, 64);
        let session = cassie.create_session("tester", None);
        let before = cassie.metrics();

        // Act
        let error = cassie
            .execute_sql(
                &session,
                "SELECT node_id FROM graph_expand('social', 'person', 'alice', 2, 'out', 'knows', 8)",
                vec![],
            )
            .expect_err("retained traversal state should exceed the query budget");
        let after = cassie.metrics();

        // Assert
        assert!(
            matches!(error, CassieError::ResourceLimit(_)),
            "expected SQLSTATE 54000 resource limit, got {error:?}"
        );
        assert_eq!(
            after["graph"]["traversals"], before["graph"]["traversals"],
            "a failed traversal must not publish success metrics"
        );
        assert_eq!(
            after["graph"]["rows"], before["graph"]["rows"],
            "a failed traversal must not publish partial rows"
        );
        assert_eq!(
            after["query"]["current_accounted_memory_bytes"].as_u64(),
            Some(0)
        );
        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_bound_shortest_path_search_by_query_memory() {
    // Arrange
    let _suite_query_scan_guard = cassie::midge::adapter::query_scan_control_test_guard();
    use_local_storage();
    let path = data_dir("graph_shortest_path_low_memory");
    let runtime = current_thread_runtime();
    runtime.block_on(async {
            let cassie = configured_cassie(&path, 64 * 1_024);
            let session = cassie.create_session("tester", None);
            execute(&cassie, &session, "CREATE GRAPH social");
            let edges = (0..8)
                .map(|index| {
                    let edge_id = format!("edge-{index:02}");
                    let target_id = format!("neighbor-{index:02}");
                    (
                        Some(edge_id.clone()),
                        graph_edge_payload(&edge_id, "alice", &target_id, "knows", 1.0),
                    )
                })
                .collect();
            cassie
                .midge
                .put_fresh_graph_documents("social_edges", edges)
                .expect("seed graph edges");
            // Bootstrap under an ample budget, then reopen the same durable graph at the target cap.
            drop(session);
            drop(cassie);
            let cassie = configured_cassie(&path, 64);
            let session = cassie.create_session("tester", None);
            let before = cassie.metrics();

            // Act
            let error = cassie
                .execute_sql(
                    &session,
                    "SELECT node_id FROM graph_shortest_path('social', 'person', 'alice', 'person', 'missing', 2, 'out', 'knows', 2)",
                    vec![],
                )
                .expect_err("retained path states should exceed the query budget");
            let after = cassie.metrics();

            // Assert
            assert!(
                matches!(error, CassieError::ResourceLimit(_)),
                "expected SQLSTATE 54000 resource limit, got {error:?}"
            );
            assert_eq!(
                after["graph"]["traversals"], before["graph"]["traversals"],
                "a failed traversal must not publish success metrics"
            );
            assert_eq!(
                after["graph"]["rows"], before["graph"]["rows"],
                "a failed traversal must not publish partial rows"
            );
            assert_eq!(
                after["query"]["current_accounted_memory_bytes"].as_u64(),
                Some(0)
            );
            let _ = std::fs::remove_dir_all(path);
        });
}

#[test]
fn should_cancel_graph_scan_at_a_deterministic_entry_without_partial_metrics() {
    // Arrange
    let _suite_query_scan_guard = cassie::midge::adapter::query_scan_control_test_guard();
    use_local_storage();
    let path = data_dir("graph_deterministic_cancellation");
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let cassie = configured_cassie(&path, 64 * 1_024);
        let session = cassie.create_session("tester", None);
        execute(&cassie, &session, "CREATE GRAPH social");
        let edges = (0..8)
            .map(|index| {
                let edge_id = format!("edge-{index:02}");
                let target_id = format!("neighbor-{index:02}");
                (
                    Some(edge_id.clone()),
                    graph_edge_payload(&edge_id, "alice", &target_id, "knows", index.into()),
                )
            })
            .collect();
        cassie
            .midge
            .put_fresh_graph_documents("social_edges", edges)
            .expect("seed graph edges");
        let before_metrics = cassie.metrics();
        let before_entries = cassie.midge.query_scan_entries_for_diagnostics();
        set_query_scan_cancellation_after_entries(Some(3));

        // Act
        let error = cassie
            .execute_sql(
                &session,
                "SELECT node_id FROM graph_neighbors('social', 'person', 'alice', 'out', 'knows', 8)",
                vec![],
            )
            .expect_err("the controlled graph scan should observe cancellation");
        set_query_scan_cancellation_after_entries(None);
        let after_metrics = cassie.metrics();
        let visited = cassie
            .midge
            .query_scan_entries_for_diagnostics()
            .saturating_sub(before_entries);

        // Assert
        assert!(
            matches!(error, CassieError::QueryCancelled),
            "expected SQLSTATE 57014 cancellation, got {error:?}"
        );
        assert_eq!(visited, 3);
        assert_eq!(
            after_metrics["graph"]["traversals"],
            before_metrics["graph"]["traversals"]
        );
        assert_eq!(
            after_metrics["graph"]["rows"],
            before_metrics["graph"]["rows"]
        );
        assert_eq!(
            after_metrics["query"]["current_accounted_memory_bytes"].as_u64(),
            Some(0)
        );
        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_match_native_results_after_transaction_overlay_commit() {
    // Arrange
    let _suite_query_scan_guard = cassie::midge::adapter::query_scan_control_test_guard();
    use_local_storage();
    let path = data_dir("graph_overlay_fallback");
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).expect("cassie");
        let session = cassie.create_session("writer", None);
        execute(&cassie, &session, "CREATE GRAPH social");
        execute(&cassie, &session, "BEGIN");
        execute(
            &cassie,
            &session,
            "INSERT INTO social_edges (edge_id, source_type, source_id, target_type, target_id, edge_type, weight) VALUES ('e2', 'person', 'alice', 'person', 'carol', 'knows', 2), ('e1', 'person', 'alice', 'person', 'bob', 'knows', 1)",
        );

        // Act
        let overlay_result = cassie
            .execute_sql(
                &session,
                "SELECT edge_id, node_id, cost FROM graph_neighbors('social', 'person', 'alice', 'out', 'knows', 10)",
                vec![],
            )
            .expect("transaction overlay traversal");
        let overlay = overlay_result.rows();
        let overlay_metrics = cassie.metrics();
        execute(&cassie, &session, "COMMIT");
        let native_result = cassie
            .execute_sql(
                &session,
                "SELECT edge_id, node_id, cost FROM graph_neighbors('social', 'person', 'alice', 'out', 'knows', 10)",
                vec![],
            )
            .expect("native traversal");
        let native = native_result.rows();

        // Assert
        assert_eq!(overlay, native);
        assert_eq!(
            overlay_metrics["graph"]["last_fallback_reason"],
            "transaction-overlay"
        );
        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_preserve_graph_adjacency_across_schema_rename() {
    // Arrange
    let _suite_query_scan_guard = cassie::midge::adapter::query_scan_control_test_guard();
    use_local_storage();
    let path = data_dir("graph_schema_rename_drop");
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).expect("cassie");
        cassie.startup().expect("startup");
        let session = cassie.create_session("tester", Some("postgres".to_string()));
        execute(&cassie, &session, "CREATE SCHEMA reporting");
        execute(
            &cassie,
            &session,
            "CREATE TABLE reporting.social_nodes (node_type TEXT, node_id TEXT)",
        );
        execute(
            &cassie,
            &session,
            "CREATE TABLE reporting.social_edges (edge_id TEXT, source_type TEXT, source_id TEXT, target_type TEXT, target_id TEXT, edge_type TEXT, weight FLOAT)",
        );
        let graph = GraphMeta::new("postgres.reporting.social");
        cassie.midge.put_graph(&graph).expect("persist graph metadata");
        cassie.catalog.register_graph(graph);
        execute(
            &cassie,
            &session,
            "INSERT INTO reporting.social_edges (edge_id, source_type, source_id, target_type, target_id, edge_type, weight) VALUES ('e1', 'person', 'alice', 'person', 'bob', 'knows', 1)",
        );
        let storage_id_before_rename = cassie
            .midge
            .list_graphs()
            .expect("graphs before rename")[0]
            .storage_id;

        // Act
        execute(
            &cassie,
            &session,
            "ALTER SCHEMA reporting RENAME TO reporting_archive",
        );
        let renamed_result = cassie
            .execute_sql(
                &session,
                "SELECT edge_id FROM graph_neighbors('reporting_archive.social', 'person', 'alice', 'out', 'knows', 10)",
                vec![],
            )
            .expect("renamed graph traversal");
        let renamed = renamed_result.rows();
        let old_name = cassie.execute_sql(
            &session,
            "SELECT edge_id FROM graph_neighbors('reporting.social', 'person', 'alice', 'out', 'knows', 10)",
            vec![],
        );
        let storage_id_after_rename = cassie
            .midge
            .list_graphs()
            .expect("graphs after rename")[0]
            .storage_id;

        // Assert
        assert_eq!(renamed, vec![vec![Value::String("e1".into())]]);
        assert!(old_name.is_err());
        assert_eq!(storage_id_after_rename, storage_id_before_rename);
        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_remove_graph_adjacency_after_edge_collection_drop() {
    // Arrange
    let _suite_query_scan_guard = cassie::midge::adapter::query_scan_control_test_guard();
    use_local_storage();
    let path = data_dir("graph_edge_collection_drop");
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).expect("cassie");
        cassie.startup().expect("startup");
        let session = cassie.create_session("tester", None);
        execute(&cassie, &session, "CREATE GRAPH social");
        execute(
            &cassie,
            &session,
            "INSERT INTO social_edges (edge_id, source_type, source_id, target_type, target_id, edge_type, weight) VALUES ('e1', 'person', 'alice', 'person', 'bob', 'knows', 1)",
        );
        let before_drop_result = cassie
            .execute_sql(
                &session,
                "SELECT edge_id FROM graph_neighbors('social', 'person', 'alice', 'out', 'knows', 10)",
                vec![],
            )
            .expect("graph traversal before edge collection drop");
        let before_drop = before_drop_result.rows();

        // Act
        execute(&cassie, &session, "DROP TABLE social_edges");
        let after_drop_result = cassie
            .execute_sql(
                &session,
                "SELECT edge_id FROM graph_neighbors('social', 'person', 'alice', 'out', 'knows', 10)",
                vec![],
            )
            .expect("graph traversal after edge collection drop");
        let after_drop = after_drop_result.rows();
        drop(cassie);
        let restarted = Cassie::new_with_data_dir(&path).expect("restarted cassie");
        restarted.startup().expect("startup after edge collection drop");
        let restarted_session = restarted.create_session("tester", None);
        let after_restart_result = restarted
            .execute_sql(
                &restarted_session,
                "SELECT edge_id FROM graph_neighbors('social', 'person', 'alice', 'out', 'knows', 10)",
                vec![],
            )
            .expect("graph traversal after dropped collection restart");
        let after_restart = after_restart_result.rows();

        // Assert
        assert_eq!(before_drop, vec![vec![Value::String("e1".into())]]);
        assert_eq!(after_drop, [] as [std::vec::Vec<cassie::types::Value>; 0]);
        assert_eq!(after_restart, [] as [std::vec::Vec<cassie::types::Value>; 0]);
        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_retain_graph_traversal_order_after_restart() {
    // Arrange
    let _suite_query_scan_guard = cassie::midge::adapter::query_scan_control_test_guard();
    use_local_storage();
    let path = data_dir("graph_restart");
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let before_restart = {
            let cassie = Cassie::new_with_data_dir(&path).expect("cassie");
            cassie.startup().expect("startup");
            let session = cassie.create_session("tester", None);
            execute(&cassie, &session, "CREATE GRAPH social");
            execute(
                &cassie,
                &session,
                "INSERT INTO social_edges (edge_id, source_type, source_id, target_type, target_id, edge_type, weight) VALUES ('e2', 'person', 'alice', 'person', 'carol', 'knows', 2), ('e1', 'person', 'alice', 'person', 'bob', 'knows', 1)",
            );
            cassie
                .execute_sql(
                    &session,
                    "SELECT edge_id, node_id, cost FROM graph_neighbors('social', 'person', 'alice', 'out', 'knows', 10)",
                    vec![],
                )
                .expect("traversal before restart")
                .into_owned_rows()
        };

        // Act
        let restarted = Cassie::new_with_data_dir(&path).expect("restarted cassie");
        restarted.startup().expect("restart startup");
        let session = restarted.create_session("tester", None);
        let after_restart_result = restarted
            .execute_sql(
                &session,
                "SELECT edge_id, node_id, cost FROM graph_neighbors('social', 'person', 'alice', 'out', 'knows', 10)",
                vec![],
            )
            .expect("traversal after restart");
        let after_restart = after_restart_result.rows();

        // Assert
        assert_eq!(after_restart, before_restart.as_slice());
        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_rebuild_inconsistent_graph_adjacency_during_startup() {
    // Arrange
    let _suite_query_scan_guard = cassie::midge::adapter::query_scan_control_test_guard();
    use_local_storage();
    let path = data_dir("graph_rebuild_inconsistent_sidecar");
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).expect("cassie");
        let session = cassie.create_session("tester", None);
        execute(&cassie, &session, "CREATE GRAPH social");
        execute(
            &cassie,
            &session,
            "INSERT INTO social_edges (edge_id, source_type, source_id, target_type, target_id, edge_type, weight) VALUES ('e1', 'person', 'alice', 'person', 'bob', 'knows', 1)",
        );
        let entries = cassie
            .midge
            .raw_scan_prefix(StorageFamily::Data, b"")
            .expect("data entries");
        let manifest_key = entries
            .iter()
            .find(|(_, value)| {
                serde_json::from_slice::<serde_json::Value>(value).is_ok_and(|value| {
                    value.get("format_version").is_some()
                        && value.get("source_generation").is_some()
                        && value.get("edge_count").is_some()
                })
            })
            .map(|(key, _)| key)
            .expect("graph manifest");
        let adjacency_key = entries
            .iter()
            .filter(|(_, value)| value.is_empty())
            .max_by_key(|(key, _)| common_prefix_len(key, manifest_key))
            .map(|(key, _)| key)
            .expect("graph adjacency entry");
        cassie
            .midge
            .raw_delete(StorageFamily::Data, adjacency_key)
            .expect("remove one adjacency entry");
        drop(cassie);

        // Act
        let restarted = Cassie::new_with_data_dir(&path).expect("restarted cassie");
        restarted.startup().expect("startup rebuild");
        let restarted_session = restarted.create_session("tester", None);
        let before_entries = restarted.midge.query_scan_entries_for_diagnostics();
        let rows_result = restarted
            .execute_sql(
                &restarted_session,
                "SELECT edge_id FROM graph_neighbors('social', 'person', 'alice', 'out', 'knows', 1)",
                vec![],
            )
            .expect("rebuilt native traversal");
        let rows = rows_result.rows();
        let visited = restarted
            .midge
            .query_scan_entries_for_diagnostics()
            .saturating_sub(before_entries);

        // Assert
        assert_eq!(rows, vec![vec![Value::String("e1".into())]]);
        assert_eq!(visited, 7, "one source row, four members, manifest and selected member");
        let _ = std::fs::remove_dir_all(path);
    });
}

fn common_prefix_len(left: &[u8], right: &[u8]) -> usize {
    left.iter()
        .zip(right)
        .take_while(|(left, right)| left == right)
        .count()
}

#[test]
fn should_verify_all_authoritative_edges_before_filtered_native_graph_reads() {
    // Arrange
    let _suite_query_scan_guard = cassie::midge::adapter::query_scan_control_test_guard();
    use_local_storage();
    let path = data_dir("graph_bounded_native_reads");
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let cassie = configured_cassie(&path, 64 * 1_024);
        let session = cassie.create_session("tester", None);
        execute(&cassie, &session, "CREATE GRAPH social");
        let mut edges = (0..64)
            .map(|index| {
                let edge_id = format!("noise-{index:02}");
                let target_id = format!("noise-node-{index:02}");
                (
                    Some(edge_id.clone()),
                    graph_edge_payload(&edge_id, "alice", &target_id, "ignored", index.into()),
                )
            })
            .collect::<Vec<_>>();
        edges.extend([
            (
                Some("knows-2".to_string()),
                graph_edge_payload("knows-2", "alice", "carol", "knows", 2.0),
            ),
            (
                Some("knows-1".to_string()),
                graph_edge_payload("knows-1", "alice", "bob", "knows", 1.0),
            ),
        ]);
        cassie
            .midge
            .put_fresh_graph_documents("social_edges", edges)
            .expect("seed graph edges");
        let before_entries = cassie.midge.query_scan_entries_for_diagnostics();
        let before_reads = cassie.metrics()["storage"]["data"]["reads"]
            .as_u64()
            .unwrap_or_default();

        // Act
        let rows_result = cassie
            .execute_sql(
                &session,
                "SELECT edge_id FROM graph_neighbors('social', 'person', 'alice', 'out', 'knows', 1)",
                vec![],
            )
            .expect("bounded filtered graph scan");
        let rows = rows_result.rows();
        let after = cassie.metrics();
        let visited = cassie
            .midge
            .query_scan_entries_for_diagnostics()
            .saturating_sub(before_entries);
        let reads = after["storage"]["data"]["reads"]
            .as_u64()
            .unwrap_or_default()
            .saturating_sub(before_reads);

        // Assert
        assert_eq!(rows, vec![vec![Value::String("knows-1".into())]]);
        assert!(visited > 0, "native graph reads must be observable");
        assert_eq!(visited, 66 * 5 + 2, "authoritative source, complete sidecar and selected member");
        assert_eq!(after["graph"]["last_reads"].as_u64(), Some(visited));
        assert!(reads <= visited * 2 + 4, "linear storage-read bound, got {reads}");
        drop(rows_result);
        assert_eq!(
            cassie.metrics()["query"]["current_accounted_memory_bytes"].as_u64(),
            Some(0)
        );
        let _ = std::fs::remove_dir_all(path);
    });
}
