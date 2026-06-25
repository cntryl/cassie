#![allow(unused_imports, dead_code)]

use cassie::app::Cassie;
use cassie::types::Value;
use serde_json::json;

#[path = "support/sql.rs"]
mod support;
use support::*;

#[test]
fn should_execute_graph_neighbors_weighted_shortest_path() {
    // Arrange
    with_fallback();
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
    with_fallback();
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
    with_fallback();
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
    with_fallback();
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
