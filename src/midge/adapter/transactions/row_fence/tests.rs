//! Published native start-sequence assertions over Cassie's row representations.

use std::time::Instant;

use super::*;
use crate::app::Cassie;
use crate::config::{CassieRuntimeConfig, CassieRuntimeLimits};
use crate::midge::StorageFamily;
use crate::runtime::QueryCancellationHandle;

fn controls(budget: usize) -> QueryExecutionControls {
    QueryExecutionControls::from_limits(
        &CassieRuntimeLimits {
            query_memory_budget_bytes: budget,
            ..CassieRuntimeLimits::default()
        },
        Instant::now(),
    )
}

fn fixture(column_store: bool) -> (Cassie, u64) {
    let cassie =
        Cassie::new_in_memory_with_config(CassieRuntimeConfig::default()).expect("memory fixture");
    cassie.startup().expect("startup");
    let session = cassie.create_session("tester", None);
    let sql = if column_store {
        "CREATE TABLE row_fence (value INT) WITH (storage = column_store)"
    } else {
        "CREATE TABLE row_fence (value INT)"
    };
    cassie
        .execute_sql(&session, sql, Vec::new())
        .expect("table");
    let identity = cassie
        .catalog
        .collection_storage_id("row_fence")
        .expect("current relation identity");
    cassie
        .midge
        .put_document(
            "row_fence",
            Some("one".to_owned()),
            serde_json::json!({"value": 1}),
        )
        .expect("seed");
    (cassie, identity)
}

#[test]
fn should_reject_same_value_row_rewrites_after_the_retained_start() {
    // Arrange
    for column_store in [false, true] {
        let (cassie, identity) = fixture(column_store);
        let origin = controls(1024 * 1024);
        let fence =
            RetainedRowFence::capture(&cassie.midge, "postgres", &origin).expect("old fence");
        cassie
            .midge
            .put_document(
                "row_fence",
                Some("one".to_owned()),
                serde_json::json!({"value": 1}),
            )
            .expect("same-value rewrite");
        let current = controls(1024 * 1024);

        // Act
        let result = fence.validate_rows([Ok(("row_fence", identity, "one"))], &current);

        // Assert
        assert_eq!(
            result
                .expect_err("changed since retained start")
                .descriptor()
                .sql_state,
            "40001"
        );
        assert_eq!(origin.current_query_memory_bytes(), 0);
        assert_eq!(current.current_query_memory_bytes(), 0);
    }
}

#[test]
fn should_allow_disjoint_rows_to_change_the_shared_data_epoch() {
    // Arrange
    let (cassie, identity) = fixture(false);
    let origin = controls(1024 * 1024);
    let fence = RetainedRowFence::capture(&cassie.midge, "postgres", &origin).expect("old fence");
    let before = cassie.midge.data_epoch().expect("old epoch");
    cassie
        .midge
        .put_document(
            "row_fence",
            Some("other".to_owned()),
            serde_json::json!({"value": 2}),
        )
        .expect("disjoint write");
    let current = controls(1024 * 1024);

    // Act
    let result = fence.validate_rows([Ok(("row_fence", identity, "one"))], &current);

    // Assert
    assert!(result.is_ok());
    assert!(cassie.midge.data_epoch().expect("later epoch") > before);
    assert_eq!(origin.current_query_memory_bytes(), 0);
    assert_eq!(current.current_query_memory_bytes(), 0);
}

#[test]
fn should_allow_a_new_row_after_completed_insert_delete() {
    // Arrange
    let (cassie, identity) = fixture(false);
    let origin = controls(1024 * 1024);
    let fence = RetainedRowFence::capture(&cassie.midge, "postgres", &origin).expect("old fence");
    cassie
        .midge
        .put_document(
            "row_fence",
            Some("missing".to_owned()),
            serde_json::json!({"value": 2}),
        )
        .expect("insert after start");
    cassie
        .midge
        .delete_document("row_fence", "missing")
        .expect("delete after start");
    let current = controls(1024 * 1024);

    // Act
    let result = fence.validate_rows([Ok(("row_fence", identity, "missing"))], &current);

    // Assert
    assert!(result.is_ok());
    assert_eq!(origin.current_query_memory_bytes(), 0);
    assert_eq!(current.current_query_memory_bytes(), 0);
}

#[test]
fn should_cancel_after_the_final_row_assertions_are_retained() {
    // Arrange
    let (cassie, identity) = fixture(false);
    let origin = controls(1024 * 1024);
    let fence = RetainedRowFence::capture(&cassie.midge, "postgres", &origin).expect("old fence");
    let cancellation = QueryCancellationHandle::new();
    let current = QueryExecutionControls::with_cancellation(
        &CassieRuntimeLimits::default(),
        Instant::now(),
        cancellation.clone(),
    );
    let mut retained = false;

    // Act
    let result =
        fence.validate_rows_observed([Ok(("row_fence", identity, "one"))], &current, || {
            retained = current.current_query_memory_bytes() > 0;
            cancellation.cancel();
        });

    // Assert
    assert!(retained);
    assert!(matches!(result, Err(CassieError::QueryCancelled)));
    assert_eq!(origin.current_query_memory_bytes(), 0);
    assert_eq!(current.current_query_memory_bytes(), 0);
}

#[test]
fn should_release_admitted_inputs_after_a_small_commit_budget_rejects_the_row() {
    // Arrange
    let (cassie, identity) = fixture(false);
    let origin = controls(1024 * 1024);
    let fence = RetainedRowFence::capture(&cassie.midge, "postgres", &origin).expect("old fence");
    let current = controls(1);

    // Act
    let result = fence.validate_rows([Ok(("row_fence", identity, "one"))], &current);

    // Assert
    assert!(matches!(result, Err(CassieError::ResourceLimit(_))));
    assert_eq!(origin.current_query_memory_bytes(), 0);
    assert_eq!(current.current_query_memory_bytes(), 0);
}

#[test]
fn should_reject_a_legacy_row_rewrite_after_the_retained_start() {
    // Arrange
    let (cassie, identity) = fixture(false);
    let row = Midge::row_key(identity, "one");
    let raw = cassie
        .midge
        .raw_get_database("postgres", &row)
        .expect("encoded row")
        .expect("row");
    let canonical = cassie.midge.canonical_collection_name("row_fence");
    let legacy = Midge::doc_key(&canonical, "one");
    cassie
        .midge
        .raw_put(StorageFamily::Data, &legacy, &raw)
        .expect("legacy fixture");
    cassie
        .midge
        .raw_delete(StorageFamily::Data, &row)
        .expect("legacy-only row");
    let origin = controls(1024 * 1024);
    let fence =
        RetainedRowFence::capture(&cassie.midge, "postgres", &origin).expect("old legacy fence");
    cassie
        .midge
        .raw_put(StorageFamily::Data, &legacy, &raw)
        .expect("same-value legacy rewrite");
    let current = controls(1024 * 1024);

    // Act
    let result = fence.validate_rows([Ok(("row_fence", identity, "one"))], &current);

    // Assert
    assert_eq!(
        result
            .expect_err("legacy changed since start")
            .descriptor()
            .sql_state,
        "40001"
    );
    assert_eq!(origin.current_query_memory_bytes(), 0);
    assert_eq!(current.current_query_memory_bytes(), 0);
}

#[test]
fn should_enforce_the_exact_peak_admission_boundary_for_row_validation() {
    // Arrange
    let (cassie, identity) = fixture(false);
    cassie
        .midge
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
    RetainedRowFence::capture(&cassie.midge, "postgres", &origin)
        .expect("measurement fence")
        .validate_rows(rows(), &measured)
        .expect("measure admitted peak");
    let peak = measured.peak_query_memory_bytes();
    assert!(peak > 1);

    // Act
    let below = controls(peak - 1);
    let denied = RetainedRowFence::capture(&cassie.midge, "postgres", &origin)
        .expect("below-boundary fence")
        .validate_rows(rows(), &below);
    let exact = controls(peak);
    let admitted = RetainedRowFence::capture(&cassie.midge, "postgres", &origin)
        .expect("exact-boundary fence")
        .validate_rows(rows(), &exact);

    // Assert
    assert!(matches!(denied, Err(CassieError::ResourceLimit(_))));
    assert!(admitted.is_ok());
    assert_eq!(below.current_query_memory_bytes(), 0);
    assert_eq!(exact.current_query_memory_bytes(), 0);
    assert_eq!(origin.current_query_memory_bytes(), 0);
}
