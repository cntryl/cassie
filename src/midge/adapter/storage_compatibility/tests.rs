use std::sync::{Arc, Barrier};

use super::super::Midge;
use crate::midge::adapter::{key_encoding, StorageFamily};

const V1: &str = "cassie-midge-layout-v1-0.1.0";
const V2: &str = "cassie-midge-layout-v2-0.2.0";
const V3: &str = "cassie-midge-layout-v3-0.2.0-index-generation";

fn memory_store() -> Midge {
    let store = Midge::new_in_memory("postgres").expect("memory store");
    store.ensure_families_ready().expect("bootstrap");
    store
}

fn marker(store: &Midge) -> Vec<u8> {
    store
        .raw_scan_prefix(StorageFamily::Schema, b"")
        .expect("schema scan")
        .into_iter()
        .find_map(|(key, value)| (key == key_encoding::layout_marker_key()).then_some(value))
        .expect("marker")
}

#[test]
fn should_keep_a_v1_store_at_its_original_compatibility_marker() {
    // Arrange
    let store = memory_store();
    let before = marker(&store);
    // Act
    let version = store.storage_layout_version().expect("read version");
    store
        .require_storage_layout_version(V1)
        .expect("v1 requirement");
    // Assert
    assert_eq!(version, V1);
    assert_eq!(marker(&store), before);
}

#[test]
fn should_acknowledge_a_monotonic_upgrade_without_rewriting_existing_rows() {
    // Arrange
    let store = memory_store();
    store
        .raw_put(StorageFamily::Data, b"opaque-v1-row", b"original")
        .expect("row");
    let before = store
        .raw_scan_prefix(StorageFamily::Data, b"")
        .expect("rows");
    // Act
    store.require_storage_layout_version(V2).expect("upgrade");
    store
        .require_storage_layout_version(V1)
        .expect("older requirement");
    store
        .require_storage_layout_version(V2)
        .expect("idempotent upgrade");
    // Assert
    assert_eq!(marker(&store), V2.as_bytes());
    assert_eq!(
        store
            .raw_scan_prefix(StorageFamily::Data, b"")
            .expect("unchanged rows"),
        before
    );
}

#[test]
fn should_never_lower_generation_fence_for_older_requirements() {
    // Arrange
    let store = memory_store();
    store.require_storage_layout_version(V2).expect("v2");
    store
        .raw_put(StorageFamily::Data, b"unrelated", b"preserved")
        .expect("row");
    // Act
    store.require_storage_layout_version(V3).expect("v3");
    store
        .require_storage_layout_version(V2)
        .expect("v2 requirement");
    store
        .require_storage_layout_version(V1)
        .expect("v1 requirement");
    // Assert
    assert_eq!(marker(&store), V3.as_bytes());
    assert!(store
        .raw_scan_prefix(StorageFamily::Data, b"unrelated")
        .expect("row readback")
        .iter()
        .any(|(_, value)| value == b"preserved"));
}

#[test]
fn should_serialize_concurrent_older_and_generation_marker_requests_monotonically() {
    // Arrange
    let store = Arc::new(memory_store());
    let start = Arc::new(Barrier::new(2));
    // Act
    std::thread::scope(|scope| {
        let workers = [V2, V3]
            .into_iter()
            .map(|version| {
                let store = Arc::clone(&store);
                let start = Arc::clone(&start);
                scope.spawn(move || {
                    start.wait();
                    store.require_storage_layout_version(version)
                })
            })
            .collect::<Vec<_>>();
        for worker in workers {
            worker.join().expect("worker").expect("marker");
        }
    });
    // Assert
    assert_eq!(marker(&store), V3.as_bytes());
}

#[test]
fn should_serialize_simultaneous_requests_for_the_same_upgrade() {
    // Arrange
    let store = Arc::new(memory_store());
    let start = Arc::new(Barrier::new(2));
    // Act
    std::thread::scope(|scope| {
        let workers = (0..2)
            .map(|_| {
                let store = Arc::clone(&store);
                let start = Arc::clone(&start);
                scope.spawn(move || {
                    start.wait();
                    store.require_storage_layout_version(V2)
                })
            })
            .collect::<Vec<_>>();
        for worker in workers {
            worker.join().expect("worker").expect("upgrade");
        }
    });
    // Assert
    assert_eq!(marker(&store), V2.as_bytes());
}

#[test]
fn should_reassert_an_existing_v2_marker_with_a_new_durability_barrier() {
    // Arrange
    let store = memory_store();
    store
        .require_storage_layout_version(V2)
        .expect("first acknowledgment");
    let schema = store.ensure_families_ready().expect("layout").schema.id();
    let sequence = || {
        store
            .engine
            .begin_tx(schema, cntryl_midge::TransactionMode::ReadOnly)
            .expect("current sequence")
            .commit_with_sequence(cntryl_midge::WriteOptions::sync())
            .expect("read snapshot sequence")
    };
    let first = sequence();
    // Act
    store
        .require_storage_layout_version(V2)
        .expect("reassert known bytes");
    let second = sequence();
    // Assert
    assert!(
        second > first,
        "locally visible bytes cannot bypass a fresh acknowledged write"
    );
    assert_eq!(marker(&store), V2.as_bytes());
}

#[test]
fn should_require_strict_durability_for_compatibility_anchors_in_every_storage_mode() {
    // Arrange
    use crate::midge::adapter::storage_config::WritePolicy;
    let cases = [
        (WritePolicy::Local, cntryl_midge::WriteOptions::sync()),
        (
            WritePolicy::CloudBackground,
            cntryl_midge::WriteOptions::cloud_strict(),
        ),
        (
            WritePolicy::CloudStrict,
            cntryl_midge::WriteOptions::cloud_strict(),
        ),
    ];
    // Act
    let actual = cases.map(|(policy, _)| policy.compatibility_upgrade());
    // Assert
    for ((_, expected), actual) in cases.into_iter().zip(actual) {
        assert_eq!(actual, expected);
    }
}

#[test]
fn should_reject_an_unknown_requirement_before_creating_storage_families() {
    // Arrange
    let store = Midge::new_in_memory("postgres").expect("uninitialized store");
    let before = store.engine.list_column_families().expect("families");
    // Act
    let result = store.require_storage_layout_version("cassie-midge-layout-v99");
    // Assert
    assert!(result.is_err());
    let after = store
        .engine
        .list_column_families()
        .expect("families after rejection");
    assert_eq!(before.len(), after.len());
    assert!(before
        .iter()
        .zip(&after)
        .all(|(before, after)| before.id() == after.id() && before.name() == after.name()));
}

#[test]
fn should_preserve_invalid_or_missing_markers_when_an_upgrade_is_requested() {
    // Arrange
    for corrupt in [false, true] {
        let store = memory_store();
        let key = key_encoding::layout_marker_key();
        if corrupt {
            store
                .raw_put(StorageFamily::Schema, &key, b"future-unknown")
                .expect("corrupt only marker");
        } else {
            store
                .raw_delete(StorageFamily::Schema, &key)
                .expect("remove only marker");
        }
        let before = store
            .raw_scan_prefix(StorageFamily::Schema, b"")
            .expect("before");
        // Act
        let result = store.require_storage_layout_version(V2);
        // Assert
        assert!(result.is_err());
        assert_eq!(
            store
                .raw_scan_prefix(StorageFamily::Schema, b"")
                .expect("after"),
            before
        );
    }
}

#[test]
fn should_reopen_an_upgraded_store_after_no_feature_was_published() {
    // Arrange
    let directory = std::env::temp_dir().join(format!("cassie-v2-marker-{}", uuid::Uuid::new_v4()));
    let open = || {
        Midge::open_with_write_policy(
            cntryl_midge::OpenOptions::local(&directory)
                .build()
                .expect("local options"),
            "postgres",
            crate::midge::adapter::storage_config::WritePolicy::Local,
        )
        .expect("local store")
    };
    let store = open();
    store.ensure_families_ready().expect("bootstrap");
    store
        .raw_put(StorageFamily::Data, b"opaque-v1-row", b"old bytes")
        .expect("old row");
    let before = store
        .raw_scan_prefix(StorageFamily::Data, b"")
        .expect("old rows");
    store
        .require_storage_layout_version(V2)
        .expect("marker barrier");
    drop(store);
    // Act
    let reopened = open();
    reopened
        .ensure_families_ready()
        .expect("admit v2 after restart");
    // Assert
    assert_eq!(reopened.storage_layout_version().expect("version"), V2);
    assert_eq!(
        reopened
            .raw_scan_prefix(StorageFamily::Data, b"")
            .expect("old rows survive"),
        before
    );
    drop(reopened);
    std::fs::remove_dir_all(directory).expect("cleanup owned local fixture");
}

thread_local! {
    static BEFORE_MARKER_LOCK: std::cell::RefCell<Option<Box<dyn FnOnce()>>> = const { std::cell::RefCell::new(None) };
}

struct MarkerLockObserver {
    previous: Option<Box<dyn FnOnce()>>,
}

fn observe_marker_lock(callback: impl FnOnce() + 'static) -> MarkerLockObserver {
    MarkerLockObserver {
        previous: BEFORE_MARKER_LOCK.with(|slot| slot.replace(Some(Box::new(callback)))),
    }
}

pub(super) fn before_marker_lock() {
    if let Some(callback) = BEFORE_MARKER_LOCK.with(std::cell::RefCell::take) {
        callback();
    }
}

impl Drop for MarkerLockObserver {
    fn drop(&mut self) {
        drop(BEFORE_MARKER_LOCK.with(|slot| slot.replace(self.previous.take())));
    }
}

fn raw_marker_after_quarantine(store: &Midge) -> Vec<u8> {
    // Test-only observation of the owned memory fixture; production marker
    // readers must reject after the recovery latch.
    let schema = store
        .ensure_families_ready()
        .expect("known layout")
        .schema
        .id();
    store
        .engine
        .begin_tx(schema, cntryl_midge::TransactionMode::ReadOnly)
        .expect("test observation")
        .get(&key_encoding::layout_marker_key())
        .expect("stored marker")
        .expect("marker")
        .to_vec()
}

#[test]
fn should_reject_marker_admission_after_storage_requires_recovery() {
    // Arrange
    let store = memory_store();
    store.latch_storage_recovery_required_for_test();

    // Act
    let read = store.storage_layout_version();
    let baseline = store.require_storage_layout_version(V1);
    let upgrade = store.require_storage_layout_version(V2);
    let retained = raw_marker_after_quarantine(&store);

    // Assert
    for result in [read.map(|_| ()), baseline, upgrade] {
        assert_eq!(
            result
                .expect_err("already quarantined")
                .descriptor()
                .sql_state,
            "58030"
        );
    }
    assert_eq!(retained, V1.as_bytes());
}

#[test]
fn should_recheck_recovery_after_waiting_for_marker_admission() {
    // Arrange
    let store = memory_store();
    let marker_lock = store.compatibility_gate.lock();
    let (send, receive) = std::sync::mpsc::sync_channel(1);

    // Act
    let result = std::thread::scope(|scope| {
        let worker = scope.spawn(|| {
            let _observer = observe_marker_lock(move || send.send(()).expect("entry signal"));
            store.require_storage_layout_version(V2)
        });
        receive
            .recv_timeout(std::time::Duration::from_secs(5))
            .expect("passed entry check before lock");
        store.latch_storage_recovery_required_for_test();
        drop(marker_lock);
        worker.join().expect("marker worker")
    });
    let retained = raw_marker_after_quarantine(&store);

    // Assert
    assert_eq!(
        result
            .expect_err("latched while waiting")
            .descriptor()
            .sql_state,
        "58030"
    );
    assert_eq!(retained, V1.as_bytes());
}

#[test]
fn should_recheck_recovery_at_marker_publication_gate() {
    // Arrange
    let store = Arc::new(memory_store());
    let schema = store.ensure_families_ready().expect("layout").schema.id();
    let gate = store.publication.family(schema);
    let (waiting_tx, waiting_rx) = std::sync::mpsc::sync_channel(1);
    let worker_store = Arc::clone(&store);

    // Act
    let worker = gate
        .with_current_operation(|| {
            let worker = std::thread::spawn(move || {
                let controls = crate::runtime::QueryExecutionControls::with_cancellation(
                    &crate::config::CassieRuntimeLimits::default(),
                    std::time::Instant::now(),
                    crate::runtime::QueryCancellationHandle::new(),
                );
                let _scope = crate::midge::adapter::publication::PublicationControlScope::enter(
                    Some(&controls),
                );
                let _observer = Midge::observe_next_publication_wait_for_test(move || {
                    waiting_tx
                        .send(())
                        .expect("marker reached publication gate");
                });
                worker_store.require_storage_layout_version(V2)
            });
            waiting_rx
                .recv_timeout(std::time::Duration::from_secs(5))
                .expect("marker waited at publication gate");
            store.latch_storage_recovery_required_for_test();
            Ok(worker)
        })
        .expect("hold schema publication gate");
    let result = worker.join().expect("marker worker");

    // Assert
    assert_eq!(
        result
            .expect_err("recovery latched before publication")
            .descriptor()
            .sql_state,
        "58030"
    );
    assert_eq!(raw_marker_after_quarantine(&store), V1.as_bytes());
}

#[test]
fn should_acknowledge_v2_before_publishing_selected_definition_records() {
    // Arrange
    let store = memory_store();
    let mut view = crate::catalog::ViewMeta::new(
        "versioned_view",
        "SELECT NULL IS NOT DISTINCT FROM NULL AS same",
        crate::types::Schema { fields: Vec::new() },
    );
    view.definition_semantics_version = 1;
    assert_eq!(marker(&store), V1.as_bytes());
    // Act
    store
        .put_view(&view)
        .expect("versioned definition publication");
    let stored = store
        .get_view(&view.name)
        .expect("view readback")
        .expect("published view");
    // Assert
    assert_eq!(marker(&store), V2.as_bytes());
    assert_eq!(stored.definition_semantics_version, 1);
}

#[test]
fn should_stop_definition_publication_when_minimum_reader_admission_fails() {
    // Arrange
    let store = memory_store();
    let mut view = crate::catalog::ViewMeta::new(
        "unpublished_view",
        "SELECT NULL IS NOT DISTINCT FROM NULL AS same",
        crate::types::Schema { fields: Vec::new() },
    );
    view.definition_semantics_version = 1;
    store
        .raw_put(
            StorageFamily::Schema,
            &key_encoding::layout_marker_key(),
            b"cassie-midge-layout-v99",
        )
        .expect("unsupported minimum reader fixture");
    let before = store
        .raw_scan_prefix(StorageFamily::Data, b"")
        .expect("before publication");
    // Act
    let rejected = store.put_view(&view);
    let after = store
        .raw_scan_prefix(StorageFamily::Data, b"")
        .expect("after rejection");
    // Assert
    assert!(rejected.is_err());
    assert_eq!(before, after);
    assert_eq!(marker(&store), b"cassie-midge-layout-v99");
}
