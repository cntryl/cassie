use std::time::Instant;

use super::super::codecs::{
    value_to_binary, value_to_binary_with_typmod_and_vector, value_to_text,
};
use super::{cell_bytes, QueryResultFrames};
use crate::config::CassieRuntimeLimits;
use crate::executor::{ColumnMeta, QueryResult};
use crate::runtime::QueryExecutionControls;
use crate::types::{Value, Vector};

fn cells() -> Vec<(i64, Value)> {
    vec![
        (16, Value::Bool(false)),
        (21, Value::Int64(i64::from(i16::MIN))),
        (23, Value::Int64(i64::from(i32::MIN))),
        (20, Value::Int64(i64::MIN)),
        (20, Value::Float64(42.0)),
        (701, Value::Float64(-f64::from_bits(1))),
        (701, Value::Float64(f64::MAX)),
        (25, Value::String("\"\\\nλ".repeat(512))),
        (17, Value::String("\\x00ff7f".to_owned())),
        (
            2950,
            Value::String("123e4567-e89b-12d3-a456-426614174000".to_owned()),
        ),
        (1082, Value::String("2026-10-11".to_owned())),
        (1083, Value::String("23:59:59.999999".to_owned())),
        (1114, Value::String("2026-10-11T23:59:59.999999".to_owned())),
        (
            114,
            Value::Json(serde_json::json!({"escaped": "\"\\\nλ".repeat(128)})),
        ),
        (
            1_007,
            Value::Json(serde_json::json!([i32::MIN, null, i32::MAX])),
        ),
        (
            1_009,
            Value::Json(serde_json::json!(["\"\\\nλ".repeat(128), null, {"nested": [1, true]}])),
        ),
        (
            199,
            Value::Json(serde_json::json!([[1, 2], {"nested": true}, "\"\\\nλ", null])),
        ),
        (1_007, Value::Json(serde_json::json!([]))),
        (
            16_400,
            Value::Vector(Vector::new(vec![-f32::from_bits(1), f32::MAX, 0.0])),
        ),
        (25, Value::Null),
    ]
}

fn result(controls: &QueryExecutionControls, cells: &[(i64, Value)]) -> QueryResult {
    let mut builder = QueryResult::try_builder(controls).expect("result builder");
    for (index, (oid, _)) in cells.iter().enumerate() {
        let mut column = ColumnMeta {
            type_oid: *oid,
            ..ColumnMeta::text(format!("column_{index}"))
        };
        if matches!(cells[index].1, Value::Vector(_)) {
            column.data_type = "vector(3)".to_owned();
        }
        builder.try_push_column(&column).expect("column");
    }
    builder
        .try_push_values(cells.iter().map(|(_, value)| value))
        .expect("row");
    builder.try_set_command("SELECT").expect("command");
    builder.finish().expect("result")
}

#[test]
fn should_bound_supported_cell_encodings_before_allocation() {
    // Arrange
    let cells = cells();

    // Act
    let observations: Vec<_> = cells
        .iter()
        .flat_map(|(oid, value)| {
            [0, 1].map(|format| {
                let is_vector = matches!(value, Value::Vector(_));
                let bound =
                    cell_bytes(value, *oid, -1, format, false, is_vector).expect("admission bound");
                let length = if matches!(value, Value::Null) {
                    0
                } else if format == 0 {
                    value_to_text(value, *oid).len()
                } else {
                    value_to_binary_with_typmod_and_vector(value, *oid, -1, is_vector)
                        .expect("supported binary carrier")
                        .len()
                };
                (*oid, format, length, bound.encoded)
            })
        })
        .collect();

    // Assert
    assert_eq!(observations.len(), 40);
    for (oid, format, actual, admitted) in observations {
        assert!(
            actual <= admitted,
            "OID {oid}, format {format}: encoded {actual}, admitted {admitted}"
        );
    }
}

#[test]
fn should_encode_mixed_result_frames_without_growing_the_admitted_frame_buffer() {
    // Arrange
    let controls =
        QueryExecutionControls::from_limits(&CassieRuntimeLimits::default(), Instant::now());
    let cells = cells();
    let result = result(&controls, &cells);
    let retained = controls.current_query_memory_bytes();

    // Act
    let observations: Vec<_> = [0, 1]
        .into_iter()
        .map(|format| {
            let mut frames = QueryResultFrames::try_new(&result, result.rows(), &[format])
                .expect("frame admission");
            let capacity = frames.bytes.capacity();
            let scratch_admitted = controls.current_query_memory_bytes() > retained;
            frames
                .description(result.columns(), &[format])
                .expect("description");
            let description = (frames.bytes()[0], frames.bytes.capacity());
            frames
                .row(0, &result.rows()[0], result.columns(), &[format])
                .expect("row");
            let row = (frames.bytes()[0], frames.bytes.capacity());
            frames.completion(false).expect("completion");
            let completion = (frames.bytes()[0], frames.bytes.capacity());
            (scratch_admitted, capacity, description, row, completion)
        })
        .collect();
    let after_frames = controls.current_query_memory_bytes();
    drop(result);

    // Assert
    for (scratch_admitted, capacity, description, row, completion) in observations {
        assert!(scratch_admitted);
        assert_eq!(description, (b'T', capacity));
        assert_eq!(row, (b'D', capacity));
        assert_eq!(completion, (b'C', capacity));
    }
    assert_eq!(after_frames, retained);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_admit_expanded_jsonb_encoder_buffers_given_short_exponents_when_encoding_results() {
    // Arrange
    let value = Value::String("{\"n\":1e1000}".to_owned());
    // Act
    let bound = cell_bytes(&value, 3802, -1, 1, false, false).expect("JSONB admission");
    let encoded = value_to_binary(&value, 3802).expect("JSONB encoding");
    // Assert
    assert!(encoded.len() <= bound.encoded);
    // Canonical construction holds formatted children and joined output together;
    // binary framing adds the versioned output while canonical text is still live.
    assert!(bound.scratch >= encoded.capacity() * 3);
}
#[test]
fn should_preserve_effect_counts_without_changing_result_page_counts() {
    // Arrange
    let cases = [
        ("SELECT 7", false, 0, 0, "SELECT 7"),
        ("SELECT", false, 0, 0, "SELECT 0"),
        ("SELECT 7", true, 3, 1, "SELECT 1"),
        ("SELECT", true, 0, 0, "SELECT 0"),
        ("SELECT 7", false, 3, 1, "SELECT 1"),
        ("CREATE TABLE", false, 0, 0, "CREATE TABLE"),
    ];
    for (command, schema, stored_rows, emitted_rows, expected) in cases {
        let controls =
            QueryExecutionControls::from_limits(&CassieRuntimeLimits::default(), Instant::now());
        let mut builder = QueryResult::try_builder(&controls).expect("result owner");
        if schema {
            builder
                .try_push_column(&ColumnMeta::text("value"))
                .expect("output schema");
        }
        for _ in 0..stored_rows {
            let values = if schema {
                vec![Value::String("v".into())]
            } else {
                vec![]
            };
            builder.try_push_row(&values).expect("result row");
        }
        builder.try_set_command(command).expect("operator command");
        let result = builder.finish().expect("managed result");

        // Act
        let mut frames = QueryResultFrames::try_new(&result, &result.rows()[..emitted_rows], &[])
            .expect("frame admission");
        frames.completion(false).expect("command completion");

        // Assert
        assert_eq!(frames.bytes()[0], b'C');
        let payload = &frames.bytes()[5..];
        assert_eq!(payload.last(), Some(&0));
        assert_eq!(&payload[..payload.len() - 1], expected.as_bytes());
    }
}

#[test]
fn should_keep_native_array_encoder_frames_within_their_admitted_capacity() {
    // Arrange
    let controls =
        QueryExecutionControls::from_limits(&CassieRuntimeLimits::default(), Instant::now());
    let numeric = crate::types::typed::TypedValue::Numeric(
        crate::types::extended::numeric::Numeric::finite(num_bigint::BigInt::from(1), 4_096),
    );
    let array =
        crate::types::SqlArray::from_nested_values(vec![Value::Typed(numeric), Value::Null])
            .expect("native numeric array");
    let cells = vec![(1_231, Value::Array(array))];
    let result = result(&controls, &cells);

    // Act
    let observations = [0, 1].map(|format| {
        let mut frames = QueryResultFrames::try_new(&result, result.rows(), &[format])
            .expect("admitted native array frames");
        let capacity = frames.bytes.capacity();
        frames
            .row(0, &result.rows()[0], result.columns(), &[format])
            .expect("native array encoder");
        (capacity, frames.bytes.capacity())
    });
    drop(result);

    // Assert
    for (admitted, actual) in observations {
        assert_eq!(actual, admitted);
    }
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_keep_expanding_native_array_encodings_within_admitted_frame_capacity() {
    // Arrange
    let controls =
        QueryExecutionControls::from_limits(&CassieRuntimeLimits::default(), Instant::now());
    let bit = crate::types::typed::TypedValue::VarBit(
        crate::types::extended::bit_string::parse_text(&"10".repeat(2_048)).expect("native bits"),
    );
    let bit_array =
        crate::types::SqlArray::from_elements(vec![Value::Typed(bit)]).expect("bit array");
    let dimensions = vec![
        crate::types::ArrayDimension {
            length: 1,
            lower_bound: -2
        };
        6
    ];
    let nested_json = serde_json::from_str(r#"{"escaped":"\"\\\nλ","nested":[1e256,{"a":true}]}"#)
        .expect("exact JSONB fixture");
    let json_array = crate::types::SqlArray::new(dimensions, vec![Value::Json(nested_json)])
        .expect("rank six JSONB array");
    let cells = vec![
        (1563, Value::Array(bit_array)),
        (3807, Value::Array(json_array)),
    ];
    let result = result(&controls, &cells);

    // Act
    let observations = [0, 1].map(|format| {
        let mut frames = QueryResultFrames::try_new(&result, result.rows(), &[format])
            .expect("admitted native frames");
        let capacity = frames.bytes.capacity();
        frames
            .row(0, &result.rows()[0], result.columns(), &[format])
            .expect("native array encoding");
        (capacity, frames.bytes.capacity())
    });
    drop(result);

    // Assert
    for (admitted, actual) in observations {
        assert_eq!(actual, admitted);
    }
    assert_eq!(controls.current_query_memory_bytes(), 0);
}
