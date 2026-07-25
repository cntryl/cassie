use cassie::app::Cassie;
use cassie::catalog::{ColumnBatchMetadata, ColumnBatchSegmentMeta};
use cassie::midge::adapter::StorageFamily;
use cassie::types::Value;
use std::sync::{Arc, Barrier};

#[path = "support/sql.rs"]
mod support;
use support::{data_dir, with_fallback};

fn segment_owns(segment: &ColumnBatchSegmentMeta, id: &str) -> bool {
    segment
        .row_id_start
        .as_deref()
        .is_some_and(|start| id >= start)
        && segment.row_id_end.as_deref().is_none_or(|end| id < end)
}

fn assert_single_segment_rewrite(
    before: &ColumnBatchMetadata,
    after: &ColumnBatchMetadata,
    touched_id: u64,
) {
    assert_eq!(after.segments.len(), before.segments.len());
    for before_segment in &before.segments {
        let after_segment = after
            .segments
            .iter()
            .find(|segment| segment.segment_id == before_segment.segment_id)
            .expect("segment remains present");
        if before_segment.segment_id == touched_id {
            assert_eq!(after_segment.revision, before_segment.revision + 1);
            assert_ne!(
                after_segment.summary_checksum,
                before_segment.summary_checksum
            );
        } else {
            assert_eq!(after_segment.revision, before_segment.revision);
            assert_eq!(
                after_segment.field_chunks, before_segment.field_chunks,
                "untouched segment chunks changed"
            );
            assert_eq!(
                after_segment.row_ids, before_segment.row_ids,
                "untouched row IDs changed"
            );
        }
    }
}

fn metric_delta(before: &serde_json::Value, after: &serde_json::Value, name: &str) -> u64 {
    after["column_batches"][name]
        .as_u64()
        .unwrap_or_default()
        .saturating_sub(before["column_batches"][name].as_u64().unwrap_or_default())
}

fn assert_median_split(before: &ColumnBatchMetadata, after: &ColumnBatchMetadata) {
    assert_eq!(after.segments.len(), 2);
    assert_eq!(after.segments[0].segment_id, before.segments[0].segment_id);
    assert_eq!(after.segments[0].revision, before.segments[0].revision + 1);
    assert_ne!(after.segments[1].segment_id, after.segments[0].segment_id);
    assert_eq!(after.segments[1].revision, 1);
    assert_eq!(
        after
            .segments
            .iter()
            .map(|segment| segment.row_count)
            .sum::<usize>(),
        5
    );
    assert_eq!(after.segments[0].row_id_end, after.segments[1].row_id_start);
}

#[test]
fn should_rewrite_only_the_touched_segment_for_single_row_dml() {
    // Arrange
    with_fallback();
    let path = data_dir("column_batch_incremental_single_row");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).expect("create Cassie");
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE incremental_single_row (status TEXT, amount INT)",
                vec![],
            )
            .expect("create table");
        for amount in 0..8 {
            cassie
                .midge
                .put_document(
                    "incremental_single_row",
                    Some(format!("row-{amount:02}")),
                    serde_json::json!({ "status": "old", "amount": amount }),
                )
                .expect("seed document");
        }
        cassie
            .execute_sql(
                &session,
                "CREATE INDEX incremental_single_row_idx ON incremental_single_row \
                 USING column (status, amount) WITH (segment_size = 2)",
                vec![],
            )
            .expect("create column index");
        let before = cassie
            .midge
            .get_column_batch_metadata("incremental_single_row", "incremental_single_row_idx")
            .expect("load metadata")
            .expect("metadata exists");
        let touched_id = before
            .segments
            .iter()
            .find(|segment| segment_owns(segment, "row-03"))
            .expect("find touched segment")
            .segment_id;
        let metrics_before = cassie.metrics();

        // Act
        cassie
            .midge
            .put_document(
                "incremental_single_row",
                Some("row-03".to_string()),
                serde_json::json!({ "status": "updated", "amount": 30 }),
            )
            .expect("update one document");
        let after_update = cassie
            .midge
            .get_column_batch_metadata("incremental_single_row", "incremental_single_row_idx")
            .expect("load updated metadata")
            .expect("updated metadata exists");
        let result = cassie
            .execute_sql(
                &session,
                "SELECT amount FROM incremental_single_row \
                 WHERE status = 'updated' ORDER BY amount",
                vec![],
            )
            .expect("query updated column batch");
        let metrics_after = cassie.metrics();
        let persisted_chunk_count = cassie
            .midge
            .raw_scan_prefix(StorageFamily::Data, b"")
            .expect("scan persisted column chunks")
            .into_iter()
            .filter(|(_, value)| {
                value.starts_with(b"CBM2")
                    || value.starts_with(b"CBR2")
                    || value.starts_with(b"CBC2")
            })
            .count();

        // Assert
        assert_single_segment_rewrite(&before, &after_update, touched_id);
        assert_eq!(result.rows, vec![vec![Value::Int64(30)]]);
        assert_eq!(persisted_chunk_count, 13);
        assert_eq!(
            metric_delta(&metrics_before, &metrics_after, "segment_rewrites"),
            1
        );
        assert_eq!(
            metric_delta(&metrics_before, &metrics_after, "orphan_revisions_cleaned"),
            3
        );
    });

    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_split_an_overflowing_range_at_the_median() {
    // Arrange
    with_fallback();
    let path = data_dir("column_batch_incremental_split");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).expect("create Cassie");
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE incremental_split (label TEXT, amount INT)",
                vec![],
            )
            .expect("create table");
        for (id, amount) in [("row-00", 0), ("row-99", 99)] {
            cassie
                .midge
                .put_document(
                    "incremental_split",
                    Some(id.to_string()),
                    serde_json::json!({ "label": id, "amount": amount }),
                )
                .expect("seed document");
        }
        cassie
            .execute_sql(
                &session,
                "CREATE INDEX incremental_split_idx ON incremental_split \
                 USING column (label, amount) WITH (segment_size = 2)",
                vec![],
            )
            .expect("create column index");
        for (id, amount) in [("row-10", 10), ("row-20", 20)] {
            cassie
                .midge
                .put_document(
                    "incremental_split",
                    Some(id.to_string()),
                    serde_json::json!({ "label": id, "amount": amount }),
                )
                .expect("grow segment within limit");
        }
        let before_split = cassie
            .midge
            .get_column_batch_metadata("incremental_split", "incremental_split_idx")
            .expect("load metadata")
            .expect("metadata exists");
        assert_eq!(before_split.segments.len(), 1);
        assert_eq!(before_split.segments[0].row_count, 4);
        let metrics_before = cassie.metrics();

        // Act
        cassie
            .midge
            .put_document(
                "incremental_split",
                Some("row-30".to_string()),
                serde_json::json!({ "label": "row-30", "amount": 30 }),
            )
            .expect("insert split row");
        let after_split = cassie
            .midge
            .get_column_batch_metadata("incremental_split", "incremental_split_idx")
            .expect("load split metadata")
            .expect("split metadata exists");
        let result = cassie
            .execute_sql(
                &session,
                "SELECT amount FROM incremental_split ORDER BY amount",
                vec![],
            )
            .expect("query split segments");
        let metrics_after = cassie.metrics();

        // Assert
        assert_median_split(&before_split, &after_split);
        assert_eq!(
            result.rows,
            vec![
                vec![Value::Int64(0)],
                vec![Value::Int64(10)],
                vec![Value::Int64(20)],
                vec![Value::Int64(30)],
                vec![Value::Int64(99)],
            ]
        );
        assert_eq!(
            metric_delta(&metrics_before, &metrics_after, "segment_rewrites"),
            2
        );
        assert_eq!(
            metric_delta(&metrics_before, &metrics_after, "segment_splits"),
            1
        );
    });

    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_rewrite_one_segment_for_actual_deletes_only() {
    // Arrange
    with_fallback();
    let path = data_dir("column_batch_incremental_delete");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).expect("create Cassie");
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE incremental_delete (label TEXT, amount INT)",
                vec![],
            )
            .expect("create table");
        for amount in 0..8 {
            cassie
                .midge
                .put_document(
                    "incremental_delete",
                    Some(format!("row-{amount:02}")),
                    serde_json::json!({ "label": format!("value-{amount}"), "amount": amount }),
                )
                .expect("seed document");
        }
        cassie
            .execute_sql(
                &session,
                "CREATE INDEX incremental_delete_idx ON incremental_delete \
                 USING column (label, amount) WITH (segment_size = 2)",
                vec![],
            )
            .expect("create column index");
        let before = cassie
            .midge
            .get_column_batch_metadata("incremental_delete", "incremental_delete_idx")
            .expect("load metadata")
            .expect("metadata exists");
        let touched_id = before
            .segments
            .iter()
            .find(|segment| segment_owns(segment, "row-03"))
            .expect("find touched segment")
            .segment_id;
        let metrics_before = cassie.metrics();

        // Act
        let missing_deleted = cassie
            .midge
            .delete_document("incremental_delete", "missing")
            .expect("delete missing document");
        let metrics_after_missing = cassie.metrics();
        let actual_deleted = cassie
            .midge
            .delete_document("incremental_delete", "row-03")
            .expect("delete actual document");
        let after = cassie
            .midge
            .get_column_batch_metadata("incremental_delete", "incremental_delete_idx")
            .expect("load updated metadata")
            .expect("updated metadata exists");
        let result = cassie
            .execute_sql(
                &session,
                "SELECT amount FROM incremental_delete ORDER BY amount",
                vec![],
            )
            .expect("query after delete");
        let metrics_after = cassie.metrics();

        // Assert
        assert!(!missing_deleted);
        assert!(actual_deleted);
        assert_eq!(
            metric_delta(&metrics_before, &metrics_after_missing, "segment_rewrites"),
            0
        );
        assert_single_segment_rewrite(&before, &after, touched_id);
        assert_eq!(
            result.rows,
            vec![
                vec![Value::Int64(0)],
                vec![Value::Int64(1)],
                vec![Value::Int64(2)],
                vec![Value::Int64(4)],
                vec![Value::Int64(5)],
                vec![Value::Int64(6)],
                vec![Value::Int64(7)],
            ]
        );
        assert_eq!(
            metric_delta(&metrics_after_missing, &metrics_after, "segment_rewrites"),
            1
        );
    });

    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_keep_concurrent_readers_on_complete_published_generations() {
    // Arrange
    with_fallback();
    let path = data_dir("column_batch_incremental_concurrent_readers");
    let cassie = Arc::new(Cassie::new_with_data_dir(&path).expect("create Cassie"));
    let session = cassie.create_session("tester", None);
    cassie
        .execute_sql(
            &session,
            "CREATE TABLE incremental_concurrent (label TEXT, amount INT)",
            vec![],
        )
        .expect("create table");
    for amount in 0..32 {
        cassie
            .midge
            .put_document(
                "incremental_concurrent",
                Some(format!("row-{amount:02}")),
                serde_json::json!({
                    "label": if amount == 3 { "target" } else { "other" },
                    "amount": amount
                }),
            )
            .expect("seed document");
    }
    cassie
        .execute_sql(
            &session,
            "CREATE INDEX incremental_concurrent_idx ON incremental_concurrent \
             USING column (label, amount) WITH (segment_size = 4)",
            vec![],
        )
        .expect("create column index");
    let start = Arc::new(Barrier::new(6));
    let mut readers = Vec::new();
    for reader_id in 0..4 {
        let reader_cassie = Arc::clone(&cassie);
        let reader_start = Arc::clone(&start);
        readers.push(std::thread::spawn(move || {
            let reader_name = format!("reader-{reader_id}");
            let reader = reader_cassie.create_session(&reader_name, None);
            reader_start.wait();
            for _ in 0..100 {
                let result = reader_cassie
                    .execute_sql(
                        &reader,
                        "SELECT amount FROM incremental_concurrent WHERE label = 'target'",
                        vec![],
                    )
                    .expect("concurrent encoded read");
                assert_eq!(result.rows.len(), 1);
                assert!(matches!(result.rows[0].as_slice(), [Value::Int64(3 | 30)]));
            }
        }));
    }
    let writer_cassie = Arc::clone(&cassie);
    let writer_start = Arc::clone(&start);
    let writer = std::thread::spawn(move || {
        writer_start.wait();
        for iteration in 0..100 {
            writer_cassie
                .midge
                .put_document(
                    "incremental_concurrent",
                    Some("row-03".to_string()),
                    serde_json::json!({
                        "label": "target",
                        "amount": if iteration % 2 == 0 { 30 } else { 3 }
                    }),
                )
                .expect("concurrent incremental update");
        }
    });

    // Act
    start.wait();
    writer.join().expect("writer thread");
    for reader in readers {
        reader.join().expect("reader thread");
    }

    // Assert
    let final_result = cassie
        .execute_sql(
            &session,
            "SELECT amount FROM incremental_concurrent WHERE label = 'target'",
            vec![],
        )
        .expect("final encoded read");
    assert_eq!(final_result.rows, vec![vec![Value::Int64(3)]]);
    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_compact_excess_sparse_ranges_without_changing_query_results() {
    // Arrange
    with_fallback();
    let path = data_dir("column_batch_incremental_compaction");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).expect("create Cassie");
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE incremental_compaction (amount INT)",
                vec![],
            )
            .expect("create table");
        for amount in 0..16 {
            cassie
                .midge
                .put_document(
                    "incremental_compaction",
                    Some(format!("row-{amount:02}")),
                    serde_json::json!({ "amount": amount }),
                )
                .expect("seed document");
        }
        cassie
            .execute_sql(
                &session,
                "CREATE INDEX incremental_compaction_idx ON incremental_compaction \
                 USING column (amount) WITH (segment_size = 4)",
                vec![],
            )
            .expect("create column index");
        let metrics_before = cassie.metrics();

        // Act
        for deleted in [0, 1, 2, 4, 5, 6, 8, 9, 10, 12, 13, 14] {
            assert!(cassie
                .midge
                .delete_document("incremental_compaction", &format!("row-{deleted:02}"))
                .expect("delete sparse document"));
        }
        let metadata = cassie
            .midge
            .get_column_batch_metadata("incremental_compaction", "incremental_compaction_idx")
            .expect("load compacted metadata")
            .expect("metadata exists");
        let result = cassie
            .execute_sql(
                &session,
                "SELECT amount FROM incremental_compaction ORDER BY amount",
                vec![],
            )
            .expect("query compacted ranges");
        let metrics_after = cassie.metrics();

        // Assert
        assert_eq!(metadata.segments.len(), 1);
        assert_eq!(
            result.rows,
            vec![
                vec![Value::Int64(3)],
                vec![Value::Int64(7)],
                vec![Value::Int64(11)],
                vec![Value::Int64(15)],
            ]
        );
        assert!(
            metric_delta(&metrics_before, &metrics_after, "compactions") >= 1,
            "sparse ranges did not trigger copy-on-write compaction"
        );
    });

    let _ = std::fs::remove_dir_all(path);
}
