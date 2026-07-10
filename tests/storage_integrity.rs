use cassie::app::Cassie;
use cassie::catalog::canonical_relation_name;
use cassie::types::{DataType, FieldSchema, Schema};

#[path = "support/sql.rs"]
mod support;

fn collection_name(collection: &str) -> String {
    canonical_relation_name("postgres", "public", collection)
}

fn register_collection(cassie: &Cassie, collection: &str) -> String {
    let collection = collection_name(collection);
    let schema = Schema {
        fields: vec![FieldSchema {
            name: "title".to_string(),
            data_type: DataType::Text,
            nullable: false,
        }],
    };
    cassie
        .midge
        .create_collection(&collection, schema.clone())
        .expect("create collection");
    cassie.register_collection(
        &collection,
        schema
            .fields
            .iter()
            .map(|field| (field.name.clone(), field.data_type.clone()))
            .collect(),
    );
    collection
}

#[test]
fn should_increment_the_durable_data_epoch_once_per_changed_batch() {
    // Arrange
    support::with_fallback();
    let path = support::data_dir("durable_data_epoch");
    let cassie = Cassie::new_with_data_dir(&path).expect("create Cassie");
    cassie.startup().expect("start Cassie");
    let collection = register_collection(&cassie, "durable_data_epoch");
    assert_eq!(cassie.midge.data_epoch().expect("read epoch"), 0);

    // Act
    cassie
        .midge
        .put_document(
            &collection,
            Some("first".to_string()),
            serde_json::json!({"title": "first"}),
        )
        .expect("write first row");
    let after_put = cassie.midge.data_epoch().expect("read epoch after put");
    let deleted = cassie
        .midge
        .delete_document(&collection, "missing")
        .expect("delete missing row");
    let after_missing_delete = cassie.midge.data_epoch().expect("read epoch after no-op");
    cassie
        .midge
        .delete_document(&collection, "first")
        .expect("delete row");
    let after_delete = cassie.midge.data_epoch().expect("read epoch after delete");

    // Assert
    assert!(!deleted);
    assert_eq!(after_put, 1);
    assert_eq!(after_missing_delete, after_put);
    assert_eq!(after_delete, 2);
}

#[test]
fn should_hydrate_the_durable_data_epoch_after_restart() {
    // Arrange
    support::with_fallback();
    let path = support::data_dir("durable_data_epoch_restart");
    let cassie = Cassie::new_with_data_dir(&path).expect("create Cassie");
    cassie.startup().expect("start Cassie");
    let collection = register_collection(&cassie, "durable_data_epoch_restart");
    cassie
        .midge
        .put_document(
            &collection,
            Some("first".to_string()),
            serde_json::json!({"title": "first"}),
        )
        .expect("write row");
    drop(cassie);

    // Act
    let restarted = Cassie::new_with_data_dir(&path).expect("reopen Cassie");
    restarted.startup().expect("restart Cassie");

    // Assert
    assert_eq!(restarted.midge.data_epoch().expect("read durable epoch"), 1);
}
