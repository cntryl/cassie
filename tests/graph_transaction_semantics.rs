#[path = "support/graph.rs"]
mod support;
use support::*;
#[path = "support/graph_neighbors.rs"]
mod graph_neighbors;
use graph_neighbors::neighbor_rows;

#[test]
fn should_read_an_inserted_edge_inside_its_transaction() {
    // Arrange
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
