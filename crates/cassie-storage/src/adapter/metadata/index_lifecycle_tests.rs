use std::collections::BTreeMap;
use std::sync::{mpsc, Arc};
use std::thread;
use std::time::Duration;

use crate::adapter::{FieldSchema, Midge, Schema};
use crate::catalog::{IndexKind, IndexMeta};
use cassie_core::types::DataType;

fn index_metadata_fixture() -> (
    Arc<Midge>,
    Arc<parking_lot::Mutex<IndexMeta>>,
    String,
    String,
) {
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
    let published = Arc::new(parking_lot::Mutex::new(initial));
    (midge, published, collection, name)
}

#[test]
fn should_not_defer_drop_while_a_same_name_publication_requires_recovery() {
    // Arrange
    let (midge, _catalog, collection, name) = index_metadata_fixture();
    let index = midge
        .get_index(&collection, &name)
        .expect("read")
        .expect("index");
    crate::adapter::set_index_publication_failure_point(true);
    let prepared = midge.put_index(&index);
    crate::adapter::set_index_publication_failure_point(false);
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
    assert_eq!(
        midge
            .pending_schema_cleanups()
            .expect("cleanup markers")
            .len(),
        0
    );
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
    crate::adapter::set_index_publication_failure_point(true);
    let prepared = midge.put_index(&index);
    crate::adapter::set_index_publication_failure_point(false);
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
        crate::adapter::set_index_publication_failure_point(true);
        let prepared = midge.put_index(&pending);
        crate::adapter::set_index_publication_failure_point(false);
        assert!(prepared.is_err(), "leave prepared journal");
        let gate = midge.collection_write_gate(&collection);
        let guard = gate.lock();

        // Act
        let renamed = midge.rename_index_and_constraints_with_held_collection_gate::<()>(
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
        drop(guard);
        midge
            .replay_pending_index_publications()
            .expect("recover journal");
    }
}

fn update_and_publish_fillfactor(
    midge: &Midge,
    published: &parking_lot::Mutex<IndexMeta>,
    collection: &str,
    name: &str,
    fillfactor: &str,
    before_publish: impl FnOnce(),
) -> Result<(), crate::adapter::CassieError> {
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
            *published.lock() = updated;
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
    let cached = catalog.lock();

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
    let expression = cassie_sql::parser::parse_expression("(SELECT 1)").expect("parse");
    metadata
        .expressions
        .push(serde_json::to_string(&expression).expect("encode"));
    let key = Midge::index_key(&collection, &name);
    let original = serde_json::to_vec(&metadata).expect("encode metadata");
    midge
        .raw_put(crate::adapter::StorageFamily::Data, &key, &original)
        .expect("seed");
    let gate = midge.collection_write_gate(&collection);
    let _guard = gate.lock();
    let mut updated = false;
    let mut published = false;

    // Act
    let rename = midge.rename_index_and_constraints_with_held_collection_gate::<()>(
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
        matches!(rename, Err( crate::adapter::CassieError::Unsupported(message)) if message.contains("index expressions cannot contain subqueries"))
    );
    assert!(
        matches!(update, Err( crate::adapter::CassieError::Unsupported(message)) if message.contains("index expressions cannot contain subqueries"))
    );
    assert!(!updated);
    assert!(!published);
    assert_eq!(
        midge
            .raw_get(crate::adapter::StorageFamily::Data, &key)
            .expect("read"),
        Some(original)
    );
    let renamed_key = Midge::index_key(&collection, "renamed_idx");
    assert!(midge
        .raw_get(crate::adapter::StorageFamily::Data, &renamed_key)
        .expect("read")
        .is_none());
}

#[test]
fn should_reject_unsafe_index_replacement_before_durable_or_catalog_publication() {
    // Arrange
    let (midge, _catalog, collection, name) = index_metadata_fixture();
    let key = Midge::index_key(&collection, &name);
    let original = midge
        .raw_get(crate::adapter::StorageFamily::Data, &key)
        .expect("read");
    let expression = cassie_sql::parser::parse_expression("(SELECT 1)").expect("parse");
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
        matches!(result, Err( crate::adapter::CassieError::Unsupported(message)) if message.contains("index expressions cannot contain subqueries"))
    );
    assert!(!published);
    assert_eq!(
        midge
            .raw_get(crate::adapter::StorageFamily::Data, &key)
            .expect("read"),
        original
    );
}

#[test]
fn should_preserve_collision_with_identity_errors_before_definition_checks() {
    // Arrange
    let (midge, _catalog, collection, name) = index_metadata_fixture();
    let mut metadata = midge
        .get_index(&collection, &name)
        .expect("read")
        .expect("index");
    let expression = cassie_sql::parser::parse_expression("(SELECT 1)").expect("parse");
    let encoded = serde_json::to_string(&expression).expect("encode");
    metadata.expressions.push(encoded.clone());
    let key = Midge::index_key(&collection, &name);
    midge
        .raw_put(
            crate::adapter::StorageFamily::Data,
            &key,
            &serde_json::to_vec(&metadata).expect("encode"),
        )
        .expect("seed");
    let next_key = Midge::index_key(&collection, "occupied_idx");
    midge
        .raw_put(crate::adapter::StorageFamily::Data, &next_key, b"occupied")
        .expect("seed destination");
    let gate = midge.collection_write_gate(&collection);
    let guard = gate.lock();
    // Act
    let collision = midge.rename_index_and_constraints_with_held_collection_gate::<()>(
        &collection,
        &name,
        "occupied_idx",
        &[],
    );
    drop(guard);
    metadata.expressions.clear();
    midge
        .raw_put(
            crate::adapter::StorageFamily::Data,
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
        Err(crate::adapter::CassieError::CatalogObjectAlreadyExists { .. })
    ));
    assert!(
        matches!(identity, Err( crate::adapter::CassieError::Execution(message)) if message.contains("cannot change index identity"))
    );
}

#[test]
fn should_reject_unknown_index_definition_version_before_update_or_publication() {
    // Arrange
    let (midge, _catalog, collection, name) = index_metadata_fixture();
    let key = Midge::index_key(&collection, &name);
    let original = midge
        .raw_get(crate::adapter::StorageFamily::Data, &key)
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
        matches!(result, Err( crate::adapter::CassieError::Unsupported(message)) if message.contains("unknown persisted definition semantics version 2"))
    );
    assert!(!published);
    assert_eq!(
        midge
            .raw_get(crate::adapter::StorageFamily::Data, &key)
            .expect("read"),
        original
    );
}

#[test]
fn should_reject_unknown_index_definition_version_before_rename_or_update() {
    // Arrange
    let (midge, _catalog, collection, name) = index_metadata_fixture();
    let key = Midge::index_key(&collection, &name);
    let mut unknown = midge
        .get_index(&collection, &name)
        .expect("read")
        .expect("index");
    unknown.definition_semantics_version = 2;
    let bytes = serde_json::to_vec(&unknown).expect("encode");
    midge
        .raw_put(crate::adapter::StorageFamily::Data, &key, &bytes)
        .expect("seed");
    let mut updated = false;
    let gate = midge.collection_write_gate(&collection);
    let guard = gate.lock();
    // Act
    let rename = midge.rename_index_and_constraints_with_held_collection_gate::<()>(
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
        matches!(rename, Err( crate::adapter::CassieError::Unsupported(message)) if message.contains("unknown persisted definition semantics version 2"))
    );
    assert!(
        matches!(update, Err( crate::adapter::CassieError::Unsupported(message)) if message.contains("unknown persisted definition semantics version 2"))
    );
    assert!(!updated);
    assert_eq!(
        midge
            .raw_get(crate::adapter::StorageFamily::Data, &key)
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
        matches!(transition, Err( crate::adapter::CassieError::Unsupported(message)) if message.contains("cannot change index definition version"))
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
            crate::adapter::StorageFamily::Data,
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

#[test]
fn should_leave_dropped_index_artifacts_absent_when_an_old_rebuild_resumes() {
    // Arrange
    let midge = Midge::new_in_memory("postgres").expect("memory adapter");
    let collection = midge.canonical_collection_name("stale_rebuild");
    midge
        .create_collection(
            &collection,
            Schema {
                fields: vec![
                    FieldSchema {
                        name: "body".to_owned(),
                        data_type: DataType::Text,
                        nullable: true,
                    },
                    FieldSchema {
                        name: "at".to_owned(),
                        data_type: DataType::Timestamp,
                        nullable: true,
                    },
                ],
            },
        )
        .expect("create source table");

    for (kind, field, options) in [
        (IndexKind::FullText, "body", BTreeMap::new()),
        (
            IndexKind::TimeSeries,
            "at",
            BTreeMap::from([("bucket_width".to_owned(), "1 hour".to_owned())]),
        ),
    ] {
        let declaration = IndexMeta {
            collection: collection.clone(),
            name: "active".to_owned(),
            field: field.to_owned(),
            fields: vec![field.to_owned()],
            definition_semantics_version: 0,
            expressions: Vec::new(),
            include_fields: Vec::new(),
            predicate: None,
            kind,
            unique: false,
            options,
        };
        assert_index_rebuild_stays_retired(&midge, &declaration);
    }
}

fn assert_index_rebuild_stays_retired(midge: &Midge, declaration: &IndexMeta) {
    let collection = &declaration.collection;
    midge.put_index(declaration).expect("create index");
    let old = midge
        .get_index(collection, &declaration.name)
        .expect("read catalog")
        .expect("published index");
    let old_prefix = rebuild_artifact_prefix(&old);
    assert!(!midge
        .raw_scan_prefix_for_collection(collection, &old_prefix)
        .expect("live artifacts")
        .is_empty());
    let (ready, captured) = mpsc::channel();
    let (resume, wait) = mpsc::channel();
    let (rebuilt, first_finished) = mpsc::channel();

    // Act
    let (after_drop, after_old_rebuild, replacement_prefix) = thread::scope(|scope| {
        let (worker_midge, worker_old) = (midge, &old);
        let (worker_collection, worker_prefix) = (collection, &old_prefix);
        let worker = scope.spawn(move || {
            ready.send(()).expect("old definition captured");
            wait.recv_timeout(Duration::from_secs(5))
                .expect("drop completed");
            rebuild_observed_index(worker_midge, worker_old);
            let result =
                worker_midge.raw_scan_prefix_for_collection(worker_collection, worker_prefix);
            rebuilt
                .send(result)
                .expect("record retired prefix before recreation");
            wait.recv_timeout(Duration::from_secs(5))
                .expect("replacement published");
            rebuild_observed_index(worker_midge, worker_old);
        });
        captured
            .recv_timeout(Duration::from_secs(5))
            .expect("rebuild input captured");
        midge
            .delete_index(collection, &declaration.name)
            .expect("drop index");
        let after_drop = midge
            .raw_scan_prefix_for_collection(collection, &old_prefix)
            .expect("inspect prefix after drop");
        assert!(midge
            .get_index(collection, &declaration.name)
            .expect("catalog read")
            .is_none());
        resume.send(()).expect("release rebuild");
        let after_old_rebuild = first_finished
            .recv_timeout(Duration::from_secs(5))
            .expect("rebuild completed before recreation")
            .expect("inspect dropped artifact prefix");
        midge.put_index(declaration).expect("recreate same name");
        let replacement = midge
            .get_index(collection, &declaration.name)
            .expect("read catalog")
            .expect("replacement");
        resume
            .send(())
            .expect("release stale post-recreation rebuild");
        worker.join().expect("rebuild worker");
        rebuild_observed_index(midge, &replacement);
        (
            after_drop,
            after_old_rebuild,
            rebuild_artifact_prefix(&replacement),
        )
    });

    // Assert
    assert!(after_drop.is_empty());
    assert!(after_old_rebuild.is_empty());
    assert_ne!(old_prefix, replacement_prefix);
    assert!(midge
        .raw_scan_prefix_for_collection(collection, &old_prefix)
        .expect("inspect old prefix after recreation")
        .is_empty());
    assert!(!midge
        .raw_scan_prefix_for_collection(collection, &replacement_prefix)
        .expect("inspect replacement prefix")
        .is_empty());
    midge
        .delete_index(collection, &declaration.name)
        .expect("drop replacement");
}

fn rebuild_artifact_prefix(index: &IndexMeta) -> Vec<u8> {
    let relation_id = index.relation_id().expect("relation id");
    let index_id = index.storage_id().expect("index id");
    if index.kind == IndexKind::FullText {
        Midge::fulltext_index_artifact_prefix(relation_id, index_id)
    } else {
        crate::adapter::key_encoding::time_series_index_artifact_prefix(relation_id, index_id)
    }
}

fn rebuild_observed_index(midge: &Midge, index: &IndexMeta) {
    if index.kind == IndexKind::FullText {
        midge
            .rebuild_fulltext_index_for_index(index)
            .expect("rebuild");
    } else {
        midge
            .rebuild_time_series_index_for_index(index)
            .expect("rebuild");
    }
}
