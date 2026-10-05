use std::time::Instant;

use crate::catalog::collections::{CollectionMeta, CollectionStorageMode};
use crate::config::CassieRuntimeLimits;
use crate::midge::adapter::{query_scan_control_test_guard, StorageFamily};
use crate::types::{DataType, FieldSchema, Schema};

use super::{CassieError, Midge, QueryExecutionControls, RowDecode};

mod row_store;

#[test]
fn should_reserve_sparse_column_store_json_before_decoding_a_compact_field() {
    // Arrange
    let _guard = query_scan_control_test_guard();
    let path = std::env::temp_dir().join(format!(
        "cassie-column-json-budget-{}",
        uuid::Uuid::new_v4()
    ));
    let midge = Midge::new_strict_with_data_dir(&path).expect("local Midge");
    midge.ensure_families_ready().expect("storage families");
    let collection = "column_cursor_json_budget";
    midge
        .create_collection_with_meta(
            collection,
            &Schema {
                fields: vec![FieldSchema {
                    name: "payload".to_owned(),
                    data_type: DataType::Json,
                    nullable: true,
                }],
            },
            &CollectionMeta::new_with_storage_mode(
                collection,
                None,
                CollectionStorageMode::ColumnStore,
            ),
        )
        .expect("ColumnStore collection");
    let mut nested = serde_json::json!(0);
    for _ in 0..32 {
        nested = serde_json::json!({"a": nested});
    }
    let serialized = serde_json::to_vec(&nested).expect("serialized nested JSON");
    let mut compact = crate::midge::row_blob::encode_compact_value(&DataType::Json, &nested)
        .expect("compact JSON");
    let start = compact
        .windows(serialized.len())
        .position(|bytes| bytes == serialized)
        .expect("serialized JSON inside compact field");
    let scalar = serialized
        .iter()
        .position(|byte| *byte == b'0')
        .expect("innermost scalar");
    compact[start + scalar] = b'X';
    midge
        .put_document(
            collection,
            Some("one".to_owned()),
            serde_json::json!({"payload": nested}),
        )
        .expect("seed nested JSON");
    let schema = midge.row_schema(collection).expect("row schema");
    midge
        .raw_put(
            StorageFamily::Data,
            &Midge::column_store_field_key(schema.relation_id, schema.fields[0].field_id, "one"),
            &compact,
        )
        .expect("invalid JSON decode canary");
    let controls = QueryExecutionControls::from_limits(
        &CassieRuntimeLimits {
            query_memory_budget_bytes: 16 * 1_024,
            ..CassieRuntimeLimits::default()
        },
        Instant::now(),
    );
    let mut cursor = midge
        .open_row_cursor(collection, RowDecode::Full)
        .expect("open cursor")
        .expect("ColumnStore cursor");
    let before = midge.query_scan_entries_for_diagnostics();

    // Act
    let result = cursor.next_accounted_document(&midge, &controls);

    // Assert
    assert!(
        matches!(result, Err(CassieError::ResourceLimit(_))),
        "sparse JSON must reserve node storage before its decode canary: {result:?}"
    );
    assert_eq!(midge.query_scan_entries_for_diagnostics() - before, 1);
    drop(cursor);
    assert_eq!(controls.current_query_memory_bytes(), 0);
    drop(midge);
    std::fs::remove_dir_all(path).expect("remove storage fixture");
}

#[test]
fn should_apply_configured_column_store_deadline_before_decode() {
    // Arrange
    let _guard = query_scan_control_test_guard();
    let path =
        std::env::temp_dir().join(format!("cassie-column-deadline-{}", uuid::Uuid::new_v4()));
    let midge = Midge::new_strict_with_data_dir(&path).expect("local Midge");
    midge.ensure_families_ready().expect("storage families");
    let collection = "column_cursor_deadline";
    midge
        .create_collection_with_meta(
            collection,
            &Schema {
                fields: vec![FieldSchema {
                    name: "payload".to_owned(),
                    data_type: DataType::Text,
                    nullable: true,
                }],
            },
            &CollectionMeta::new_with_storage_mode(
                collection,
                None,
                CollectionStorageMode::ColumnStore,
            ),
        )
        .expect("ColumnStore collection");
    midge
        .put_document(
            collection,
            Some("one".to_owned()),
            serde_json::json!({"payload": "first"}),
        )
        .expect("first row");
    midge
        .put_document(
            collection,
            Some("two".to_owned()),
            serde_json::json!({"payload": "second"}),
        )
        .expect("second row");
    let schema = midge.row_schema(collection).expect("row schema");
    let second_key = Midge::column_store_row_key(schema.relation_id, "two");
    midge
        .raw_put(
            StorageFamily::Data,
            &Midge::column_store_field_key(schema.relation_id, schema.fields[0].field_id, "two"),
            &[u8::MAX],
        )
        .expect("decode canary");
    let limits = CassieRuntimeLimits {
        query_timeout_ms: 0,
        ..CassieRuntimeLimits::default()
    };
    let mut controls = QueryExecutionControls::from_limits(&limits, Instant::now());
    assert!(controls.deadline.is_none());
    let mut cursor = midge
        .open_row_cursor(collection, RowDecode::Full)
        .expect("open cursor")
        .expect("ColumnStore cursor");
    controls.deadline = Some(Instant::now());
    let before = midge.query_scan_entries_for_diagnostics();

    // Act
    let expired_scan = cursor.next_accounted_document(&midge, &controls);
    let expired_field = cursor.decode_column_store_document(&second_key, &controls);
    controls.deadline = None;
    let first = cursor
        .next_accounted_document(&midge, &controls)
        .expect("timeout zero scan")
        .expect("first document");
    let decoded_canary = cursor.decode_column_store_document(&second_key, &controls);

    // Assert
    assert!(matches!(expired_scan, Err(CassieError::DeadlineExceeded)));
    assert!(matches!(expired_field, Err(CassieError::DeadlineExceeded)));
    assert_eq!(midge.query_scan_entries_for_diagnostics() - before, 1);
    assert_eq!(first.id(), "one");
    assert_eq!(
        first.document().payload,
        serde_json::json!({"payload": "first"})
    );
    assert!(matches!(decoded_canary, Err(CassieError::Parse(_))));
    drop(first);
    drop(cursor);
    assert_eq!(controls.current_query_memory_bytes(), 0);
    drop(midge);
    std::fs::remove_dir_all(path).expect("remove storage fixture");
}
