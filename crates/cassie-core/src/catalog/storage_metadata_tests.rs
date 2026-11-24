use super::storage_metadata::{CollectionMeta, CollectionStorageMode, ColumnBatchNumericSum};

#[test]
fn should_preserve_storage_catalog_default_and_skip_semantics() {
    // Arrange
    let collection = CollectionMeta::new("public.events", Some("events".to_owned()));

    // Act
    let encoded = serde_json::to_value(&collection).expect("serialize collection metadata");
    let decoded: CollectionMeta = serde_json::from_value(serde_json::json!({
        "name": "public.events",
        "description": null
    }))
    .expect("deserialize legacy collection metadata");
    let numeric_sum = serde_json::to_value(ColumnBatchNumericSum::IntegerOverflow)
        .expect("serialize batch summary tag");

    // Assert
    assert_eq!(collection.storage_mode, CollectionStorageMode::RowStore);
    assert_eq!(encoded["storage_version"], 1);
    assert!(encoded.get("row_type_revision").is_none());
    assert_eq!(decoded.row_type_revision, 1);
    assert_eq!(decoded.storage_version, 1);
    assert_eq!(numeric_sum, serde_json::json!({"kind": "integer_overflow"}));
}
