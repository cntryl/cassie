use super::*;
use crate::midge::row_blob::{decode_row, encode_row};
use crate::types::{FieldSchema, Schema};

fn schema() -> RowSchema {
    RowSchema::from_schema(&Schema {
        fields: vec![
            FieldSchema {
                name: "id".into(),
                data_type: DataType::Text,
                nullable: true,
            },
            FieldSchema {
                name: "embedding".into(),
                data_type: DataType::Vector(Some(5)),
                nullable: true,
            },
            FieldSchema {
                name: "tenant".into(),
                data_type: DataType::Text,
                nullable: true,
            },
        ],
    })
}

fn row_with(embedding: &serde_json::Value) -> Vec<u8> {
    encode_row(
        &schema(),
        &serde_json::json!({"id": "a", "embedding": embedding, "tenant": "t1"}),
    )
    .expect("encode row")
}

#[test]
fn should_decode_the_same_components_as_the_generic_row_decoder() {
    // Arrange
    let schema = schema();
    let components = [0.25_f32, -1.5, 3.0e-7, 1.0e20, -0.0];
    let row = row_with(&serde_json::json!(components));
    let column = VectorColumn::resolve(&schema, "embedding").expect("vector column");
    let generic = decode_row(&schema, &row).expect("generic decode");

    // Act
    let parts = split_vector_row(&schema, &column, &row, false).expect("split");
    let direct = parts.vector.expect("vector present").to_f32().expect("f32");

    // Assert
    let expected = generic["embedding"]
        .as_array()
        .expect("array")
        .iter()
        .map(|value| {
            crate::vector::f64_to_finite_f32(value.as_f64().expect("number")).expect("finite")
        })
        .collect::<Vec<_>>();
    assert_eq!(direct, expected);
}

#[test]
fn should_report_four_decoded_bytes_per_component() {
    // Arrange
    let schema = schema();
    let row = row_with(&serde_json::json!([1.0, 2.0, 3.0, 4.0, 5.0]));
    let column = VectorColumn::resolve(&schema, "embedding").expect("vector column");

    // Act
    let parts = split_vector_row(&schema, &column, &row, false).expect("split");
    let packed = parts.vector.expect("vector present");

    // Assert
    assert_eq!((packed.dimensions(), packed.decoded_bytes()), (5, 20));
}

#[test]
fn should_report_no_vector_for_a_sql_null_value() {
    // Arrange
    let schema = schema();
    let row = row_with(&serde_json::Value::Null);
    let column = VectorColumn::resolve(&schema, "embedding").expect("vector column");

    // Act
    let parts = split_vector_row(&schema, &column, &row, false).expect("split");

    // Assert
    assert!(parts.vector.is_none());
}

#[test]
fn should_decode_the_other_fields_without_the_vector() {
    // Arrange
    let schema = schema();
    let row = row_with(&serde_json::json!([1.0, 2.0, 3.0, 4.0, 5.0]));
    let column = VectorColumn::resolve(&schema, "embedding").expect("vector column");

    // Act
    let parts = split_vector_row(&schema, &column, &row, true).expect("split");

    // Assert
    assert_eq!(
        parts.payload,
        Some(serde_json::json!({"id": "a", "tenant": "t1"}))
    );
}

#[test]
fn should_not_resolve_a_field_that_is_not_a_vector_column() {
    // Arrange
    let schema = schema();

    // Act
    let text = VectorColumn::resolve(&schema, "tenant");
    let missing = VectorColumn::resolve(&schema, "nope");

    // Assert
    assert!(text.is_none() && missing.is_none());
}

#[test]
fn should_reject_a_non_finite_component_as_the_generic_decoder_does() {
    // Arrange
    let schema = schema();
    let mut row = row_with(&serde_json::json!([1.0, 2.0, 3.0, 4.0, 5.0]));
    let last = 5.0_f32.to_be_bytes();
    let at = row
        .windows(4)
        .position(|window| window == last)
        .expect("last component bytes");
    row[at..at + 4].copy_from_slice(&f32::NAN.to_be_bytes());
    let column = VectorColumn::resolve(&schema, "embedding").expect("vector column");
    let generic = decode_row(&schema, &row).expect_err("generic decode rejects NaN");

    // Act
    let direct = split_vector_row(&schema, &column, &row, false)
        .expect("split")
        .vector
        .expect("vector present")
        .to_f32()
        .expect_err("direct decode rejects NaN");

    // Assert
    assert_eq!(direct.to_string(), generic.to_string());
}
