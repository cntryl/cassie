#[path = "support/graph.rs"]
mod support;
use support::*;
#[path = "support/graph_neighbors.rs"]
mod graph_neighbors;
use graph_neighbors::neighbor_rows;

#[test]
fn should_read_an_updated_edge_inside_its_transaction() {
    // Arrange
    with_fallback();
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
