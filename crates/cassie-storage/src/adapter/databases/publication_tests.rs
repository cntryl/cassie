use super::super::Midge;
use crate::adapter::request_test_support::TestRequest;
use crate::adapter::NativePublicationControlScope;
use crate::adapter::{CassieError, StorageRetryKind};
use std::cell::RefCell;
use std::sync::{mpsc, Arc};
use std::time::Duration;

#[test]
fn should_exclude_native_family_creation_until_schema_publication_retires() {
    // Arrange
    for quarantine in [false, true] {
        let midge = Arc::new(Midge::new_in_memory("postgres").expect("memory adapter"));
        midge
            .ensure_families_ready()
            .expect("bootstrap schema family");
        let other = Arc::clone(&midge);
        let (waiting_tx, waiting_rx) = mpsc::channel();

        // Act
        let worker = midge
            .with_schema_publication(|| {
                let worker = std::thread::spawn(move || {
                    let controls = TestRequest::default();
                    let _request = NativePublicationControlScope::enter(Some(&controls.context()));
                    let _wait = Midge::observe_next_publication_wait_for_test(move || {
                        waiting_tx
                            .send(())
                            .expect("physical family reached schema guard");
                    });
                    other.create_physical_family("guarded-database-family")
                });
                waiting_rx
                    .recv_timeout(Duration::from_secs(5))
                    .expect("actual publication wait");
                assert!(midge
                    .engine
                    .get_column_family("guarded-database-family")
                    .is_none());
                if quarantine {
                    midge.latch_storage_recovery_required_for_test();
                }
                Ok(worker)
            })
            .expect("held schema publication boundary");
        let outcome = worker.join().expect("physical publisher");

        // Assert
        if quarantine {
            assert!(
                matches!(outcome, Err(CassieError::RecoveryRequired(message)) if message.contains("storage recovery required"))
            );
            assert!(midge
                .engine
                .get_column_family("guarded-database-family")
                .is_none());
        } else {
            assert_eq!(
                outcome.expect("released publisher").name(),
                "guarded-database-family"
            );
            assert!(midge
                .engine
                .get_column_family("guarded-database-family")
                .is_some());
        }
    }
}

#[test]
fn should_retire_an_unpublished_staged_family_after_request_cancellation() {
    // Arrange
    for quarantine in [false, true] {
        let midge = Midge::new_in_memory("postgres").expect("memory adapter");
        let staged = midge
            .stage_database_family("canceled-stage")
            .expect("owned unpublished family");
        let controls = TestRequest::default();
        let _request = NativePublicationControlScope::enter(Some(&controls.context()));
        controls.cancel();
        if quarantine {
            midge.latch_storage_recovery_required_for_test();
        }

        // Act
        let result = midge.abort_staged_database_family(&staged);
        let remains = midge
            .engine
            .get_column_family(&staged.metadata.physical_family)
            .is_some();

        // Assert
        if quarantine {
            assert!(
                matches!(result, Err(CassieError::RecoveryRequired(message)) if message.contains("storage recovery required"))
            );
            assert!(remains, "unknown outcomes remain quarantined");
        } else {
            result.expect("known unpublished cleanup retires despite canceled request");
            assert!(!remains);
            assert!(midge
                .get_database("canceled-stage")
                .expect("metadata read")
                .is_none());
        }
    }
}

#[test]
fn should_preserve_native_create_error_when_family_becomes_visible() {
    // Arrange
    for unknown in [false, true] {
        let midge = Midge::new_in_memory("postgres").expect("memory adapter");
        midge.ensure_families_ready().expect("bootstrap");
        let name = "visible-after-failure";

        // Act
        let result = midge.create_physical_family_with(name, || {
            midge
                .engine
                .create_column_family(name)
                .expect("visible family");
            if unknown {
                Err(cntryl_midge::MidgeError::OutcomeUnknown(
                    "controlled unknown".to_owned(),
                ))
            } else {
                Err(cntryl_midge::MidgeError::Internal(
                    "controlled internal".to_owned(),
                ))
            }
        });

        // Assert
        assert!(midge.engine.get_column_family(name).is_some());
        let error = result.expect_err("native create error must not become success");
        assert!(error.to_string().contains(if unknown {
            "controlled unknown"
        } else {
            "controlled internal"
        }));
        assert_eq!(midge.check_storage_recovery_required().is_err(), unknown);
    }
}

#[test]
fn should_preserve_retryable_stall_before_physical_family_publication() {
    // Arrange
    let midge = Midge::new_in_memory("postgres").expect("memory adapter");
    midge.ensure_families_ready().expect("bootstrap");
    let name = "stalled-family";

    // Act
    let stalled = midge.create_physical_family_with(name, || {
        Err(cntryl_midge::MidgeError::WriteStall(
            "held WAL group".into(),
        ))
    });

    // Assert
    assert_eq!(
        stalled.unwrap_err().storage_retry_kind(),
        Some(StorageRetryKind::WriteStall)
    );
    assert!(midge.engine.get_column_family(name).is_none());
    midge
        .check_storage_recovery_required()
        .expect("known stall permits retry");
    let handle = midge
        .create_physical_family(name)
        .expect("explicit create retry");
    assert_eq!(
        midge.engine.get_column_family(name).unwrap().id(),
        handle.id()
    );
}

#[test]
fn should_reuse_existing_family_without_issuing_a_native_create() {
    // Arrange
    let midge = Midge::new_in_memory("postgres").expect("memory adapter");
    midge.ensure_families_ready().expect("bootstrap");
    let existing = midge
        .engine
        .create_column_family("existing-family")
        .expect("existing family");

    // Act
    let result = midge.create_physical_family_with("existing-family", || {
        panic!("existing family must not submit a native create")
    });

    // Assert
    assert_eq!(result.expect("reuse").id(), existing.id());
}

#[test]
fn should_recheck_family_after_waiting_for_schema_publication() {
    // Arrange
    let midge = Arc::new(Midge::new_in_memory("postgres").expect("memory adapter"));
    midge.ensure_families_ready().expect("bootstrap");
    let other = Arc::clone(&midge);
    let (waiting_tx, waiting_rx) = mpsc::channel();

    // Act
    let (worker, existing) = midge
        .with_schema_publication(|| {
            let worker = std::thread::spawn(move || {
                let controls = TestRequest::default();
                let _request = NativePublicationControlScope::enter(Some(&controls.context()));
                let _observer = Midge::observe_next_publication_wait_for_test(move || {
                    waiting_tx.send(()).expect("creator reached schema gate");
                });
                other.create_physical_family_with("racing-family", || {
                    panic!("must not issue duplicate create")
                })
            });
            waiting_rx
                .recv_timeout(Duration::from_secs(5))
                .expect("waiting creator");
            let existing = midge
                .engine
                .create_column_family("racing-family")
                .expect("other creator");
            Ok((worker, existing))
        })
        .expect("held schema gate");
    let result = worker.join().expect("worker").expect("reused family");

    // Assert
    assert_eq!(result.id(), existing.id());
}

#[test]
fn should_refuse_database_drop_for_a_current_row_without_catalog_or_sidecars() {
    use cntryl_midge::TransactionMode;

    // Arrange
    let midge = Midge::new_in_memory("postgres").expect("memory adapter");
    let database = "retained_row";
    midge
        .create_database(database, None)
        .expect("create database");
    let (key, row) = retained_row();
    let mut writer = midge
        .database_tx(database, TransactionMode::ReadWrite)
        .expect("native writer");
    writer
        .put(key.clone(), row.clone(), None)
        .expect("store raw row");
    writer
        .commit(midge.write_options_sync())
        .expect("publish raw row");
    assert_eq!(
        midge.database_catalog_entries(database).expect("catalog"),
        Vec::<crate::adapter::RawStorageEntry>::new(),
        "the test row has no corresponding catalog declaration",
    );

    // Act
    let result = midge.drop_database(database);

    // Assert
    assert!(matches!(result, Err(CassieError::Unsupported(message))
        if message.contains("is not empty")));
    let reader = midge
        .database_tx(database, TransactionMode::ReadOnly)
        .expect("database remains registered");
    assert_eq!(
        reader.get(&key).expect("read retained row").as_deref(),
        Some(row.as_slice())
    );
}

#[test]
fn should_refuse_database_drop_for_retained_hash_or_catalog_contents() {
    use cassie_core::types::{DataType, FieldSchema, Schema};
    use cntryl_midge::TransactionMode;

    // Arrange
    let midge = Midge::new_in_memory("postgres").expect("memory adapter");
    midge
        .create_database("retained_hash", None)
        .expect("hash database");
    midge
        .create_database("retained_catalog", None)
        .expect("catalog database");
    midge
        .create_database("empty", None)
        .expect("empty database");
    let mut writer = midge
        .database_tx("retained_hash", TransactionMode::ReadWrite)
        .expect("native writer");
    let key = super::key_encoding::row_hash_key("retained_hash.public.docs", "native-row");
    writer.put(key, vec![1], None).expect("store retained hash");
    writer
        .commit(midge.write_options_sync())
        .expect("publish retained hash");
    midge
        .create_collection(
            "retained_catalog.public.docs",
            Schema {
                fields: vec![FieldSchema {
                    name: "value".to_owned(),
                    data_type: DataType::Int,
                    nullable: false,
                }],
            },
        )
        .expect("create empty table");

    // Act
    let hash = midge.drop_database("retained_hash");
    let catalog = midge.drop_database("retained_catalog");
    let empty = midge.drop_database("empty");

    // Assert
    for result in [hash, catalog] {
        assert!(matches!(result, Err(CassieError::Unsupported(message))
            if message.contains("is not empty")));
    }
    empty.expect("drop without retained contents");
    assert!(midge
        .get_database("empty")
        .expect("read registry")
        .is_none());
}

type DropPause = Box<dyn FnOnce()>;

thread_local! {
    static AFTER_DROP_EMPTY_CHECK: RefCell<Option<DropPause>> = const { RefCell::new(None) };
}

pub(super) fn pause_after_drop_empty_check() {
    let pause = AFTER_DROP_EMPTY_CHECK.with(|slot| slot.borrow_mut().take());
    if let Some(pause) = pause {
        pause();
    }
}

thread_local! {
    static AFTER_DROP_RESOLUTION: RefCell<Option<DropPause>> = const { RefCell::new(None) };
}

pub(super) fn pause_after_drop_resolution() {
    if let Some(pause) = AFTER_DROP_RESOLUTION.with(RefCell::take) {
        pause();
    }
}

thread_local! {
    static AFTER_DROP_REGISTRY_RETIREMENT: RefCell<Option<DropPause>> = const { RefCell::new(None) };
}

pub(super) fn pause_after_drop_registry_retirement() {
    if let Some(pause) = AFTER_DROP_REGISTRY_RETIREMENT.with(RefCell::take) {
        pause();
    }
}

#[test]
fn should_recover_retired_drop_without_discarding_the_recreated_database() {
    // Arrange
    use cassie_core::request::RequestCancellation;

    let path =
        std::env::temp_dir().join(format!("cassie-drop-name-reuse-{}", uuid::Uuid::new_v4()));
    let midge = Midge::new_strict_with_data_dir(&path).expect("open local fixture");
    let database = "recreated";
    midge.create_database(database, None).unwrap();
    let original = midge.database_family(database).unwrap().name().to_owned();
    let cancellation = RequestCancellation::new();
    let controls = TestRequest::with_cancellation(cancellation.clone());
    AFTER_DROP_REGISTRY_RETIREMENT.with(|slot| {
        slot.replace(Some(Box::new(move || cancellation.cancel())));
    });

    // Act
    let interrupted = {
        let _request = NativePublicationControlScope::enter(Some(&controls.context()));
        midge.drop_database(database)
    };
    assert!(matches!(interrupted, Err(CassieError::QueryCanceled)));
    assert!(midge.get_database(database).unwrap().is_none());
    assert!(midge.engine.get_column_family(&original).is_some());

    midge.create_database(database, None).unwrap();
    let replacement = midge.database_family(database).unwrap().name().to_owned();
    let (key, row) = retained_row();
    let mut write = midge
        .database_tx(database, super::TransactionMode::ReadWrite)
        .unwrap();
    write.put(key.clone(), row.clone(), None).unwrap();
    write.commit(midge.write_options_sync()).unwrap();
    drop(midge);

    // Assert
    assert_ne!(original, replacement);
    for _ in 0..2 {
        let reopened = Midge::new_strict_with_data_dir(&path).expect("recover database families");
        assert_eq!(
            reopened.database_family(database).unwrap().name(),
            replacement
        );
        assert!(reopened.engine.get_column_family(&original).is_none());
        assert_eq!(
            reopened.raw_get_database(database, &key).unwrap(),
            Some(row.clone())
        );
    }
    std::fs::remove_dir_all(path).unwrap();
}

#[test]
fn should_preserve_recreated_database_when_an_earlier_drop_resumes() {
    // Arrange
    let midge = Midge::new_in_memory("postgres").expect("memory adapter");
    let name = "name_reuse";
    midge.create_database(name, None).unwrap();
    let original = midge.database_family(name).unwrap();
    let (resolved, observed) = mpsc::channel();
    let (resume, retired) = mpsc::channel();

    // Act
    let (result, replacement) = std::thread::scope(|scope| {
        let adapter = &midge;
        let old_drop = scope.spawn(move || {
            AFTER_DROP_RESOLUTION.with(|slot| {
                slot.replace(Some(Box::new(move || {
                    resolved.send(()).unwrap();
                    retired.recv_timeout(Duration::from_secs(5)).unwrap();
                })));
            });
            adapter.drop_database(name)
        });
        observed.recv_timeout(Duration::from_secs(5)).unwrap();
        midge.drop_database(name).unwrap();
        midge.create_database(name, None).unwrap();
        let replacement = midge.database_family(name).unwrap();
        resume.send(()).unwrap();
        (old_drop.join().unwrap(), replacement)
    });

    // Assert
    assert_ne!(original.name(), replacement.name());
    assert!(result.is_err(), "stale DROP must reject the replacement");
    let durable = midge
        .get_database(name)
        .unwrap()
        .expect("durable replacement");
    assert_eq!(durable.physical_family, replacement.name());
    assert_eq!(midge.database_family(name).unwrap().id(), replacement.id());
    assert!(midge.engine.get_column_family(original.name()).is_none());
    midge
        .drop_database(name)
        .expect("fresh DROP owns replacement");
    assert!(midge.get_database(name).unwrap().is_none());
    assert!(midge.engine.get_column_family(replacement.name()).is_none());
}

#[test]
fn should_never_discard_a_native_write_racing_an_empty_database_drop() {
    // Arrange
    let path = std::env::temp_dir().join(format!("cassie-racing-drop-{}", uuid::Uuid::new_v4()));
    let midge = Midge::new_strict_with_data_dir(&path).expect("open local fixture");
    let database = "racing_drop";
    midge
        .create_database(database, None)
        .expect("create database");
    let physical_name = midge
        .get_database(database)
        .expect("read registry")
        .expect("registered database")
        .physical_family;
    let (key, row) = retained_row();

    // Act
    let (write_result, drop_result) = write_after_drop_empty_check(&midge, database, &key, &row);
    drop(midge);
    let reopened = Midge::new_strict_with_data_dir(&path).expect("reopen local store");
    let metadata = reopened
        .get_database(database)
        .expect("read durable registry");
    let family = reopened.engine.get_column_family(&physical_name);

    // Assert
    assert!(
        write_result.is_ok() || drop_result.is_ok(),
        "both failed: {write_result:?}, {drop_result:?}"
    );
    assert_eq!(
        metadata.is_some(),
        family.is_some(),
        "registry/family diverged"
    );
    if write_result.is_ok() {
        assert!(drop_result.is_err(), "drop discarded a committed row");
        assert_eq!(
            reopened
                .raw_get_database(database, &key)
                .expect("read retained row"),
            Some(row)
        );
    } else {
        drop_result.expect("drop completed before writer admission");
        assert!(metadata.is_none());
    }
    drop(reopened);
    std::fs::remove_dir_all(&path).expect("remove local fixture");
}

fn write_after_drop_empty_check(
    midge: &Midge,
    database: &str,
    key: &[u8],
    row: &[u8],
) -> (Result<(), CassieError>, Result<(), CassieError>) {
    let (paused, checked) = mpsc::channel();
    let (release, proceed) = mpsc::channel();
    std::thread::scope(|scope| {
        let dropper = scope.spawn(move || {
            AFTER_DROP_EMPTY_CHECK.with(|slot| {
                slot.replace(Some(Box::new(move || {
                    paused.send(()).expect("drop verified emptiness");
                    proceed
                        .recv_timeout(Duration::from_secs(5))
                        .expect("writer attempted commit");
                })));
            });
            midge.drop_database(database)
        });
        checked
            .recv_timeout(Duration::from_secs(5))
            .expect("drop paused at empty check");
        let write = midge
            .database_tx(database, cntryl_midge::TransactionMode::ReadWrite)
            .and_then(|mut tx| {
                tx.put(key.to_vec(), row.to_vec(), None)
                    .map_err(CassieError::from)?;
                tx.commit(midge.write_options_sync())
                    .map_err(CassieError::from)
            });
        release.send(()).expect("resume database retirement");
        (write, dropper.join().expect("drop worker"))
    })
}

fn retained_row() -> (Vec<u8>, Vec<u8>) {
    use cassie_core::types::{DataType, FieldSchema, Schema};

    let mut schema = crate::row_blob::RowSchema::from_schema(&Schema {
        fields: vec![FieldSchema {
            name: "value".to_owned(),
            data_type: DataType::Int,
            nullable: false,
        }],
    });
    schema.relation_id = 42;
    let key = super::key_encoding::row_key(schema.relation_id, "native-row");
    let row = crate::row_blob::encode_row(&schema, &serde_json::json!({"value": 7}))
        .expect("encode a current-format row");
    (key, row)
}

#[test]
fn should_drop_an_empty_database_after_reopening_storage_without_its_native_writer() {
    // Arrange
    let path = std::env::temp_dir().join(format!("cassie-drop-reopen-{}", uuid::Uuid::new_v4()));
    let database = "drop_recovered";
    let midge = Midge::new_strict_with_data_dir(&path).expect("open local fixture");
    midge
        .create_database(database, None)
        .expect("create database");
    let physical_name = midge
        .get_database(database)
        .expect("metadata")
        .expect("created database")
        .physical_family;
    midge
        .database_tx(database, cntryl_midge::TransactionMode::ReadWrite)
        .expect("native writer")
        .rollback()
        .expect("finish native writer");

    // Act
    let blocked = midge.drop_database(database);
    drop(midge);
    let reopened = Midge::new_strict_with_data_dir(&path).expect("reopen after handle retirement");
    let dropped = reopened.drop_database(database);

    // Assert
    assert!(matches!(blocked, Err(CassieError::Unsupported(message))
        if message.contains("native writable transaction") && message.contains("reopen")));
    dropped.expect("empty database can drop after recovery");
    assert!(reopened
        .get_database(database)
        .expect("registry read")
        .is_none());
    assert!(reopened.engine.get_column_family(&physical_name).is_none());
    drop(reopened);
    std::fs::remove_dir_all(&path).expect("remove local fixture");
}
