//! Published native start-sequence assertions over Cassie's row representations.

use super::*;
use crate::adapter::StorageFamily;
use cassie_core::catalog::{CollectionMeta, CollectionStorageMode};
use cassie_core::request::{Interruption, RequestCancellation, RequestMemory};
use cassie_core::types::{DataType, FieldSchema, Schema};

struct Controls {
    memory: RequestMemory,
    cancellation: RequestCancellation,
}

impl Controls {
    fn context(&self) -> StorageRequestContext<'_> {
        StorageRequestContext::new(
            None,
            Interruption::new(None, &self.cancellation),
            &self.memory,
        )
    }
}

fn controls(budget: usize) -> Controls {
    Controls {
        memory: RequestMemory::new(budget),
        cancellation: RequestCancellation::new(),
    }
}

fn fixture(column_store: bool) -> (Arc<Midge>, u64) {
    let midge = Arc::new(Midge::new_in_memory("postgres").expect("memory fixture"));
    midge.ensure_families_ready().expect("families");
    let schema = Schema {
        fields: vec![FieldSchema {
            name: "value".to_owned(),
            data_type: DataType::Int,
            nullable: true,
        }],
    };
    let storage_mode = if column_store {
        CollectionStorageMode::ColumnStore
    } else {
        CollectionStorageMode::RowStore
    };
    midge
        .create_collection_with_meta(
            "row_fence",
            &schema,
            &CollectionMeta::new_with_storage_mode("row_fence", None, storage_mode),
        )
        .expect("table");
    let identity = midge
        .row_schema("row_fence")
        .expect("row schema")
        .relation_id;
    midge
        .put_document(
            "row_fence",
            Some("one".to_owned()),
            serde_json::json!({"value": 1}),
        )
        .expect("seed");
    (midge, identity)
}

#[test]
fn should_reject_same_value_row_rewrites_after_the_retained_start() {
    // Arrange
    for column_store in [false, true] {
        let (midge, identity) = fixture(column_store);
        let origin = controls(1024 * 1024);
        let fence =
            RetainedRowFence::capture(&midge, "postgres", &origin.context()).expect("old fence");
        midge
            .put_document(
                "row_fence",
                Some("one".to_owned()),
                serde_json::json!({"value": 1}),
            )
            .expect("same-value rewrite");
        let current = controls(1024 * 1024);

        // Act
        let result = fence.validate_rows([Ok(("row_fence", identity, "one"))], &current.context());

        // Assert
        assert!(matches!(
            result,
            Err(CassieError::Transaction {
                kind: TransactionErrorKind::SerializationFailure,
                ..
            })
        ));
        assert_eq!(origin.memory.current_bytes(), 0);
        assert_eq!(current.memory.current_bytes(), 0);
    }
}

#[test]
fn should_allow_disjoint_rows_to_change_the_shared_data_epoch() {
    // Arrange
    let (midge, identity) = fixture(false);
    let origin = controls(1024 * 1024);
    let fence =
        RetainedRowFence::capture(&midge, "postgres", &origin.context()).expect("old fence");
    let before = midge.data_epoch().expect("old epoch");
    midge
        .put_document(
            "row_fence",
            Some("other".to_owned()),
            serde_json::json!({"value": 2}),
        )
        .expect("disjoint write");
    let current = controls(1024 * 1024);

    // Act
    let result = fence.validate_rows([Ok(("row_fence", identity, "one"))], &current.context());

    // Assert
    assert!(result.is_ok());
    assert!(midge.data_epoch().expect("later epoch") > before);
    assert_eq!(origin.memory.current_bytes(), 0);
    assert_eq!(current.memory.current_bytes(), 0);
}

#[test]
fn should_allow_a_new_row_after_completed_insert_delete() {
    // Arrange
    let (midge, identity) = fixture(false);
    let origin = controls(1024 * 1024);
    let fence =
        RetainedRowFence::capture(&midge, "postgres", &origin.context()).expect("old fence");
    midge
        .put_document(
            "row_fence",
            Some("missing".to_owned()),
            serde_json::json!({"value": 2}),
        )
        .expect("insert after start");
    midge
        .delete_document("row_fence", "missing")
        .expect("delete after start");
    let current = controls(1024 * 1024);

    // Act
    let result = fence.validate_rows([Ok(("row_fence", identity, "missing"))], &current.context());

    // Assert
    assert!(result.is_ok());
    assert_eq!(origin.memory.current_bytes(), 0);
    assert_eq!(current.memory.current_bytes(), 0);
}

#[test]
fn should_cancel_after_the_final_row_assertions_are_retained() {
    // Arrange
    let (midge, identity) = fixture(false);
    let origin = controls(1024 * 1024);
    let fence =
        RetainedRowFence::capture(&midge, "postgres", &origin.context()).expect("old fence");
    let current = controls(1024 * 1024);
    let mut retained = false;

    // Act
    let result = fence.validate_rows_observed(
        [Ok(("row_fence", identity, "one"))],
        &current.context(),
        || {
            retained = current.memory.current_bytes() > 0;
            current.cancellation.cancel();
        },
    );

    // Assert
    assert!(retained);
    assert!(matches!(result, Err(CassieError::QueryCanceled)));
    assert_eq!(origin.memory.current_bytes(), 0);
    assert_eq!(current.memory.current_bytes(), 0);
}

#[test]
fn should_release_admitted_inputs_after_a_small_commit_budget_rejects_the_row() {
    // Arrange
    let (midge, identity) = fixture(false);
    let origin = controls(1024 * 1024);
    let fence =
        RetainedRowFence::capture(&midge, "postgres", &origin.context()).expect("old fence");
    let current = controls(1);

    // Act
    let result = fence.validate_rows([Ok(("row_fence", identity, "one"))], &current.context());

    // Assert
    assert!(matches!(result, Err(CassieError::ResourceLimit(_))));
    assert_eq!(origin.memory.current_bytes(), 0);
    assert_eq!(current.memory.current_bytes(), 0);
}

#[test]
fn should_reject_a_legacy_row_rewrite_after_the_retained_start() {
    // Arrange
    let (midge, identity) = fixture(false);
    let row = Midge::row_key(identity, "one");
    let raw = midge
        .raw_get_database("postgres", &row)
        .expect("encoded row")
        .expect("row");
    let canonical = midge.canonical_collection_name("row_fence");
    let legacy = Midge::doc_key(&canonical, "one");
    midge
        .raw_put(StorageFamily::Data, &legacy, &raw)
        .expect("legacy fixture");
    midge
        .raw_delete(StorageFamily::Data, &row)
        .expect("legacy-only row");
    let origin = controls(1024 * 1024);
    let fence =
        RetainedRowFence::capture(&midge, "postgres", &origin.context()).expect("old legacy fence");
    midge
        .raw_put(StorageFamily::Data, &legacy, &raw)
        .expect("same-value legacy rewrite");
    let current = controls(1024 * 1024);

    // Act
    let result = fence.validate_rows([Ok(("row_fence", identity, "one"))], &current.context());

    // Assert
    assert!(matches!(
        result,
        Err(CassieError::Transaction {
            kind: TransactionErrorKind::SerializationFailure,
            ..
        })
    ));
    assert_eq!(origin.memory.current_bytes(), 0);
    assert_eq!(current.memory.current_bytes(), 0);
}

#[test]
fn should_enforce_the_exact_peak_admission_boundary_for_row_validation() {
    // Arrange
    let (midge, identity) = fixture(false);
    midge
        .put_document(
            "row_fence",
            Some("two".to_owned()),
            serde_json::json!({"value": 2, "extra": "second-input"}),
        )
        .expect("second retained row");
    let rows = || {
        [
            Ok(("row_fence", identity, "one")),
            Ok(("row_fence", identity, "two")),
        ]
    };
    let origin = controls(1024 * 1024);
    let measured = controls(1024 * 1024);
    RetainedRowFence::capture(&midge, "postgres", &origin.context())
        .expect("measurement fence")
        .validate_rows(rows(), &measured.context())
        .expect("measure admitted peak");
    let peak = measured.memory.peak_bytes();
    assert!(peak > 1);

    // Act
    let below = controls(peak - 1);
    let denied = RetainedRowFence::capture(&midge, "postgres", &origin.context())
        .expect("below-boundary fence")
        .validate_rows(rows(), &below.context());
    let exact = controls(peak);
    let admitted = RetainedRowFence::capture(&midge, "postgres", &origin.context())
        .expect("exact-boundary fence")
        .validate_rows(rows(), &exact.context());

    // Assert
    assert!(matches!(denied, Err(CassieError::ResourceLimit(_))));
    assert!(admitted.is_ok());
    assert_eq!(below.memory.current_bytes(), 0);
    assert_eq!(exact.memory.current_bytes(), 0);
    assert_eq!(origin.memory.current_bytes(), 0);
}
