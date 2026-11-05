use std::sync::{Arc, Barrier};

use super::Midge;
use crate::midge::adapter::schema_write_control::{
    schema_write_conflict_test_guard, schema_write_conflict_worker_guard,
    set_schema_write_commit_barriers, SchemaWritePausePoint,
};

#[test]
fn should_leave_a_live_database_creation_untouched_by_repeated_bootstrap() {
    // Arrange
    let _guard = schema_write_conflict_test_guard();
    let store = Arc::new(Midge::new_in_memory("postgres").expect("memory store"));
    store.ensure_families_ready().expect("first initialization");
    store
        .create_database("existing", None)
        .expect("existing database");
    let ready = Arc::new(Barrier::new(2));
    let resume = Arc::new(Barrier::new(2));
    set_schema_write_commit_barriers(
        Some(SchemaWritePausePoint::DatabaseCreateFinalize),
        Some(Arc::clone(&ready)),
        Some(Arc::clone(&resume)),
    );
    let writer_store = Arc::clone(&store);
    let worker = std::thread::spawn(move || {
        let worker_guard = schema_write_conflict_worker_guard();
        let result = writer_store.create_database("staged", None);
        worker_guard.complete(SchemaWritePausePoint::DatabaseCreateFinalize);
        result
    });
    ready.wait();

    // Act
    let during = store.bootstrap_families();
    set_schema_write_commit_barriers(None, None, None);
    resume.wait();
    let result = worker.join().expect("database worker");
    let after = store.bootstrap_families();

    // Assert
    let during = during.expect("bootstrap with staged create");
    assert!(during.database_families.contains_key("existing"));
    assert!(!during.database_families.contains_key("staged"));
    result.expect("staged create completes");
    let after = after.expect("fresh snapshot");
    assert!(after.database_families.contains_key("existing"));
    assert!(after.database_families.contains_key("staged"));
    let family = store.database_family("staged").expect("registered family");
    assert_eq!(
        store
            .engine
            .get_column_family(family.name())
            .expect("physical family remains present")
            .id(),
        family.id()
    );
}

#[test]
fn should_retry_bootstrap_after_initial_layout_admission_fails() {
    // Arrange
    use crate::midge::adapter::{key_encoding, StorageFamily};
    let store = Midge::new_in_memory("postgres").expect("memory store");
    let schema = store
        .get_or_create_family(StorageFamily::Schema)
        .expect("schema family");
    let write_marker = |value: &[u8]| {
        let mut tx = store
            .engine
            .begin_tx(schema.id(), cntryl_midge::TransactionMode::ReadWrite)
            .expect("marker writer");
        tx.put(key_encoding::layout_marker_key(), value.to_vec(), None)
            .expect("write marker");
        tx.commit(cntryl_midge::WriteOptions::sync())
            .expect("commit marker");
    };
    write_marker(b"unsupported marker");
    let first = store.ensure_families_ready();
    write_marker(key_encoding::LAYOUT_MARKER_VALUE);

    // Act
    let second = store.ensure_families_ready();

    // Assert
    assert!(first.is_err());
    assert!(second.is_ok());
}

thread_local! {
    static BEFORE_GATE: std::cell::RefCell<Option<Box<dyn FnOnce()>>> = const { std::cell::RefCell::new(None) };
    static INITIALIZING: std::cell::RefCell<Option<Box<dyn FnOnce()>>> = const { std::cell::RefCell::new(None) };
    static FAIL_AFTER_INITIALIZATION: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

pub(super) fn before_bootstrap_gate() {
    if let Some(callback) = BEFORE_GATE.with(std::cell::RefCell::take) {
        callback();
    }
}

pub(super) fn initializing_bootstrap() {
    if let Some(callback) = INITIALIZING.with(std::cell::RefCell::take) {
        callback();
    }
}

pub(super) fn fail_after_initialization() -> bool {
    FAIL_AFTER_INITIALIZATION.replace(false)
}

#[test]
fn should_retry_after_bootstrap_has_persisted_families() {
    // Arrange
    let store = Midge::new_in_memory("postgres").expect("memory store");
    FAIL_AFTER_INITIALIZATION.set(true);
    let first = store.ensure_families_ready();

    // Act
    let retry = store.bootstrap_families();

    // Assert
    assert!(first.is_err());
    let layout = retry.expect("retry after persisted initialization");
    assert!(layout.database_families.contains_key("postgres"));
    assert!(store.database_family("postgres").is_ok());
}

#[test]
fn should_initialize_once_for_concurrent_first_callers() {
    // Arrange
    let store = Arc::new(Midge::new_in_memory("postgres").expect("memory store"));
    let first_store = Arc::clone(&store);
    let (entered_tx, entered_rx) = std::sync::mpsc::channel();
    let (release_tx, release_rx) = std::sync::mpsc::channel();
    let first = std::thread::spawn(move || {
        INITIALIZING.with(|slot| {
            *slot.borrow_mut() = Some(Box::new(move || {
                entered_tx.send(()).expect("initialization entered");
                release_rx
                    .recv_timeout(std::time::Duration::from_secs(5))
                    .expect("release initializer");
            }));
        });
        first_store.bootstrap_families()
    });
    entered_rx
        .recv_timeout(std::time::Duration::from_secs(5))
        .expect("first initializer");
    let second_store = Arc::clone(&store);
    let (waiting_tx, waiting_rx) = std::sync::mpsc::channel();
    let (duplicate_tx, duplicate_rx) = std::sync::mpsc::channel();
    let second = std::thread::spawn(move || {
        BEFORE_GATE.with(|slot| {
            *slot.borrow_mut() = Some(Box::new(move || {
                waiting_tx.send(()).expect("second reached gate");
            }))
        });
        INITIALIZING.with(|slot| {
            *slot.borrow_mut() = Some(Box::new(move || {
                duplicate_tx
                    .send(())
                    .expect("unexpected second initialization");
            }))
        });
        second_store.create_database("concurrent", None)
    });

    // Act
    waiting_rx
        .recv_timeout(std::time::Duration::from_secs(5))
        .expect("second caller reached gate");
    release_tx.send(()).expect("release first");
    let first = first.join().expect("first worker").expect("first layout");
    second
        .join()
        .expect("second worker")
        .expect("database create");
    let second = store.bootstrap_families().expect("fresh layout");

    // Assert
    assert!(duplicate_rx.try_recv().is_err());
    assert_eq!(first.schema.id(), second.schema.id());
    assert_eq!(first.data.id(), second.data.id());
    assert_eq!(first.temp.id(), second.temp.id());
    assert!(second.database_families.contains_key("concurrent"));
}

#[test]
fn should_reject_repeated_bootstrap_after_recovery_is_required() {
    // Arrange
    let store = Midge::new_in_memory("postgres").expect("memory store");
    store.bootstrap_families().expect("initial bootstrap");
    store.latch_storage_recovery_required_for_test();

    // Act
    let result = store.bootstrap_families();

    // Assert
    assert_eq!(
        result
            .expect_err("quarantined storage")
            .descriptor()
            .sql_state,
        "58030"
    );
}
