use super::support_data_dir::data_dir;
use cassie::app::CassieError;
use cassie::midge::adapter::StorageFamily;
use cassie::types::{DataType, FieldSchema, Schema};

#[test]
fn should_decode_minimum_width_array_elements_without_rejecting_valid_counts() {
    // Arrange
    let path = data_dir("row_blob_array_minimum_width");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");
    runtime.block_on(async {
        let cassie = crate::support_sql::new_in_memory_cassie().expect("Cassie");
        let cases = [
            (
                "nulls",
                DataType::Int,
                serde_json::json!([null, null, null]),
            ),
            (
                "bools",
                DataType::Boolean,
                serde_json::json!([false, true, false]),
            ),
            ("strings", DataType::Text, serde_json::json!(["", "", ""])),
            (
                "nested",
                DataType::Array(Box::new(DataType::Int)),
                serde_json::json!([[1, 2], [3, 4], [5, 6]]),
            ),
        ];
        for (collection, element_type, values) in cases {
            let payload = serde_json::json!({"items": values});
            cassie
                .midge
                .create_collection(
                    collection,
                    Schema {
                        fields: vec![FieldSchema {
                            name: "items".to_string(),
                            data_type: DataType::Array(Box::new(element_type)),
                            nullable: true,
                        }],
                    },
                )
                .expect("create collection");
            cassie
                .midge
                .put_document(collection, Some("owner".to_string()), payload.clone())
                .expect("write array");

            // Act
            let stored = cassie
                .midge
                .get_document(collection, "owner")
                .expect("decode valid array")
                .expect("stored row");

            // Assert
            assert_eq!(
                cassie::types::SqlArray::from_storage_json(&stored.payload["items"])
                    .expect("decode stored array shape")
                    .to_nested_json(),
                payload["items"],
            );
        }
    });
    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_reject_impossible_stored_array_count_before_allocating() {
    // Arrange
    let path = data_dir("row_blob_array_count");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");
    runtime.block_on(async {
        let cassie = crate::support_sql::new_in_memory_cassie().expect("Cassie");
        let collection = "array_count_bounds";
        cassie
            .midge
            .create_collection(
                collection,
                Schema {
                    fields: vec![FieldSchema {
                        name: "items".to_string(),
                        data_type: DataType::Array(Box::new(DataType::BigInt)),
                        nullable: true,
                    }],
                },
            )
            .expect("create collection");
        cassie
            .midge
            .put_document(
                collection,
                Some("owner".to_string()),
                serde_json::json!({"items": [1]}),
            )
            .expect("write valid array");
        let (key, mut blob) = cassie
            .midge
            .raw_scan_prefix(StorageFamily::Data, b"")
            .expect("read raw rows")
            .into_iter()
            .find(|(_, value)| value.starts_with(b"CRB3"))
            .expect("row blob");
        assert_eq!(&blob[..4], b"CRB3");
        // One field: 23 header/bitmap bytes, 13 directory bytes, then payload.
        let payload_offset = 36;
        blob.truncate(payload_offset);
        blob[32..36].copy_from_slice(&9_u32.to_be_bytes());
        blob.extend_from_slice(&[0x80, 0x80, 0x80, 0x80, 0x80, 0x80, 0x80, 0x80, 0x10]);
        cassie
            .midge
            .raw_put(StorageFamily::Data, &key, &blob)
            .expect("inject corrupt array count");

        // Act
        let decoded = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            cassie.midge.get_document(collection, "owner")
        }));

        // Assert
        assert!(
            matches!(decoded, Ok(Err(CassieError::Parse(_)))),
            "invalid stored count must return a parse error without panicking"
        );
    });
    let _ = std::fs::remove_dir_all(path);
}
