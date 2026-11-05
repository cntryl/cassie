//! The returned native handle itself is the sticky write capability.

use std::sync::{Arc, Barrier};

use super::*;
use crate::midge::{Midge, StorageFamily};
use cntryl_midge::TransactionMode;

fn fixture() -> Midge {
    let midge = Midge::new_in_memory("postgres").expect("memory engine");
    midge.ensure_families_ready().expect("private bootstrap");
    midge
}

#[test]
fn should_keep_tracked_mode_after_the_last_registration_retires() {
    // Arrange
    let midge = fixture();
    midge
        .publication
        .admit_tracked_write(|| Ok(()))
        .expect("tracked registration");

    // Act
    let results = [
        midge.schema_tx(TransactionMode::ReadWrite),
        midge.data_tx(TransactionMode::ReadWrite),
        midge.database_tx("postgres", TransactionMode::ReadWrite),
        midge.temp_tx(TransactionMode::ReadWrite),
        midge.default_tx(TransactionMode::ReadWrite),
        midge.begin_families_tx(&[StorageFamily::Data], TransactionMode::ReadWrite),
    ];

    // Assert
    for result in results {
        let error = result.err().expect("native writable factory blocked");
        assert_eq!(error.descriptor().sql_state, "0A000");
    }
}

#[test]
fn should_remember_a_returned_native_writer_after_rollback() {
    // Arrange
    let midge = fixture();
    let tx = midge
        .data_tx(TransactionMode::ReadWrite)
        .expect("native capability");
    tx.rollback().expect("retire handle");

    // Act
    let result = midge.publication.admit_tracked_write(|| Ok(()));

    // Assert
    assert_eq!(
        result
            .expect_err("unobservable native capability was issued")
            .descriptor()
            .sql_state,
        "0A000"
    );
}

#[test]
fn should_preserve_native_read_only_handles_in_tracked_mode() {
    // Arrange
    let midge = fixture();
    midge
        .publication
        .admit_tracked_write(|| Ok(()))
        .expect("tracked registration");
    let mut tx = midge
        .data_tx(TransactionMode::ReadOnly)
        .expect("read-only capability");
    tx.set_conflict_policy(cntryl_midge::ConflictPolicy::LastWriteWins);

    // Act
    let put = tx.put(b"key".to_vec(), b"value".to_vec(), None);
    let insert = tx.insert(b"key".to_vec(), b"value".to_vec(), None);
    let delete = tx.delete(b"key".to_vec());
    let range = tx.delete_range(b"a".to_vec(), b"z".to_vec());
    let read = tx.get(b"key");

    // Assert
    assert!(matches!(
        put,
        Err(cntryl_midge::MidgeError::InvalidArgument(_))
    ));
    assert!(matches!(
        insert,
        Err(cntryl_midge::MidgeError::InvalidArgument(_))
    ));
    assert!(matches!(
        delete,
        Err(cntryl_midge::MidgeError::InvalidArgument(_))
    ));
    assert!(matches!(
        range,
        Err(cntryl_midge::MidgeError::InvalidArgument(_))
    ));
    assert!(read.expect("read remains available").is_none());
}

#[test]
fn should_not_latch_an_interop_mode_when_admission_fails() {
    // Arrange
    let access = NativeAccess::default();
    let denied: Result<(), CassieError> =
        access.tracked(|| Err(CassieError::ResourceLimit("denied".to_owned())));

    // Act
    let native = access.native(TransactionMode::ReadWrite, || Ok(()));

    // Assert
    assert!(denied.is_err());
    assert!(native.is_ok());
}

#[test]
fn should_admit_only_one_competing_interop_mode() {
    // Arrange
    let access = Arc::new(NativeAccess::default());
    let start = Arc::new(Barrier::new(2));
    let other = Arc::clone(&access);
    let other_start = Arc::clone(&start);
    let worker = std::thread::spawn(move || {
        other_start.wait();
        other.native(TransactionMode::ReadWrite, || Ok(()))
    });

    // Act
    start.wait();
    let tracked = access.tracked(|| Ok(()));
    let native = worker.join().expect("competing native opener");

    // Assert
    assert_ne!(tracked.is_ok(), native.is_ok());
}

#[test]
fn should_reject_tracked_capture_after_each_native_writable_factory() {
    // Arrange
    for factory in 0..6 {
        let midge = fixture();
        let handle = match factory {
            0 => midge.schema_tx(TransactionMode::ReadWrite),
            1 => midge.data_tx(TransactionMode::ReadWrite),
            2 => midge.database_tx("postgres", TransactionMode::ReadWrite),
            3 => midge.temp_tx(TransactionMode::ReadWrite),
            4 => midge.default_tx(TransactionMode::ReadWrite),
            _ => midge.begin_families_tx(&[StorageFamily::Data], TransactionMode::ReadWrite),
        }
        .expect("returned writable capability");

        // Act
        let retained = midge.publication.admit_tracked_write(|| Ok(()));
        handle.rollback().expect("retired external handle");
        let retired = midge.publication.admit_tracked_write(|| Ok(()));

        // Assert
        assert_eq!(
            retained
                .expect_err("still-live writable handle")
                .descriptor()
                .sql_state,
            "0A000"
        );
        assert_eq!(
            retired
                .expect_err("unobservable completed writable handle")
                .descriptor()
                .sql_state,
            "0A000"
        );
    }
}

#[test]
fn should_preserve_tracked_admission_after_a_failed_native_factory() {
    // Arrange
    let midge = fixture();
    let missing = midge.database_tx("missing", TransactionMode::ReadWrite);

    // Act
    let tracked = midge.publication.admit_tracked_write(|| Ok(()));

    // Assert
    assert!(missing.is_err());
    assert!(tracked.is_ok());
}

#[test]
fn should_fence_local_reopen_until_returned_native_handles_retire() {
    // Arrange
    let directory = std::env::temp_dir().join(format!(
        "cassie-native-interop-recovery-{}",
        uuid::Uuid::new_v4()
    ));
    let midge =
        Midge::new_strict_with_data_dir(&directory).expect("explicit local lifetime fixture");
    midge.ensure_families_ready().expect("local families");
    let issued = midge
        .data_tx(TransactionMode::ReadWrite)
        .expect("owned native capability");
    drop(midge);

    // Act
    let blocked = Midge::new_strict_with_data_dir(&directory);
    issued
        .rollback()
        .expect("retire the final old runtime transaction guard");
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
    let reopened = loop {
        match Midge::new_strict_with_data_dir(&directory) {
            Ok(midge) => break midge,
            Err(error) => {
                assert!(
                    std::time::Instant::now() < deadline,
                    "native fencing resources did not retire: {error}"
                );
                std::thread::yield_now();
            }
        }
    };
    reopened
        .ensure_families_ready()
        .expect("recovered families");
    let controls = crate::runtime::QueryExecutionControls::from_limits(
        &crate::config::CassieRuntimeLimits::default(),
        std::time::Instant::now(),
    );
    let tracked = reopened.capture_repeatable_row_history("postgres", &controls);

    // Assert
    assert!(
        blocked.is_err(),
        "a new authority cannot coexist with an escaped old writer"
    );
    assert!(
        tracked.is_ok(),
        "recovered instance can select tracked mode after old capabilities retire"
    );
    drop(tracked);
    drop(reopened);
    std::fs::remove_dir_all(directory).expect("retire isolated local lifetime fixture");
}
