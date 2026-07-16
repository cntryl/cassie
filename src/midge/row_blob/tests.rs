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
