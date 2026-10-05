use std::path::PathBuf;
use std::time::Instant;

use crate::config::CassieRuntimeLimits;
use crate::midge::adapter::{query_scan_control_test_guard, RowDecode, StorageFamily};
use crate::runtime::QueryExecutionControls;
use crate::types::{DataType, FieldSchema, Schema};

use super::{Midge, OrderedRowBound, OrderedRowScanRequest};

const COLLECTION: &str = "ordered_cursor_qualification";

struct Fixture {
    midge: Option<Midge>,
    path: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let path =
            std::env::temp_dir().join(format!("cassie-ordered-cursor-{}", uuid::Uuid::new_v4()));
        let midge = Midge::new_strict_with_data_dir(&path).expect("local Midge");
        midge.ensure_families_ready().expect("storage families");
        midge
            .create_collection(
                COLLECTION,
                Schema {
                    fields: vec![FieldSchema {
                        name: "value".to_owned(),
                        data_type: DataType::Text,
                        nullable: true,
                    }],
                },
            )
            .expect("RowStore collection");
        for (id, value) in [("a", "authoritative-a"), ("c", "snapshot-c")] {
            midge
                .put_document(
                    COLLECTION,
                    Some(id.to_owned()),
                    serde_json::json!({"value": value}),
                )
                .expect("authoritative row");
        }
        let collection = midge.canonical_collection_name(COLLECTION);
        midge
            .raw_put(
                StorageFamily::Data,
                &Midge::doc_key(&collection, "a"),
                &[u8::MAX],
            )
            .expect("duplicate legacy decode canary");
        midge
            .put_document(
                COLLECTION,
                Some("b".to_owned()),
                serde_json::json!({"value": "legacy-b"}),
            )
            .expect("encoded legacy fixture row");
        let schema = midge.row_schema(COLLECTION).expect("fixture row schema");
        let modern_key = Midge::row_key(schema.relation_id, "b");
        let legacy_blob = midge
            .raw_get(StorageFamily::Data, &modern_key)
            .expect("encoded fixture row")
            .expect("modern fixture row exists");
        midge
            .raw_put(
                StorageFamily::Data,
                &Midge::doc_key(&collection, "b"),
                &legacy_blob,
            )
            .expect("legacy-only row");
        midge
            .raw_delete(StorageFamily::Data, &modern_key)
            .expect("retain only the legacy fixture key");
        Self {
            midge: Some(midge),
            path,
        }
    }

    fn midge(&self) -> &Midge {
        self.midge.as_ref().expect("live fixture")
    }

    fn entries(
        &self,
        reverse: bool,
        start: Option<&OrderedRowBound>,
        end: Option<&OrderedRowBound>,
        limit: usize,
    ) -> Vec<(String, String)> {
        let controls = controls();
        let mut cursor = self
            .midge()
            .open_controlled_ordered_row_cursor(
                OrderedRowScanRequest {
                    collection: COLLECTION,
                    decode: RowDecode::ProjectedHistorical(vec!["value".to_owned()]),
                    start_bound: start,
                    end_bound: end,
                    reverse,
                    limit: Some(limit),
                },
                &controls,
            )
            .expect("controlled ordered cursor");
        let mut documents = Vec::new();
        while let Some(document) = cursor
            .next_accounted_document(self.midge(), &controls)
            .expect("ordered document")
        {
            documents.push(document);
        }
        let entries = documents
            .iter()
            .map(|document| {
                (
                    document.id().to_owned(),
                    document.document().payload["value"]
                        .as_str()
                        .expect("projected value")
                        .to_owned(),
                )
            })
            .collect();
        assert!(controls.current_query_memory_bytes() > 0);
        drop(documents);
        drop(cursor);
        assert_eq!(controls.current_query_memory_bytes(), 0);
        entries
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        drop(self.midge.take());
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

fn controls() -> QueryExecutionControls {
    QueryExecutionControls::from_limits(
        &CassieRuntimeLimits {
            query_timeout_ms: 0,
            query_memory_budget_bytes: 1024 * 1024,
            ..CassieRuntimeLimits::default()
        },
        Instant::now(),
    )
}

#[test]
fn should_preserve_authoritative_row_precedence_in_forward_order() {
    // Arrange
    let _guard = query_scan_control_test_guard();
    let fixture = Fixture::new();

    // Act
    let entries = fixture.entries(false, None, None, 3);

    // Assert
    assert_eq!(
        entries,
        [
            ("a".to_owned(), "authoritative-a".to_owned()),
            ("b".to_owned(), "legacy-b".to_owned()),
            ("c".to_owned(), "snapshot-c".to_owned()),
        ]
    );
}

#[test]
fn should_preserve_authoritative_row_precedence_in_reverse_order() {
    // Arrange
    let _guard = query_scan_control_test_guard();
    let fixture = Fixture::new();

    // Act
    let entries = fixture.entries(true, None, None, 2);

    // Assert
    assert_eq!(
        entries,
        [
            ("c".to_owned(), "snapshot-c".to_owned()),
            ("b".to_owned(), "legacy-b".to_owned()),
        ]
    );
    assert_eq!(
        fixture.entries(true, None, None, 3)[2],
        ("a".to_owned(), "authoritative-a".to_owned())
    );
}

#[test]
fn should_apply_inclusive_bounds_to_both_ordered_sources() {
    // Arrange
    let _guard = query_scan_control_test_guard();
    let fixture = Fixture::new();
    let start = OrderedRowBound {
        id: "b".to_owned(),
        inclusive: true,
    };
    let end = OrderedRowBound {
        id: "c".to_owned(),
        inclusive: true,
    };

    // Act
    let forward = fixture.entries(false, Some(&start), Some(&end), 3);
    let reverse = fixture.entries(true, Some(&start), Some(&end), 3);

    // Assert
    assert_eq!(
        forward,
        [
            ("b".to_owned(), "legacy-b".to_owned()),
            ("c".to_owned(), "snapshot-c".to_owned()),
        ]
    );
    assert_eq!(reverse, forward.into_iter().rev().collect::<Vec<_>>());
}

#[test]
fn should_apply_exclusive_bounds_to_both_ordered_sources() {
    // Arrange
    let _guard = query_scan_control_test_guard();
    let fixture = Fixture::new();
    let start = OrderedRowBound {
        id: "a".to_owned(),
        inclusive: false,
    };
    let end = OrderedRowBound {
        id: "c".to_owned(),
        inclusive: false,
    };

    // Act
    let forward = fixture.entries(false, Some(&start), Some(&end), 3);
    let reverse = fixture.entries(true, Some(&start), Some(&end), 3);

    // Assert
    assert_eq!(forward, [("b".to_owned(), "legacy-b".to_owned())]);
    assert_eq!(reverse, forward);
}

#[test]
fn should_keep_ordered_pages_on_the_open_read_snapshot() {
    // Arrange
    let _guard = query_scan_control_test_guard();
    let fixture = Fixture::new();
    let controls = controls();
    let mut cursor = fixture
        .midge()
        .open_controlled_ordered_row_cursor(
            OrderedRowScanRequest {
                collection: COLLECTION,
                decode: RowDecode::ProjectedHistorical(vec!["value".to_owned()]),
                start_bound: None,
                end_bound: None,
                reverse: false,
                limit: Some(3),
            },
            &controls,
        )
        .expect("snapshot cursor");
    let first = cursor
        .next_accounted_document(fixture.midge(), &controls)
        .expect("first page")
        .expect("first row");
    assert_eq!(first.id(), "a");
    fixture
        .midge()
        .put_document(
            COLLECTION,
            Some("c".to_owned()),
            serde_json::json!({"value": "changed-after-open"}),
        )
        .expect("concurrent write");

    // Act
    let second = cursor
        .next_accounted_document(fixture.midge(), &controls)
        .expect("second page")
        .expect("legacy row");
    let third = cursor
        .next_accounted_document(fixture.midge(), &controls)
        .expect("third page")
        .expect("snapshot row");

    // Assert
    assert_eq!(second.id(), "b");
    assert_eq!(third.id(), "c");
    assert_eq!(third.document().payload["value"], "snapshot-c");
    assert!(cursor
        .next_accounted_document(fixture.midge(), &controls)
        .expect("bounded cursor")
        .is_none());
    drop((first, second, third));
    drop(cursor);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}
