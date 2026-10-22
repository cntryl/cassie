use super::{FieldSchema, Midge, Schema, StorageFamily};

#[test]
fn should_persist_relation_catalog_in_database_family() {
    // Arrange
    let path = std::env::temp_dir().join(format!(
        "cassie_relation_catalog_family_{}",
        uuid::Uuid::new_v4()
    ));
    let midge = Midge::new_with_data_dir(&path).expect("create Midge");
    let collection = midge.canonical_collection_name("family_catalog_items");
    let key = Midge::collection_schema_key(&collection);

    // Act
    midge
        .create_collection(
            "family_catalog_items",
            Schema {
                fields: vec![FieldSchema {
                    name: "id".to_string(),
                    data_type: crate::types::DataType::Int,
                    nullable: false,
                }],
            },
        )
        .expect("create collection");

    // Assert
    assert!(midge
        .raw_get(StorageFamily::Data, &key)
        .expect("read database-family catalog")
        .is_some());
    assert!(midge
        .raw_get(StorageFamily::Schema, &key)
        .expect("read schema-family catalog")
        .is_none());
}
