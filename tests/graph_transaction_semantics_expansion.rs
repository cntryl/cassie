#[path = "support/graph.rs"]
mod support;
use support::*;

#[test]
fn should_expand_across_edges_staged_in_one_transaction() {
    // Arrange
    with_fallback();
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
