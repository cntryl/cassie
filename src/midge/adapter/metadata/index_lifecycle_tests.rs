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
