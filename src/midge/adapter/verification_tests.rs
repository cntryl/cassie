use std::sync::{mpsc, Arc};
use std::thread;
use std::time::Duration;

use super::{
    set_projection_output_failure_point, FieldSchema, Midge, ProjectionOutputFailurePoint, Schema,
};

#[test]
fn should_bound_fresh_projection_output_write_batches() {
    // Arrange
    let row_count = 10_001;

    // Act
    let batches = super::verification::storage::write_ranges(row_count, 256)
        .map(|range| range.len())
        .collect::<Vec<_>>();

    // Assert
    assert_eq!(
        batches,
        vec![
            256, 256, 256, 256, 256, 256, 256, 256, 256, 256, 256, 256, 256, 256, 256, 256, 256,
            256, 256, 256, 256, 256, 256, 256, 256, 256, 256, 256, 256, 256, 256, 256, 256, 256,
            256, 256, 256, 256, 256, 17,
        ]
    );
}

#[test]
fn should_leave_batched_projection_hash_rebuild_unpublished_until_retry_completes() {
    // Arrange
    let path = std::env::temp_dir().join(format!(
        "cassie_projection_batched_hash_rebuild_{}",
        uuid::Uuid::new_v4()
    ));
    let midge = Midge::new_with_data_dir(&path).expect("create Midge");
    midge
        .create_collection(
            "projection_batched_hash_rebuild",
            Schema {
                fields: vec![FieldSchema {
                    name: "value".to_string(),
                    data_type: crate::types::DataType::Text,
                    nullable: false,
                }],
            },
        )
        .expect("create projection collection");
    let rows = (0..5_001)
        .map(|index| {
            (
                Some(format!("row-{index:05}")),
                serde_json::json!({"value": format!("value-{index:05}")}),
            )
        })
        .collect();
    midge
        .put_documents("projection_batched_hash_rebuild", rows)
        .expect("seed projection rows");
    set_projection_output_failure_point(Some(ProjectionOutputFailurePoint::AfterRowBatches));

    // Act
    let interrupted = midge.rebuild_projection_hashes("projection_batched_hash_rebuild");
    let unpublished = midge
        .root_hash("projection_batched_hash_rebuild")
        .expect("read interrupted root");
    let retried = midge
        .rebuild_projection_hashes("projection_batched_hash_rebuild")
        .expect("retry projection hash rebuild");

    // Assert
    assert!(interrupted
        .expect_err("rebuild should stop after row batches")
        .to_string()
        .contains("after row batches"));
    assert!(unpublished.is_none());
    assert_eq!(retried.row_count, 5_001);
    assert_eq!(
        midge
            .list_row_hashes("projection_batched_hash_rebuild")
            .expect("list rebuilt row hashes")
            .len(),
        5_001
    );

    drop(midge);
    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_publish_complete_fresh_projection_output_across_bounded_batches() {
    // Arrange
    let path = std::env::temp_dir().join(format!(
        "cassie_projection_batched_output_{}",
        uuid::Uuid::new_v4()
    ));
    let midge = Midge::new_with_data_dir(&path).expect("create Midge");
    midge
        .create_collection(
            "projection_batched_output",
            Schema {
                fields: vec![FieldSchema {
                    name: "value".to_string(),
                    data_type: crate::types::DataType::Text,
                    nullable: false,
                }],
            },
        )
        .expect("create projection output collection");
    let rows = (0..5_001)
        .map(|index| {
            (
                format!("row-{index:05}"),
                serde_json::json!({"value": format!("value-{index:05}")}),
            )
        })
        .collect::<Vec<_>>();

    // Act
    let (report, root) = midge
        .write_fresh_projection_output_rows("projection_batched_output", rows)
        .expect("write batched projection output");
    let first = midge
        .get_document("projection_batched_output", "row-00000")
        .expect("read first output row");
    let last = midge
        .get_document("projection_batched_output", "row-05000")
        .expect("read last output row");

    // Assert
    assert_eq!(report.stats.row_puts, 5_001);
    assert_eq!(report.stats.batch_flushes, 21);
    assert_eq!(root.row_count, 5_001);
    assert_eq!(root.range_count, 20);
    assert_eq!(
        first.expect("first output row").payload["value"],
        "value-00000"
    );
    assert_eq!(
        last.expect("last output row").payload["value"],
        "value-05000"
    );
    assert_eq!(
        midge
            .list_row_hashes("projection_batched_output")
            .expect("list output row hashes")
            .len(),
        5_001
    );

    drop(midge);
    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_serialize_projection_hash_repair_with_collection_writes() {
    // Arrange
    let path = std::env::temp_dir().join(format!(
        "cassie_projection_repair_gate_{}",
        uuid::Uuid::new_v4()
    ));
    let midge = Arc::new(Midge::new_with_data_dir(&path).expect("create Midge"));
    midge
        .create_collection(
            "projection_repair_gate",
            Schema {
                fields: vec![FieldSchema {
                    name: "value".to_string(),
                    data_type: crate::types::DataType::Text,
                    nullable: true,
                }],
            },
        )
        .expect("create collection");
    midge
        .put_document(
            "projection_repair_gate",
            Some("row-1".to_string()),
            serde_json::json!({"value": "alpha"}),
        )
        .expect("seed document");
    let collection = midge.canonical_collection_name("projection_repair_gate");
    let gate = midge.collection_write_gate(&collection);
    let write_guard = gate.lock();
    let (started_tx, started_rx) = mpsc::channel();
    let (completed_tx, completed_rx) = mpsc::channel();
    let worker_midge = Arc::clone(&midge);

    let worker = thread::spawn(move || {
        started_tx.send(()).expect("signal repair start");
        let result = worker_midge.rebuild_projection_hashes("projection_repair_gate");
        completed_tx.send(result).expect("signal repair finish");
    });
    started_rx
        .recv_timeout(Duration::from_secs(1))
        .expect("repair should start");

    // Act
    let early_result = completed_rx.recv_timeout(Duration::from_millis(100)).ok();
    let blocked_while_write_guard_held = early_result.is_none();
    drop(write_guard);
    let result = early_result.unwrap_or_else(|| {
        completed_rx
            .recv_timeout(Duration::from_secs(2))
            .expect("repair should finish after the write gate is released")
    });
    worker.join().expect("join repair");

    // Assert
    assert!(blocked_while_write_guard_held);
    assert!(result.is_ok());

    drop(midge);
    let _ = std::fs::remove_dir_all(path);
}
