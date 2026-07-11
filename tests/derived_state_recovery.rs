use cassie::app::{Cassie, CassieSession};
use cassie::midge::adapter::{
    set_column_batch_maintenance_failure_point, set_projection_hash_maintenance_failure_point,
    ColumnBatchScanDecision, RowFilter,
};
use cassie::types::{DataType, FieldSchema, Value};
#[path = "support/sql.rs"]
mod support;
use support::{canonical_test_collection, canonical_test_index, data_dir, with_fallback};

static COLUMN_BATCH_FAILPOINT_GUARD: std::sync::Mutex<()> = std::sync::Mutex::new(());

#[test]
fn should_recover_column_batch_debt_without_serving_stale_rows() {
    // Arrange
    let _failpoint_guard = COLUMN_BATCH_FAILPOINT_GUARD
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
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
            ColumnBatchScanDecision::Fallback(reason)
                if reason.as_str() == "maintenance_pending"
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

#[test]
fn should_recover_add_column_column_batch_debt_after_restart() {
    // Arrange
    let _failpoint_guard = COLUMN_BATCH_FAILPOINT_GUARD
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    with_fallback();
    let path = data_dir("derived_state_add_column_batch_recovery");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");
    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).expect("create Cassie");
        cassie.startup().expect("start Cassie");
        let session = cassie.create_session("tester", None);
        let collection = seed_add_column_batch_recovery(&cassie, &session);

        // Act
        set_column_batch_maintenance_failure_point(true);
        let alter_result = cassie.execute_sql(
            &session,
            "ALTER TABLE derived_state_add_column_batch_docs ADD COLUMN subtitle TEXT",
            vec![],
        );
        let schema_after_failure = cassie
            .midge
            .collection_schema(&collection)
            .expect("durable schema after interrupted maintenance");
        let debt_after_failure = cassie
            .midge
            .has_column_batch_maintenance_debt(&collection)
            .expect("read durable debt");
        let artifact_read = stale_artifact_read(&cassie, &collection);
        let fallback_result = cassie
            .execute_sql(
                &session,
                "SELECT title, score FROM derived_state_add_column_batch_docs WHERE score = 2",
                vec![],
            )
            .expect("stale artifact must fall back to rows");
        let fallback_metrics = cassie.metrics();
        drop(cassie);
        let recovered = recover_add_column_batch(&path);

        // Assert
        assert!(alter_result.is_err());
        assert!(schema_after_failure
            .fields
            .iter()
            .any(|field| field.name == "subtitle"));
        assert!(debt_after_failure);
        assert!(matches!(
            artifact_read,
            ColumnBatchScanDecision::Fallback(reason)
                if reason.as_str() == "maintenance_pending"
        ));
        assert_eq!(
            fallback_result.rows,
            vec![vec![Value::String("alpha".to_string()), Value::Int64(2)]]
        );
        assert!(fallback_metrics["column_batches"]["fallback_scans"]
            .as_u64()
            .is_some_and(|count| count > 0));
        assert_eq!(
            fallback_metrics["column_batches"]["last_fallback_reason"],
            "maintenance_pending"
        );
        assert_eq!(
            recovered.rows,
            vec![vec![
                Value::String("alpha".to_string()),
                Value::Int64(2),
                Value::Null,
            ]]
        );
        assert!(matches!(
            recovered.artifact_read,
            ColumnBatchScanDecision::Hit(_)
        ));
        assert!(!recovered.debt);
        assert_eq!(recovered.built_generation, recovered.collection_generation);
    });
    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_recover_add_column_projection_hash_debt_after_restart() {
    // Arrange
    with_fallback();
    let path = data_dir("derived_state_add_column_recovery");
    let cassie = Cassie::new_with_data_dir(&path).expect("create Cassie");
    cassie.startup().expect("start Cassie");
    let collection = "derived_state_add_column_docs";
    cassie
        .midge
        .create_collection(
            collection,
            cassie::types::Schema {
                fields: vec![FieldSchema {
                    name: "title".to_string(),
                    data_type: DataType::Text,
                    nullable: true,
                }],
            },
        )
        .expect("create collection");
    cassie
        .midge
        .put_document(
            collection,
            Some("doc-1".to_string()),
            serde_json::json!({"title": "alpha"}),
        )
        .expect("seed document");
    cassie
        .midge
        .rebuild_projection_hashes(collection)
        .expect("build initial hashes");

    // Act
    set_projection_hash_maintenance_failure_point(true);
    assert!(cassie
        .midge
        .alter_collection_add_column(
            collection,
            FieldSchema {
                name: "subtitle".to_string(),
                data_type: DataType::Text,
                nullable: true,
            },
        )
        .is_err());
    assert!(cassie
        .midge
        .has_projection_hash_maintenance_debt(collection)
        .expect("read durable debt"));
    drop(cassie);
    let restarted = Cassie::new_with_data_dir(&path).expect("reopen Cassie");
    restarted.startup().expect("recover maintenance debt");

    // Assert
    assert!(!restarted
        .midge
        .has_projection_hash_maintenance_debt(collection)
        .expect("debt cleared"));
    assert!(restarted
        .midge
        .collection_schema(collection)
        .expect("schema")
        .fields
        .iter()
        .any(|field| field.name == "subtitle"));

    let _ = std::fs::remove_dir_all(path);
}

fn seed_add_column_batch_recovery(cassie: &Cassie, session: &CassieSession) -> String {
    cassie
        .execute_sql(
            session,
            "CREATE TABLE derived_state_add_column_batch_docs (title TEXT, score INT)",
            vec![],
        )
        .expect("create table");
    cassie
        .execute_sql(
            session,
            "INSERT INTO derived_state_add_column_batch_docs (title, score) VALUES ('alpha', 2)",
            vec![],
        )
        .expect("insert row");
    cassie
        .execute_sql(
            session,
            "CREATE INDEX derived_state_add_column_batch_idx ON derived_state_add_column_batch_docs USING column (title, score)",
            vec![],
        )
        .expect("create column index");
    canonical_test_collection(cassie, "derived_state_add_column_batch_docs")
}

struct RecoveredColumnBatch {
    rows: Vec<Vec<Value>>,
    artifact_read: ColumnBatchScanDecision,
    debt: bool,
    built_generation: u64,
    collection_generation: u64,
}

fn recover_add_column_batch(path: &str) -> RecoveredColumnBatch {
    let restarted = Cassie::new_with_data_dir(path).expect("reopen Cassie");
    restarted.startup().expect("retry maintenance debt");
    let session = restarted.create_session("tester", None);
    let rows = restarted
        .execute_sql(
            &session,
            "SELECT title, score, subtitle FROM derived_state_add_column_batch_docs WHERE score = 2",
            vec![],
        )
        .expect("query after recovery")
        .rows;
    let collection = canonical_test_collection(&restarted, "derived_state_add_column_batch_docs");
    let index = canonical_test_index(
        &restarted,
        &collection,
        "derived_state_add_column_batch_idx",
    );
    let metadata = restarted
        .midge
        .get_column_batch_metadata(&collection, &index)
        .expect("read metadata")
        .expect("metadata after recovery");
    RecoveredColumnBatch {
        rows,
        artifact_read: stale_artifact_read(&restarted, &collection),
        debt: restarted
            .midge
            .has_column_batch_maintenance_debt(&collection)
            .expect("read recovered debt state"),
        built_generation: metadata.built_generation,
        collection_generation: restarted
            .midge
            .collection_generation(&collection)
            .expect("collection generation"),
    }
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
