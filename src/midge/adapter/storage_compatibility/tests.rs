use std::sync::{Arc, Barrier};

use super::super::Midge;
use crate::midge::adapter::{key_encoding, StorageFamily};

const V1: &str = "cassie-midge-layout-v1-0.1.0";
const V2: &str = "cassie-midge-layout-v2-0.2.0";

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
