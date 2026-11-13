//! Byte-exact pins of the column-store key and value layout.
//!
//! Keys are `cassie \0 \x01 \0 \x18 \0 <relation u64 BE> \0 <discriminator> \0 ...`
//! and every integer is big-endian, so the bytes are identical on every
//! architecture.

use super::validation::{decode_marker_parts, encode_deleted_marker, encode_marker};
use crate::catalog::collections::{CollectionMeta, CollectionStorageMode};
use crate::midge::adapter::{Midge, StorageFamily};
use crate::types::{DataType, FieldSchema, Schema};

const HEADER: &[u8] = b"cassie\0\x01\0\x18\0";

fn golden_midge() -> (Midge, u64) {
    let midge = Midge::new_in_memory("postgres").expect("memory Midge");
    midge.ensure_families_ready().expect("families");
    let schema = Schema {
        fields: vec![
            FieldSchema {
                name: "title".to_string(),
                data_type: DataType::Text,
                nullable: true,
            },
            FieldSchema {
                name: "qty".to_string(),
                data_type: DataType::BigInt,
                nullable: true,
            },
            FieldSchema {
                name: "note".to_string(),
                data_type: DataType::Text,
                nullable: true,
            },
        ],
    };
    midge
        .create_collection_with_meta(
            "golden",
            &schema,
            &CollectionMeta::new_with_storage_mode(
                "golden",
                None,
                CollectionStorageMode::ColumnStore,
            ),
        )
        .expect("column collection");
    let relation = midge.row_schema("golden").expect("row schema").relation_id;
    (midge, relation)
}

fn key(relation: u64, tail: &[u8]) -> Vec<u8> {
    let mut key = HEADER.to_vec();
    key.extend_from_slice(&relation.to_be_bytes());
    key.push(0);
    key.extend_from_slice(tail);
    key
}

#[test]
fn should_pin_the_exact_key_and_value_bytes_of_a_column_row() {
    // Arrange
    let (midge, relation) = golden_midge();

    // Act
    midge
        .put_document(
            "golden",
            Some("r1".to_string()),
            serde_json::json!({"title": "ab", "qty": 5}),
        )
        .expect("write row");
    let prefix = midge
        .column_store_prefix_for_diagnostics("golden")
        .expect("prefix");
    let entries = midge
        .raw_scan_prefix(StorageFamily::Data, &prefix)
        .expect("raw scan");

    // Assert
    let expected: Vec<(Vec<u8>, Vec<u8>)> = vec![
        // field 1: text tag, raw UTF-8
        (key(relation, b"f\0\0\0\0\x01\0r1"), vec![0x05, b'a', b'b']),
        // field 2: i64 tag, 8 bytes big-endian
        (
            key(relation, b"f\0\0\0\0\x02\0r1"),
            vec![0x02, 0, 0, 0, 0, 0, 0, 0, 5],
        ),
        // field 3: omitted by the document, stored as explicit SQL NULL
        (key(relation, b"f\0\0\0\0\x03\0r1"), vec![0x00]),
        // live marker: schema version u32 BE, write revision u64 BE
        (
            key(relation, b"r\0r1"),
            vec![0, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0, 1],
        ),
    ];
    assert_eq!(entries, expected);
}

#[test]
fn should_pin_the_deleted_marker_and_the_revision_that_survives_a_delete() {
    // Arrange
    let (midge, relation) = golden_midge();
    midge
        .put_document(
            "golden",
            Some("r1".to_string()),
            serde_json::json!({"title": "ab", "qty": 5}),
        )
        .expect("write row");

    // Act
    midge.delete_document("golden", "r1").expect("delete row");
    let after_delete = midge
        .raw_scan_prefix(
            StorageFamily::Data,
            &midge
                .column_store_prefix_for_diagnostics("golden")
                .expect("prefix"),
        )
        .expect("raw scan");
    midge
        .put_document(
            "golden",
            Some("r1".to_string()),
            serde_json::json!({"title": "cd", "qty": 6}),
        )
        .expect("re-insert row");
    let marker = midge
        .raw_get(StorageFamily::Data, &key(relation, b"r\0r1"))
        .expect("marker read")
        .expect("marker");

    // Assert
    assert_eq!(
        after_delete,
        vec![(key(relation, b"x\0r1"), vec![0, 0, 0, 0, 0, 0, 0, 2])]
    );
    assert_eq!(marker, vec![0, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0, 3]);
}

#[test]
fn should_round_trip_markers_through_their_big_endian_layout() {
    // Arrange
    let marker = encode_marker(0x0102_0304, 0x0a0b_0c0d_0e0f_1011);

    // Act
    let decoded = decode_marker_parts("golden", "r1", &marker);

    // Assert
    assert_eq!(
        marker,
        vec![1, 2, 3, 4, 0x0a, 0x0b, 0x0c, 0x0d, 0x0e, 0x0f, 0x10, 0x11]
    );
    assert_eq!(
        decoded.expect("decodes"),
        (0x0102_0304, 0x0a0b_0c0d_0e0f_1011)
    );
    assert_eq!(encode_deleted_marker(2), vec![0, 0, 0, 0, 0, 0, 0, 2]);
}
