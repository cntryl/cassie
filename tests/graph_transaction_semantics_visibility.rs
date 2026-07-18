#[path = "support/graph.rs"]
mod support;
use support::*;
#[path = "support/graph_neighbors.rs"]
mod graph_neighbors;
use graph_neighbors::neighbor_rows;

#[test]
fn should_publish_a_graph_edge_to_other_sessions_only_after_commit() {
    // Arrange
    with_fallback();
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
