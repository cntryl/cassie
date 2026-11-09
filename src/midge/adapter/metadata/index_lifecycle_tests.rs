use std::collections::BTreeMap;
use std::sync::{mpsc, Arc};
use std::thread;
use std::time::Duration;

use crate::catalog::{Catalog, IndexKind, IndexMeta};
use crate::midge::adapter::{FieldSchema, Midge, Schema};
use crate::types::DataType;

thread_local! {
    static AFTER_JOURNAL_CHECK: std::cell::RefCell<Option<Box<dyn FnOnce()>>> = std::cell::RefCell::new(None);
}

pub(super) fn after_journal_check() {
    let observer = AFTER_JOURNAL_CHECK.with(|slot| slot.borrow_mut().take());
    if let Some(observer) = observer {
        observer();
    }
}

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

#[test]
fn should_publish_and_register_target_when_an_unrelated_journal_appears_after_the_check() {
    // Arrange
    let cassie = crate::app::Cassie::new_in_memory_with_config(
        crate::config::CassieRuntimeConfig::default(),
    )
    .expect("Cassie");
    cassie.startup().expect("startup");
    let session = cassie.create_session("tester", None);
    cassie
        .execute_sql(&session, "CREATE TABLE target_rows (id INT)", vec![])
        .expect("target");
    cassie
        .execute_sql(&session, "CREATE TABLE other_rows (id INT)", vec![])
        .expect("other");
    let other = IndexMeta {
        definition_semantics_version: 0,
        collection: "other_rows".to_owned(),
        name: "other_idx".to_owned(),
        field: "id".to_owned(),
        fields: vec!["id".to_owned()],
        expressions: Vec::new(),
        include_fields: Vec::new(),
        predicate: None,
        kind: IndexKind::Scalar,
        unique: false,
        options: BTreeMap::new(),
    };
    let midge = Arc::clone(&cassie.midge);
    AFTER_JOURNAL_CHECK.with(|slot| {
        assert!(slot
            .borrow_mut()
            .replace(Box::new(move || {
                std::thread::spawn(move || {
                    crate::midge::adapter::set_index_publication_failure_point(true);
                    let result = midge.put_index(&other);
                    crate::midge::adapter::set_index_publication_failure_point(false);
                    assert!(result.is_err(), "leave an unrelated prepared journal");
                })
                .join()
                .expect("journal worker");
            }))
            .is_none());
    });

    // Act
    let target = cassie.execute_sql(
        &session,
        "CREATE INDEX target_idx ON target_rows (id)",
        vec![],
    );

    // Assert
    target.expect("target publication remains successful");
    assert!(cassie
        .catalog
        .get_index("target_rows", "target_idx")
        .is_some());
    assert!(cassie
        .midge
        .get_index("target_rows", "target_idx")
        .expect("durable target")
        .is_some());
    assert!(cassie
        .midge
        .has_prepared_index_publication("other_rows", "other_idx")
        .expect("journal remains"));
    cassie
        .midge
        .replay_pending_index_publications()
        .expect("explicit recovery");
    assert!(!cassie
        .midge
        .has_prepared_index_publication("other_rows", "other_idx")
        .expect("journal removed"));
}

#[test]
fn should_not_defer_drop_while_a_same_name_publication_requires_recovery() {
    // Arrange
    let (midge, _catalog, collection, name) = index_metadata_fixture();
    let index = midge
        .get_index(&collection, &name)
        .expect("read")
        .expect("index");
    crate::midge::adapter::set_index_publication_failure_point(true);
    let prepared = midge.put_index(&index);
    crate::midge::adapter::set_index_publication_failure_point(false);
    assert!(
        prepared.is_err(),
        "injection must leave a prepared publication"
    );

    // Act
    let drop_attempt = midge.defer_drop_index(&collection, &name, 0);
    midge
        .replay_pending_index_publications()
        .expect("recover journal");

    // Assert
    assert!(
        drop_attempt.is_err(),
        "DROP must not leave a future resurrection"
    );
    assert!(midge
        .pending_schema_cleanups()
        .expect("cleanup markers")
        .is_empty());
    assert!(midge.get_index(&collection, &name).expect("read").is_some());
}

#[test]
fn should_recover_an_observed_unrelated_journal_before_preparing_a_new_index() {
    // Arrange
    let (midge, _catalog, collection, name) = index_metadata_fixture();
    let index = midge
        .get_index(&collection, &name)
        .expect("read")
        .expect("index");
    crate::midge::adapter::set_index_publication_failure_point(true);
    let prepared = midge.put_index(&index);
    crate::midge::adapter::set_index_publication_failure_point(false);
    assert!(prepared.is_err(), "leave an older prepared record");
    let mut other = index.clone();
    other.name = "unrelated_pending_idx".to_owned();

    // Act
    let rejected = midge.put_index(&other);

    // Assert
    assert!(
        rejected.is_err(),
        "observed journal must be recovered before new preparation"
    );
    assert!(midge
        .get_index(&collection, &other.name)
        .expect("read")
        .is_none());
    midge
        .replay_pending_index_publications()
        .expect("recover older journal");
    midge.put_index(&other).expect("retry new index");
}

#[test]
fn should_reject_index_rename_when_source_or_destination_has_a_prepared_journal() {
    // Arrange
    for pending_source in [true, false] {
        let (midge, _catalog, collection, name) = index_metadata_fixture();
        let mut pending = midge
            .get_index(&collection, &name)
            .expect("read")
            .expect("index");
        let destination = "rename_pending_idx";
        if !pending_source {
            pending.name = destination.to_owned();
        }
        crate::midge::adapter::set_index_publication_failure_point(true);
        let prepared = midge.put_index(&pending);
        crate::midge::adapter::set_index_publication_failure_point(false);
        assert!(prepared.is_err(), "leave prepared journal");
        let gate = midge.collection_write_gate(&collection);
        let _guard = gate.lock();

        // Act
        let renamed = midge.rename_index_and_constraints_with_held_collection_gate(
            &collection,
            &name,
            destination,
            &[],
        );

        // Assert
        assert!(
            renamed.is_err(),
            "pending source or destination must block rename"
        );
        assert!(midge.get_index(&collection, &name).expect("read").is_some());
        drop(_guard);
        midge
            .replay_pending_index_publications()
            .expect("recover journal");
    }
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
