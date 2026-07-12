use cassie::app::Cassie;
use cassie::midge::adapter::{
    set_collection_drop_failure_point, set_collection_rename_failure_point,
    set_field_add_failure_point, set_field_drop_failure_point, set_field_rename_failure_point,
    set_index_drop_failure_point, Midge, StorageFamily,
};
use cassie::types::{DataType, FieldSchema, Schema};

#[path = "support/sql.rs"]
mod support;
use support::{canonical_test_collection, data_dir, with_fallback};

static COLLECTION_DROP_FAILPOINT_GUARD: std::sync::Mutex<()> = std::sync::Mutex::new(());
static COLLECTION_RENAME_FAILPOINT_GUARD: std::sync::Mutex<()> = std::sync::Mutex::new(());
static INDEX_DROP_FAILPOINT_GUARD: std::sync::Mutex<()> = std::sync::Mutex::new(());
static FIELD_ADD_FAILPOINT_GUARD: std::sync::Mutex<()> = std::sync::Mutex::new(());

#[test]
fn should_replay_add_column_derived_state_after_schema_commit_interruption() {
    // Arrange
    with_fallback();
    let _failpoint_guard = FIELD_ADD_FAILPOINT_GUARD.lock().unwrap();
    let path = data_dir("schema_operation_field_add_recovery");
    let cassie = Cassie::new_with_data_dir(&path).expect("create Cassie");
    cassie.startup().expect("start Cassie");
    let session = cassie.create_session("tester", None);
    cassie
        .execute_sql(
            &session,
            "CREATE TABLE field_add_recovery (keep TEXT)",
            vec![],
        )
        .expect("create table");
    cassie
        .execute_sql(
            &session,
            "INSERT INTO field_add_recovery (keep) VALUES ('alpha')",
            vec![],
        )
        .expect("seed row");
    cassie
        .execute_sql(
            &session,
            "CREATE INDEX field_add_recovery_cover ON field_add_recovery USING column (keep)",
            vec![],
        )
        .expect("create column index");
    let collection = canonical_test_collection(&cassie, "field_add_recovery");
    // Act
    set_field_add_failure_point(true);
    assert!(cassie
        .midge
        .alter_collection_add_column(
            &collection,
            FieldSchema {
                name: "added".to_string(),
                data_type: DataType::Text,
                nullable: true,
            },
        )
        .is_err());
    let schema_after_interrupt = cassie
        .midge
        .collection_schema(&collection)
        .expect("schema remains durable");
    assert!(schema_after_interrupt
        .fields
        .iter()
        .any(|field| field.name == "added"));
    drop(cassie);

    let restarted = Cassie::new_with_data_dir(&path).expect("reopen Cassie");
    restarted.startup().expect("replay field add");
    let restarted_session = restarted.create_session("tester", None);
    let result = restarted
        .execute_sql(
            &restarted_session,
            "SELECT keep, added FROM field_add_recovery",
            vec![],
        )
        .expect("query after field-add replay");
    let root = restarted
        .midge
        .root_hash(&collection)
        .expect("read rebuilt projection root")
        .expect("projection root after replay");
    let generation = restarted
        .midge
        .collection_generation(&collection)
        .expect("read collection generation");
    let metadata = restarted
        .midge
        .get_column_batch_metadata(&collection, "field_add_recovery_cover")
        .expect("read rebuilt column batches")
        .expect("column batches after replay");
    drop(restarted);
    let restarted_again = Cassie::new_with_data_dir(&path).expect("reopen Cassie again");
    restarted_again
        .startup()
        .expect("replay field add idempotently");

    // Assert
    assert_eq!(
        result.rows,
        vec![vec![
            cassie::types::Value::String("alpha".to_string()),
            cassie::types::Value::Null,
        ]]
    );
    assert_eq!(generation, 2);
    assert_eq!(root.built_generation, generation);
    assert_eq!(metadata.built_generation, generation);
    assert!(restarted_again
        .midge
        .root_hash(&collection)
        .expect("read root after second restart")
        .is_some());

    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_discard_rejected_collection_rename_intent_on_restart() {
    // Arrange
    with_fallback();
    let path = data_dir("schema_operation_rejected_rename");
    let cassie = Cassie::new_with_data_dir(&path).expect("create Cassie");
    cassie.startup().expect("start Cassie");
    let schema = Schema {
        fields: vec![FieldSchema {
            name: "title".to_string(),
            data_type: DataType::Text,
            nullable: true,
        }],
    };
    cassie
        .midge
        .create_collection("rename_rejected_source", schema.clone())
        .expect("create source collection");
    cassie
        .midge
        .create_collection("rename_rejected_target", schema)
        .expect("create target collection");
    cassie
        .midge
        .put_document(
            "rename_rejected_source",
            Some("source-row".to_string()),
            serde_json::json!({"title": "source"}),
        )
        .expect("seed source document");
    cassie
        .midge
        .put_document(
            "rename_rejected_target",
            Some("target-row".to_string()),
            serde_json::json!({"title": "target"}),
        )
        .expect("seed target document");

    // Act
    assert!(cassie
        .midge
        .rename_collection("rename_rejected_source", "rename_rejected_target")
        .is_err());
    drop(cassie);
    let restarted = Cassie::new_with_data_dir(&path).expect("reopen Cassie");
    restarted.startup().expect("discard rejected rename intent");

    // Assert
    let source = restarted
        .midge
        .scan_documents("rename_rejected_source")
        .expect("scan source collection");
    let target = restarted
        .midge
        .scan_documents("rename_rejected_target")
        .expect("scan target collection");
    assert_eq!(source.len(), 1);
    assert_eq!(source[0].id, "source-row");
    assert_eq!(source[0].payload, serde_json::json!({"title": "source"}));
    assert_eq!(target.len(), 1);
    assert_eq!(target[0].id, "target-row");
    assert_eq!(target[0].payload, serde_json::json!({"title": "target"}));

    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_replay_drop_collection_cleanup_after_schema_commit() {
    // Arrange
    with_fallback();
    let _failpoint_guard = COLLECTION_DROP_FAILPOINT_GUARD.lock().unwrap();
    let path = data_dir("schema_operation_drop_recovery");
    let cassie = Cassie::new_with_data_dir(&path).expect("create Cassie");
    cassie.startup().expect("start Cassie");
    let schema = Schema {
        fields: vec![FieldSchema {
            name: "title".to_string(),
            data_type: DataType::Text,
            nullable: true,
        }],
    };
    cassie
        .midge
        .create_collection("drop_recovery", schema)
        .expect("create collection");
    cassie
        .midge
        .put_document(
            "drop_recovery",
            Some("doc-1".to_string()),
            serde_json::json!({"title": "alpha"}),
        )
        .expect("seed document");
    cassie
        .midge
        .defer_drop_collection("drop_recovery", 0)
        .expect("defer collection drop");
    set_collection_drop_failure_point(true);

    // Act
    assert!(cassie
        .run_deferred_schema_cleanup_for_diagnostics()
        .is_err());
    let schema_after_interrupt = cassie.midge.collection_schema("drop_recovery");
    let generation_after_interrupt = cassie
        .midge
        .collection_generation("drop_recovery")
        .expect("read retained generation");
    drop(cassie);
    let restarted = Cassie::new_with_data_dir(&path).expect("reopen Cassie");
    restarted.startup().expect("replay collection drop");
    let restarted_again = Cassie::new_with_data_dir(&path).expect("reopen Cassie again");
    restarted_again
        .startup()
        .expect("replay collection drop idempotently");

    // Assert
    assert!(schema_after_interrupt.is_none());
    assert_eq!(generation_after_interrupt, 1);
    assert!(restarted.midge.collection_schema("drop_recovery").is_none());
    assert_eq!(
        restarted
            .midge
            .collection_generation("drop_recovery")
            .unwrap(),
        0
    );
    assert!(restarted_again
        .midge
        .collection_schema("drop_recovery")
        .is_none());

    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_replay_drop_graph_collection_cleanup_without_orphaned_adjacency() {
    // Arrange
    with_fallback();
    let _failpoint_guard = COLLECTION_DROP_FAILPOINT_GUARD.lock().unwrap();
    let path = data_dir("schema_operation_drop_graph_recovery");
    let cassie = Cassie::new_with_data_dir(&path).expect("create Cassie");
    cassie.startup().expect("start Cassie");
    let session = cassie.create_session("tester", None);
    cassie
        .execute_sql(
            &session,
            "CREATE GRAPH drop_graph_recovery (NODES (label TEXT), EDGES (source TEXT))",
            vec![],
        )
        .expect("create graph");
    cassie
        .execute_sql(
            &session,
            "INSERT INTO drop_graph_recovery_nodes (node_type, node_id, label) VALUES ('person', 'alice', 'Alice'), ('person', 'bob', 'Bob')",
            vec![],
        )
        .expect("seed graph nodes");
    cassie
        .execute_sql(
            &session,
            "INSERT INTO drop_graph_recovery_edges (edge_id, source_type, source_id, target_type, target_id, edge_type, weight, source) VALUES ('e1', 'person', 'alice', 'person', 'bob', 'knows', 1, 'direct')",
            vec![],
        )
        .expect("seed graph edge");
    let before_drop = cassie
        .execute_sql(
            &session,
            "SELECT edge_id FROM graph_neighbors('drop_graph_recovery', 'person', 'alice', 'out', 'knows', 10)",
            vec![],
        )
        .expect("read graph adjacency before drop");
    assert_eq!(before_drop.rows.len(), 1);
    let edge_collection = canonical_test_collection(&cassie, "drop_graph_recovery_edges");
    cassie
        .midge
        .defer_drop_collection(&edge_collection, 0)
        .expect("defer graph edge collection drop");
    set_collection_drop_failure_point(true);

    // Act
    assert!(cassie
        .run_deferred_schema_cleanup_for_diagnostics()
        .is_err());
    drop(cassie);
    let restarted = Cassie::new_with_data_dir(&path).expect("reopen Cassie");
    restarted.startup().expect("replay graph collection drop");
    let restarted_session = restarted.create_session("tester", None);
    let after_drop = restarted
        .execute_sql(
            &restarted_session,
            "SELECT edge_id FROM graph_neighbors('drop_graph_recovery', 'person', 'alice', 'out', 'knows', 10)",
            vec![],
        )
        .expect("read graph adjacency after drop");
    drop(restarted);
    let restarted_again = Cassie::new_with_data_dir(&path).expect("reopen Cassie again");
    restarted_again
        .startup()
        .expect("replay graph collection drop idempotently");

    // Assert
    assert!(after_drop.rows.is_empty());
    let after_second_restart = restarted_again
        .execute_sql(
            &restarted_again.create_session("tester", None),
            "SELECT edge_id FROM graph_neighbors('drop_graph_recovery', 'person', 'alice', 'out', 'knows', 10)",
            vec![],
        )
        .expect("read graph adjacency after second restart");
    assert!(after_second_restart.rows.is_empty());

    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_replay_drop_index_cleanup_after_metadata_interrupt() {
    // Arrange
    with_fallback();
    let _failpoint_guard = INDEX_DROP_FAILPOINT_GUARD.lock().unwrap();
    let path = data_dir("schema_operation_drop_index_recovery");
    let cassie = Cassie::new_with_data_dir(&path).expect("create Cassie");
    cassie.startup().expect("start Cassie");
    let session = cassie.create_session("tester", None);
    cassie
        .execute_sql(
            &session,
            "CREATE TABLE drop_index_recovery (title TEXT)",
            vec![],
        )
        .expect("create table");
    cassie
        .execute_sql(
            &session,
            "INSERT INTO drop_index_recovery (title) VALUES ('alpha')",
            vec![],
        )
        .expect("seed row");
    cassie
        .execute_sql(
            &session,
            "CREATE INDEX drop_index_recovery_title_idx ON drop_index_recovery USING btree (title)",
            vec![],
        )
        .expect("create index");
    let collection = cassie
        .catalog
        .get_schema("drop_index_recovery")
        .expect("catalog collection")
        .collection;
    let prefix = Midge::scalar_index_collection_prefix_for_diagnostics(&collection);
    assert!(!cassie
        .midge
        .raw_scan_prefix(StorageFamily::Data, &prefix)
        .expect("read scalar sidecars")
        .is_empty());
    cassie
        .midge
        .defer_drop_index("drop_index_recovery", "drop_index_recovery_title_idx", 0)
        .expect("defer index cleanup");
    set_index_drop_failure_point(true);

    // Act
    assert!(cassie
        .run_deferred_schema_cleanup_for_diagnostics()
        .is_err());
    assert!(cassie
        .midge
        .get_index(&collection, "drop_index_recovery_title_idx")
        .expect("read interrupted metadata")
        .is_some());
    assert!(cassie
        .midge
        .raw_scan_prefix(StorageFamily::Data, &prefix)
        .expect("read cleaned scalar sidecars")
        .is_empty());
    drop(cassie);
    let restarted = Cassie::new_with_data_dir(&path).expect("reopen Cassie");
    restarted.startup().expect("replay index cleanup");
    let restarted_again = Cassie::new_with_data_dir(&path).expect("reopen Cassie again");
    restarted_again
        .startup()
        .expect("replay index cleanup idempotently");

    // Assert
    assert!(restarted
        .midge
        .get_index(&collection, "drop_index_recovery_title_idx")
        .expect("read removed metadata")
        .is_none());
    assert!(restarted
        .midge
        .raw_scan_prefix(StorageFamily::Data, &prefix)
        .expect("read removed scalar sidecars")
        .is_empty());
    assert!(restarted_again
        .midge
        .get_index(&collection, "drop_index_recovery_title_idx")
        .expect("read removed metadata after second restart")
        .is_none());

    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_replay_collection_rename_data_after_schema_commit_interruption() {
    // Arrange
    with_fallback();
    let _failpoint_guard = COLLECTION_RENAME_FAILPOINT_GUARD.lock().unwrap();
    let path = data_dir("schema_operation_rename_recovery");
    let cassie = Cassie::new_with_data_dir(&path).expect("create Cassie");
    cassie.startup().expect("start Cassie");
    let schema = Schema {
        fields: vec![FieldSchema {
            name: "title".to_string(),
            data_type: DataType::Text,
            nullable: true,
        }],
    };
    cassie
        .midge
        .create_collection("rename_recovery_before", schema)
        .expect("create collection");
    cassie
        .midge
        .put_document(
            "rename_recovery_before",
            Some("doc-1".to_string()),
            serde_json::json!({"title": "alpha"}),
        )
        .expect("seed document");

    // Act
    set_collection_rename_failure_point(true);
    assert!(cassie
        .midge
        .rename_collection("rename_recovery_before", "rename_recovery_after")
        .is_err());
    drop(cassie);
    let restarted = Cassie::new_with_data_dir(&path).expect("reopen Cassie");
    restarted.startup().expect("replay rename");

    // Assert
    let documents = restarted
        .midge
        .scan_documents("rename_recovery_after")
        .expect("scan renamed documents");
    assert_eq!(documents.len(), 1);
    assert_eq!(documents[0].id, "doc-1");
    assert_eq!(documents[0].payload, serde_json::json!({"title": "alpha"}));

    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_preserve_destination_write_when_collection_rename_replays() {
    // Arrange
    with_fallback();
    let _failpoint_guard = COLLECTION_RENAME_FAILPOINT_GUARD.lock().unwrap();
    let path = data_dir("schema_operation_rename_destination_write");
    let cassie = Cassie::new_with_data_dir(&path).expect("create Cassie");
    cassie.startup().expect("start Cassie");
    let schema = Schema {
        fields: vec![FieldSchema {
            name: "title".to_string(),
            data_type: DataType::Text,
            nullable: true,
        }],
    };
    cassie
        .midge
        .create_collection("rename_destination_before", schema)
        .expect("create collection");
    cassie
        .midge
        .put_document(
            "rename_destination_before",
            Some("shared-row".to_string()),
            serde_json::json!({"title": "before"}),
        )
        .expect("seed source document");

    // Act
    set_collection_rename_failure_point(true);
    assert!(cassie
        .midge
        .rename_collection("rename_destination_before", "rename_destination_after")
        .is_err());
    cassie
        .midge
        .put_document(
            "rename_destination_after",
            Some("shared-row".to_string()),
            serde_json::json!({"title": "after"}),
        )
        .expect("write to committed destination collection");
    drop(cassie);
    let restarted = Cassie::new_with_data_dir(&path).expect("reopen Cassie");
    restarted
        .startup()
        .expect("replay rename without data loss");

    // Assert
    let documents = restarted
        .midge
        .scan_documents("rename_destination_after")
        .expect("scan renamed documents");
    assert_eq!(documents.len(), 1);
    assert_eq!(documents[0].id, "shared-row");
    assert_eq!(documents[0].payload, serde_json::json!({"title": "after"}));
    assert!(restarted
        .midge
        .collection_schema("rename_destination_before")
        .is_none());

    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_replay_field_rename_data_after_schema_commit_interruption() {
    // Arrange
    with_fallback();
    let path = data_dir("schema_operation_field_rename_recovery");
    let cassie = Cassie::new_with_data_dir(&path).expect("create Cassie");
    cassie.startup().expect("start Cassie");
    let schema = Schema {
        fields: vec![FieldSchema {
            name: "before".to_string(),
            data_type: DataType::Text,
            nullable: true,
        }],
    };
    cassie
        .midge
        .create_collection("field_rename_recovery", schema)
        .expect("create collection");
    cassie
        .midge
        .put_document(
            "field_rename_recovery",
            Some("doc-1".to_string()),
            serde_json::json!({"before": "alpha"}),
        )
        .expect("seed document");

    // Act
    set_field_rename_failure_point(true);
    assert!(cassie
        .midge
        .alter_collection_rename_column("field_rename_recovery", "before", "after")
        .is_err());
    drop(cassie);
    let restarted = Cassie::new_with_data_dir(&path).expect("reopen Cassie");
    restarted.startup().expect("replay field rename");

    // Assert
    let documents = restarted
        .midge
        .scan_documents("field_rename_recovery")
        .expect("scan renamed documents");
    assert_eq!(documents[0].payload, serde_json::json!({"after": "alpha"}));

    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_replay_field_drop_data_after_schema_commit_interruption() {
    // Arrange
    with_fallback();
    let path = data_dir("schema_operation_field_drop_recovery");
    let cassie = Cassie::new_with_data_dir(&path).expect("create Cassie");
    cassie.startup().expect("start Cassie");
    let schema = Schema {
        fields: vec![
            FieldSchema {
                name: "keep".to_string(),
                data_type: DataType::Text,
                nullable: true,
            },
            FieldSchema {
                name: "remove".to_string(),
                data_type: DataType::Text,
                nullable: true,
            },
        ],
    };
    cassie
        .midge
        .create_collection("field_drop_recovery", schema)
        .expect("create collection");
    cassie
        .midge
        .put_document(
            "field_drop_recovery",
            Some("doc-1".to_string()),
            serde_json::json!({"keep": "alpha", "remove": "discard"}),
        )
        .expect("seed document");

    // Act
    set_field_drop_failure_point(true);
    assert!(cassie
        .midge
        .alter_collection_drop_column("field_drop_recovery", "remove")
        .is_err());
    drop(cassie);
    let restarted = Cassie::new_with_data_dir(&path).expect("reopen Cassie");
    restarted.startup().expect("replay field drop");

    // Assert
    let documents = restarted
        .midge
        .scan_documents("field_drop_recovery")
        .expect("scan dropped-field documents");
    assert_eq!(documents[0].payload, serde_json::json!({"keep": "alpha"}));

    let _ = std::fs::remove_dir_all(path);
}
