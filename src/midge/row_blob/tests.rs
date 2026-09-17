use super::*;

#[test]
fn should_decode_sparse_rows_without_field_names() {
    // Arrange
    let schema = RowSchema::from_schema(&Schema {
        fields: vec![
            FieldSchema {
                name: "title".to_string(),
                data_type: DataType::Text,
                nullable: true,
            },
            FieldSchema {
                name: "score".to_string(),
                data_type: DataType::Int,
                nullable: true,
            },
        ],
    });

    // Act
    let encoded = encode_row(&schema, &serde_json::json!({"score": 42})).unwrap();
    let decoded = decode_row(&schema, &encoded).unwrap();

    // Assert
    assert_eq!(decoded, serde_json::json!({"score": 42}));
    let raw = String::from_utf8_lossy(&encoded);
    assert!(!raw.contains("score"));
}

#[test]
fn should_roundtrip_binary_temporal_uuid_array_fields() {
    // Arrange
    let schema = RowSchema::from_schema(&Schema {
        fields: vec![
            FieldSchema {
                name: "id".to_string(),
                data_type: DataType::Uuid,
                nullable: true,
            },
            FieldSchema {
                name: "created_on".to_string(),
                data_type: DataType::Date,
                nullable: true,
            },
            FieldSchema {
                name: "created_at".to_string(),
                data_type: DataType::Timestamp,
                nullable: true,
            },
            FieldSchema {
                name: "updated_at".to_string(),
                data_type: DataType::Time,
                nullable: true,
            },
            FieldSchema {
                name: "ints".to_string(),
                data_type: DataType::Array(Box::new(DataType::Int)),
                nullable: true,
            },
        ],
    });
    let payload = serde_json::json!({
        "id": "550e8400-e29b-41d4-a716-446655440000",
        "created_on": "2026-06-18",
        "created_at": "2026-06-18T12:34:56Z",
        "updated_at": "12:34:56",
        "ints": [1, 2, 3],
    });

    // Act
    let encoded = encode_row(&schema, &payload).unwrap();
    let decoded = decode_row(&schema, &encoded).unwrap();

    // Assert
    assert_eq!(
        decoded,
        serde_json::json!({
            "id": "550e8400-e29b-41d4-a716-446655440000",
            "created_on": "2026-06-18",
            "created_at": "2026-06-18T12:34:56Z",
            "updated_at": "12:34:56",
            "ints": [1, 2, 3],
        })
    );
    assert_eq!(&encoded[0..4], b"CRB2");
}

#[test]
fn should_roundtrip_crb2_text_arrays_with_nulls_in_every_position() {
    // Arrange
    let schema = RowSchema::from_schema(&Schema {
        fields: vec![
            FieldSchema {
                name: "first".to_string(),
                data_type: DataType::Array(Box::new(DataType::Text)),
                nullable: true,
            },
            FieldSchema {
                name: "middle".to_string(),
                data_type: DataType::Array(Box::new(DataType::Text)),
                nullable: true,
            },
            FieldSchema {
                name: "last".to_string(),
                data_type: DataType::Array(Box::new(DataType::Text)),
                nullable: true,
            },
            FieldSchema {
                name: "consecutive".to_string(),
                data_type: DataType::Array(Box::new(DataType::Text)),
                nullable: true,
            },
        ],
    });
    let payload = serde_json::json!({
        "first": [null, "alpha", "bravo"],
        "middle": ["alpha", null, "bravo"],
        "last": ["alpha", "bravo", null],
        "consecutive": ["alpha", null, null, "bravo"],
    });

    // Act
    let encoded = encode_row(&schema, &payload).unwrap();
    let decoded = decode_row(&schema, &encoded).unwrap();

    // Assert
    assert_eq!(decoded, payload);
}

#[test]
fn should_roundtrip_crb2_fixed_width_arrays_with_null_elements() {
    // Arrange
    let schema = RowSchema::from_schema(&Schema {
        fields: vec![
            FieldSchema {
                name: "ints".to_string(),
                data_type: DataType::Array(Box::new(DataType::Int)),
                nullable: true,
            },
            FieldSchema {
                name: "bools".to_string(),
                data_type: DataType::Array(Box::new(DataType::Boolean)),
                nullable: true,
            },
        ],
    });
    let payload = serde_json::json!({
        "ints": [1, null, 2],
        "bools": [true, null, false],
    });

    // Act
    let encoded = encode_row(&schema, &payload).unwrap();
    let decoded = decode_row(&schema, &encoded).unwrap();

    // Assert
    assert_eq!(decoded, payload);
}

#[test]
fn should_roundtrip_crb2_nested_arrays_with_null_elements() {
    // Arrange
    let schema = RowSchema::from_schema(&Schema {
        fields: vec![FieldSchema {
            name: "nested".to_string(),
            data_type: DataType::Array(Box::new(DataType::Array(Box::new(DataType::Text)))),
            nullable: true,
        }],
    });
    let payload = serde_json::json!({
        "nested": [["alpha", null, "bravo"], null, [null, "charlie"]],
    });

    // Act
    let encoded = encode_row(&schema, &payload).unwrap();
    let decoded = decode_row(&schema, &encoded).unwrap();

    // Assert
    assert_eq!(decoded, payload);
}

#[test]
fn should_reject_crb2_array_null_with_nonzero_payload_length() {
    // Arrange
    let encoded_array = [2, TYPE_NULL, 1, 0xff, TYPE_STRING, 1, b'a'];
    let mut cursor = Cursor::new(&encoded_array);

    // Act
    let result = decode_value(TYPE_ARRAY, &mut cursor);

    // Assert
    assert!(result.is_err());
}

#[test]
fn should_not_visit_unrequested_field_payloads() {
    // Arrange
    let schema = RowSchema::from_schema(&Schema {
        fields: vec![
            FieldSchema {
                name: "title".to_string(),
                data_type: DataType::Text,
                nullable: true,
            },
            FieldSchema {
                name: "body".to_string(),
                data_type: DataType::Text,
                nullable: true,
            },
        ],
    });
    let projection = ["title".to_string()].into_iter().collect::<HashSet<_>>();
    let mut encoded = encode_row(
        &schema,
        &serde_json::json!({"title": "alpha", "body": "payload"}),
    )
    .unwrap();
    *encoded.last_mut().expect("body payload byte") = 0xff;

    // Act
    let projected = decode_projected_row(&schema, &encoded, &projection).unwrap();
    let full = decode_row(&schema, &encoded);

    // Assert
    assert_eq!(projected, serde_json::json!({"title": "alpha"}));
    assert!(full.is_err());
}

#[test]
fn should_reject_invalid_uuid_values_during_row_blob_encoding() {
    // Arrange
    let schema = RowSchema::from_schema(&Schema {
        fields: vec![FieldSchema {
            name: "id".to_string(),
            data_type: DataType::Uuid,
            nullable: true,
        }],
    });
    let payload = serde_json::json!({"id": "not-a-uuid"});

    // Act
    let result = encode_row(&schema, &payload);

    // Assert
    assert!(result.is_err());
}

#[test]
fn should_retain_retired_field_ids() {
    // Arrange
    let mut schema = RowSchema::from_schema(&Schema {
        fields: vec![FieldSchema {
            name: "title".to_string(),
            data_type: DataType::Text,
            nullable: true,
        }],
    });

    // Act
    assert!(schema.retire_field("title"));
    schema
        .add_field(FieldSchema {
            name: "status".to_string(),
            data_type: DataType::Text,
            nullable: true,
        })
        .unwrap();

    // Assert
    assert_eq!(schema.fields[0].field_id, 1);
    assert!(schema.fields[0].retired);
    assert_eq!(schema.fields[1].field_id, 2);
    assert!(!schema.fields[1].retired);
}

#[test]
fn should_decode_projected_row_with_case_insensitive_field_names() {
    // Arrange
    let schema = RowSchema::from_schema(&Schema {
        fields: vec![
            FieldSchema {
                name: "Title".to_string(),
                data_type: DataType::Text,
                nullable: true,
            },
            FieldSchema {
                name: "Score".to_string(),
                data_type: DataType::Int,
                nullable: true,
            },
        ],
    });
    let projection = ["title".to_string()].into_iter().collect::<HashSet<_>>();
    let encoded = encode_row(
        &schema,
        &serde_json::json!({
            "Title": "alpha",
            "Score": 42
        }),
    )
    .unwrap();

    // Act
    let decoded = decode_projected_row(&schema, &encoded, &projection).unwrap();

    // Assert
    assert_eq!(decoded, serde_json::json!({"Title": "alpha"}));
}

#[test]
fn should_decode_projected_row_when_filter_matches() {
    // Arrange
    let schema = RowSchema::from_schema(&Schema {
        fields: vec![
            FieldSchema {
                name: "title".to_string(),
                data_type: DataType::Text,
                nullable: true,
            },
            FieldSchema {
                name: "body".to_string(),
                data_type: DataType::Text,
                nullable: true,
            },
        ],
    });
    let projection = ["title".to_string()].into_iter().collect::<HashSet<_>>();
    let encoded = encode_row(
        &schema,
        &serde_json::json!({
            "title": "alpha",
            "body": "large payload"
        }),
    )
    .unwrap();

    // Act
    let decoded = decode_projected_row_matching(
        &schema,
        &encoded,
        &projection,
        "title",
        &serde_json::json!("alpha"),
    )
    .unwrap();

    // Assert
    assert_eq!(decoded, Some(serde_json::json!({"title": "alpha"})));
}

#[test]
fn should_skip_projected_row_when_filter_does_not_match() {
    // Arrange
    let schema = RowSchema::from_schema(&Schema {
        fields: vec![
            FieldSchema {
                name: "title".to_string(),
                data_type: DataType::Text,
                nullable: true,
            },
            FieldSchema {
                name: "body".to_string(),
                data_type: DataType::Text,
                nullable: true,
            },
        ],
    });
    let projection = ["title".to_string()].into_iter().collect::<HashSet<_>>();
    let encoded = encode_row(
        &schema,
        &serde_json::json!({
            "title": "beta",
            "body": "large payload"
        }),
    )
    .unwrap();

    // Act
    let decoded = decode_projected_row_matching(
        &schema,
        &encoded,
        &projection,
        "title",
        &serde_json::json!("alpha"),
    )
    .unwrap();

    // Assert
    assert_eq!(decoded, None);
}

#[test]
fn should_roundtrip_extended_scalar_types() {
    // Arrange
    let schema = RowSchema::from_schema(&Schema {
        fields: vec![
            FieldSchema {
                name: "tiny".to_string(),
                data_type: DataType::SmallInt,
                nullable: true,
            },
            FieldSchema {
                name: "score".to_string(),
                data_type: DataType::Int,
                nullable: true,
            },
            FieldSchema {
                name: "balance".to_string(),
                data_type: DataType::BigInt,
                nullable: true,
            },
            FieldSchema {
                name: "code".to_string(),
                data_type: DataType::Char { length: Some(4) },
                nullable: true,
            },
            FieldSchema {
                name: "label".to_string(),
                data_type: DataType::Varchar { length: Some(8) },
                nullable: true,
            },
            FieldSchema {
                name: "payload".to_string(),
                data_type: DataType::Bytea,
                nullable: true,
            },
        ],
    });
    let payload = serde_json::json!({
        "tiny": 7,
        "score": 42,
        "balance": 9_223_372_036_854_775_807_i64,
        "code": "ab12",
        "label": "alpha",
        "payload": "\\x01020aff"
    });

    // Act
    let encoded = encode_row(&schema, &payload).unwrap();
    let mut cursor = Cursor::new(&encoded);
    cursor.expect_bytes(MAGIC).unwrap();
    let version = cursor.read_u8().unwrap();
    assert_eq!(version, FORMAT_VERSION);
    let _schema_version = cursor.read_u32().unwrap();
    let _flags = cursor.read_u8().unwrap();
    let field_count = cursor.read_varint().unwrap();
    for index in 0..field_count {
        let field_id = cursor.read_varint().unwrap();
        let tag = cursor.read_u8().unwrap();
        let value = decode_value(tag, &mut cursor);
        assert!(
            value.is_ok(),
            "field {index} id={field_id} tag={tag}: {value:?}"
        );
    }
    let decoded = decode_row(&schema, &encoded).unwrap();

    // Assert
    assert_eq!(
        decoded,
        serde_json::json!({
            "tiny": 7,
            "score": 42,
            "balance": 9_223_372_036_854_775_807_i64,
            "code": "ab12",
            "label": "alpha",
            "payload": "\\x01020aff"
        })
    );
}

#[test]
fn should_reject_invalid_bytea_payloads_for_row_blob_encoding() {
    // Arrange
    let schema = RowSchema::from_schema(&Schema {
        fields: vec![FieldSchema {
            name: "payload".to_string(),
            data_type: DataType::Bytea,
            nullable: true,
        }],
    });
    let payload = serde_json::json!({"payload": "not-a-bytea"});

    // Act
    let result = encode_row(&schema, &payload);

    // Assert
    assert!(result.is_err());
}

#[test]
fn should_reject_row_encoding_with_trailing_bytes() {
    // Arrange
    let schema = RowSchema::from_schema(&Schema {
        fields: vec![FieldSchema {
            name: "score".to_string(),
            data_type: DataType::Int,
            nullable: true,
        }],
    });
    let mut encoded = encode_row(&schema, &serde_json::json!({"score": 42})).unwrap();
    encoded.push(0xff);

    // Act
    let result = decode_row(&schema, &encoded);

    // Assert
    assert!(result.is_err());
}

#[test]
fn should_reject_row_encoding_with_truncated_values() {
    // Arrange
    let schema = RowSchema::from_schema(&Schema {
        fields: vec![FieldSchema {
            name: "score".to_string(),
            data_type: DataType::Int,
            nullable: true,
        }],
    });
    let mut encoded = encode_row(&schema, &serde_json::json!({"score": 42})).unwrap();
    encoded.pop();

    // Act
    let result = decode_row(&schema, &encoded);

    // Assert
    assert!(result.is_err());
}

/// Directory entry layout inside an encoded blob: MAGIC, format version,
/// schema version, max field id, bitmap length, presence, nulls, field
/// count, then `field_count` entries of (`field_id`, `type_tag`, `offset`,
/// `len`).
fn directory_entry_offset(bitmap_len: usize, entry: usize) -> usize {
    MAGIC.len() + 1 + 4 + 4 + 4 + bitmap_len + bitmap_len + 4 + entry * (4 + 1 + 4 + 4)
}

fn two_text_field_schema() -> RowSchema {
    RowSchema::from_schema(&Schema {
        fields: vec![
            FieldSchema {
                name: "left".to_string(),
                data_type: DataType::Text,
                nullable: true,
            },
            FieldSchema {
                name: "right".to_string(),
                data_type: DataType::Text,
                nullable: true,
            },
        ],
    })
}

#[test]
fn should_reject_overlapping_row_blob_directory_entries() {
    // Arrange
    let schema = two_text_field_schema();
    let mut encoded = encode_row(
        &schema,
        &serde_json::json!({"left": "aaaa", "right": "bbbb"}),
    )
    .unwrap();
    // Point the second field at the first field's bytes. Both entries stay
    // in bounds and the maximum end offset still equals the payload length,
    // so only an overlap check rejects this.
    let bitmap_len = 1;
    let second = directory_entry_offset(bitmap_len, 1);
    let offset_at = second + 4 + 1;
    encoded[offset_at..offset_at + 4].copy_from_slice(&0_u32.to_be_bytes());

    // Act
    let decoded = decode_row(&schema, &encoded);

    // Assert
    let error = decoded.expect_err("overlapping directory entries must be rejected");
    assert!(
        error.to_string().contains("partition the payload"),
        "unexpected error: {error}"
    );
}

#[test]
fn should_reject_a_row_blob_type_tag_that_contradicts_the_schema() {
    // Arrange
    let schema = RowSchema::from_schema(&Schema {
        fields: vec![FieldSchema {
            name: "score".to_string(),
            data_type: DataType::Float,
            nullable: true,
        }],
    });
    let mut encoded = encode_row(&schema, &serde_json::json!({"score": 1.5})).unwrap();
    // TYPE_F64 (0x04) and TYPE_STRING (0x05) are one bit apart, and eight
    // big-endian float bytes are also a valid eight-byte string payload, so
    // this decodes as a plausible value unless the tag is checked.
    let tag_at = directory_entry_offset(1, 0) + 4;
    assert_eq!(encoded[tag_at], TYPE_F64);
    encoded[tag_at] = TYPE_STRING;

    // Act
    let decoded = decode_row(&schema, &encoded);

    // Assert
    let error = decoded.expect_err("a tag contradicting the schema must be rejected");
    assert!(
        error.to_string().contains("type tag"),
        "unexpected error: {error}"
    );
}

#[test]
fn should_reject_a_row_blob_field_payload_that_decodes_short() {
    // Arrange
    let schema = RowSchema::from_schema(&Schema {
        fields: vec![FieldSchema {
            name: "flag".to_string(),
            data_type: DataType::Boolean,
            nullable: true,
        }],
    });
    let mut encoded = encode_row(&schema, &serde_json::json!({"flag": true})).unwrap();
    // A boolean consumes exactly one byte. Widening its declared length
    // leaves trailing bytes the decoder would otherwise ignore.
    let len_at = directory_entry_offset(1, 0) + 4 + 1 + 4;
    encoded[len_at..len_at + 4].copy_from_slice(&2_u32.to_be_bytes());
    encoded.push(0xFF);

    // Act
    let decoded = decode_row(&schema, &encoded);

    // Assert
    let error = decoded.expect_err("a partially consumed field payload must be rejected");
    assert!(
        error.to_string().contains("unconsumed bytes"),
        "unexpected error: {error}"
    );
}
