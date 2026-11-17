//! Admission and cancellation at the native conversion scan boundary.

use std::cell::RefCell;
use std::time::Instant;

use super::{CassieError, CollectionStorageMode, Midge};
use crate::catalog::CollectionMeta;
use crate::config::CassieRuntimeLimits;
use crate::midge::adapter::{RawStorageEntry, StorageFamily};
use crate::runtime::QueryExecutionControls;
use crate::types::{DataType, FieldSchema, Schema};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct ScanCounts {
    yielded: usize,
    copied: usize,
}

struct Observation {
    prefix: Vec<u8>,
    counts: ScanCounts,
}

thread_local! {
    static SCAN_OBSERVATION: RefCell<Option<Observation>> = const { RefCell::new(None) };
}

struct ScanGuard;

impl ScanGuard {
    fn new(prefix: Vec<u8>) -> Self {
        SCAN_OBSERVATION.with(|slot| {
            let previous = slot.borrow_mut().replace(Observation {
                prefix,
                counts: ScanCounts::default(),
            });
            assert!(
                previous.is_none(),
                "conversion scan observers must not nest"
            );
        });
        Self
    }
}

impl Drop for ScanGuard {
    fn drop(&mut self) {
        SCAN_OBSERVATION.with(|slot| {
            slot.borrow_mut().take();
        });
    }
}

pub(super) fn observe_yield(key: &[u8]) {
    observe(key, false);
}

pub(super) fn observe_copy(key: &[u8]) {
    observe(key, true);
}

fn observe(key: &[u8], copied: bool) {
    SCAN_OBSERVATION.with(|slot| {
        if let Some(observation) = slot.borrow_mut().as_mut() {
            if key.starts_with(&observation.prefix) {
                if copied {
                    observation.counts.copied += 1;
                } else {
                    observation.counts.yielded += 1;
                }
            }
        }
    });
}

fn scan_counts() -> ScanCounts {
    SCAN_OBSERVATION.with(|slot| slot.borrow().as_ref().expect("owned observer").counts)
}

struct Fixture {
    midge: Midge,
    relation_id: u64,
}

impl Fixture {
    fn new(mode: CollectionStorageMode, live_rows: bool) -> Self {
        let midge = Midge::new_in_memory("postgres").expect("memory storage");
        let schema = Schema {
            fields: vec![FieldSchema {
                name: "body".to_owned(),
                data_type: DataType::Text,
                nullable: true,
            }],
        };
        let relation_id = midge
            .create_collection_with_meta(
                "conversion_admission",
                &schema,
                &CollectionMeta::new_with_storage_mode("conversion_admission", None, mode),
            )
            .expect("collection");
        for id in ["a", "b", "c"] {
            midge
                .put_document(
                    "conversion_admission",
                    Some(id.to_owned()),
                    serde_json::json!({"body": id}),
                )
                .expect("seed row");
            if !live_rows {
                assert!(midge
                    .delete_document("conversion_admission", id)
                    .expect("retain a real deleted marker"));
            }
        }
        Self { midge, relation_id }
    }

    fn entries(&self, prefix: &[u8]) -> Vec<RawStorageEntry> {
        self.midge
            .raw_scan_prefix(StorageFamily::Data, prefix)
            .expect("source entries")
    }

    fn assert_mode(&self, expected: CollectionStorageMode) {
        assert_eq!(
            self.midge
                .collection_metadata("conversion_admission")
                .expect("metadata read")
                .expect("collection metadata")
                .storage_mode,
            expected
        );
    }
}

fn tiny_controls() -> QueryExecutionControls {
    let limits = CassieRuntimeLimits {
        query_memory_budget_bytes: 1,
        ..CassieRuntimeLimits::default()
    };
    QueryExecutionControls::from_limits(&limits, Instant::now())
}

#[test]
fn should_admit_row_source_entries_before_copying_or_advancing_conversion() {
    // Arrange
    let fixture = Fixture::new(CollectionStorageMode::RowStore, true);
    let prefix = Midge::row_prefix(fixture.relation_id);
    let before = fixture.entries(&prefix);
    let _guard = ScanGuard::new(prefix.clone());
    let controls = tiny_controls();

    // Act
    let result = fixture.midge.convert_collection_storage_mode(
        "conversion_admission",
        CollectionStorageMode::ColumnStore,
        &mut |bytes| controls.reserve_query_memory(bytes).map(drop),
    );

    // Assert
    assert!(matches!(result, Err(CassieError::ResourceLimit(_))));
    assert_eq!(
        scan_counts(),
        ScanCounts {
            yielded: 1,
            copied: 0
        }
    );
    fixture.assert_mode(CollectionStorageMode::RowStore);
    assert_eq!(fixture.entries(&prefix), before);
}

#[test]
fn should_admit_column_markers_before_copying_or_advancing_conversion() {
    // Arrange
    let fixture = Fixture::new(CollectionStorageMode::ColumnStore, true);
    let prefix = Midge::column_store_row_prefix(fixture.relation_id);
    let before = fixture.entries(&Midge::column_store_collection_prefix(fixture.relation_id));
    let _guard = ScanGuard::new(prefix);
    let controls = tiny_controls();

    // Act
    let result = fixture.midge.convert_collection_storage_mode(
        "conversion_admission",
        CollectionStorageMode::RowStore,
        &mut |bytes| controls.reserve_query_memory(bytes).map(drop),
    );

    // Assert
    assert!(matches!(result, Err(CassieError::ResourceLimit(_))));
    assert_eq!(
        scan_counts(),
        ScanCounts {
            yielded: 1,
            copied: 0
        }
    );
    fixture.assert_mode(CollectionStorageMode::ColumnStore);
    assert_eq!(
        fixture.entries(&Midge::column_store_collection_prefix(fixture.relation_id)),
        before
    );
}

#[test]
fn should_admit_deleted_column_cleanup_before_copying_or_removing_residue() {
    // Arrange
    let fixture = Fixture::new(CollectionStorageMode::ColumnStore, false);
    let prefix = Midge::column_store_collection_prefix(fixture.relation_id);
    let before = fixture.entries(&prefix);
    let _guard = ScanGuard::new(prefix.clone());
    let controls = tiny_controls();

    // Act
    let result = fixture.midge.convert_collection_storage_mode(
        "conversion_admission",
        CollectionStorageMode::RowStore,
        &mut |bytes| controls.reserve_query_memory(bytes).map(drop),
    );

    // Assert
    assert!(matches!(result, Err(CassieError::ResourceLimit(_))));
    assert_eq!(
        scan_counts(),
        ScanCounts {
            yielded: 1,
            copied: 0
        }
    );
    fixture.assert_mode(CollectionStorageMode::ColumnStore);
    assert_eq!(fixture.entries(&prefix), before);
}

#[test]
fn should_cancel_deleted_column_cleanup_before_copying_or_removing_residue() {
    // Arrange
    let fixture = Fixture::new(CollectionStorageMode::ColumnStore, false);
    let prefix = Midge::column_store_collection_prefix(fixture.relation_id);
    let before = fixture.entries(&prefix);
    let _guard = ScanGuard::new(prefix.clone());

    // Act
    let result = fixture.midge.convert_collection_storage_mode(
        "conversion_admission",
        CollectionStorageMode::RowStore,
        &mut |_| {
            if scan_counts().yielded == 0 {
                Ok(())
            } else {
                Err(CassieError::QueryCanceled)
            }
        },
    );

    // Assert
    assert!(matches!(result, Err(CassieError::QueryCanceled)));
    assert_eq!(
        scan_counts(),
        ScanCounts {
            yielded: 1,
            copied: 0
        }
    );
    fixture.assert_mode(CollectionStorageMode::ColumnStore);
    assert_eq!(fixture.entries(&prefix), before);
}
