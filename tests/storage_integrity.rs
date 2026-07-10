use cassie::app::Cassie;
use cassie::catalog::canonical_relation_name;
use cassie::midge::adapter::set_document_write_conflicts_remaining;
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

#[test]
fn should_increment_data_epoch_for_concurrent_writes_to_different_collections() {
    // Arrange
    support::with_fallback();
    let path = support::data_dir("concurrent_data_epoch");
    let cassie = std::sync::Arc::new(Cassie::new_with_data_dir(&path).expect("create Cassie"));
    cassie.startup().expect("start Cassie");
    let first = register_collection(&cassie, "concurrent_data_epoch_first");
    let second = register_collection(&cassie, "concurrent_data_epoch_second");
    let barrier = std::sync::Arc::new(std::sync::Barrier::new(3));
    let workers = [first, second]
        .into_iter()
        .enumerate()
        .map(|(index, collection)| {
            let cassie = std::sync::Arc::clone(&cassie);
            let barrier = std::sync::Arc::clone(&barrier);
            std::thread::spawn(move || {
                barrier.wait();
                cassie.midge.put_document(
                    &collection,
                    Some(format!("row-{index}")),
                    serde_json::json!({"title": format!("row-{index}")}),
                )
            })
        })
        .collect::<Vec<_>>();
    barrier.wait();

    // Act
    let results = workers
        .into_iter()
        .map(|worker| worker.join().expect("worker completed"))
        .collect::<Result<Vec<_>, _>>();

    // Assert
    assert!(results.is_ok());
    assert_eq!(cassie.midge.data_epoch().expect("read data epoch"), 2);
}

#[test]
fn should_leave_data_unchanged_when_write_conflict_retries_are_exhausted() {
    // Arrange
    support::with_fallback();
    let path = support::data_dir("write_conflict_retry_exhaustion");
    let cassie = Cassie::new_with_data_dir(&path).expect("create Cassie");
    cassie.startup().expect("start Cassie");
    let collection = register_collection(&cassie, "write_conflict_retry_exhaustion");
    set_document_write_conflicts_remaining(8);

    // Act
    let result = cassie.midge.put_document(
        &collection,
        Some("row-1".to_string()),
        serde_json::json!({"title": "alpha"}),
    );
    set_document_write_conflicts_remaining(0);

    // Assert
    assert!(matches!(
        result,
        Err(cassie::app::CassieError::StorageRetryable(_))
    ));
    assert!(cassie
        .midge
        .get_document(&collection, "row-1")
        .expect("read row")
        .is_none());
    assert_eq!(cassie.midge.data_epoch().expect("read data epoch"), 0);
}

#[test]
fn should_persist_collection_generation_for_changed_writes_only() {
    // Arrange
    support::with_fallback();
    let path = support::data_dir("collection_generation");
    let cassie = Cassie::new_with_data_dir(&path).expect("create Cassie");
    cassie.startup().expect("start Cassie");
    let collection = register_collection(&cassie, "collection_generation");
    assert_eq!(
        cassie
            .midge
            .collection_generation(&collection)
            .expect("read generation"),
        0
    );

    // Act
    cassie
        .midge
        .put_document(
            &collection,
            Some("row-1".to_string()),
            serde_json::json!({"title": "alpha"}),
        )
        .expect("write row");
    let after_write = cassie
        .midge
        .collection_generation(&collection)
        .expect("read generation after write");
    cassie
        .midge
        .delete_document(&collection, "missing")
        .expect("delete missing row");
    let after_no_op = cassie
        .midge
        .collection_generation(&collection)
        .expect("read generation after no-op");
    drop(cassie);
    let restarted = Cassie::new_with_data_dir(&path).expect("reopen Cassie");
    restarted.startup().expect("restart Cassie");

    // Assert
    assert_eq!(after_write, 1);
    assert_eq!(after_no_op, after_write);
    assert_eq!(
        restarted
            .midge
            .collection_generation(&collection)
            .expect("read durable generation"),
        after_write
    );
}
