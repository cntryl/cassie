use cassie::app::Cassie;
use cassie::types::Value;

#[path = "support/sql.rs"]
mod support;
use support::{data_dir, with_fallback};

#[test]
fn should_preserve_graph_transaction_visibility_across_savepoint_rollback() {
    // Arrange
    with_fallback();
    let path = data_dir("graph_transaction_overlay");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");
    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).expect("cassie");
        let writer = cassie.create_session("writer", None);
        let reader = cassie.create_session("reader", None);
        cassie
            .execute_sql(&writer, "CREATE GRAPH social", vec![])
            .expect("create graph");
        cassie.execute_sql(&writer, "BEGIN", vec![]).expect("begin");
        cassie
            .execute_sql(
                &writer,
                "INSERT INTO social_edges (edge_id, source_type, source_id, target_type, target_id, edge_type, weight) VALUES ('e1', 'person', 'alice', 'person', 'bob', 'knows', 2)",
                vec![],
            )
            .expect("stage edge");

        // Act
        let writer_neighbors = cassie
            .execute_sql(
                &writer,
                "SELECT node_id, cost FROM graph_neighbors('social', 'person', 'alice', 'out', 'knows', 10)",
                vec![],
            )
            .expect("writer neighbors");
        let writer_path = cassie
            .execute_sql(
                &writer,
                "SELECT node_id FROM graph_shortest_path('social', 'person', 'alice', 'person', 'bob', 2, 'out', 'knows', 1)",
                vec![],
            )
            .expect("writer path");
        let reader_before = cassie
            .execute_sql(
                &reader,
                "SELECT node_id FROM graph_neighbors('social', 'person', 'alice', 'out', 'knows', 10)",
                vec![],
            )
            .expect("reader before commit");
        cassie
            .execute_sql(&writer, "SAVEPOINT before_delete", vec![])
            .expect("savepoint");
        cassie
            .execute_sql(&writer, "DELETE FROM social_edges WHERE edge_id = 'e1'", vec![])
            .expect("stage delete");
        let after_delete = cassie
            .execute_sql(
                &writer,
                "SELECT node_id FROM graph_neighbors('social', 'person', 'alice', 'out', 'knows', 10)",
                vec![],
            )
            .expect("after delete");
        cassie
            .execute_sql(&writer, "ROLLBACK TO SAVEPOINT before_delete", vec![])
            .expect("rollback savepoint");
        let after_rollback = cassie
            .execute_sql(
                &writer,
                "SELECT node_id FROM graph_expand('social', 'person', 'alice', 1, 'out', 'knows', 10)",
                vec![],
            )
            .expect("after rollback");
        cassie.execute_sql(&writer, "COMMIT", vec![]).expect("commit");
        let reader_after = cassie
            .execute_sql(
                &reader,
                "SELECT node_id FROM graph_neighbors('social', 'person', 'alice', 'out', 'knows', 10)",
                vec![],
            )
            .expect("reader after commit");

        // Assert
        assert_eq!(writer_neighbors.rows, vec![vec![Value::String("bob".into()), Value::Float64(2.0)]]);
        assert_eq!(writer_path.rows, vec![vec![Value::String("bob".into())]]);
        assert!(reader_before.rows.is_empty());
        assert!(after_delete.rows.is_empty());
        assert_eq!(after_rollback.rows, vec![vec![Value::String("bob".into())]]);
        assert_eq!(reader_after.rows, vec![vec![Value::String("bob".into())]]);
        assert_eq!(cassie.metrics()["graph"]["last_fallback_reason"], "transaction-overlay");
        let _ = std::fs::remove_dir_all(path);
    });
}
