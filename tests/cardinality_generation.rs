use cassie::app::Cassie;
use cassie::types::{DataType, FieldSchema, Schema};

#[path = "support/sql.rs"]
mod support;
use support::{data_dir, with_fallback};

#[test]
fn should_reject_cardinality_stats_from_an_older_collection_generation() {
    // Arrange
    with_fallback();
    let path = data_dir("cardinality_generation");
    let cassie = Cassie::new_with_data_dir(&path).expect("create Cassie");
    cassie.startup().expect("start Cassie");
    let collection = "cardinality_generation_docs";
    let schema = Schema {
        fields: vec![FieldSchema {
            name: "title".to_string(),
            data_type: DataType::Text,
            nullable: true,
        }],
    };
    cassie
        .midge
        .create_collection(collection, schema.clone())
        .expect("create collection");
    cassie.register_collection(
        collection,
        schema
            .fields
            .iter()
            .map(|field| (field.name.clone(), field.data_type.clone()))
            .collect(),
    );
    cassie
        .midge
        .put_document(
            collection,
            Some("doc-1".to_string()),
            serde_json::json!({"title": "before"}),
        )
        .expect("seed document");
    cassie
        .midge
        .rebuild_cardinality_stats_for_collection(collection)
        .expect("build current stats");
    assert!(cassie
        .midge
        .get_cardinality_stats(collection)
        .expect("read current stats")
        .is_some());

    // Act
    cassie
        .midge
        .put_document(
            collection,
            Some("doc-2".to_string()),
            serde_json::json!({"title": "after"}),
        )
        .expect("advance collection generation");

    // Assert
    assert!(cassie
        .midge
        .get_cardinality_stats(collection)
        .expect("read stale stats")
        .is_none());

    let _ = std::fs::remove_dir_all(path);
}
