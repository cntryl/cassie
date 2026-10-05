use std::cell::Cell;
use std::path::PathBuf;
use std::time::Instant;

use crate::config::CassieRuntimeLimits;
use crate::midge::adapter::{
    query_scan_control_test_guard, set_query_scan_cancellation_after_entries, StorageFamily,
};
use crate::types::{DataType, FieldSchema, Schema};

use super::{CassieError, Midge, QueryExecutionControls, RowDecode};

const COLLECTION: &str = "bounded_row_source";
const BUDGET: usize = 4 * 1_024;

struct Fixture {
    midge: Option<Midge>,
    path: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "cassie-bounded-row-source-{}",
            uuid::Uuid::new_v4()
        ));
        let midge = Midge::new_strict_with_data_dir(&path).expect("local Midge");
        midge.ensure_families_ready().expect("storage families");
        midge
            .create_collection(
                COLLECTION,
                Schema {
                    fields: vec![FieldSchema {
                        name: "payload".to_owned(),
                        data_type: DataType::Text,
                        nullable: true,
                    }],
                },
            )
            .expect("RowStore collection");
        Self {
            midge: Some(midge),
            path,
        }
    }

    fn midge(&self) -> &Midge {
        self.midge.as_ref().expect("live fixture")
    }

    fn put(&self, id: &str, payload: &str) {
        self.midge()
            .put_document(
                COLLECTION,
                Some(id.to_owned()),
                serde_json::json!({"payload": payload}),
            )
            .expect("seed source document");
    }

    fn row_key(&self, id: &str) -> Vec<u8> {
        let schema = self.midge().row_schema(COLLECTION).expect("row schema");
        Midge::row_key(schema.relation_id, id)
    }

    fn poison_wide_row(&self) {
        let payload = "x".repeat(8_192);
        self.put("a", &payload);
        let key = self.row_key("a");
        let mut raw = self
            .midge()
            .raw_get(StorageFamily::Data, &key)
            .expect("raw source row")
            .expect("seeded source row");
        let start = raw
            .windows(payload.len())
            .position(|bytes| bytes == payload.as_bytes())
            .expect("TEXT payload inside row blob");
        raw[start] = u8::MAX;
        self.midge()
            .raw_put(StorageFamily::Data, &key, &raw)
            .expect("invalid UTF-8 decode canary");
    }

    fn assert_narrow_source_fits(&self) {
        self.put("a", "narrow-source");
        let controls = controls();
        let before = self.midge().query_scan_entries_for_diagnostics();
        let scanned = self
            .midge()
            .scan_rows_until::<CassieError, _>(COLLECTION, RowDecode::Full, &controls, |document| {
                assert_eq!(document.id, "a");
                assert_eq!(
                    document.payload,
                    serde_json::json!({"payload": "narrow-source"})
                );
                Ok(false)
            })
            .expect("narrow source fits the same budget");
        assert_eq!(scanned, 1);
        assert_eq!(
            self.midge().query_scan_entries_for_diagnostics() - before,
            1
        );
        assert_eq!(controls.current_query_memory_bytes(), 0);
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        self.midge.take();
        std::fs::remove_dir_all(&self.path).expect("remove source fixture");
    }
}

fn controls() -> QueryExecutionControls {
    QueryExecutionControls::from_limits(
        &CassieRuntimeLimits {
            query_timeout_ms: 0,
            query_memory_budget_bytes: BUDGET,
            ..CassieRuntimeLimits::default()
        },
        Instant::now(),
    )
}

#[test]
fn should_reserve_a_bounded_row_store_source_before_its_first_decode() {
    // Arrange
    let _guard = query_scan_control_test_guard();
    let fixture = Fixture::new();
    fixture.assert_narrow_source_fits();
    fixture.poison_wide_row();
    let controls = controls();
    let before = fixture.midge().query_scan_entries_for_diagnostics();
    let visits = Cell::new(0);

    // Act
    let result = fixture.midge().scan_rows_until::<CassieError, _>(
        COLLECTION,
        RowDecode::Full,
        &controls,
        |_document| {
            visits.set(visits.get() + 1);
            Ok(false)
        },
    );

    // Assert
    assert!(
        matches!(result, Err(CassieError::ResourceLimit(_))),
        "source reservation must beat the first invalid UTF-8 row: {result:?}"
    );
    assert_eq!(
        fixture.midge().query_scan_entries_for_diagnostics() - before,
        1
    );
    assert_eq!(visits.get(), 0);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_cancel_a_bounded_row_store_source_before_its_first_decode() {
    // Arrange
    let _guard = query_scan_control_test_guard();
    let fixture = Fixture::new();
    fixture.assert_narrow_source_fits();
    fixture.poison_wide_row();
    let controls = controls();
    let before = fixture.midge().query_scan_entries_for_diagnostics();
    let visits = Cell::new(0);
    set_query_scan_cancellation_after_entries(Some(1));

    // Act
    let result = fixture.midge().scan_rows_until::<CassieError, _>(
        COLLECTION,
        RowDecode::Full,
        &controls,
        |_document| {
            visits.set(visits.get() + 1);
            Ok(false)
        },
    );
    set_query_scan_cancellation_after_entries(None);

    // Assert
    assert!(
        matches!(result, Err(CassieError::QueryCancelled)),
        "first controlled-read cancellation must beat row decoding: {result:?}"
    );
    assert_eq!(
        fixture.midge().query_scan_entries_for_diagnostics() - before,
        1
    );
    assert_eq!(visits.get(), 0);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_expire_a_bounded_row_store_source_before_its_first_read() {
    // Arrange
    let _guard = query_scan_control_test_guard();
    let fixture = Fixture::new();
    fixture.assert_narrow_source_fits();
    fixture.poison_wide_row();
    let mut controls = controls();
    controls.deadline = Some(Instant::now());
    let before = fixture.midge().query_scan_entries_for_diagnostics();
    let visits = Cell::new(0);

    // Act
    let result = fixture.midge().scan_rows_until::<CassieError, _>(
        COLLECTION,
        RowDecode::Full,
        &controls,
        |_document| {
            visits.set(visits.get() + 1);
            Ok(false)
        },
    );

    // Assert
    assert!(
        matches!(result, Err(CassieError::DeadlineExceeded)),
        "an expired source must not read or decode its canary: {result:?}"
    );
    assert_eq!(fixture.midge().query_scan_entries_for_diagnostics(), before);
    assert_eq!(visits.get(), 0);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_preserve_bounded_modern_row_precedence_over_legacy_rows() {
    // Arrange
    let _guard = query_scan_control_test_guard();
    let fixture = Fixture::new();
    fixture.put("a", "authoritative-a");
    fixture.put("b", "legacy-b");
    fixture.put("c", "modern-c");
    let collection = fixture.midge().canonical_collection_name(COLLECTION);
    let modern_key = fixture.row_key("b");
    let legacy_blob = fixture
        .midge()
        .raw_get(StorageFamily::Data, &modern_key)
        .expect("encoded legacy fixture")
        .expect("modern fixture row exists");
    fixture
        .midge()
        .raw_put(
            StorageFamily::Data,
            &Midge::doc_key(&collection, "b"),
            &legacy_blob,
        )
        .expect("legacy-only encoded row");
    fixture
        .midge()
        .raw_delete(StorageFamily::Data, &modern_key)
        .expect("retain only legacy fixture key");
    fixture
        .midge()
        .raw_put(
            StorageFamily::Data,
            &Midge::doc_key(&collection, "a"),
            &[u8::MAX],
        )
        .expect("duplicate legacy decode canary");
    let controls = controls();
    let before = fixture.midge().query_scan_entries_for_diagnostics();
    let mut rows = Vec::new();

    // Act
    let scanned = fixture
        .midge()
        .scan_rows_until::<CassieError, _>(COLLECTION, RowDecode::Full, &controls, |document| {
            rows.push((document.id, document.payload));
            Ok(true)
        })
        .expect("modern and legacy source scan");
    rows.sort_by(|left, right| left.0.cmp(&right.0));

    // Assert
    assert_eq!(scanned, 3);
    assert_eq!(
        fixture.midge().query_scan_entries_for_diagnostics() - before,
        4
    );
    assert_eq!(
        rows,
        [
            (
                "a".to_owned(),
                serde_json::json!({"payload": "authoritative-a"})
            ),
            ("b".to_owned(), serde_json::json!({"payload": "legacy-b"})),
            ("c".to_owned(), serde_json::json!({"payload": "modern-c"})),
        ]
    );
    assert_eq!(controls.current_query_memory_bytes(), 0);
}
