use cassie_core::types::{DataType, FieldSchema, Schema};

use super::{RowBlobEncodeError, RowSchema, RowBlobValueError, TYPE_JSON, decode_row, encode_row, encode_row_with};
use crate::StorageCodecError;

#[test]
fn should_decode_json_payload_through_supplied_decoder() {
    // Arrange
    let schema = RowSchema::from_schema(&Schema {
        fields: vec![FieldSchema {
            name: "document".to_owned(),
            data_type: DataType::Jsonb,
            nullable: true,
        }],
    });
    let raw = br#"{"$cassie_array_v1":{"reserved":true}}"#;
    let row = encode_row_with(&schema, &serde_json::json!({"document": {}}), |_, _| {
        Ok::<_, StorageCodecError>((TYPE_JSON, raw.to_vec()))
    })
    .expect("row");
    let expected = serde_json::json!({"parsed_by_engine": true});

    // Act
    let decoded = decode_row(&schema, &row, |bytes| {
        assert_eq!(bytes, raw);
        Ok::<_, StorageCodecError>(expected.clone())
    });

    // Assert
    assert_eq!(decoded.expect("decoded row"), serde_json::json!({
        "document": {"parsed_by_engine": true}
    }));
}

#[test]
fn should_encode_text_row_with_established_crb1_bytes() {
    // Arrange
    let schema = RowSchema::from_schema(&Schema {
        fields: vec![FieldSchema {
            name: "v".to_owned(),
            data_type: DataType::Text,
            nullable: true,
        }],
    });
    let expected = b"CRB1\x01\x00\x00\x00\x01\x00\x00\x00\x01\x00\x00\x00\x01\x01\x00\x00\x00\x01\x00\x00\x00\x01\x05\x00\x00\x00\x00\x00\x00\x00\x01x";

    // Act
    let actual = encode_row(&schema, &serde_json::json!({"v": "x"}));

    // Assert
    assert_eq!(actual.expect("row").as_slice(), expected);
}

#[test]
fn should_preserve_invalid_field_category_from_storage_encoder() {
    // Arrange
    let schema = RowSchema::from_schema(&Schema {
        fields: vec![FieldSchema {
            name: "v".to_owned(),
            data_type: DataType::Text,
            nullable: true,
        }],
    });

    // Act
    let error = encode_row(&schema, &serde_json::json!({"v": true})).expect_err("bad text");

    // Assert
    assert!(matches!(
        error,
        RowBlobEncodeError::FieldValue(RowBlobValueError::InvalidValue(message))
            if message == "text field expects string"
    ));
}
