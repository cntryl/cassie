#![allow(unused_imports, dead_code)]

use cassie::app::Cassie;
use cassie::catalog::{ColumnBatchPayload, IndexKind};
use cassie::midge::adapter::StorageFamily;
use cassie::sql::ast::QueryStatement;
use cassie::sql::parse_statement;
use cassie::types::Value;
use cntryl_midge::{TransactionMode, WriteOptions};

#[path = "support/sql.rs"]
mod support;
use support::*;

#[test]
fn should_parse_column_index_with_segment_size() {
    // Arrange
    let sql =
        "CREATE INDEX idx_docs_column ON docs USING column (title, body) WITH (segment_size = 2)";

    // Act
    let parsed = parse_statement(sql).expect("parse should succeed");

    // Assert
    let QueryStatement::CreateIndex(statement) = parsed.statement else {
        panic!("expected create index statement");
    };
    assert_eq!(statement.name, "idx_docs_column");
    assert_eq!(
        statement.fields,
        vec!["title".to_string(), "body".to_string()]
    );
    assert_eq!(statement.kind, IndexKind::Column);
    assert_eq!(
        statement.options.get("segment_size"),
        Some(&"2".to_string())
    );
}

#[test]
fn should_read_covered_projection_from_column_batch_index() {
    // Arrange
    with_fallback();
    let path = data_dir("column_batch_projection");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE column_batch_projection (title TEXT, body TEXT, score INT)",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO column_batch_projection (title, body, score) VALUES ('alpha', 'one', 10)",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO column_batch_projection (title, score) VALUES ('beta', 20)",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "CREATE INDEX idx_column_batch_projection ON column_batch_projection USING column (title, body) WITH (segment_size = 1)",
                vec![],
            )
            .unwrap();

        // Act
        let result = cassie
            .execute_sql(
                &session,
                "SELECT title, body FROM column_batch_projection WHERE title = 'alpha'",
                vec![],
            )
            .unwrap();
        let explain = cassie
            .execute_sql(
                &session,
                "EXPLAIN SELECT title, body FROM column_batch_projection WHERE title = 'alpha'",
                vec![],
            )
            .unwrap();
        let metrics = cassie.metrics();

        // Assert
        assert_eq!(result.rows, vec![vec![
            Value::String("alpha".to_string()),
            Value::String("one".to_string()),
        ]]);
        let Value::String(plan) = &explain.rows[0][0] else {
            panic!("expected textual plan");
        };
        assert!(plan.contains("column_batch_index=idx_column_batch_projection"));
        assert_eq!(metrics["column_batches"]["scans"], 1);
        assert_eq!(metrics["column_batches"]["row_fetches_avoided"], 1);
    });

    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_persist_hydrate_drop_column_batch_metadata() {
    // Arrange
    with_fallback();
    let path = data_dir("column_batch_metadata");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        {
            let cassie = Cassie::new_with_data_dir(&path).unwrap();
            let session = cassie.create_session("tester", None);
            cassie
                .execute_sql(
                    &session,
                    "CREATE TABLE column_batch_metadata (title TEXT, body TEXT)",
                    vec![],
                )
                .unwrap();
            cassie
                .execute_sql(
                    &session,
                    "INSERT INTO column_batch_metadata (title, body) VALUES ('alpha', 'one')",
                    vec![],
                )
                .unwrap();
            cassie
                .execute_sql(
                    &session,
                    "CREATE INDEX idx_column_batch_metadata ON column_batch_metadata USING column (title, body) WITH (segment_size = 1)",
                    vec![],
                )
                .unwrap();
        }

        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        cassie.hydrate_catalog().unwrap();
        let session = cassie.create_session("tester", None);

        // Act
        let metadata = cassie
            .midge
            .get_column_batch_metadata("column_batch_metadata", "idx_column_batch_metadata")
            .unwrap()
            .expect("column batch metadata should hydrate");
        let result = cassie
            .execute_sql(
                &session,
                "SELECT title, body FROM column_batch_metadata WHERE title = 'alpha'",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "DROP INDEX idx_column_batch_metadata ON column_batch_metadata",
                vec![],
            )
            .unwrap();
        let dropped = cassie
            .midge
            .get_column_batch_metadata("column_batch_metadata", "idx_column_batch_metadata")
            .unwrap();

        // Assert
        assert_eq!(metadata.fields, vec!["title".to_string(), "body".to_string()]);
        assert_eq!(metadata.segments.len(), 1);
        assert_eq!(result.rows.len(), 1);
        assert!(dropped.is_none());
    });

    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_select_dictionary_rle_codec_for_repeated_column_values() {
    // Arrange
    with_fallback();
    let path = data_dir("column_batch_rle_codec");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE column_batch_rle_codec (status TEXT, body TEXT)",
                vec![],
            )
            .unwrap();
        for _ in 0..8 {
            cassie
                .execute_sql(
                    &session,
                    "INSERT INTO column_batch_rle_codec (status, body) VALUES ('active', 'same')",
                    vec![],
                )
                .unwrap();
        }

        // Act
        cassie
            .execute_sql(
                &session,
                "CREATE INDEX idx_column_batch_rle_codec ON column_batch_rle_codec USING column (status, body) WITH (segment_size = 8)",
                vec![],
            )
            .unwrap();
        let metadata = cassie
            .midge
            .get_column_batch_metadata("column_batch_rle_codec", "idx_column_batch_rle_codec")
            .unwrap()
            .expect("column batch metadata should exist");
        let result = cassie
            .execute_sql(
                &session,
                "SELECT status, body FROM column_batch_rle_codec WHERE status = 'active'",
                vec![],
            )
            .unwrap();
        let metrics = cassie.metrics();

        // Assert
        assert_eq!(metadata.segments.len(), 1);
        let codec = &metadata.segments[0].codec;
        assert_eq!(codec.codec_name, "dictionary_rle");
        assert!(codec.compressed_len < codec.uncompressed_len);
        assert_eq!(codec.value_count, 16);
        assert_eq!(result.rows.len(), 8);
        assert_eq!(metrics["column_batches"]["scans"], 1);
        assert!(metrics["column_batches"]["compressed_bytes_total"]
            .as_u64()
            .unwrap()
            < metrics["column_batches"]["uncompressed_bytes_total"]
                .as_u64()
                .unwrap());
    });

    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_fallback_to_row_blobs_for_corrupt_column_segment() {
    // Arrange
    with_fallback();
    let path = data_dir("column_batch_corrupt_fallback");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE column_batch_corrupt_fallback (title TEXT, body TEXT)",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO column_batch_corrupt_fallback (title, body) VALUES ('alpha', 'one')",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "CREATE INDEX idx_column_batch_corrupt_fallback ON column_batch_corrupt_fallback USING column (title, body) WITH (segment_size = 1)",
                vec![],
            )
            .unwrap();
        let entries = cassie
            .midge
            .raw_scan_prefix(StorageFamily::Data, b"")
            .unwrap();
        let segment_key = entries
            .into_iter()
            .find_map(|(key, value)| {
                serde_json::from_slice::<ColumnBatchPayload>(&value)
                    .ok()
                    .map(|_| key)
            })
            .expect("column batch segment should be persisted");
        let mut tx = cassie.midge.data_tx(TransactionMode::ReadWrite).unwrap();
        tx.put(segment_key, b"not-json".to_vec(), None)
            .unwrap();
        tx.commit(WriteOptions::sync()).unwrap();

        // Act
        let result = cassie
            .execute_sql(
                &session,
                "SELECT title, body FROM column_batch_corrupt_fallback WHERE title = 'alpha'",
                vec![],
            )
            .unwrap();
        let metrics = cassie.metrics();

        // Assert
        assert_eq!(
            result.rows,
            vec![vec![
                Value::String("alpha".to_string()),
                Value::String("one".to_string()),
            ]]
        );
        assert_eq!(metrics["column_batches"]["decode_fallbacks"], 1);
        assert_eq!(metrics["column_batches"]["fallback_scans"], 1);
    });

    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_prune_column_batch_segments_for_range_filter() {
    // Arrange
    with_fallback();
    let path = data_dir("column_batch_range_pruning");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE column_batch_range_pruning (label TEXT, score INT)",
                vec![],
            )
            .unwrap();
        for score in 0..6 {
            cassie
                .execute_sql(
                    &session,
                    &format!(
                        "INSERT INTO column_batch_range_pruning (label, score) VALUES ('row{score}', {score})"
                    ),
                    vec![],
                )
                .unwrap();
        }
        cassie
            .execute_sql(
                &session,
                "CREATE INDEX idx_column_batch_range_pruning ON column_batch_range_pruning USING column (label, score) WITH (segment_size = 2)",
                vec![],
            )
            .unwrap();

        // Act
        let result = cassie
            .execute_sql(
                &session,
                "SELECT label FROM column_batch_range_pruning WHERE score >= 4 ORDER BY label",
                vec![],
            )
            .unwrap();
        let explain = cassie
            .execute_sql(
                &session,
                "EXPLAIN SELECT label FROM column_batch_range_pruning WHERE score >= 4 ORDER BY label",
                vec![],
            )
            .unwrap();
        let metrics = cassie.metrics();

        // Assert
        assert_eq!(
            result.rows,
            vec![
                vec![Value::String("row4".to_string())],
                vec![Value::String("row5".to_string())],
            ]
        );
        let Value::String(plan) = &explain.rows[0][0] else {
            panic!("expected textual plan");
        };
        assert!(plan.contains("column_batch_index=idx_column_batch_range_pruning"));
        assert_eq!(metrics["column_batches"]["scans"], 1);
        assert!(metrics["column_batches"]["skipped_segments"].as_u64().unwrap() > 0);
        assert!(metrics["column_batches"]["decoded_columns"].as_u64().unwrap() < 6);
    });

    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_preserve_sparse_nulls_during_column_batch_scan_pruning() {
    // Arrange
    with_fallback();
    let path = data_dir("column_batch_sparse_null_pruning");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE column_batch_sparse_null_pruning (title TEXT, category TEXT)",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO column_batch_sparse_null_pruning (title) VALUES ('alpha')",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO column_batch_sparse_null_pruning (title, category) VALUES ('beta', 'kept')",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "CREATE INDEX idx_column_batch_sparse_null_pruning ON column_batch_sparse_null_pruning USING column (title, category) WITH (segment_size = 1)",
                vec![],
            )
            .unwrap();

        // Act
        let result = cassie
            .execute_sql(
                &session,
                "SELECT title, category FROM column_batch_sparse_null_pruning WHERE category IS NULL",
                vec![],
            )
            .unwrap();
        let metrics = cassie.metrics();

        // Assert
        assert_eq!(
            result.rows,
            vec![vec![Value::String("alpha".to_string()), Value::Null]]
        );
        assert_eq!(metrics["column_batches"]["scans"], 1);
        assert_eq!(metrics["column_batches"]["skipped_segments"], 1);
    });

    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_rebuild_column_batch_scan_metadata_after_update_delete() {
    // Arrange
    with_fallback();
    let path = data_dir("column_batch_scan_rebuild");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE column_batch_scan_rebuild (title TEXT, score INT)",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO column_batch_scan_rebuild (title, score) VALUES ('alpha', 1)",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO column_batch_scan_rebuild (title, score) VALUES ('beta', 2)",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "CREATE INDEX idx_column_batch_scan_rebuild ON column_batch_scan_rebuild USING column (title, score) WITH (segment_size = 1)",
                vec![],
            )
            .unwrap();

        // Act
        cassie
            .execute_sql(
                &session,
                "UPDATE column_batch_scan_rebuild SET score = 10 WHERE title = 'alpha'",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "DELETE FROM column_batch_scan_rebuild WHERE title = 'beta'",
                vec![],
            )
            .unwrap();
        let result = cassie
            .execute_sql(
                &session,
                "SELECT title FROM column_batch_scan_rebuild WHERE score >= 10",
                vec![],
            )
            .unwrap();
        let metadata = cassie
            .midge
            .get_column_batch_metadata("column_batch_scan_rebuild", "idx_column_batch_scan_rebuild")
            .unwrap()
            .expect("column batch metadata should exist");

        // Assert
        assert_eq!(result.rows, vec![vec![Value::String("alpha".to_string())]]);
        assert_eq!(metadata.segments.len(), 1);
        assert_eq!(metadata.segments[0].summaries["score"].non_null_count, 1);
    });

    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_fallback_to_row_blobs_for_active_session_changes() {
    // Arrange
    with_fallback();
    let path = data_dir("column_batch_session_fallback");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE column_batch_session_fallback (title TEXT)",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO column_batch_session_fallback (title) VALUES ('alpha')",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "CREATE INDEX idx_column_batch_session_fallback ON column_batch_session_fallback USING column (title) WITH (segment_size = 1)",
                vec![],
            )
            .unwrap();

        // Act
        cassie.execute_sql(&session, "BEGIN", vec![]).unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO column_batch_session_fallback (title) VALUES ('beta')",
                vec![],
            )
            .unwrap();
        let result = cassie
            .execute_sql(
                &session,
                "SELECT title FROM column_batch_session_fallback ORDER BY title",
                vec![],
            )
            .unwrap();
        let metrics = cassie.metrics();

        // Assert
        assert_eq!(
            result.rows,
            vec![
                vec![Value::String("alpha".to_string())],
                vec![Value::String("beta".to_string())],
            ]
        );
        assert_eq!(metrics["column_batches"]["scans"], 0);
        assert_eq!(metrics["column_batches"]["row_blob_fetches"], 2);
        assert_eq!(
            metrics["column_batches"]["last_fallback_reason"],
            "session-changes"
        );
    });

    let _ = std::fs::remove_dir_all(path);
}
