#[path = "support/graph.rs"]
mod support;
use support::*;
#[path = "support/graph_neighbors.rs"]
mod graph_neighbors;
use graph_neighbors::neighbor_rows;

#[test]
fn should_read_an_inserted_edge_inside_its_transaction() {
    // Arrange
    with_fallback();
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
