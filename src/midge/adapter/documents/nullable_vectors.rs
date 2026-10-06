//! Nullable vector shape rules at the direct Midge boundary.

use super::Midge;
use crate::app::CassieError;
use crate::types::{DataType, FieldSchema, Schema};

fn vector_schema(nullable: bool) -> Schema {
    Schema {
        fields: vec![FieldSchema {
            name: "v".to_string(),
            data_type: DataType::Vector(2),
            nullable,
        }],
    }
}

#[test]
fn should_allow_nullable_vector_sql_null_documents() {
    // Arrange
    let schema = vector_schema(true);
    let payload = serde_json::json!({"v": null});

    // Act
    let result = Midge::validate_document(&schema, &payload);

    // Assert
    result.expect("nullable vector retains SQL NULL");
}

#[test]
fn should_reject_nonnullable_vector_sql_null_documents() {
    // Arrange
    let schema = vector_schema(false);
    let payload = serde_json::json!({"v": null});

    // Act
    let result = Midge::validate_document(&schema, &payload);

    // Assert
    assert!(matches!(result, Err(CassieError::InvalidVector(_))));
}

#[test]
fn should_preserve_nonnull_vector_document_shape_checks() {
    // Arrange
    let values = [
        serde_json::json!({"v": [1.5, -2.0]}),
        serde_json::json!({"v": [1.5]}),
        serde_json::json!({"v": "[1.5,-2.0]"}),
    ];

    // Act
    let results = [false, true].map(|nullable| {
        values
            .each_ref()
            .map(|payload| Midge::validate_document(&vector_schema(nullable), payload))
    });

    // Assert
    for [valid, wrong_width, wrong_shape] in results {
        valid.expect("valid non-NULL vector");
        assert!(matches!(wrong_width, Err(CassieError::InvalidVector(_))));
        assert!(matches!(wrong_shape, Err(CassieError::InvalidVector(_))));
    }
}
