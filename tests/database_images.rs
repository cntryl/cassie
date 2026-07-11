use cassie::app::Cassie;
use cassie::catalog::canonical_relation_name;
use cassie::types::{DataType, FieldSchema, Schema};
use uuid::Uuid;

fn data_dir(label: &str) -> String {
    let mut path = std::env::temp_dir();
    path.push(format!("cassie-image-{label}-{}", Uuid::new_v4()));
    path.to_string_lossy().into_owned()
}

#[test]
fn should_round_trip_one_database_as_bounded_chunks() {
    // Arrange
    let source_path = data_dir("source");
    let cassie = Cassie::new_with_data_dir(&source_path).expect("cassie");
    cassie.startup().expect("startup");
    cassie
        .midge
        .create_database("analytics", None)
        .expect("database");
    cassie
        .midge
        .create_namespace("analytics.public")
        .expect("namespace");
    let source_collection = canonical_relation_name("analytics", "public", "docs");
    cassie
        .midge
        .create_collection(
            &source_collection,
            Schema {
                fields: vec![FieldSchema {
                    name: "value".to_string(),
                    data_type: DataType::Text,
                    nullable: false,
                }],
            },
        )
        .expect("collection");
    cassie
        .midge
        .put_document(
            &source_collection,
            Some("row-1".to_string()),
            serde_json::json!({"value": "from-image"}),
        )
        .expect("row");

    let mut backup = cassie
        .begin_database_backup("analytics")
        .expect("begin backup");
    let mut image = Vec::new();
    while let Some(chunk) = backup.next_chunk().expect("backup chunk") {
        assert!(chunk.len() <= 64 * 1024);
        image.extend_from_slice(&chunk);
    }

    // Act
    let mut restore = cassie
        .begin_database_restore("restored")
        .expect("begin restore");
    for chunk in image.chunks(3) {
        restore.push_chunk(chunk).expect("restore chunk");
    }
    restore.finish().expect("finish restore");
    cassie.hydrate_catalog().expect("hydrate restored catalog");

    // Assert
    let target_collection = canonical_relation_name("restored", "public", "docs");
    let row = cassie
        .midge
        .get_document(&target_collection, "row-1")
        .expect("restored row lookup")
        .expect("restored row");
    assert_eq!(row.payload["value"], "from-image");
    assert!(cassie
        .midge
        .get_document(&source_collection, "row-1")
        .expect("source row lookup")
        .is_some());

    let _ = std::fs::remove_dir_all(source_path);
}
