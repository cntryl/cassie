use super::super::Midge;
use crate::adapter::request_test_support::TestRequest;
use crate::adapter::NativePublicationControlScope;
use crate::adapter::{CassieError, StorageRetryKind};
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
    use cassie_core::types::{DataType, FieldSchema, Schema};
    use cntryl_midge::TransactionMode;

    // Arrange
    let midge = Midge::new_in_memory("postgres").expect("memory adapter");
    let database = "retained_row";
    midge
        .create_database(database, None)
        .expect("create database");
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
    let mut writer = midge
        .database_tx(database, TransactionMode::ReadWrite)
        .expect("native writer");
    writer
        .put(key.clone(), row.clone(), None)
        .expect("store raw row");
    writer
        .commit(midge.write_options_sync())
        .expect("publish raw row");
    assert!(midge
        .database_catalog_entries(database)
        .expect("catalog")
        .is_empty());

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
    use cassie_core::types::Schema;
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
            Schema { fields: Vec::new() },
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
