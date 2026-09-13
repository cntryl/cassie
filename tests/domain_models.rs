// Consolidated integration suite: domain_models.
// Shared fixtures live in tests/support; former test targets remain named modules.

#[path = "support/graph.rs"]
mod support_graph;
#[path = "support/graph_neighbors.rs"]
mod support_graph_neighbors;
#[path = "support/sql.rs"]
mod support_sql;

// Formerly tests/graph_resilience.rs.
mod graph_resilience {
    use cassie::app::{Cassie, CassieError, CassieSession};
    use cassie::catalog::GraphMeta;
    use cassie::config::{CassieRuntimeConfig, ExecutionResultCacheEnabled};
    use cassie::midge::adapter::{set_query_scan_cancellation_after_entries, StorageFamily};
    use cassie::types::Value;
    use serde_json::json;

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
        let rows = cassie
            .execute_sql(
                &session,
                "SELECT node_type, node_id, cost, depth FROM graph_shortest_path('social', 'root', 'start', 'goal', 'finish', 3, 'out', 'knows', 1)",
                vec![],
            )
            .expect("shortest path")
            .rows;

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
        let rows = cassie
            .execute_sql(
                &session,
                "SELECT edge_id, cost, node_id FROM graph_neighbors('social', 'person', 'alice', 'both', 'knows', 4)",
                vec![],
            )
            .expect("weighted neighbors")
            .rows;

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
        let rows = cassie
            .execute_sql(
                &session,
                "SELECT edge_id, node_id FROM graph_neighbors('social', 'person', 'alice', 'both', 'knows', 10)",
                vec![],
            )
            .expect("self-loop neighbors")
            .rows;

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
        let cassie = configured_cassie(&path, 64);
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
        let overlay = cassie
            .execute_sql(
                &session,
                "SELECT edge_id, node_id, cost FROM graph_neighbors('social', 'person', 'alice', 'out', 'knows', 10)",
                vec![],
            )
            .expect("transaction overlay traversal")
            .rows;
        let overlay_metrics = cassie.metrics();
        execute(&cassie, &session, "COMMIT");
        let native = cassie
            .execute_sql(
                &session,
                "SELECT edge_id, node_id, cost FROM graph_neighbors('social', 'person', 'alice', 'out', 'knows', 10)",
                vec![],
            )
            .expect("native traversal")
            .rows;

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
        let renamed = cassie
            .execute_sql(
                &session,
                "SELECT edge_id FROM graph_neighbors('reporting_archive.social', 'person', 'alice', 'out', 'knows', 10)",
                vec![],
            )
            .expect("renamed graph traversal")
            .rows;
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
        let before_drop = cassie
            .execute_sql(
                &session,
                "SELECT edge_id FROM graph_neighbors('social', 'person', 'alice', 'out', 'knows', 10)",
                vec![],
            )
            .expect("graph traversal before edge collection drop")
            .rows;

        // Act
        execute(&cassie, &session, "DROP TABLE social_edges");
        let after_drop = cassie
            .execute_sql(
                &session,
                "SELECT edge_id FROM graph_neighbors('social', 'person', 'alice', 'out', 'knows', 10)",
                vec![],
            )
            .expect("graph traversal after edge collection drop")
            .rows;
        drop(cassie);
        let restarted = Cassie::new_with_data_dir(&path).expect("restarted cassie");
        restarted.startup().expect("startup after edge collection drop");
        let restarted_session = restarted.create_session("tester", None);
        let after_restart = restarted
            .execute_sql(
                &restarted_session,
                "SELECT edge_id FROM graph_neighbors('social', 'person', 'alice', 'out', 'knows', 10)",
                vec![],
            )
            .expect("graph traversal after dropped collection restart")
            .rows;

        // Assert
        assert_eq!(before_drop, vec![vec![Value::String("e1".into())]]);
        assert!(after_drop.is_empty());
        assert!(after_restart.is_empty());
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
                .rows
        };

        // Act
        let restarted = Cassie::new_with_data_dir(&path).expect("restarted cassie");
        restarted.startup().expect("restart startup");
        let session = restarted.create_session("tester", None);
        let after_restart = restarted
            .execute_sql(
                &session,
                "SELECT edge_id, node_id, cost FROM graph_neighbors('social', 'person', 'alice', 'out', 'knows', 10)",
                vec![],
            )
            .expect("traversal after restart")
            .rows;

        // Assert
        assert_eq!(after_restart, before_restart);
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
        let rows = restarted
            .execute_sql(
                &restarted_session,
                "SELECT edge_id FROM graph_neighbors('social', 'person', 'alice', 'out', 'knows', 1)",
                vec![],
            )
            .expect("rebuilt native traversal")
            .rows;
        let visited = restarted
            .midge
            .query_scan_entries_for_diagnostics()
            .saturating_sub(before_entries);

        // Assert
        assert_eq!(rows, vec![vec![Value::String("e1".into())]]);
        assert_eq!(visited, 1, "startup should rebuild the bounded sidecar");
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
    fn should_bound_filtered_native_graph_reads_to_the_requested_edge_type() {
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
        let rows = cassie
            .execute_sql(
                &session,
                "SELECT edge_id FROM graph_neighbors('social', 'person', 'alice', 'out', 'knows', 1)",
                vec![],
            )
            .expect("bounded filtered graph scan")
            .rows;
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
        assert!(visited <= 2, "expected bounded edge-type reads, got {visited}");
        assert!(reads <= 4, "expected bounded storage reads, got {reads}");
        assert_eq!(
            after["query"]["current_accounted_memory_bytes"].as_u64(),
            Some(0)
        );
        let _ = std::fs::remove_dir_all(path);
    });
    }
}

// Formerly tests/graph_transaction_semantics.rs.
mod graph_transaction_semantics {
    use super::support_graph as support;
    use super::support_graph_neighbors as graph_neighbors;
    use graph_neighbors::neighbor_rows;
    use support::*;

    #[test]
    fn should_read_an_inserted_edge_inside_its_transaction() {
        // Arrange
        let _suite_query_scan_guard = cassie::midge::adapter::query_scan_control_test_guard();
        use_local_storage();
        let path = data_dir("graph_transaction_insert");
        let runtime = current_thread_runtime();
        runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).expect("cassie");
        let writer = cassie.create_session("writer", None);
        create_graph(&cassie, &writer);
        execute(&cassie, &writer, "BEGIN");

        // Act
        execute(
            &cassie,
            &writer,
            "INSERT INTO social_edges (edge_id, source_type, source_id, target_type, target_id, edge_type, weight) VALUES ('e1', 'person', 'alice', 'person', 'bob', 'knows', 2)",
        );
        let rows = neighbor_rows(&cassie, &writer, "out");

        // Assert
        assert_eq!(
            rows,
            vec![vec![Value::String("bob".into()), Value::Float64(2.0)]]
        );
        execute(&cassie, &writer, "ROLLBACK");
        let _ = std::fs::remove_dir_all(path);
    });
    }

    #[test]
    fn should_hide_a_deleted_edge_only_from_its_transaction_until_commit() {
        // Arrange
        let _suite_query_scan_guard = cassie::midge::adapter::query_scan_control_test_guard();
        use_local_storage();
        let path = data_dir("graph_transaction_delete");
        let runtime = current_thread_runtime();
        runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).expect("cassie");
        let writer = cassie.create_session("writer", None);
        let reader = cassie.create_session("reader", None);
        create_graph(&cassie, &writer);
        execute(
            &cassie,
            &writer,
            "INSERT INTO social_edges (edge_id, source_type, source_id, target_type, target_id, edge_type, weight) VALUES ('e1', 'person', 'alice', 'person', 'bob', 'knows', 2)",
        );
        execute(&cassie, &writer, "BEGIN");

        // Act
        execute(
            &cassie,
            &writer,
            "DELETE FROM social_edges WHERE edge_id = 'e1'",
        );
        let writer_rows = neighbor_rows(&cassie, &writer, "out");
        let reader_before = neighbor_rows(&cassie, &reader, "out");
        execute(&cassie, &writer, "COMMIT");
        let reader_after = neighbor_rows(&cassie, &reader, "out");

        // Assert
        assert!(writer_rows.is_empty());
        assert_eq!(
            reader_before,
            vec![vec![Value::String("bob".into()), Value::Float64(2.0)]]
        );
        assert!(reader_after.is_empty());
        let _ = std::fs::remove_dir_all(path);
    });
    }

    #[test]
    fn should_expand_across_edges_staged_in_one_transaction() {
        // Arrange
        let _suite_query_scan_guard = cassie::midge::adapter::query_scan_control_test_guard();
        use_local_storage();
        let path = data_dir("graph_transaction_expansion");
        let runtime = current_thread_runtime();
        runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).expect("cassie");
        let writer = cassie.create_session("writer", None);
        create_graph(&cassie, &writer);
        execute(&cassie, &writer, "BEGIN");
        execute(
            &cassie,
            &writer,
            "INSERT INTO social_edges (edge_id, source_type, source_id, target_type, target_id, edge_type, weight) VALUES ('e1', 'person', 'alice', 'person', 'bob', 'knows', 1), ('e2', 'person', 'bob', 'person', 'carol', 'knows', 1)",
        );

        // Act
        let rows = cassie
            .execute_sql(
                &writer,
                "SELECT node_id, depth FROM graph_expand('social', 'person', 'alice', 2, 'out', 'knows', 10) ORDER BY depth, node_id",
                vec![],
            )
            .expect("expand transaction overlay")
            .rows;

        // Assert
        assert_eq!(
            rows,
            vec![
                vec![Value::String("bob".into()), Value::Int64(1)],
                vec![Value::String("carol".into()), Value::Int64(2)],
            ]
        );
        execute(&cassie, &writer, "ROLLBACK");
        let _ = std::fs::remove_dir_all(path);
    });
    }

    #[test]
    fn should_merge_both_directions_for_transactional_neighbors() {
        // Arrange
        let _suite_query_scan_guard = cassie::midge::adapter::query_scan_control_test_guard();
        use_local_storage();
        let path = data_dir("graph_transaction_neighbors");
        let runtime = current_thread_runtime();
        runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).expect("cassie");
        let writer = cassie.create_session("writer", None);
        create_graph(&cassie, &writer);
        execute(&cassie, &writer, "BEGIN");
        execute(
            &cassie,
            &writer,
            "INSERT INTO social_edges (edge_id, source_type, source_id, target_type, target_id, edge_type, weight) VALUES ('e1', 'person', 'bob', 'person', 'alice', 'knows', 1), ('e2', 'person', 'alice', 'person', 'carol', 'knows', 2)",
        );

        // Act
        let rows = neighbor_rows(&cassie, &writer, "both");
        let metrics = cassie.metrics();

        // Assert
        assert_eq!(
            rows,
            vec![
                vec![Value::String("bob".into()), Value::Float64(1.0)],
                vec![Value::String("carol".into()), Value::Float64(2.0)],
            ]
        );
        assert_eq!(
            metrics["graph"]["last_fallback_reason"],
            "transaction-overlay"
        );
        execute(&cassie, &writer, "ROLLBACK");
        let _ = std::fs::remove_dir_all(path);
    });
    }

    #[test]
    fn should_restore_a_graph_edge_after_savepoint_rollback() {
        // Arrange
        let _suite_query_scan_guard = cassie::midge::adapter::query_scan_control_test_guard();
        use_local_storage();
        let path = data_dir("graph_transaction_savepoint");
        let runtime = current_thread_runtime();
        runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).expect("cassie");
        let writer = cassie.create_session("writer", None);
        create_graph(&cassie, &writer);
        execute(&cassie, &writer, "BEGIN");
        execute(
            &cassie,
            &writer,
            "INSERT INTO social_edges (edge_id, source_type, source_id, target_type, target_id, edge_type, weight) VALUES ('e1', 'person', 'alice', 'person', 'bob', 'knows', 2)",
        );
        execute(&cassie, &writer, "SAVEPOINT before_delete");
        execute(
            &cassie,
            &writer,
            "DELETE FROM social_edges WHERE edge_id = 'e1'",
        );

        // Act
        execute(&cassie, &writer, "ROLLBACK TO SAVEPOINT before_delete");
        let rows = cassie
            .execute_sql(
                &writer,
                "SELECT node_id FROM graph_expand('social', 'person', 'alice', 1, 'out', 'knows', 10)",
                vec![],
            )
            .expect("expand after rollback")
            .rows;

        // Assert
        assert_eq!(rows, vec![vec![Value::String("bob".into())]]);
        execute(&cassie, &writer, "ROLLBACK");
        let _ = std::fs::remove_dir_all(path);
    });
    }

    #[test]
    fn should_choose_the_lowest_cost_path_from_transactional_edges() {
        // Arrange
        let _suite_query_scan_guard = cassie::midge::adapter::query_scan_control_test_guard();
        use_local_storage();
        let path = data_dir("graph_transaction_shortest_path");
        let runtime = current_thread_runtime();
        runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).expect("cassie");
        let writer = cassie.create_session("writer", None);
        create_graph(&cassie, &writer);
        execute(&cassie, &writer, "BEGIN");
        execute(
            &cassie,
            &writer,
            "INSERT INTO social_edges (edge_id, source_type, source_id, target_type, target_id, edge_type, weight) VALUES ('direct', 'person', 'alice', 'person', 'carol', 'knows', 10), ('e1', 'person', 'alice', 'person', 'bob', 'knows', 1), ('e2', 'person', 'bob', 'person', 'carol', 'knows', 1)",
        );

        // Act
        let rows = cassie
            .execute_sql(
                &writer,
                "SELECT node_id, cost, depth FROM graph_shortest_path('social', 'person', 'alice', 'person', 'carol', 3, 'out', 'knows', 1)",
                vec![],
            )
            .expect("shortest path through transaction overlay")
            .rows;

        // Assert
        assert_eq!(
            rows,
            vec![vec![
                Value::String("carol".into()),
                Value::Float64(2.0),
                Value::Int64(2),
            ]]
        );
        execute(&cassie, &writer, "ROLLBACK");
        let _ = std::fs::remove_dir_all(path);
    });
    }

    #[test]
    fn should_read_an_updated_edge_inside_its_transaction() {
        // Arrange
        let _suite_query_scan_guard = cassie::midge::adapter::query_scan_control_test_guard();
        use_local_storage();
        let path = data_dir("graph_transaction_update");
        let runtime = current_thread_runtime();
        runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).expect("cassie");
        let writer = cassie.create_session("writer", None);
        let reader = cassie.create_session("reader", None);
        create_graph(&cassie, &writer);
        execute(
            &cassie,
            &writer,
            "INSERT INTO social_edges (edge_id, source_type, source_id, target_type, target_id, edge_type, weight) VALUES ('e1', 'person', 'alice', 'person', 'bob', 'knows', 2)",
        );
        execute(&cassie, &writer, "BEGIN");

        // Act
        execute(
            &cassie,
            &writer,
            "UPDATE social_edges SET target_id = 'carol', weight = 1 WHERE edge_id = 'e1'",
        );
        let writer_rows = neighbor_rows(&cassie, &writer, "out");
        let reader_rows = neighbor_rows(&cassie, &reader, "out");

        // Assert
        assert_eq!(
            writer_rows,
            vec![vec![Value::String("carol".into()), Value::Float64(1.0)]]
        );
        assert_eq!(
            reader_rows,
            vec![vec![Value::String("bob".into()), Value::Float64(2.0)]]
        );
        execute(&cassie, &writer, "ROLLBACK");
        let _ = std::fs::remove_dir_all(path);
    });
    }

    #[test]
    fn should_publish_a_graph_edge_to_other_sessions_only_after_commit() {
        // Arrange
        let _suite_query_scan_guard = cassie::midge::adapter::query_scan_control_test_guard();
        use_local_storage();
        let path = data_dir("graph_transaction_visibility");
        let runtime = current_thread_runtime();
        runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).expect("cassie");
        let writer = cassie.create_session("writer", None);
        let reader = cassie.create_session("reader", None);
        create_graph(&cassie, &writer);
        execute(&cassie, &writer, "BEGIN");
        execute(
            &cassie,
            &writer,
            "INSERT INTO social_edges (edge_id, source_type, source_id, target_type, target_id, edge_type, weight) VALUES ('e1', 'person', 'alice', 'person', 'bob', 'knows', 2)",
        );

        // Act
        let writer_rows = neighbor_rows(&cassie, &writer, "out");
        let reader_before = neighbor_rows(&cassie, &reader, "out");
        execute(&cassie, &writer, "COMMIT");
        let reader_after = neighbor_rows(&cassie, &reader, "out");

        // Assert
        assert_eq!(
            writer_rows,
            vec![vec![Value::String("bob".into()), Value::Float64(2.0)]]
        );
        assert!(reader_before.is_empty());
        assert_eq!(reader_after, writer_rows);
        let _ = std::fs::remove_dir_all(path);
    });
    }
}

// Formerly tests/integration_sql_graph.rs.
mod integration_sql_graph {
    #![allow(unused_imports, dead_code)]

    use cassie::app::Cassie;
    use cassie::types::Value;
    use serde_json::json;

    use super::support_sql as support;
    use support::*;

    #[test]
    fn should_execute_graph_neighbors_weighted_shortest_path() {
        // Arrange
        let _suite_query_scan_guard = cassie::midge::adapter::query_scan_control_test_guard();
        use_local_storage();
        let path = data_dir("graph_neighbors_shortest_path");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        let session = cassie.create_session("tester", None);
        seed_social_graph(&cassie, &session);

        // Act
        let neighbors = cassie
            .execute_sql(
                &session,
                "SELECT node_id, edge_type, cost FROM graph_neighbors('social', 'person', 'alice', 'out', 'knows', 10) ORDER BY node_id",
                vec![],
            )
            .unwrap();
        let shortest = cassie
            .execute_sql(
                &session,
                "SELECT node_id, cost, depth FROM graph_shortest_path('social', 'person', 'alice', 'person', 'carol', 4, 'out', 'knows', 1)",
                vec![],
            )
            .unwrap();

        // Assert
        assert_eq!(neighbors.rows.len(), 2);
        assert_eq!(neighbors.rows[0][0], Value::String("bob".to_string()));
        assert_eq!(neighbors.rows[1][0], Value::String("carol".to_string()));
        assert_eq!(shortest.rows.len(), 1);
        assert_eq!(shortest.rows[0][0], Value::String("carol".to_string()));
        assert_eq!(shortest.rows[0][1], Value::Float64(2.0));
        assert_eq!(shortest.rows[0][2], Value::Int64(2));

        let _ = std::fs::remove_dir_all(path);
    });
    }

    #[test]
    fn should_execute_lateral_graph_expansion_explain_metrics_after_restart() {
        // Arrange
        let _suite_query_scan_guard = cassie::midge::adapter::query_scan_control_test_guard();
        use_local_storage();
        let path = data_dir("graph_lateral_restart");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        let session = cassie.create_session("tester", None);
        seed_social_graph(&cassie, &session);

        // Act
        let expanded = cassie
            .execute_sql(
                &session,
                "SELECT graph_expand.node_id, graph_expand.depth FROM (SELECT node_type, node_id FROM social_nodes WHERE node_id = 'alice') AS seeds CROSS JOIN LATERAL graph_expand('social', seeds.node_type, seeds.node_id, 2, 'out', 'knows', 10) ORDER BY graph_expand.depth, graph_expand.node_id",
                vec![],
            )
            .unwrap();
        let explain = cassie
            .execute_sql(
                &session,
                "EXPLAIN SELECT node_id FROM graph_expand('social', 'person', 'alice', 2, 'out', 'knows', 10)",
                vec![],
            )
            .unwrap();
        let metrics = cassie.metrics();
        drop(cassie);

        let restarted = Cassie::new_with_data_dir(&path).unwrap();
        restarted.hydrate_catalog().unwrap();
        let restarted_session = restarted.create_session("tester", None);
        let after_restart = restarted
            .execute_sql(
                &restarted_session,
                "SELECT node_id FROM graph_neighbors('social', 'person', 'alice', 'out', 'knows', 10) ORDER BY node_id",
                vec![],
            )
            .unwrap();

        // Assert
        assert_eq!(
            expanded.rows,
            vec![
                vec![Value::String("bob".to_string()), Value::Int64(1)],
                vec![Value::String("carol".to_string()), Value::Int64(1)],
                vec![Value::String("carol".to_string()), Value::Int64(2)],
            ]
        );
        let plan = match &explain.rows[0][0] {
            Value::String(plan) => plan,
            other => panic!("expected explain string, got {other:?}"),
        };
        assert!(plan.contains("access_path=graph_adjacency"));
        assert_eq!(metrics["graph"]["traversals"].as_u64(), Some(1));
        assert_eq!(metrics["graph"]["last_graph"].as_str(), Some("social"));
        assert_eq!(after_restart.rows.len(), 2);

        let _ = std::fs::remove_dir_all(path);
    });
    }

    #[test]
    fn should_reject_negative_graph_edge_weight() {
        // Arrange
        let _suite_query_scan_guard = cassie::midge::adapter::query_scan_control_test_guard();
        use_local_storage();
        let path = data_dir("graph_negative_weight");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(&session, "CREATE GRAPH social", vec![])
            .unwrap();

        // Act
        let error = cassie
            .execute_sql(
                &session,
                "INSERT INTO social_edges (edge_id, source_type, source_id, target_type, target_id, edge_type, weight) VALUES ('bad', 'person', 'alice', 'person', 'bob', 'knows', -1)",
                vec![],
            )
            .expect_err("negative weights should be rejected");

        // Assert
        assert!(error.to_string().contains("non-negative"));

        let _ = std::fs::remove_dir_all(path);
    });
    }

    #[test]
    fn should_bulk_load_fresh_graph_documents_for_adjacency_reads() {
        // Arrange
        let _suite_query_scan_guard = cassie::midge::adapter::query_scan_control_test_guard();
        use_local_storage();
        let path = data_dir("graph_fresh_bulk_load");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(
                &session,
                "CREATE GRAPH social (NODES (label TEXT), EDGES (source TEXT))",
                vec![],
            )
            .unwrap();
        cassie
            .midge
            .put_fresh_graph_documents(
                "social_nodes",
                vec![
                    (
                        Some("alice".to_string()),
                        json!({"node_type": "person", "node_id": "alice", "label": "Alice"}),
                    ),
                    (
                        Some("bob".to_string()),
                        json!({"node_type": "person", "node_id": "bob", "label": "Bob"}),
                    ),
                    (
                        Some("carol".to_string()),
                        json!({"node_type": "person", "node_id": "carol", "label": "Carol"}),
                    ),
                ],
            )
            .unwrap();
        cassie
            .midge
            .put_fresh_graph_documents(
                "social_edges",
                vec![
                    (
                        Some("e1".to_string()),
                        json!({
                            "edge_id": "e1",
                            "source_type": "person",
                            "source_id": "alice",
                            "target_type": "person",
                            "target_id": "bob",
                            "edge_type": "knows",
                            "weight": 1,
                            "source": "bulk",
                        }),
                    ),
                    (
                        Some("e2".to_string()),
                        json!({
                            "edge_id": "e2",
                            "source_type": "person",
                            "source_id": "bob",
                            "target_type": "person",
                            "target_id": "carol",
                            "edge_type": "knows",
                            "weight": 1,
                            "source": "bulk",
                        }),
                    ),
                ],
            )
            .unwrap();

        // Act
        let expanded = cassie
            .execute_sql(
                &session,
                "SELECT node_id, depth FROM graph_expand('social', 'person', 'alice', 2, 'out', 'knows', 10) ORDER BY depth, node_id",
                vec![],
            )
            .unwrap();

        // Assert
        assert_eq!(
            expanded.rows,
            vec![
                vec![Value::String("bob".to_string()), Value::Int64(1)],
                vec![Value::String("carol".to_string()), Value::Int64(2)],
            ]
        );

        let _ = std::fs::remove_dir_all(path);
    });
    }

    fn seed_social_graph(cassie: &Cassie, session: &cassie::app::CassieSession) {
        cassie
            .execute_sql(
                session,
                "CREATE GRAPH social (NODES (label TEXT), EDGES (source TEXT))",
                vec![],
            )
            .unwrap();
        cassie
        .execute_sql(
            session,
            "INSERT INTO social_nodes (node_type, node_id, label) VALUES ('person', 'alice', 'Alice'), ('person', 'bob', 'Bob'), ('person', 'carol', 'Carol')",
            vec![],
        )
        .unwrap();
        cassie
        .execute_sql(
            session,
            "INSERT INTO social_edges (edge_id, source_type, source_id, target_type, target_id, edge_type, weight, source) VALUES ('e1', 'person', 'alice', 'person', 'bob', 'knows', 1, 'direct'), ('e2', 'person', 'bob', 'person', 'carol', 'knows', 1, 'direct'), ('e3', 'person', 'alice', 'person', 'carol', 'knows', 10, 'direct')",
            vec![],
        )
        .unwrap();
    }
}

// Formerly tests/time_series_index_completeness.rs.
mod time_series_index_completeness {
    use cassie::app::Cassie;
    use cassie::midge::adapter::StorageFamily;
    use cassie::types::Value;
    use serde_json::{json, Value as JsonValue};

    use super::support_sql as support;

    const TABLE: &str = "ts_complete_events";
    const INDEX: &str = "idx_ts_complete_time";
    const QUERY: &str = "SELECT amount FROM ts_complete_events WHERE event_at >= '2026-01-01T00:00:00Z' AND event_at < '2026-01-01T02:00:00Z' ORDER BY amount";

    fn fixture(label: &str) -> (Cassie, String) {
        support::use_local_storage();
        let path = support::data_dir(label);
        let cassie = Cassie::new_with_data_dir(&path).expect("cassie");
        cassie.startup().expect("startup");
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE ts_complete_events (tenant TEXT, event_at TIMESTAMP, amount INT)",
                vec![],
            )
            .expect("create table");
        cassie
        .execute_sql(
            &session,
            "CREATE INDEX idx_ts_complete_time ON ts_complete_events USING time_series (event_at) WITH (bucket_width = '1 hour', partition_by = tenant)",
            vec![],
        )
        .expect("create index");
        cassie
            .midge
            .put_fresh_time_series_documents(
                &canonical_collection(&cassie),
                vec![
                    (
                        Some("event-1".to_string()),
                        json!({"tenant":"acme","event_at":"2026-01-01T00:00:00Z","amount":10}),
                    ),
                    (
                        Some("event-2".to_string()),
                        json!({"tenant":"acme","event_at":"2026-01-01T00:30:00Z","amount":20}),
                    ),
                    (
                        Some("event-3".to_string()),
                        json!({"tenant":"acme","event_at":"2026-01-01T01:00:00Z","amount":30}),
                    ),
                ],
            )
            .expect("seed documents");
        (cassie, path)
    }

    fn canonical_collection(cassie: &Cassie) -> String {
        cassie
            .catalog
            .get_schema(TABLE)
            .expect("table metadata")
            .collection
    }

    fn canonical_index(cassie: &Cassie) -> String {
        cassie
            .catalog
            .get_index(&canonical_collection(cassie), INDEX)
            .expect("index metadata")
            .name
    }

    fn membership_entries(cassie: &Cassie) -> Vec<(Vec<u8>, Vec<u8>)> {
        let prefix = cassie
            .midge
            .time_series_index_prefix_for_diagnostics(
                &canonical_collection(cassie),
                &canonical_index(cassie),
            )
            .expect("membership prefix");
        cassie
            .midge
            .raw_scan_prefix(StorageFamily::Data, &prefix)
            .expect("membership entries")
    }

    fn manifest_key(cassie: &Cassie) -> Vec<u8> {
        cassie
            .midge
            .time_series_manifest_key_for_diagnostics(
                &canonical_collection(cassie),
                &canonical_index(cassie),
            )
            .expect("manifest key")
    }

    fn manifest(cassie: &Cassie) -> JsonValue {
        serde_json::from_slice(
            &cassie
                .midge
                .raw_get(StorageFamily::Data, &manifest_key(cassie))
                .expect("read manifest")
                .expect("manifest exists"),
        )
        .expect("valid manifest")
    }

    fn bucket_count_entries(cassie: &Cassie) -> Vec<(Vec<u8>, Vec<u8>)> {
        let prefix = cassie
            .midge
            .time_series_bucket_count_prefix_for_diagnostics(
                &canonical_collection(cassie),
                &canonical_index(cassie),
            )
            .expect("bucket count prefix");
        cassie
            .midge
            .raw_scan_prefix(StorageFamily::Data, &prefix)
            .expect("bucket count entries")
    }

    fn execute_query(cassie: &Cassie) -> cassie::executor::QueryResult {
        cassie
            .execute_sql(&cassie.create_session("tester", None), QUERY, vec![])
            .expect("time-series query")
    }

    fn expected_rows() -> Vec<Vec<Value>> {
        vec![
            vec![Value::Int64(10)],
            vec![Value::Int64(20)],
            vec![Value::Int64(30)],
        ]
    }

    fn assert_fallback(cassie: &Cassie, reason: &str) {
        let metrics = cassie.metrics();
        assert_eq!(metrics["time_series"]["fallback_scans"].as_u64(), Some(1));
        assert_eq!(
            metrics["time_series"]["last_fallback_reason"].as_str(),
            Some(reason)
        );
    }

    #[test]
    fn should_fallback_given_one_missing_membership_among_valid_bucket_entries() {
        // Arrange
        let _suite_query_scan_guard = cassie::midge::adapter::query_scan_control_test_guard();
        let (cassie, path) = fixture("time-series-one-missing-membership");
        let missing = membership_entries(&cassie)
            .into_iter()
            .find(|(key, _)| key.windows(b"event-2".len()).any(|part| part == b"event-2"))
            .expect("event-2 membership");
        cassie
            .midge
            .raw_delete(StorageFamily::Data, &missing.0)
            .expect("delete one membership");

        // Act
        let result = execute_query(&cassie);

        // Assert
        assert_eq!(result.rows, expected_rows());
        assert_fallback(&cassie, "incomplete-bucket-membership");
        drop(cassie);
        let _ = std::fs::remove_dir_all(path);
    }

    #[test]
    fn should_fallback_given_dangling_membership_with_surviving_valid_entries() {
        // Arrange
        let _suite_query_scan_guard = cassie::midge::adapter::query_scan_control_test_guard();
        let (cassie, path) = fixture("time-series-dangling-membership");
        let (key, value) = membership_entries(&cassie)
            .into_iter()
            .find(|(key, _)| key.windows(b"event-2".len()).any(|part| part == b"event-2"))
            .expect("event-2 membership");
        let mut dangling_key = key.clone();
        let offset = dangling_key
            .windows(b"event-2".len())
            .position(|part| part == b"event-2")
            .expect("membership id offset");
        dangling_key[offset..offset + b"ghost-2".len()].copy_from_slice(b"ghost-2");
        cassie
            .midge
            .raw_delete(StorageFamily::Data, &key)
            .expect("delete source membership");
        cassie
            .midge
            .raw_put(StorageFamily::Data, &dangling_key, &value)
            .expect("write dangling membership");

        // Act
        let result = execute_query(&cassie);

        // Assert
        assert_eq!(result.rows, expected_rows());
        assert_fallback(&cassie, "dangling-bucket-membership");
        drop(cassie);
        let _ = std::fs::remove_dir_all(path);
    }

    #[test]
    fn should_fallback_given_missing_bucket_count_metadata() {
        // Arrange
        let _suite_query_scan_guard = cassie::midge::adapter::query_scan_control_test_guard();
        let (cassie, path) = fixture("time-series-missing-bucket-count");
        let (key, _) = bucket_count_entries(&cassie)
            .into_iter()
            .next()
            .expect("bucket count");
        cassie
            .midge
            .raw_delete(StorageFamily::Data, &key)
            .expect("delete bucket count");

        // Act
        let result = execute_query(&cassie);

        // Assert
        assert_eq!(result.rows, expected_rows());
        assert_fallback(&cassie, "missing-bucket-metadata");
        drop(cassie);
        let _ = std::fs::remove_dir_all(path);
    }

    #[test]
    fn should_fallback_given_corrupt_bucket_count() {
        // Arrange
        let _suite_query_scan_guard = cassie::midge::adapter::query_scan_control_test_guard();
        let (cassie, path) = fixture("time-series-corrupt-bucket-count");
        let (key, _) = bucket_count_entries(&cassie)
            .into_iter()
            .next()
            .expect("bucket count");
        cassie
            .midge
            .raw_put(StorageFamily::Data, &key, b"corrupt")
            .expect("corrupt bucket count");

        // Act
        let result = execute_query(&cassie);

        // Assert
        assert_eq!(result.rows, expected_rows());
        assert_fallback(&cassie, "corrupt-bucket-metadata");
        drop(cassie);
        let _ = std::fs::remove_dir_all(path);
    }

    #[test]
    fn should_fallback_given_corrupt_manifest_total() {
        // Arrange
        let _suite_query_scan_guard = cassie::midge::adapter::query_scan_control_test_guard();
        let (cassie, path) = fixture("time-series-corrupt-total");
        let mut corrupted = manifest(&cassie);
        corrupted["total_membership"] = json!(99);
        cassie
            .midge
            .raw_put(
                StorageFamily::Data,
                &manifest_key(&cassie),
                &serde_json::to_vec(&corrupted).expect("encode corrupt manifest"),
            )
            .expect("corrupt manifest total");

        // Act
        let result = execute_query(&cassie);

        // Assert
        assert_eq!(result.rows, expected_rows());
        assert_fallback(&cassie, "corrupt-bucket-metadata");
        drop(cassie);
        let _ = std::fs::remove_dir_all(path);
    }

    #[test]
    fn should_fallback_given_stale_manifest_generation() {
        // Arrange
        let _suite_query_scan_guard = cassie::midge::adapter::query_scan_control_test_guard();
        let (cassie, path) = fixture("time-series-stale-generation");
        let mut corrupted = manifest(&cassie);
        corrupted["generation"] = json!(0);
        cassie
            .midge
            .raw_put(
                StorageFamily::Data,
                &manifest_key(&cassie),
                &serde_json::to_vec(&corrupted).expect("encode stale manifest"),
            )
            .expect("stale manifest generation");

        // Act
        let result = execute_query(&cassie);

        // Assert
        assert_eq!(result.rows, expected_rows());
        assert_fallback(&cassie, "stale-bucket-metadata");
        drop(cassie);
        let _ = std::fs::remove_dir_all(path);
    }

    #[test]
    fn should_rebuild_missing_manifest_during_restart() {
        // Arrange
        let _suite_query_scan_guard = cassie::midge::adapter::query_scan_control_test_guard();
        let (cassie, path) = fixture("time-series-restart-missing-manifest");
        cassie
            .midge
            .raw_delete(StorageFamily::Data, &manifest_key(&cassie))
            .expect("delete manifest");
        drop(cassie);

        // Act
        let restarted = Cassie::new_with_data_dir(&path).expect("restarted cassie");
        restarted.startup().expect("restart reconciliation");
        let result = execute_query(&restarted);

        // Assert
        assert_eq!(result.rows, expected_rows());
        assert_eq!(manifest(&restarted)["version"].as_u64(), Some(1));
        assert_eq!(
            restarted.metrics()["time_series"]["bucket_native_hits"].as_u64(),
            Some(1)
        );
        drop(restarted);
        let _ = std::fs::remove_dir_all(path);
    }

    #[test]
    fn should_rebuild_old_manifest_version_during_restart() {
        // Arrange
        let _suite_query_scan_guard = cassie::midge::adapter::query_scan_control_test_guard();
        let (cassie, path) = fixture("time-series-restart-old-manifest");
        let mut old = manifest(&cassie);
        old["version"] = json!(0);
        cassie
            .midge
            .raw_put(
                StorageFamily::Data,
                &manifest_key(&cassie),
                &serde_json::to_vec(&old).expect("encode old manifest"),
            )
            .expect("write old manifest");
        drop(cassie);

        // Act
        let restarted = Cassie::new_with_data_dir(&path).expect("restarted cassie");
        restarted.startup().expect("restart reconciliation");
        let result = execute_query(&restarted);

        // Assert
        assert_eq!(result.rows, expected_rows());
        assert_eq!(manifest(&restarted)["version"].as_u64(), Some(1));
        assert_eq!(
            restarted.metrics()["time_series"]["bucket_native_hits"].as_u64(),
            Some(1)
        );
        drop(restarted);
        let _ = std::fs::remove_dir_all(path);
    }

    #[test]
    fn should_maintain_exact_metadata_after_bulk_update() {
        // Arrange
        let _suite_query_scan_guard = cassie::midge::adapter::query_scan_control_test_guard();
        let (cassie, path) = fixture("time-series-lifecycle-metadata");
        let session = cassie.create_session("tester", None);

        // Act
        cassie
            .execute_sql(
                &session,
                "UPDATE ts_complete_events SET event_at = '2026-01-01T01:30:00Z' WHERE amount = 20",
                vec![],
            )
            .expect("update bucket membership");
        let metadata = manifest(&cassie);
        let counts = bucket_count_entries(&cassie);

        // Assert
        assert_eq!(metadata["version"].as_u64(), Some(1));
        assert_eq!(metadata["total_membership"].as_u64(), Some(3));
        assert_eq!(
            metadata["generation"].as_u64(),
            Some(
                cassie
                    .midge
                    .collection_generation(&canonical_collection(&cassie))
                    .expect("collection generation")
            )
        );
        assert_eq!(counts.len(), 2);
        drop(cassie);
        let _ = std::fs::remove_dir_all(path);
    }

    #[test]
    fn should_maintain_exact_metadata_after_bulk_delete() {
        // Arrange
        let _suite_query_scan_guard = cassie::midge::adapter::query_scan_control_test_guard();
        let (cassie, path) = fixture("time-series-lifecycle-delete-metadata");
        let session = cassie.create_session("tester", None);

        // Act
        cassie
            .execute_sql(
                &session,
                "DELETE FROM ts_complete_events WHERE amount = 30",
                vec![],
            )
            .expect("delete bucket membership");
        let metadata = manifest(&cassie);
        let counts = bucket_count_entries(&cassie);

        // Assert
        assert_eq!(metadata["version"].as_u64(), Some(1));
        assert_eq!(metadata["total_membership"].as_u64(), Some(2));
        assert_eq!(
            metadata["generation"].as_u64(),
            Some(
                cassie
                    .midge
                    .collection_generation(&canonical_collection(&cassie))
                    .expect("collection generation")
            )
        );
        assert_eq!(counts.len(), 1);
        drop(cassie);
        let _ = std::fs::remove_dir_all(path);
    }
}

// Formerly tests/time_series_indexes.rs.
mod time_series_indexes {
    #![allow(unused_imports, dead_code)]

    use cassie::app::Cassie;
    use cassie::catalog::IndexKind;
    use cassie::sql::ast::QueryStatement;
    use cassie::types::Value;
    use serde_json::json;

    use super::support_sql as support;
    use support::*;

    #[test]
    fn should_parse_time_series_index_options() {
        // Arrange
        let _suite_query_scan_guard = cassie::midge::adapter::query_scan_control_test_guard();
        let sql = "CREATE INDEX idx_events_time ON events USING time_series (event_at) WITH (bucket_width = '1 hour', partition_by = 'tenant,status')";

        // Act
        let parsed = cassie::sql::parse_statement(sql).unwrap();

        // Assert
        let QueryStatement::CreateIndex(statement) = parsed.statement else {
            panic!("expected CREATE INDEX");
        };
        assert_eq!(statement.kind, IndexKind::TimeSeries);
        assert_eq!(statement.fields, vec!["event_at"]);
        assert_eq!(
            statement.options.get("bucket_width"),
            Some(&"1 hour".to_string())
        );
        assert_eq!(
            statement.options.get("partition_by"),
            Some(&"tenant,status".to_string())
        );
    }

    #[test]
    fn should_select_time_series_index_for_timestamp_range_explain() {
        // Arrange
        let _suite_query_scan_guard = cassie::midge::adapter::query_scan_control_test_guard();
        use_local_storage();
        let path = data_dir("time_series_index_explain");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        cassie.startup().unwrap();
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE ts_events (tenant TEXT, event_at TIMESTAMP, amount INT)",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "CREATE INDEX idx_ts_events_time ON ts_events USING time_series (event_at) WITH (bucket_width = '1 hour', partition_by = tenant)",
                vec![],
            )
            .unwrap();

        // Act
        let explained = cassie
            .execute_sql(
                &session,
                "EXPLAIN SELECT tenant FROM ts_events WHERE event_at >= '2026-01-01T00:00:00Z'",
                vec![],
            )
            .unwrap();

        // Assert
        let plan = match &explained.rows[0][0] {
            cassie::types::Value::String(value) => value,
            other => panic!("expected explain string, got {other:?}"),
        };
        assert!(plan.contains(&format!(
            "index={}",
            canonical_test_index(&cassie, "ts_events", "idx_ts_events_time")
        )));
        assert!(plan.contains("time_series=bucket_width:1 hour"));
        assert!(plan.contains("time_series_storage=bucket-native-v1"));
        assert!(plan.contains("partition_by:tenant"));
        assert!(plan.contains("range_filter:true"));
        assert!(plan.contains("cost_model=v2"));

        let _ = std::fs::remove_dir_all(path);
    });
    }

    #[test]
    fn should_execute_timestamp_range_with_time_series_metrics() {
        // Arrange
        let _suite_query_scan_guard = cassie::midge::adapter::query_scan_control_test_guard();
        use_local_storage();
        let path = data_dir("time_series_index_execute");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        cassie.startup().unwrap();
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE ts_execute_events (tenant TEXT, event_at TIMESTAMP, amount INT)",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO ts_execute_events (tenant, event_at, amount) VALUES ('acme', '2026-01-01T00:00:00Z', 10), ('acme', '2026-01-01T01:00:00Z', 20), ('acme', '2026-01-02T00:00:00Z', 30)",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "CREATE INDEX idx_ts_execute_time ON ts_execute_events USING time_series (event_at) WITH (bucket_width = '1 hour', partition_by = tenant)",
                vec![],
            )
            .unwrap();

        // Act
        let result = cassie
            .execute_sql(
                &session,
                "SELECT tenant, amount FROM ts_execute_events WHERE event_at >= '2026-01-01T01:00:00Z' ORDER BY event_at",
                vec![],
            )
            .unwrap();
        let metrics = cassie.metrics();
        let sidecars =
            time_series_sidecar_records(&cassie, "ts_execute_events", "idx_ts_execute_time");
        let index_name = canonical_test_index(&cassie, "ts_execute_events", "idx_ts_execute_time");

        // Assert
        assert_eq!(
            result.rows,
            vec![
                vec![
                    cassie::types::Value::String("acme".to_string()),
                    cassie::types::Value::Int64(20),
                ],
                vec![
                    cassie::types::Value::String("acme".to_string()),
                    cassie::types::Value::Int64(30),
                ],
            ]
        );
        assert_eq!(sidecars.len(), 3);
        let generation = cassie
            .midge
            .collection_generation("ts_execute_events")
            .unwrap();
        assert!(sidecars
            .iter()
            .all(|record| record.generation == generation));
        assert_eq!(metrics["time_series"]["scans"].as_u64(), Some(1));
        assert_eq!(
            metrics["time_series"]["bucket_native_hits"].as_u64(),
            Some(1)
        );
        assert_eq!(metrics["time_series"]["rows"].as_u64(), Some(2));
        assert_eq!(metrics["time_series"]["buckets_scanned"].as_u64(), Some(2));
        assert_eq!(metrics["time_series"]["buckets_skipped"].as_u64(), Some(1));
        assert_eq!(
            metrics["time_series"]["last_index"].as_str(),
            Some(index_name.as_str())
        );

        let _ = std::fs::remove_dir_all(path);
    });
    }

    #[test]
    fn should_bulk_load_fresh_time_series_documents_for_bucket_reads() {
        // Arrange
        let _suite_query_scan_guard = cassie::midge::adapter::query_scan_control_test_guard();
        use_local_storage();
        let path = data_dir("time_series_fresh_bulk_load");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        cassie.startup().unwrap();
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE ts_fresh_events (tenant TEXT, event_at TIMESTAMP, amount INT)",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "CREATE INDEX idx_ts_fresh_time ON ts_fresh_events USING time_series (event_at) WITH (bucket_width = '1 hour', partition_by = tenant)",
                vec![],
            )
            .unwrap();
        cassie
            .midge
            .put_fresh_time_series_documents(
                &canonical_test_collection(&cassie, "ts_fresh_events"),
                vec![
                    (
                        Some("event-1".to_string()),
                        json!({
                            "tenant": "acme",
                            "event_at": "2026-01-01T00:00:00Z",
                            "amount": 10,
                        }),
                    ),
                    (
                        Some("event-2".to_string()),
                        json!({
                            "tenant": "acme",
                            "event_at": "2026-01-01T01:00:00Z",
                            "amount": 20,
                        }),
                    ),
                    (
                        Some("event-3".to_string()),
                        json!({
                            "tenant": "globex",
                            "event_at": "2026-01-01T01:00:00Z",
                            "amount": 30,
                        }),
                    ),
                ],
            )
            .unwrap();

        // Act
        let result = cassie
            .execute_sql(
                &session,
                "SELECT tenant, amount FROM ts_fresh_events WHERE event_at >= '2026-01-01T01:00:00Z' ORDER BY tenant, amount",
                vec![],
            )
            .unwrap();
        let sidecars = time_series_sidecar_records(
            &cassie,
            "ts_fresh_events",
            "idx_ts_fresh_time",
        );
        let metrics = cassie.metrics();

        // Assert
        assert_eq!(
            result.rows,
            vec![
                vec![Value::String("acme".to_string()), Value::Int64(20)],
                vec![Value::String("globex".to_string()), Value::Int64(30)],
            ]
        );
        assert_eq!(sidecars.len(), 3);
        assert_eq!(metrics["time_series"]["bucket_native_hits"].as_u64(), Some(1));

        let _ = std::fs::remove_dir_all(path);
    });
    }

    #[test]
    fn should_bound_partitioned_time_series_bucket_reads() {
        // Arrange
        let _suite_query_scan_guard = cassie::midge::adapter::query_scan_control_test_guard();
        use_local_storage();
        let path = data_dir("time_series_bounded_partition_reads");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        cassie.startup().unwrap();
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE ts_bounded_events (tenant TEXT, event_at TIMESTAMP, amount INT)",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO ts_bounded_events (tenant, event_at, amount) VALUES ('acme', '2026-01-01T00:00:00Z', 10), ('acme', '2026-01-01T01:00:00Z', 20), ('acme', '2026-01-01T02:00:00Z', 30), ('globex', '2026-01-01T01:00:00Z', 40), ('acme', '2026-01-01T03:00:00Z', 50)",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "CREATE INDEX idx_ts_bounded_time ON ts_bounded_events USING time_series (event_at) WITH (bucket_width = '1 hour', partition_by = tenant)",
                vec![],
            )
            .unwrap();
        let before = cassie.metrics();

        // Act
        let result = cassie
            .execute_sql(
                &session,
                "SELECT amount FROM ts_bounded_events WHERE tenant = 'acme' AND event_at >= '2026-01-01T01:00:00Z' AND event_at < '2026-01-01T03:00:00Z' ORDER BY event_at",
                vec![],
            )
            .unwrap();
        let after = cassie.metrics();

        // Assert
        assert_eq!(
            result.rows,
            vec![
                vec![cassie::types::Value::Int64(20)],
                vec![cassie::types::Value::Int64(30)],
            ]
        );
        assert_eq!(
            after["time_series"]["index_entries_scanned"].as_u64().unwrap()
                - before["time_series"]["index_entries_scanned"].as_u64().unwrap(),
            2
        );
        assert_eq!(
            after["time_series"]["row_point_fetches"].as_u64().unwrap()
                - before["time_series"]["row_point_fetches"].as_u64().unwrap(),
            2
        );

        let _ = std::fs::remove_dir_all(path);
    });
    }

    #[test]
    fn should_preserve_time_series_range_reads_after_mutations_restart() {
        // Arrange
        let _suite_query_scan_guard = cassie::midge::adapter::query_scan_control_test_guard();
        use_local_storage();
        let path = data_dir("time_series_index_mutations");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        cassie.startup().unwrap();
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE ts_mutation_events (tenant TEXT, event_at TIMESTAMP, amount INT)",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO ts_mutation_events (tenant, event_at, amount) VALUES ('acme', '2026-01-01T00:00:00Z', 10), ('acme', '2026-01-01T01:00:00Z', 20), ('acme', '2026-01-01T03:00:00Z', 30)",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "CREATE INDEX idx_ts_mutation_time ON ts_mutation_events USING time_series (event_at) WITH (bucket_width = '1 hour', partition_by = tenant)",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "UPDATE ts_mutation_events SET event_at = '2026-01-01T04:00:00Z' WHERE amount = 20",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "DELETE FROM ts_mutation_events WHERE amount = 30",
                vec![],
            )
            .unwrap();
        drop(cassie);

        let restarted = Cassie::new_with_data_dir(&path).unwrap();
        restarted.startup().unwrap();
        let restarted_session = restarted.create_session("tester", None);

        // Act
        let result = restarted
            .execute_sql(
                &restarted_session,
                "SELECT amount FROM ts_mutation_events WHERE event_at >= '2026-01-01T02:00:00Z' ORDER BY event_at",
                vec![],
            )
            .unwrap();
        let explain = restarted
            .execute_sql(
                &restarted_session,
                "EXPLAIN SELECT amount FROM ts_mutation_events WHERE event_at >= '2026-01-01T02:00:00Z' ORDER BY event_at",
                vec![],
            )
            .unwrap();
        let metrics = restarted.metrics();
        let sidecars = time_series_sidecar_records(
            &restarted,
            "ts_mutation_events",
            "idx_ts_mutation_time",
        );
        let index_name =
            canonical_test_index(&restarted, "ts_mutation_events", "idx_ts_mutation_time");

        // Assert
        assert_eq!(result.rows, vec![vec![cassie::types::Value::Int64(20)]]);
        let plan = match &explain.rows[0][0] {
            cassie::types::Value::String(value) => value,
            other => panic!("expected explain string, got {other:?}"),
        };
        assert!(plan.contains(&format!(
            "index={}",
            canonical_test_index(&restarted, "ts_mutation_events", "idx_ts_mutation_time")
        )));
        assert!(plan.contains("time_series=bucket_width:1 hour"));
        assert!(plan.contains("time_series_storage=bucket-native-v1"));
        assert_eq!(sidecars.len(), 2);
        assert_eq!(metrics["time_series"]["scans"].as_u64(), Some(1));
        assert_eq!(
            metrics["time_series"]["bucket_native_hits"].as_u64(),
            Some(1)
        );
        assert_eq!(
            metrics["time_series"]["last_index"].as_str(),
            Some(index_name.as_str())
        );

        let _ = std::fs::remove_dir_all(path);
    });
    }

    #[test]
    fn should_keep_time_series_sidecars_current_during_concurrent_rebuilds() {
        // Arrange
        let _suite_query_scan_guard = cassie::midge::adapter::query_scan_control_test_guard();
        use_local_storage();
        let path = data_dir("time_series_concurrent_rebuilds");
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        cassie.startup().unwrap();
        let session = cassie.create_session("tester", None);
        cassie
        .execute_sql(
            &session,
            "CREATE TABLE ts_concurrent_rebuild_events (id INT, tenant TEXT, event_at TIMESTAMP, amount INT)",
            vec![],
        )
        .unwrap();
        cassie
        .execute_sql(
            &session,
            "CREATE INDEX idx_ts_concurrent_rebuild_time ON ts_concurrent_rebuild_events USING time_series (event_at) WITH (bucket_width = '1 hour', partition_by = tenant)",
            vec![],
        )
        .unwrap();
        let cassie = std::sync::Arc::new(cassie);
        let barrier = std::sync::Arc::new(std::sync::Barrier::new(9));

        // Act
        let writers = (0..8)
        .map(|offset| {
            let cassie = std::sync::Arc::clone(&cassie);
            let barrier = std::sync::Arc::clone(&barrier);
            std::thread::spawn(move || {
                barrier.wait();
                let session = cassie.create_session("tester", None);
                cassie
                    .execute_sql(
                        &session,
                        &format!(
                            "INSERT INTO ts_concurrent_rebuild_events (id, tenant, event_at, amount) VALUES ({offset}, 'acme', '2026-01-01T{offset:02}:00:00Z', {offset})"
                        ),
                        vec![],
                    )
                    .unwrap();
            })
        })
        .collect::<Vec<_>>();
        barrier.wait();
        for writer in writers {
            writer.join().unwrap();
        }
        let sidecars = time_series_sidecar_records(
            cassie.as_ref(),
            "ts_concurrent_rebuild_events",
            "idx_ts_concurrent_rebuild_time",
        );
        let result = cassie
            .execute_sql(
                &cassie.create_session("tester", None),
                "SELECT count(*) FROM ts_concurrent_rebuild_events",
                vec![],
            )
            .unwrap();

        // Assert
        assert_eq!(result.rows, vec![vec![Value::Int64(8)]]);
        assert_eq!(sidecars.len(), 8);

        let _ = std::fs::remove_dir_all(path);
    }

    #[test]
    fn should_fallback_to_row_blobs_when_bucket_membership_is_missing() {
        // Arrange
        let _suite_query_scan_guard = cassie::midge::adapter::query_scan_control_test_guard();
        use_local_storage();
        let path = data_dir("time_series_index_missing_sidecar");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        cassie.startup().unwrap();
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE ts_missing_bucket_events (tenant TEXT, event_at TIMESTAMP, amount INT)",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO ts_missing_bucket_events (tenant, event_at, amount) VALUES ('acme', '2026-01-01T00:00:00Z', 10), ('acme', '2026-01-01T01:00:00Z', 20)",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "CREATE INDEX idx_ts_missing_bucket_time ON ts_missing_bucket_events USING time_series (event_at) WITH (bucket_width = '1 hour', partition_by = tenant)",
                vec![],
            )
            .unwrap();
        clear_time_series_sidecars(
            &cassie,
            "ts_missing_bucket_events",
            "idx_ts_missing_bucket_time",
        );

        // Act
        let result = cassie
            .execute_sql(
                &session,
                "SELECT amount FROM ts_missing_bucket_events WHERE event_at >= '2026-01-01T01:00:00Z' ORDER BY event_at",
                vec![],
            )
            .unwrap();
        let metrics = cassie.metrics();

        // Assert
        assert_eq!(result.rows, vec![vec![Value::Int64(20)]]);
        assert_eq!(
            metrics["time_series"]["bucket_native_hits"].as_u64(),
            Some(0)
        );
        assert_eq!(metrics["time_series"]["fallback_scans"].as_u64(), Some(1));
        assert_eq!(
            metrics["time_series"]["last_fallback_reason"].as_str(),
            Some("incomplete-bucket-membership")
        );

        let _ = std::fs::remove_dir_all(path);
    });
    }

    #[test]
    fn should_cleanup_bucket_membership_after_retention() {
        // Arrange
        let _suite_query_scan_guard = cassie::midge::adapter::query_scan_control_test_guard();
        use_local_storage();
        let path = data_dir("time_series_index_retention_sidecar");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        cassie.startup().unwrap();
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE ts_retention_bucket_events (tenant TEXT, event_at TIMESTAMP, amount INT)",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO ts_retention_bucket_events (tenant, event_at, amount) VALUES ('acme', '2026-01-01T00:00:00Z', 10), ('acme', '2026-01-03T00:00:00Z', 20)",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "CREATE INDEX idx_ts_retention_bucket_time ON ts_retention_bucket_events USING time_series (event_at) WITH (bucket_width = '1 hour', partition_by = tenant)",
                vec![],
            )
            .unwrap();
        let sidecars_before = time_series_sidecar_records(
            &cassie,
            "ts_retention_bucket_events",
            "idx_ts_retention_bucket_time",
        );
        cassie
            .execute_sql(
                &session,
                "CREATE RETENTION POLICY ts_retention_bucket_policy ON ts_retention_bucket_events USING event_at RETAIN FOR '1 day'",
                vec![],
            )
            .unwrap();

        // Act
        cassie
            .execute_sql(
                &session,
                "ENFORCE RETENTION POLICY ts_retention_bucket_policy AT '2026-01-03T12:00:00Z'",
                vec![],
            )
            .unwrap();
        let sidecars_after = time_series_sidecar_records(
            &cassie,
            "ts_retention_bucket_events",
            "idx_ts_retention_bucket_time",
        );
        let result = cassie
            .execute_sql(
                &session,
                "SELECT amount FROM ts_retention_bucket_events WHERE event_at >= '2026-01-01T00:00:00Z' ORDER BY event_at",
                vec![],
            )
            .unwrap();
        let metrics = cassie.metrics();

        // Assert
        assert_eq!(sidecars_before.len(), 2);
        assert_eq!(sidecars_after.len(), 1);
        assert_eq!(result.rows, vec![vec![Value::Int64(20)]]);
        assert_eq!(
            metrics["time_series"]["bucket_native_hits"].as_u64(),
            Some(1)
        );

        let _ = std::fs::remove_dir_all(path);
    });
    }

    #[test]
    fn should_reject_time_series_index_on_non_timestamp_field() {
        // Arrange
        let _suite_query_scan_guard = cassie::midge::adapter::query_scan_control_test_guard();
        use_local_storage();
        let path = data_dir("time_series_index_invalid_type");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
            let cassie = Cassie::new_with_data_dir(&path).unwrap();
            cassie.startup().unwrap();
            let session = cassie.create_session("tester", None);
            cassie
                .execute_sql(
                    &session,
                    "CREATE TABLE ts_bad_events (event_at TEXT)",
                    vec![],
                )
                .unwrap();

            // Act
            let error = cassie
            .execute_sql(
                &session,
                "CREATE INDEX idx_ts_bad_events_time ON ts_bad_events USING time_series (event_at)",
                vec![],
            )
            .unwrap_err();

            // Assert
            assert!(error.to_string().contains("requires timestamp field"));

            let _ = std::fs::remove_dir_all(path);
        });
    }

    #[test]
    fn should_prune_parameterized_time_series_ranges() {
        // Arrange
        let _suite_query_scan_guard = cassie::midge::adapter::query_scan_control_test_guard();
        use_local_storage();
        let path = data_dir("time_series_parameterized_controls");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).expect("cassie");
        cassie.startup().expect("startup");
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE ts_control_events (tenant TEXT, event_at TIMESTAMP, amount INT)",
                vec![],
            )
            .expect("table");
        for (tenant, event_at, amount) in [
            ("acme", "1969-12-31T23:00:00Z", 10_i64),
            ("acme", "1970-01-01T00:00:00Z", 20_i64),
            ("globex", "1970-01-01T01:00:00Z", 30_i64),
        ] {
            cassie
                .execute_sql(
                    &session,
                    "INSERT INTO ts_control_events (tenant, event_at, amount) VALUES ($1, $2, $3)",
                    vec![
                        Value::String(tenant.to_string()),
                        Value::String(event_at.to_string()),
                        Value::Int64(amount),
                    ],
                )
                .expect("insert");
        }
        cassie
            .execute_sql(
                &session,
                "CREATE INDEX idx_ts_control_events ON ts_control_events USING time_series (event_at) WITH (bucket_width = '1 hour', partition_by = tenant)",
                vec![],
            )
            .expect("index");

        // Act
        let result = cassie
            .execute_sql(
                &session,
                "SELECT amount FROM ts_control_events WHERE tenant = $1 AND event_at >= $2 AND event_at < $3 ORDER BY event_at",
                vec![
                    Value::String("acme".to_string()),
                    Value::String("1969-12-31T23:30:00Z".to_string()),
                    Value::String("1970-01-01T00:30:00Z".to_string()),
                ],
            )
            .expect("parameterized range");
        let cancellation = cassie::runtime::QueryCancellationHandle::new();
        cancellation.cancel();
        let cancelled = cassie.execute_sql_with_cancellation(
            &session,
            "SELECT amount FROM ts_control_events WHERE tenant = $1 AND event_at >= $2",
            vec![
                Value::String("acme".to_string()),
                Value::String("1969-12-31T00:00:00Z".to_string()),
            ],
            &cancellation,
        );
        let metrics = cassie.metrics();

        // Assert
        assert_eq!(result.rows, vec![vec![Value::Int64(20)]]);
        assert_eq!(metrics["time_series"]["bucket_native_hits"].as_u64(), Some(1));
        assert!(cancelled.expect_err("cancelled query").to_string().contains("canceled"));

        let _ = std::fs::remove_dir_all(path);
    });
    }
}

// Formerly tests/time_series_retention.rs.
mod time_series_retention {
    use cassie::app::Cassie;
    use cassie::catalog::canonical_relation_name;
    use cassie::sql::ast::QueryStatement;
    use cassie::sql::parse_statement;
    use cassie::types::Value;

    use super::support_sql as support;
    use support::*;

    fn canonical_name(name: &str) -> String {
        canonical_relation_name("postgres", "public", name)
    }

    fn runtime() -> tokio::runtime::Runtime {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime")
    }

    #[test]
    fn should_parse_retention_policy_commands() {
        // Arrange
        let _suite_query_scan_guard = cassie::midge::adapter::query_scan_control_test_guard();
        let create = "CREATE RETENTION POLICY IF NOT EXISTS events_retention ON events USING event_at RETAIN FOR '7 days'";
        let alter = "ALTER RETENTION POLICY events_retention RETAIN FOR '2 days'";
        let enforce = "ENFORCE RETENTION POLICY events_retention AT '2026-01-10T00:00:00Z'";
        let drop = "DROP RETENTION POLICY IF EXISTS events_retention";

        // Act
        let create = parse_statement(create).expect("create retention policy parses");
        let alter = parse_statement(alter).expect("alter retention policy parses");
        let enforce = parse_statement(enforce).expect("enforce retention policy parses");
        let drop = parse_statement(drop).expect("drop retention policy parses");

        // Assert
        assert!(matches!(
            create.statement,
            QueryStatement::CreateRetentionPolicy(_)
        ));
        assert!(matches!(
            alter.statement,
            QueryStatement::AlterRetentionPolicy(_)
        ));
        assert!(matches!(
            enforce.statement,
            QueryStatement::EnforceRetentionPolicy(_)
        ));
        assert!(matches!(
            drop.statement,
            QueryStatement::DropRetentionPolicy(_)
        ));
    }

    #[test]
    fn should_lifecycle_retention_policy_metadata() {
        // Arrange
        let _suite_query_scan_guard = cassie::midge::adapter::query_scan_control_test_guard();
        use_local_storage();
        let path = data_dir("retention_catalog");

        runtime().block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        cassie.startup().unwrap();
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE retention_catalog_events (event_at TEXT, kind TEXT)",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "CREATE RETENTION POLICY retention_catalog_policy ON retention_catalog_events USING event_at RETAIN FOR '7 days'",
                vec![],
            )
            .unwrap();

        // Act
        cassie
            .execute_sql(
                &session,
                "ALTER RETENTION POLICY retention_catalog_policy RETAIN FOR '2 days'",
                vec![],
            )
            .unwrap();
        drop(cassie);
        let restarted = Cassie::new_with_data_dir(&path).unwrap();
        restarted.startup().unwrap();
        let restarted_session = restarted.create_session("tester", None);
        let policy_name = canonical_name("retention_catalog_policy");
        let policies = restarted
            .execute_sql(
                &restarted_session,
                &format!(
                    "SELECT policy_name, retention_duration, state FROM pg_catalog.pg_retention_policies WHERE policy_name = '{policy_name}'"
                ),
                vec![],
            )
            .unwrap();
        restarted
            .execute_sql(
                &restarted_session,
                "DROP RETENTION POLICY retention_catalog_policy",
                vec![],
            )
            .unwrap();
        let dropped = restarted
            .execute_sql(
                &restarted_session,
                &format!(
                    "SELECT policy_name FROM pg_catalog.pg_retention_policies WHERE policy_name = '{policy_name}'"
                ),
                vec![],
            )
            .unwrap();

        // Assert
        assert_eq!(
            policies.rows,
            vec![vec![
                Value::String(policy_name),
                Value::String("2 days".to_string()),
                Value::String("ready".to_string()),
            ]]
        );
        assert!(dropped.rows.is_empty());

        let _ = std::fs::remove_dir_all(path);
    });
    }

    #[test]
    fn should_enforce_retention_idempotently() {
        // Arrange
        let _suite_query_scan_guard = cassie::midge::adapter::query_scan_control_test_guard();
        use_local_storage();
        let path = data_dir("retention_enforce");

        runtime().block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        cassie.startup().unwrap();
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE retention_enforce_events (event_at TEXT, kind TEXT)",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "CREATE INDEX retention_enforce_kind_idx ON retention_enforce_events (kind)",
                vec![],
            )
            .unwrap();
        for sql in [
            "INSERT INTO retention_enforce_events (event_at, kind) VALUES ('2026-01-01T00:00:00Z', 'old')",
            "INSERT INTO retention_enforce_events (event_at, kind) VALUES ('2026-01-02T12:00:00Z', 'fresh')",
            "INSERT INTO retention_enforce_events (event_at, kind) VALUES ('not-a-time', 'bad')",
            "INSERT INTO retention_enforce_events (event_at, kind) VALUES (NULL, 'missing')",
        ] {
            cassie.execute_sql(&session, sql, vec![]).unwrap();
        }
        cassie
            .execute_sql(
                &session,
                "CREATE RETENTION POLICY retention_enforce_policy ON retention_enforce_events USING event_at RETAIN FOR '1 day'",
                vec![],
            )
            .unwrap();

        // Act
        let first = cassie
            .execute_sql(
                &session,
                "ENFORCE RETENTION POLICY retention_enforce_policy AT '2026-01-03T00:00:00Z'",
                vec![],
            )
            .unwrap();
        let second = cassie
            .execute_sql(
                &session,
                "ENFORCE RETENTION POLICY retention_enforce_policy AT '2026-01-03T00:00:00Z'",
                vec![],
            )
            .unwrap();
        let rows = cassie
            .execute_sql(
                &session,
                "SELECT kind FROM retention_enforce_events ORDER BY kind",
                vec![],
            )
            .unwrap();
        let indexed = cassie
            .execute_sql(
                &session,
                "SELECT kind FROM retention_enforce_events WHERE kind = 'old'",
                vec![],
            )
            .unwrap();
        let metrics = cassie.metrics();
        let policy_name = canonical_name("retention_enforce_policy");
        let policies = cassie
            .execute_sql(
                &session,
                &format!(
                    "SELECT last_deleted_rows, last_skipped_rows FROM pg_catalog.pg_retention_policies WHERE policy_name = '{policy_name}'"
                ),
                vec![],
            )
            .unwrap();

        // Assert
        assert_eq!(first.command, "ENFORCE RETENTION 1");
        assert_eq!(second.command, "ENFORCE RETENTION 0");
        assert_eq!(
            rows.rows,
            vec![
                vec![Value::String("bad".to_string())],
                vec![Value::String("fresh".to_string())],
                vec![Value::String("missing".to_string())],
            ]
        );
        assert!(indexed.rows.is_empty());
        assert_eq!(metrics["retention"]["enforcements"].as_u64(), Some(2));
        assert_eq!(metrics["retention"]["deleted_rows"].as_u64(), Some(1));
        assert_eq!(metrics["retention"]["skipped_rows"].as_u64(), Some(4));
        assert_eq!(
            policies.rows,
            vec![vec![Value::Int64(0), Value::Int64(2)]]
        );

        let _ = std::fs::remove_dir_all(path);
    });
    }

    #[test]
    fn should_mark_materialized_projection_stale_after_retention() {
        // Arrange
        let _suite_query_scan_guard = cassie::midge::adapter::query_scan_control_test_guard();
        use_local_storage();
        let path = data_dir("retention_projection_stale");

        runtime().block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        cassie.startup().unwrap();
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE retention_projection_events (event_at TEXT, title TEXT)",
                vec![],
            )
            .unwrap();
        for sql in [
            "INSERT INTO retention_projection_events (event_at, title) VALUES ('2026-01-01T00:00:00Z', 'old')",
            "INSERT INTO retention_projection_events (event_at, title) VALUES ('2026-01-02T12:00:00Z', 'fresh')",
        ] {
            cassie.execute_sql(&session, sql, vec![]).unwrap();
        }
        cassie
            .execute_sql(
                &session,
                "CREATE MATERIALIZED PROJECTION retention_projection_view AS SELECT title FROM retention_projection_events",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "CREATE RETENTION POLICY retention_projection_policy ON retention_projection_events USING event_at RETAIN FOR '1 day'",
                vec![],
            )
            .unwrap();

        // Act
        cassie
            .execute_sql(
                &session,
                "ENFORCE RETENTION POLICY retention_projection_policy AT '2026-01-03T00:00:00Z'",
                vec![],
            )
            .unwrap();
        let projection_name = canonical_name("retention_projection_view");
        let operations = cassie
            .execute_sql(
                &session,
                &format!(
                    "SELECT freshness, rebuild_state, verification_state FROM pg_catalog.pg_projection_operations WHERE projection_name = '{projection_name}'"
                ),
                vec![],
            )
            .unwrap();

        // Assert
        assert_eq!(
            operations.rows,
            vec![vec![
                Value::String("stale".to_string()),
                Value::String("idle".to_string()),
                Value::String("pending".to_string()),
            ]]
        );

        let _ = std::fs::remove_dir_all(path);
    });
    }

    #[test]
    fn should_refresh_rollup_after_retention_enforcement() {
        // Arrange
        let _suite_query_scan_guard = cassie::midge::adapter::query_scan_control_test_guard();
        use_local_storage();
        let path = data_dir("retention_rollup");

        runtime().block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        cassie.startup().unwrap();
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE retention_rollup_events (tenant TEXT, event_at TEXT, amount INT)",
                vec![],
            )
            .unwrap();
        for sql in [
            "INSERT INTO retention_rollup_events (tenant, event_at, amount) VALUES ('a', '2026-01-01T00:05:00Z', 7)",
            "INSERT INTO retention_rollup_events (tenant, event_at, amount) VALUES ('a', '2026-01-02T12:00:00Z', 5)",
        ] {
            cassie.execute_sql(&session, sql, vec![]).unwrap();
        }
        cassie
            .execute_sql(
                &session,
                "CREATE ROLLUP retention_rollup_hourly ON retention_rollup_events USING time_bucket('1 hour', event_at) GROUP BY tenant AGGREGATES COUNT(*) AS total, SUM(amount) AS amount_sum",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "CREATE RETENTION POLICY retention_rollup_policy ON retention_rollup_events USING event_at RETAIN FOR '1 day'",
                vec![],
            )
            .unwrap();

        // Act
        cassie
            .execute_sql(
                &session,
                "ENFORCE RETENTION POLICY retention_rollup_policy AT '2026-01-03T00:00:00Z'",
                vec![],
            )
            .unwrap();
        let rollup = cassie
            .execute_sql(
                &session,
                "SELECT time_bucket('1 hour', event_at) AS bucket, tenant, COUNT(*) AS total, SUM(amount) AS amount_sum FROM retention_rollup_events GROUP BY time_bucket('1 hour', event_at), tenant ORDER BY bucket, tenant",
                vec![],
            )
            .unwrap();

        // Assert
        assert_eq!(
            rollup.rows,
            vec![vec![
                Value::String("2026-01-02T12:00:00Z".to_string()),
                Value::String("a".to_string()),
                Value::Int64(1),
                Value::Int64(5),
            ]]
        );

        let _ = std::fs::remove_dir_all(path);
    });
    }
}

// Formerly tests/time_series_rollups.rs.
mod time_series_rollups {
    use cassie::app::Cassie;
    use cassie::catalog::{canonical_relation_name, RollupState};
    use cassie::midge::adapter::set_rollup_maintenance_failure_point;
    use cassie::types::Value;

    static ROLLUP_FAILPOINT_GUARD: std::sync::Mutex<()> = std::sync::Mutex::new(());

    use super::support_sql as support;
    use support::*;

    fn canonical_name(name: &str) -> String {
        canonical_relation_name("postgres", "public", name)
    }

    fn runtime() -> tokio::runtime::Runtime {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime")
    }

    fn seed_events(cassie: &Cassie, session: &cassie::app::CassieSession, table: &str) {
        cassie
            .execute_sql(
                session,
                &format!("CREATE TABLE {table} (tenant TEXT, event_at TEXT, amount INT)"),
                vec![],
            )
            .unwrap();
        for sql in [
            format!(
            "INSERT INTO {table} (tenant, event_at, amount) VALUES ('a', '2026-01-01T00:05:00Z', 7)"
        ),
            format!(
            "INSERT INTO {table} (tenant, event_at, amount) VALUES ('a', '2026-01-01T00:25:00Z', 5)"
        ),
            format!(
            "INSERT INTO {table} (tenant, event_at, amount) VALUES ('b', '2026-01-01T01:05:00Z', 3)"
        ),
        ] {
            cassie.execute_sql(session, &sql, vec![]).unwrap();
        }
    }

    fn create_hourly_rollup(cassie: &Cassie, session: &cassie::app::CassieSession, table: &str) {
        cassie
        .execute_sql(
            session,
            &format!(
                "CREATE ROLLUP {table}_hourly ON {table} USING time_bucket('1 hour', event_at) GROUP BY tenant AGGREGATES COUNT(*) AS total, SUM(amount) AS amount_sum"
            ),
            vec![],
        )
        .unwrap();
    }

    fn hourly_query(table: &str) -> String {
        format!(
        "SELECT time_bucket('1 hour', event_at) AS bucket, tenant, COUNT(*) AS total, SUM(amount) AS amount_sum FROM {table} GROUP BY time_bucket('1 hour', event_at), tenant ORDER BY bucket, tenant"
    )
    }

    #[test]
    fn should_rewrite_query_after_rollup_creation() {
        // Arrange
        let _suite_query_scan_guard = cassie::midge::adapter::query_scan_control_test_guard();
        use_local_storage();
        let _rollup_guard = ROLLUP_FAILPOINT_GUARD.lock().unwrap();
        let path = data_dir("rollup_rewrite");

        runtime().block_on(async {
            let cassie = Cassie::new_with_data_dir(&path).unwrap();
            cassie.startup().unwrap();
            let session = cassie.create_session("tester", None);
            seed_events(&cassie, &session, "rollup_rewrite_events");
            create_hourly_rollup(&cassie, &session, "rollup_rewrite_events");

            // Act
            let selected = cassie
                .execute_sql(&session, &hourly_query("rollup_rewrite_events"), vec![])
                .unwrap();
            let explain = cassie
                .execute_sql(
                    &session,
                    &format!("EXPLAIN {}", hourly_query("rollup_rewrite_events")),
                    vec![],
                )
                .unwrap();
            let metrics = cassie.metrics();

            // Assert
            let rollup_name = canonical_name("rollup_rewrite_events_hourly");
            assert_eq!(
                selected.rows,
                vec![
                    vec![
                        Value::String("2026-01-01T00:00:00Z".to_string()),
                        Value::String("a".to_string()),
                        Value::Int64(2),
                        Value::Int64(12)
                    ],
                    vec![
                        Value::String("2026-01-01T01:00:00Z".to_string()),
                        Value::String("b".to_string()),
                        Value::Int64(1),
                        Value::Int64(3)
                    ],
                ]
            );
            assert_eq!(metrics["rollups"]["rewrite_hits"].as_u64(), Some(1));
            assert!(matches!(
                &explain.rows[0][0],
                Value::String(plan) if plan.contains(&format!("rollup_rewrite={rollup_name}"))
            ));
            assert!(cassie.catalog.get_rollup(&rollup_name).is_some());

            let _ = std::fs::remove_dir_all(path);
        });
    }

    #[test]
    fn should_reject_rollup_with_mismatched_source_generation() {
        // Arrange
        let _suite_query_scan_guard = cassie::midge::adapter::query_scan_control_test_guard();
        use_local_storage();
        let path = data_dir("rollup_generation_fence");

        runtime().block_on(async {
            let cassie = Cassie::new_with_data_dir(&path).unwrap();
            cassie.startup().unwrap();
            let session = cassie.create_session("tester", None);
            seed_events(&cassie, &session, "rollup_generation_events");
            create_hourly_rollup(&cassie, &session, "rollup_generation_events");
            let mut rollup = cassie
                .catalog
                .get_rollup(&canonical_name("rollup_generation_events_hourly"))
                .expect("rollup metadata");
            rollup.refresh_cursor.source_generation = 0;
            cassie.midge.put_rollup(&rollup).unwrap();
            cassie.catalog.register_rollup(rollup);

            // Act
            let result = cassie
                .execute_sql(&session, &hourly_query("rollup_generation_events"), vec![])
                .unwrap();
            let metrics = cassie.metrics();

            // Assert
            assert_eq!(result.rows.len(), 2);
            assert_eq!(metrics["rollups"]["rewrite_hits"].as_u64(), Some(0));

            let _ = std::fs::remove_dir_all(path);
        });
    }

    #[test]
    fn should_refresh_rollup_for_dml_movement() {
        // Arrange
        let _suite_query_scan_guard = cassie::midge::adapter::query_scan_control_test_guard();
        use_local_storage();
        let _rollup_guard = ROLLUP_FAILPOINT_GUARD.lock().unwrap();
        let path = data_dir("rollup_dml");

        runtime().block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        cassie.startup().unwrap();
        let session = cassie.create_session("tester", None);
        seed_events(&cassie, &session, "rollup_dml_events");
        create_hourly_rollup(&cassie, &session, "rollup_dml_events");

        // Act
        cassie
            .execute_sql(
                &session,
                "INSERT INTO rollup_dml_events (tenant, event_at, amount) VALUES ('a', '2026-01-01T01:30:00Z', 11)",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "UPDATE rollup_dml_events SET event_at = '2026-01-01T02:05:00Z' WHERE amount = 11",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "DELETE FROM rollup_dml_events WHERE tenant = 'b'",
                vec![],
            )
            .unwrap();
        let selected = cassie
            .execute_sql(&session, &hourly_query("rollup_dml_events"), vec![])
            .unwrap();

        // Assert
        assert_eq!(
            selected.rows,
            vec![
                vec![
                    Value::String("2026-01-01T00:00:00Z".to_string()),
                    Value::String("a".to_string()),
                    Value::Int64(2),
                    Value::Int64(12)
                ],
                vec![
                    Value::String("2026-01-01T02:00:00Z".to_string()),
                    Value::String("a".to_string()),
                    Value::Int64(1),
                    Value::Int64(11)
                ],
            ]
        );

        let _ = std::fs::remove_dir_all(path);
    });
    }

    #[test]
    fn should_cleanup_rollup_after_restart_drop() {
        // Arrange
        let _suite_query_scan_guard = cassie::midge::adapter::query_scan_control_test_guard();
        use_local_storage();
        let _rollup_guard = ROLLUP_FAILPOINT_GUARD.lock().unwrap();
        let path = data_dir("rollup_restart");

        runtime().block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        cassie.startup().unwrap();
        let session = cassie.create_session("tester", None);
        seed_events(&cassie, &session, "rollup_restart_events");
        create_hourly_rollup(&cassie, &session, "rollup_restart_events");
        drop(cassie);

        let restarted = Cassie::new_with_data_dir(&path).unwrap();
        restarted.startup().unwrap();
        let session = restarted.create_session("tester", None);
        let rollup_name = canonical_name("rollup_restart_events_hourly");

        // Act
        let catalog_rows = restarted
            .execute_sql(
                &session,
                &format!(
                    "SELECT rollup_name, state FROM pg_catalog.pg_rollups WHERE rollup_name = '{rollup_name}'"
                ),
                vec![],
            )
            .unwrap();
        restarted
            .execute_sql(&session, "DROP ROLLUP rollup_restart_events_hourly", vec![])
            .unwrap();

        // Assert
        assert_eq!(
            catalog_rows.rows,
            vec![vec![
                Value::String(rollup_name.clone()),
                Value::String("ready".to_string())
            ]]
        );
        assert!(restarted.catalog.get_rollup(&rollup_name).is_none());

        let _ = std::fs::remove_dir_all(path);
    });
    }

    #[test]
    fn should_fallback_to_source_when_rollup_is_stale() {
        // Arrange
        let _suite_query_scan_guard = cassie::midge::adapter::query_scan_control_test_guard();
        use_local_storage();
        let _rollup_guard = ROLLUP_FAILPOINT_GUARD.lock().unwrap();
        let path = data_dir("rollup_stale");

        runtime().block_on(async {
            let cassie = Cassie::new_with_data_dir(&path).unwrap();
            cassie.startup().unwrap();
            let session = cassie.create_session("tester", None);
            seed_events(&cassie, &session, "rollup_stale_events");
            create_hourly_rollup(&cassie, &session, "rollup_stale_events");
            let mut meta = cassie
                .catalog
                .get_rollup("rollup_stale_events_hourly")
                .unwrap();
            meta.state = RollupState::Stale;
            meta.refresh_cursor.lag_rows = 1;
            cassie.midge.put_rollup(&meta).unwrap();
            cassie.catalog.register_rollup(meta);

            // Act
            let selected = cassie
                .execute_sql(&session, &hourly_query("rollup_stale_events"), vec![])
                .unwrap();
            let metrics = cassie.metrics();

            // Assert
            assert_eq!(selected.rows.len(), 2);
            assert_eq!(metrics["rollups"]["stale_fallbacks"].as_u64(), Some(1));

            let _ = std::fs::remove_dir_all(path);
        });
    }

    fn inject_rollup_refresh_failure(
        cassie: &Cassie,
        session: &cassie::app::CassieSession,
        table: &str,
    ) {
        set_rollup_maintenance_failure_point(true);
        let inserted = cassie
        .execute_sql(
            session,
            &format!(
                "INSERT INTO {table} (tenant, event_at, amount) VALUES ('a', '2026-01-01T01:30:00Z', 11)"
            ),
            vec![],
        )
        .unwrap();
        assert_eq!(inserted.command, "INSERT 0 1");
    }

    #[test]
    fn should_record_rollup_debt_after_refresh_failure() {
        // Arrange
        let _suite_query_scan_guard = cassie::midge::adapter::query_scan_control_test_guard();
        use_local_storage();
        let _failpoint_guard = ROLLUP_FAILPOINT_GUARD.lock().unwrap();
        let path = data_dir("rollup_maintenance_debt");

        runtime().block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        cassie.startup().unwrap();
        let session = cassie.create_session("tester", None);
        seed_events(&cassie, &session, "rollup_maintenance_events");
        create_hourly_rollup(&cassie, &session, "rollup_maintenance_events");
        inject_rollup_refresh_failure(&cassie, &session, "rollup_maintenance_events");

        // Act
        let source_rows = cassie
            .execute_sql(
                &session,
                &hourly_query("rollup_maintenance_events"),
                vec![],
            )
            .unwrap();
        let debt = cassie
            .execute_sql(
                &session,
                "SELECT artifact, target_generation, retry_count, last_error, fallback_reason FROM pg_catalog.pg_maintenance_debt WHERE collection = 'postgres.public.rollup_maintenance_events'",
                vec![],
            )
            .unwrap();

        // Assert
        assert_eq!(source_rows.rows.len(), 3, "{:?}", source_rows.rows);
        assert_eq!(source_rows.rows[0][2], Value::Int64(2));
        assert_eq!(source_rows.rows[0][3], Value::Int64(12));
        assert_eq!(debt.rows.len(), 1);
        assert_eq!(
            debt.rows[0],
            vec![
                Value::String("rollup".to_string()),
                Value::Int64(4),
                Value::Int64(1),
                Value::String("rollup maintenance failed (details redacted)".to_string()),
                Value::String("maintenance_pending".to_string()),
            ]
        );
        assert_eq!(
            cassie.metrics()["rollups"]["last_fallback_reason"].as_str(),
            Some("maintenance_pending")
        );

        let _ = std::fs::remove_dir_all(path);
    });
    }

    #[test]
    fn should_retry_rollup_debt_on_startup() {
        // Arrange
        let _suite_query_scan_guard = cassie::midge::adapter::query_scan_control_test_guard();
        use_local_storage();
        let _failpoint_guard = ROLLUP_FAILPOINT_GUARD.lock().unwrap();
        let path = data_dir("rollup_maintenance_restart");

        runtime().block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        cassie.startup().unwrap();
        let session = cassie.create_session("tester", None);
        seed_events(&cassie, &session, "rollup_restart_maintenance_events");
        create_hourly_rollup(&cassie, &session, "rollup_restart_maintenance_events");
        inject_rollup_refresh_failure(
            &cassie,
            &session,
            "rollup_restart_maintenance_events",
        );

        drop(cassie);

        // Act
        let restarted = Cassie::new_with_data_dir(&path).unwrap();
        restarted.startup().unwrap();
        let restarted_session = restarted.create_session("tester", None);
        let recovered = restarted
            .execute_sql(
                &restarted_session,
                &hourly_query("rollup_restart_maintenance_events"),
                vec![],
            )
            .unwrap();
        let remaining_debt = restarted
            .execute_sql(
                &restarted_session,
                "SELECT artifact FROM pg_catalog.pg_maintenance_debt WHERE collection = 'postgres.public.rollup_restart_maintenance_events'",
                vec![],
            )
            .unwrap();

        // Assert
        assert_eq!(recovered.rows.len(), 3);
        assert_eq!(recovered.rows[0][2], Value::Int64(2));
        assert_eq!(recovered.rows[0][3], Value::Int64(12));
        assert!(remaining_debt.rows.is_empty());

        let _ = std::fs::remove_dir_all(path);
    });
    }

    #[test]
    fn should_move_rollup_debt_with_collection_rename() {
        // Arrange
        let _suite_query_scan_guard = cassie::midge::adapter::query_scan_control_test_guard();
        use_local_storage();
        let _failpoint_guard = ROLLUP_FAILPOINT_GUARD.lock().unwrap();
        let path = data_dir("rollup_rename_debt");

        runtime().block_on(async {
            let cassie = Cassie::new_with_data_dir(&path).unwrap();
            cassie.startup().unwrap();
            let session = cassie.create_session("tester", None);
            seed_events(&cassie, &session, "rollup_rename_events");
            create_hourly_rollup(&cassie, &session, "rollup_rename_events");
            inject_rollup_refresh_failure(&cassie, &session, "rollup_rename_events");
            assert!(cassie
                .midge
                .has_rollup_maintenance_debt("rollup_rename_events")
                .unwrap());

            // Act
            cassie
                .midge
                .rename_collection("rollup_rename_events", "rollup_renamed_events")
                .unwrap();

            // Assert
            assert!(!cassie
                .midge
                .has_rollup_maintenance_debt("rollup_rename_events")
                .unwrap());
            assert!(cassie
                .midge
                .has_rollup_maintenance_debt("rollup_renamed_events")
                .unwrap());

            let _ = std::fs::remove_dir_all(path);
        });
    }
}
