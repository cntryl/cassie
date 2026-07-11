use cassie::app::Cassie;
use cassie::midge::adapter::{
    set_column_batch_maintenance_failure_point, ColumnBatchScanDecision, RowFilter,
};
use cassie::types::Value;
#[path = "support/sql.rs"]
mod support;
use support::{canonical_test_collection, canonical_test_index, data_dir, with_fallback};
#[test]
fn should_recover_column_batch_debt_without_serving_stale_rows() {
    // Arrange
    with_fallback();
    let path = data_dir("derived_state_column_batch_recovery");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");
    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).expect("create Cassie");
        cassie.startup().expect("start Cassie");
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE derived_state_docs (title TEXT, score INT)",
                vec![],
            )
            .expect("create table");
        cassie
            .execute_sql(
                &session,
                "INSERT INTO derived_state_docs (title, score) VALUES ('alpha', 1)",
                vec![],
            )
            .expect("insert row");
        cassie
            .execute_sql(
                &session,
                "CREATE INDEX derived_state_docs_column_idx ON derived_state_docs USING column (title, score)",
                vec![],
            )
            .expect("create column index");
        // Act
        set_column_batch_maintenance_failure_point(true);
        cassie
            .execute_sql(
                &session,
                "UPDATE derived_state_docs SET score = 2 WHERE title = 'alpha'",
                vec![],
            )
            .expect("durable write must succeed when maintenance fails");
        let collection = canonical_test_collection(&cassie, "derived_state_docs");
        let artifact_read = stale_artifact_read(&cassie, &collection);
        let fallback_result = cassie
            .execute_sql(
                &session,
                "SELECT title, score FROM derived_state_docs WHERE score = 2",
                vec![],
            )
            .expect("stale artifact must fall back to rows");
        drop(cassie);
        let restarted = Cassie::new_with_data_dir(&path).expect("reopen Cassie");
        restarted.startup().expect("retry maintenance debt");
        let restarted_session = restarted.create_session("tester", None);
        let recovered_result = restarted
            .execute_sql(
                &restarted_session,
                "SELECT title, score FROM derived_state_docs WHERE score = 2",
                vec![],
            )
            .expect("query after recovery");
        let collection = canonical_test_collection(&restarted, "derived_state_docs");
        let index = canonical_test_index(
            &restarted,
            &collection,
            "derived_state_docs_column_idx",
        );
        let metadata = restarted
            .midge
            .get_column_batch_metadata(&collection, &index)
            .expect("read metadata")
            .expect("metadata after recovery");
        // Assert
        assert_eq!(
            fallback_result.rows,
            vec![vec![Value::String("alpha".to_string()), Value::Int64(2)]]
        );
        assert!(matches!(
            artifact_read,
            ColumnBatchScanDecision::Fallback(reason) if reason.as_str() == "generation_mismatch"
        ));
        assert_eq!(recovered_result.rows, fallback_result.rows);
        assert_eq!(
            metadata.built_generation,
            restarted
                .midge
                .collection_generation(&collection)
                .expect("collection generation")
        );
    });
    let _ = std::fs::remove_dir_all(path);
}

fn stale_artifact_read(cassie: &Cassie, collection: &str) -> ColumnBatchScanDecision {
    cassie
        .midge
        .scan_column_batch_projected_rows(
            collection,
            128,
            &["title".to_string(), "score".to_string()],
            Some(&RowFilter {
                field: "score".to_string(),
                value: serde_json::json!(2),
            }),
            None,
            None,
        )
        .expect("read stale artifact")
}
