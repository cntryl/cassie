use cassie::app::Cassie;
use cassie::midge::adapter::{
    set_collection_rename_failure_point, set_field_drop_failure_point,
    set_field_rename_failure_point,
};
use cassie::types::{DataType, FieldSchema, Schema};

#[path = "support/sql.rs"]
mod support;
use support::{data_dir, with_fallback};

#[test]
fn should_replay_collection_rename_data_after_schema_commit_interruption() {
    // Arrange
    with_fallback();
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
