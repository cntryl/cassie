use super::support_graph as support;
use support::*;

fn insert_edge(cassie: &Cassie, session: &cassie::app::CassieSession, edge: &str) {
    execute(
            cassie,
            session,
            &format!(
                "INSERT INTO social_edges (edge_id, source_type, source_id, target_type, target_id, edge_type, weight) VALUES ({edge})"
            ),
        );
}

fn query(cassie: &Cassie, session: &cassie::app::CassieSession, sql: &str) -> Vec<Vec<Value>> {
    cassie
        .execute_sql(session, sql, vec![])
        .expect("read graph function")
        .rows()
        .to_vec()
}

fn with_graph(create_sql: &str, body: impl FnOnce(&Cassie, &cassie::app::CassieSession)) {
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let cassie = new_in_memory_cassie().expect("cassie");
        let session = cassie.create_session("tester", None);
        execute(&cassie, &session, create_sql);
        body(&cassie, &session);
    });
}

fn seed_shared_intermediate_node(cassie: &Cassie, session: &cassie::app::CassieSession) {
    insert_edge(cassie, session, "'e1', 'n', 'A', 'n', 'X', 'r', 1");
    insert_edge(cassie, session, "'e2', 'n', 'A', 'n', 'B', 'r', 1");
    insert_edge(cassie, session, "'e3', 'n', 'B', 'n', 'X', 'r', 1");
    insert_edge(cassie, session, "'e4', 'n', 'X', 'n', 'T', 'r', 1");
}

#[test]
fn should_return_every_requested_path_when_alternates_share_an_intermediate_node() {
    // Arrange
    with_graph("CREATE GRAPH social", |cassie, session| {
        seed_shared_intermediate_node(cassie, session);

        // Act
        let rows = query(
                cassie,
                session,
                "SELECT cost, depth, path_edges FROM graph_shortest_path('social', 'n', 'A', 'n', 'T', 5, 'out', '*', 3)",
            );

        // Assert
        assert_eq!(
            rows,
            vec![
                vec![
                    Value::Float64(2.0),
                    Value::Int64(2),
                    Value::Json(serde_json::json!(["e1", "e4"])),
                ],
                vec![
                    Value::Float64(3.0),
                    Value::Int64(3),
                    Value::Json(serde_json::json!(["e2", "e3", "e4"])),
                ],
            ]
        );
    });
}

#[test]
fn should_cap_shortest_paths_at_max_paths_when_alternates_share_a_node() {
    // Arrange
    with_graph("CREATE GRAPH social", |cassie, session| {
        seed_shared_intermediate_node(cassie, session);

        // Act
        let rows = query(
            cassie,
            session,
            "SELECT cost FROM graph_shortest_path('social', 'n', 'A', 'n', 'T', 5, 'out', '*', 1)",
        );

        // Assert
        assert_eq!(rows, vec![vec![Value::Float64(2.0)]]);
    });
}

#[test]
fn should_match_shortest_path_target_node_types_case_insensitively() {
    // Arrange
    with_graph("CREATE GRAPH social", |cassie, session| {
        insert_edge(
            cassie,
            session,
            "'e1', 'person', 'S', 'person', 'A', 'r', 0",
        );
        insert_edge(
            cassie,
            session,
            "'e2', 'person', 'A', 'person', 'T', 'r', 1",
        );

        // Act
        let rows = query(cassie, session, "SELECT cost, path_edges FROM graph_shortest_path('social', 'PERSON', 'S', 'PERSON', 'T', 2, 'out', '*', 1)");

        // Assert
        assert_eq!(
            rows,
            vec![vec![
                Value::Float64(1.0),
                Value::Json(serde_json::json!(["e1", "e2"]))
            ]]
        );
    });
}

#[test]
fn should_reject_shortest_path_cycles_through_node_type_case_aliases() {
    // Arrange
    with_graph("CREATE GRAPH social", |cassie, session| {
        insert_edge(
            cassie,
            session,
            "'e1', 'person', 'S', 'person', 'A', 'r', 0",
        );
        insert_edge(
            cassie,
            session,
            "'e2', 'person', 'A', 'PERSON', 'S', 'r', 0",
        );
        insert_edge(
            cassie,
            session,
            "'e3', 'person', 'S', 'person', 'T', 'r', 1",
        );

        // Act
        let rows = query(cassie, session, "SELECT cost, depth, path_edges FROM graph_shortest_path('social', 'person', 'S', 'person', 'T', 3, 'out', '*', 2)");

        // Assert
        assert_eq!(
            rows,
            vec![vec![
                Value::Float64(1.0),
                Value::Int64(1),
                Value::Json(serde_json::json!(["e3"]))
            ]]
        );
    });
}

#[test]
fn should_not_prune_a_costlier_prefix_needed_for_another_simple_path() {
    // Arrange
    with_graph("CREATE GRAPH social", |cassie, session| {
        insert_edge(cassie, session, "'e1', 'n', 'S', 'n', 'A', 'r', 1");
        insert_edge(cassie, session, "'e2', 'n', 'A', 'n', 'B', 'r', 1");
        insert_edge(cassie, session, "'e3', 'n', 'B', 'n', 'X', 'r', 1");
        insert_edge(cassie, session, "'e4', 'n', 'A', 'n', 'X', 'r', 2");
        insert_edge(cassie, session, "'e5', 'n', 'S', 'n', 'X', 'r', 4");
        insert_edge(cassie, session, "'e6', 'n', 'X', 'n', 'A', 'r', 7");
        insert_edge(cassie, session, "'e7', 'n', 'A', 'n', 'C', 'r', 1");
        insert_edge(cassie, session, "'e8', 'n', 'C', 'n', 'T', 'r', 3");

        // Act
        let rows = query(
                    cassie,
                    session,
                    "SELECT cost, path_edges FROM graph_shortest_path('social', 'n', 'S', 'n', 'T', 5, 'out', '*', 2)",
                );

        // Assert
        assert_eq!(
            rows,
            vec![
                vec![
                    Value::Float64(5.0),
                    Value::Json(serde_json::json!(["e1", "e7", "e8"])),
                ],
                vec![
                    Value::Float64(15.0),
                    Value::Json(serde_json::json!(["e5", "e6", "e7", "e8"])),
                ],
            ]
        );
    });
}

#[test]
fn should_match_expand_path_nodes_for_neighbors() {
    // Arrange
    with_graph("CREATE GRAPH social", |cassie, session| {
        insert_edge(
            cassie,
            session,
            "'e1', 'person', 'alice', 'person', 'bob', 'knows', 1",
        );

        // Act
        let neighbors = query(
                cassie,
                session,
                "SELECT path_nodes, path_edges FROM graph_neighbors('social', 'person', 'alice', 'out', 'knows', 10)",
            );
        let expanded = query(
                cassie,
                session,
                "SELECT path_nodes, path_edges FROM graph_expand('social', 'person', 'alice', 1, 'out', 'knows', 10)",
            );

        // Assert
        assert_eq!(neighbors.len(), 1);
        assert_eq!(neighbors, expanded);
        assert_eq!(
            neighbors[0][0],
            Value::Json(serde_json::json!([
                {"node_type": "person", "node_id": "alice"},
                {"node_type": "person", "node_id": "bob"},
            ]))
        );
    });
}

#[test]
fn should_keep_a_surviving_edge_when_a_duplicate_keyed_edge_row_is_deleted() {
    // Arrange
    with_graph(
        "CREATE GRAPH social (EDGES (tag TEXT))",
        |cassie, session| {
            for tag in ["a", "b"] {
                execute(
                        cassie,
                        session,
                        &format!(
                            "INSERT INTO social_edges (edge_id, source_type, source_id, target_type, target_id, edge_type, weight, tag) VALUES ('dup', 'person', 'alice', 'person', 'bob', 'knows', 1, '{tag}')"
                        ),
                    );
            }

            // Act
            execute(cassie, session, "DELETE FROM social_edges WHERE tag = 'b'");
            let neighbors = query(
                    cassie,
                    session,
                    "SELECT node_id FROM graph_neighbors('social', 'person', 'alice', 'out', 'knows', 10)",
                );
            let expanded = query(
                    cassie,
                    session,
                    "SELECT node_id FROM graph_expand('social', 'person', 'alice', 1, 'out', 'knows', 10)",
                );

            // Assert
            assert_eq!(neighbors, vec![vec![Value::String("bob".into())]]);
            assert_eq!(expanded, vec![vec![Value::String("bob".into())]]);
        },
    );
}
