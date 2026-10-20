use cassie::app::CassieError;
use cassie::midge::adapter::{
    query_scan_control_test_guard, set_query_scan_cancellation_after_entries,
};
use cassie::types::Value;

use super::support_column_store_controls::{payload, Fixture, COLLECTION};

// Captured statement ownership admits 704 bytes before native reads.
// 4 KiB admits narrow identity paths while remaining below each 8 KiB payload.
// The second document is the smallest later-row corruption canary.
const POISONED_LATER_ROW: usize = 1;
const ROWS_THROUGH_POISONED_LATER_ROW: usize = POISONED_LATER_ROW + 1;

#[test]
fn should_reject_each_storage_layout_at_the_first_wide_row_boundary() {
    let _guard = query_scan_control_test_guard();
    for column_store in [false, true] {
        // Arrange
        let fixture = if column_store {
            Fixture::new(
                "paired-column-memory",
                4_096,
                1,
                ROWS_THROUGH_POISONED_LATER_ROW,
                8_192,
            )
        } else {
            Fixture::row_store(
                "paired-row-memory",
                4_096,
                ROWS_THROUGH_POISONED_LATER_ROW,
                8_192,
            )
        };
        fixture.poison_payload(POISONED_LATER_ROW);
        let before_reads = fixture.cassie.midge.query_scan_entries_for_diagnostics();
        set_query_scan_cancellation_after_entries(Some(2));

        // Act
        let result = fixture.cassie.execute_sql(
            &fixture.session,
            &format!("SELECT payload FROM {COLLECTION}"),
            vec![],
        );
        set_query_scan_cancellation_after_entries(None);
        let error = result.expect_err("first wide row must exceed the budget");

        // Assert
        assert!(
            matches!(error, CassieError::ResourceLimit(_)),
            "column_store={column_store}: {error:?}"
        );
        assert_eq!(fixture.reads_since(before_reads), 1);
        fixture.assert_cleanup();
    }
}

#[test]
fn should_bound_full_shape_scans_before_later_decode_errors_with_each_worker_count() {
    let _guard = query_scan_control_test_guard();
    for workers in [1, 4] {
        for column_store in [true, false] {
            // Arrange
            let fixture = if column_store {
                Fixture::new(
                    "full-shape-column-memory",
                    4_096,
                    workers,
                    ROWS_THROUGH_POISONED_LATER_ROW,
                    8_192,
                )
            } else {
                Fixture::row_store_with_workers(
                    "full-shape-row-memory",
                    4_096,
                    workers,
                    ROWS_THROUGH_POISONED_LATER_ROW,
                    8_192,
                )
            };
            fixture.poison_payload(POISONED_LATER_ROW);
            let before_reads = fixture.cassie.midge.query_scan_entries_for_diagnostics();
            let before_metrics = fixture.cassie.metrics();

            // Act
            let error = fixture
                .cassie
                .execute_sql(
                    &fixture.session,
                    &format!("SELECT length(payload) AS payload_length FROM {COLLECTION}"),
                    vec![],
                )
                .expect_err("first full-shape row must exhaust the budget before its canary");
            let reads = fixture.reads_since(before_reads);
            let after_metrics = fixture.cassie.metrics();

            // Assert
            assert!(
                matches!(error, CassieError::ResourceLimit(_)),
                "column_store={column_store}, workers={workers}, reads={reads}: {error:?}"
            );
            assert_eq!(reads, 1, "column_store={column_store}, workers={workers}");
            assert_eq!(
                after_metrics["query"]["rows_returned_total"],
                before_metrics["query"]["rows_returned_total"]
            );
            assert_eq!(after_metrics["runtime"]["active_operator_workers"], 0);
            fixture.assert_cleanup();
        }
    }
}

#[test]
fn should_cancel_a_bounded_column_store_join_before_decoding_the_remaining_probe_rows() {
    // Arrange
    let _guard = query_scan_control_test_guard();
    let fixture = Fixture::new("column-store-bounded-join", 4 * 1_024 * 1_024, 1, 16, 64);
    fixture.poison_payload(15);
    fixture
        .cassie
        .execute_sql(
            &fixture.session,
            "CREATE TABLE controlled_column_join_rhs (payload TEXT)",
            vec![],
        )
        .expect("create build relation");
    fixture
        .cassie
        .midge
        .put_document(
            "controlled_column_join_rhs",
            Some("right".to_owned()),
            serde_json::json!({"payload": "unmatched"}),
        )
        .expect("seed build relation");
    let before_reads = fixture.cassie.midge.query_scan_entries_for_diagnostics();
    set_query_scan_cancellation_after_entries(Some(3));

    // Act
    let result = fixture.cassie.execute_sql(
            &fixture.session,
            &format!("SELECT {COLLECTION}._id FROM {COLLECTION} INNER JOIN controlled_column_join_rhs ON {COLLECTION}.payload = controlled_column_join_rhs.payload LIMIT 1"),
            vec![],
        );
    set_query_scan_cancellation_after_entries(None);
    let error = result.expect_err("bounded join must stop at its controlled probe boundary");

    // Assert
    assert!(matches!(error, CassieError::QueryCancelled), "{error:?}");
    assert_eq!(
        fixture.reads_since(before_reads),
        3,
        "one build row and two probe markers must reach cancellation before the canary"
    );
    fixture.assert_cleanup();
}

#[test]
fn should_reject_wide_column_store_scans_before_decoding_later_rows() {
    let _guard = query_scan_control_test_guard();
    for workers in [1, 4] {
        // Arrange
        let fixture = Fixture::new(
            "column-store-pre-decode-budget",
            4_096,
            workers,
            ROWS_THROUGH_POISONED_LATER_ROW,
            8_192,
        );
        fixture.poison_payload(POISONED_LATER_ROW);
        for projection in [
            format!("SELECT payload FROM {COLLECTION}"),
            format!("SELECT length(payload) FROM {COLLECTION}"),
            format!("SELECT payload FROM {COLLECTION} ORDER BY payload LIMIT 5"),
        ] {
            let before_reads = fixture.cassie.midge.query_scan_entries_for_diagnostics();
            let before_metrics = fixture.cassie.metrics();
            set_query_scan_cancellation_after_entries(Some(2));

            // Act
            let result = fixture
                .cassie
                .execute_sql(&fixture.session, &projection, vec![]);
            set_query_scan_cancellation_after_entries(None);
            let error = result.expect_err("the first wide row must exhaust the budget");

            // Assert
            assert!(
                matches!(error, CassieError::ResourceLimit(_)),
                "workers={workers}, {projection}: {error:?}"
            );
            assert_eq!(fixture.reads_since(before_reads), 1, "the first-row reservation must fail before the second controlled read or later canary decode");
            assert_eq!(
                fixture.cassie.metrics()["query"]["rows_returned_total"],
                before_metrics["query"]["rows_returned_total"],
                "failed scan published rows"
            );
            fixture.assert_cleanup();
        }
    }
}

#[test]
fn should_cancel_each_column_store_scan_shape_after_three_reads() {
    let _guard = query_scan_control_test_guard();
    for workers in [1, 4] {
        // Arrange
        let fixture = Fixture::new(
            "column-store-mid-scan-cancel",
            4 * 1_024 * 1_024,
            workers,
            16,
            64,
        );
        for statement in [
            format!("SELECT payload FROM {COLLECTION}"),
            format!("SELECT length(payload) FROM {COLLECTION}"),
            format!("SELECT payload FROM {COLLECTION} ORDER BY payload LIMIT 3"),
            format!("SELECT payload FROM {COLLECTION} ORDER BY _id DESC LIMIT 3"),
        ] {
            let before_reads = fixture.cassie.midge.query_scan_entries_for_diagnostics();
            let before_metrics = fixture.cassie.metrics();
            set_query_scan_cancellation_after_entries(Some(3));

            // Act
            let result = fixture
                .cassie
                .execute_sql(&fixture.session, &statement, vec![]);
            set_query_scan_cancellation_after_entries(None);
            let error =
                result.expect_err("ColumnStore must observe the controlled cancellation hook");

            // Assert
            assert!(
                matches!(error, CassieError::QueryCancelled),
                "workers={workers}, {statement}: {error:?}"
            );
            assert_eq!(
                fixture.reads_since(before_reads),
                3,
                "workers={workers}, {statement}"
            );
            assert_eq!(
                fixture.cassie.metrics()["query"]["rows_returned_total"],
                before_metrics["query"]["rows_returned_total"],
                "cancelled scan published rows"
            );
            fixture.assert_cleanup();
        }
    }
}

#[test]
fn should_stop_column_store_identity_limit_before_reading_unneeded_wide_fields() {
    let _guard = query_scan_control_test_guard();
    for workers in [1, 4] {
        // Arrange
        let fixture = Fixture::new("column-store-bounded-limit", 4_096, workers, 1, 8_192);
        fixture.poison_payload(0);
        let before_reads = fixture.cassie.midge.query_scan_entries_for_diagnostics();
        set_query_scan_cancellation_after_entries(Some(2));

        // Act
        let result = fixture.cassie.execute_sql(
            &fixture.session,
            &format!("SELECT _id FROM {COLLECTION} LIMIT 1"),
            vec![],
        );
        set_query_scan_cancellation_after_entries(None);
        let result = result.expect("identity-only LIMIT should not decode payload fields");

        // Assert
        assert_eq!(
            result.rows(),
            vec![vec![Value::String("doc-0000".to_owned())]]
        );
        assert_eq!(
            fixture.reads_since(before_reads),
            1,
            "LIMIT must enumerate exactly one live marker"
        );
        assert!(
            fixture.cassie.metrics()["query"]["current_accounted_memory_bytes"]
                .as_u64()
                .expect("live result memory")
                > 0
        );
        drop(result);
        fixture.assert_cleanup();
    }
}

#[test]
fn should_merge_column_store_staged_changes_without_an_eager_scan() {
    // Arrange
    let _guard = query_scan_control_test_guard();
    let fixture = Fixture::new("column-store-staged-limit", 64 * 1_024, 1, 16, 64);
    for statement in [
        "BEGIN".to_owned(),
        format!("UPDATE {COLLECTION} SET payload = 'staged' WHERE _id = 'doc-0000'"),
        format!("DELETE FROM {COLLECTION} WHERE _id = 'doc-0001'"),
    ] {
        fixture
            .cassie
            .execute_sql(&fixture.session, &statement, vec![])
            .expect("stage ColumnStore mutation");
    }
    let before_reads = fixture.cassie.midge.query_scan_entries_for_diagnostics();
    set_query_scan_cancellation_after_entries(Some(4));

    // Act
    let result = fixture.cassie.execute_sql(
        &fixture.session,
        &format!("SELECT _id, payload FROM {COLLECTION} LIMIT 2"),
        vec![],
    );
    set_query_scan_cancellation_after_entries(None);
    let result = result.expect("staged merge should retain its existing row-ID order");

    // Assert
    assert_eq!(
        result.rows(),
        vec![
            vec![
                Value::String("doc-0000".to_owned()),
                Value::String("staged".to_owned())
            ],
            vec![
                Value::String("doc-0002".to_owned()),
                Value::String(payload(2, 64))
            ],
        ]
    );
    assert_eq!(
        fixture.reads_since(before_reads),
        3,
        "two outputs plus the staged delete must consume three persisted markers"
    );
    assert!(
        fixture.cassie.metrics()["query"]["current_accounted_memory_bytes"]
            .as_u64()
            .expect("live result memory")
            > 0
    );
    drop(result);
    fixture.assert_cleanup();
    fixture
        .cassie
        .execute_sql(&fixture.session, "ROLLBACK", vec![])
        .expect("rollback");
}
