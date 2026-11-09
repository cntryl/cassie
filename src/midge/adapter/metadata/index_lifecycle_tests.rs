use std::collections::BTreeMap;
use std::sync::{mpsc, Arc};
use std::thread;
use std::time::Duration;

use crate::catalog::{Catalog, IndexKind, IndexMeta};
use crate::midge::adapter::{FieldSchema, Midge, Schema};
use crate::types::DataType;

fn index_metadata_fixture() -> (Arc<Midge>, Arc<Catalog>, String, String) {
    let midge = Arc::new(Midge::new_in_memory("postgres").expect("create Midge"));
    midge
        .create_collection(
            "index_metadata_publication_order",
            Schema {
                fields: vec![FieldSchema {
                    name: "payload".to_string(),
                    data_type: DataType::Text,
                    nullable: true,
                }],
            },
        )
        .expect("create collection");
    let initial = IndexMeta {
        definition_semantics_version: 0,
        collection: "index_metadata_publication_order".to_string(),
        name: "index_metadata_publication_order_idx".to_string(),
        field: "payload".to_string(),
        fields: vec!["payload".to_string()],
        expressions: Vec::new(),
        include_fields: Vec::new(),
        predicate: None,
        kind: IndexKind::Scalar,
        unique: false,
        options: BTreeMap::new(),
    };
    midge.put_index(&initial).expect("publish index metadata");
    let initial = midge
        .get_index(&initial.collection, &initial.name)
        .expect("read initial metadata")
        .expect("index should exist");
    let collection = initial.collection.clone();
    let name = initial.name.clone();
    let catalog = Arc::new(Catalog::new());
    catalog.register_index(initial);
    (midge, catalog, collection, name)
}

fn update_and_publish_fillfactor(
    midge: &Midge,
    catalog: &Catalog,
    collection: &str,
    name: &str,
    fillfactor: &str,
    before_publish: impl FnOnce(),
) -> Result<(), crate::app::CassieError> {
    midge.update_index_metadata(
        collection,
        name,
        |metadata| {
            metadata
                .options
                .insert("fillfactor".to_string(), fillfactor.to_string());
            Ok(())
        },
        |updated| {
            before_publish();
            catalog.register_index(updated);
        },
    )
}

#[test]
fn should_serialize_index_metadata_publication_order() {
    // Arrange
    let (midge, catalog, collection, name) = index_metadata_fixture();
    let (first_update_tx, first_update_rx) = mpsc::channel();
    let (release_first_tx, release_first_rx) = mpsc::channel();
    let (second_gate_tx, second_gate_rx) = mpsc::channel();
    let (second_update_tx, second_update_rx) = mpsc::channel();
    let first_midge = Arc::clone(&midge);
    let first_catalog = Arc::clone(&catalog);
    let first_collection = collection.clone();
    let first_name = name.clone();
    let first = thread::spawn(move || {
        update_and_publish_fillfactor(
            &first_midge,
            &first_catalog,
            &first_collection,
            &first_name,
            "81",
            || {
                first_update_tx
                    .send(())
                    .expect("signal first durable update");
                release_first_rx
                    .recv_timeout(Duration::from_secs(2))
                    .expect("release first catalog publication");
            },
        )
        .expect("persist first option update");
    });

    first_update_rx
        .recv_timeout(Duration::from_secs(2))
        .expect("first durable update should finish");
    let second_midge = Arc::clone(&midge);
    let second_catalog = Arc::clone(&catalog);
    let second_collection = collection.clone();
    let second_name = name.clone();
    let second = thread::spawn(move || {
        let gate = second_midge.collection_write_gate(&second_collection);
        second_gate_tx
            .send(gate.try_lock().is_none())
            .expect("signal collection gate state");
        update_and_publish_fillfactor(
            &second_midge,
            &second_catalog,
            &second_collection,
            &second_name,
            "82",
            || {
                second_update_tx
                    .send(())
                    .expect("signal second catalog publication");
            },
        )
        .expect("persist second option update");
    });

    // Act
    let second_observed_first_gate = second_gate_rx
        .recv_timeout(Duration::from_secs(2))
        .expect("second update should reach the collection gate");
    if second_observed_first_gate {
        release_first_tx
            .send(())
            .expect("release first catalog publication");
        second_update_rx
            .recv_timeout(Duration::from_secs(2))
            .expect("second update should publish after the first");
    } else {
        second_update_rx
            .recv_timeout(Duration::from_secs(2))
            .expect("ungated second update should finish before publication resumes");
        release_first_tx
            .send(())
            .expect("release first catalog publication");
    }
    first.join().expect("join first update");
    second.join().expect("join second update");

    let durable = midge
        .get_index(&collection, &name)
        .expect("read durable metadata")
        .expect("index should remain present");
    let cached = catalog
        .get_index(&collection, &name)
        .expect("read catalog cache");

    // Assert
    assert!(second_observed_first_gate);
    assert_eq!(durable.options.get("fillfactor"), Some(&"82".to_string()));
    assert_eq!(cached.options, durable.options);
}

#[test]
fn should_reject_unsafe_index_metadata_before_lifecycle_mutation() {
    // Arrange
    let (midge, _catalog, collection, name) = index_metadata_fixture();
    let mut metadata = midge
        .get_index(&collection, &name)
        .expect("read")
        .expect("index");
    let expression = crate::sql::parser::parse_expression("(SELECT 1)").expect("parse");
    metadata
        .expressions
        .push(serde_json::to_string(&expression).expect("encode"));
    let key = Midge::index_key(&collection, &name);
    let original = serde_json::to_vec(&metadata).expect("encode metadata");
    midge
        .raw_put(crate::midge::adapter::StorageFamily::Data, &key, &original)
        .expect("seed");
    let gate = midge.collection_write_gate(&collection);
    let _guard = gate.lock();
    let mut updated = false;
    let mut published = false;

    // Act
    let rename = midge.rename_index_and_constraints_with_held_collection_gate(
        &collection,
        &name,
        "renamed_idx",
        &[],
    );
    let update = midge.update_index_metadata(
        &collection,
        &name,
        |_| {
            updated = true;
            Ok(())
        },
        |_| published = true,
    );

    // Assert
    assert!(
        matches!(rename, Err(crate::app::CassieError::Unsupported(message)) if message.contains("index expressions cannot contain subqueries"))
    );
    assert!(
        matches!(update, Err(crate::app::CassieError::Unsupported(message)) if message.contains("index expressions cannot contain subqueries"))
    );
    assert!(!updated);
    assert!(!published);
    assert_eq!(
        midge
            .raw_get(crate::midge::adapter::StorageFamily::Data, &key)
            .expect("read"),
        Some(original)
    );
    let renamed_key = Midge::index_key(&collection, "renamed_idx");
    assert!(midge
        .raw_get(crate::midge::adapter::StorageFamily::Data, &renamed_key)
        .expect("read")
        .is_none());
}

#[test]
fn should_reject_unsafe_index_replacement_before_durable_or_catalog_publication() {
    // Arrange
    let (midge, _catalog, collection, name) = index_metadata_fixture();
    let key = Midge::index_key(&collection, &name);
    let original = midge
        .raw_get(crate::midge::adapter::StorageFamily::Data, &key)
        .expect("read");
    let expression = crate::sql::parser::parse_expression("(SELECT 1)").expect("parse");
    let encoded = serde_json::to_string(&expression).expect("encode");
    let mut published = false;

    // Act
    let result = midge.update_index_metadata(
        &collection,
        &name,
        |metadata| {
            metadata.predicate = Some(encoded);
            Ok(())
        },
        |_| published = true,
    );

    // Assert
    assert!(
        matches!(result, Err(crate::app::CassieError::Unsupported(message)) if message.contains("index expressions cannot contain subqueries"))
    );
    assert!(!published);
    assert_eq!(
        midge
            .raw_get(crate::midge::adapter::StorageFamily::Data, &key)
            .expect("read"),
        original
    );
}

#[test]
fn should_preserve_collision_and_identity_errors_before_definition_checks() {
    // Arrange
    let (midge, _catalog, collection, name) = index_metadata_fixture();
    let mut metadata = midge
        .get_index(&collection, &name)
        .expect("read")
        .expect("index");
    let expression = crate::sql::parser::parse_expression("(SELECT 1)").expect("parse");
    let encoded = serde_json::to_string(&expression).expect("encode");
    metadata.expressions.push(encoded.clone());
    let key = Midge::index_key(&collection, &name);
    midge
        .raw_put(
            crate::midge::adapter::StorageFamily::Data,
            &key,
            &serde_json::to_vec(&metadata).expect("encode"),
        )
        .expect("seed");
    let next_key = Midge::index_key(&collection, "occupied_idx");
    midge
        .raw_put(
            crate::midge::adapter::StorageFamily::Data,
            &next_key,
            b"occupied",
        )
        .expect("seed destination");
    let gate = midge.collection_write_gate(&collection);
    let guard = gate.lock();
    // Act
    let collision = midge.rename_index_and_constraints_with_held_collection_gate(
        &collection,
        &name,
        "occupied_idx",
        &[],
    );
    drop(guard);
    metadata.expressions.clear();
    midge
        .raw_put(
            crate::midge::adapter::StorageFamily::Data,
            &key,
            &serde_json::to_vec(&metadata).expect("encode"),
        )
        .expect("restore");
    let identity = midge.update_index_metadata(
        &collection,
        &name,
        |updated| {
            updated.name = "other".into();
            updated.predicate = Some(encoded);
            Ok(())
        },
        |_| panic!("must not publish"),
    );
    // Assert
    assert!(matches!(
        collision,
        Err(crate::app::CassieError::CatalogObjectAlreadyExists { .. })
    ));
    assert!(
        matches!(identity, Err(crate::app::CassieError::Execution(message)) if message.contains("cannot change index identity"))
    );
}

#[test]
fn should_reject_unknown_index_definition_version_before_update_or_publication() {
    // Arrange
    let (midge, _catalog, collection, name) = index_metadata_fixture();
    let key = Midge::index_key(&collection, &name);
    let original = midge
        .raw_get(crate::midge::adapter::StorageFamily::Data, &key)
        .expect("read");
    let mut published = false;
    // Act
    let result = midge.update_index_metadata(
        &collection,
        &name,
        |updated| {
            updated.definition_semantics_version = 2;
            Ok(())
        },
        |_| published = true,
    );
    // Assert
    assert!(
        matches!(result, Err(crate::app::CassieError::Unsupported(message)) if message.contains("unknown persisted definition semantics version 2"))
    );
    assert!(!published);
    assert_eq!(
        midge
            .raw_get(crate::midge::adapter::StorageFamily::Data, &key)
            .expect("read"),
        original
    );

    // Arrange
    let mut unknown = midge
        .get_index(&collection, &name)
        .expect("read")
        .expect("index");
    unknown.definition_semantics_version = 2;
    let bytes = serde_json::to_vec(&unknown).expect("encode");
    midge
        .raw_put(crate::midge::adapter::StorageFamily::Data, &key, &bytes)
        .expect("seed");
    let mut updated = false;
    let gate = midge.collection_write_gate(&collection);
    let guard = gate.lock();
    // Act
    let rename = midge.rename_index_and_constraints_with_held_collection_gate(
        &collection,
        &name,
        "next_idx",
        &[],
    );
    drop(guard);
    let update = midge.update_index_metadata(
        &collection,
        &name,
        |_| {
            updated = true;
            Ok(())
        },
        |_| panic!("must not publish"),
    );
    // Assert
    assert!(
        matches!(rename, Err(crate::app::CassieError::Unsupported(message)) if message.contains("unknown persisted definition semantics version 2"))
    );
    assert!(
        matches!(update, Err(crate::app::CassieError::Unsupported(message)) if message.contains("unknown persisted definition semantics version 2"))
    );
    assert!(!updated);
    assert_eq!(
        midge
            .raw_get(crate::midge::adapter::StorageFamily::Data, &key)
            .expect("read"),
        Some(bytes)
    );
}

#[test]
fn should_preserve_definition_version_during_index_update() {
    // Arrange
    let (midge, _catalog, collection, name) = index_metadata_fixture();
    let mut published = false;
    // Act
    let transition = midge.update_index_metadata(
        &collection,
        &name,
        |updated| {
            updated.definition_semantics_version = 1;
            Ok(())
        },
        |_| published = true,
    );
    assert!(
        matches!(transition, Err(crate::app::CassieError::Unsupported(message)) if message.contains("cannot change index definition version"))
    );
    assert!(!published);
    let mut versioned = midge
        .get_index(&collection, &name)
        .expect("read")
        .expect("index");
    midge.require_definition_version(1).expect("admit layout");
    versioned.definition_semantics_version = 1;
    let key = Midge::index_key(&collection, &name);
    midge
        .raw_put(
            crate::midge::adapter::StorageFamily::Data,
            &key,
            &serde_json::to_vec(&versioned).expect("encode"),
        )
        .expect("seed versioned index");
    midge
        .update_index_metadata(
            &collection,
            &name,
            |updated| {
                updated.options.insert("fillfactor".into(), "80".into());
                Ok(())
            },
            |_| published = true,
        )
        .expect("unchanged version");
    let stored = midge
        .get_index(&collection, &name)
        .expect("read")
        .expect("index");
    // Assert
    assert!(published);
    assert_eq!(stored.definition_semantics_version, 1);
    assert_eq!(
        stored.options.get("fillfactor").map(String::as_str),
        Some("80")
    );
}
