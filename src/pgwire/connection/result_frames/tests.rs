use std::time::Instant;

use super::super::codecs::{value_to_binary, value_to_text};
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
            100_003,
            Value::Vector(Vector::new(vec![-f32::from_bits(1), f32::MAX, 0.0])),
        ),
        (25, Value::Null),
    ]
}

fn result(controls: &QueryExecutionControls, cells: &[(i64, Value)]) -> QueryResult {
    let mut builder = QueryResult::try_builder(controls).expect("result builder");
    for (index, (oid, _)) in cells.iter().enumerate() {
        let column = ColumnMeta {
            type_oid: *oid,
            ..ColumnMeta::text(format!("column_{index}"))
        };
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
                let bound = cell_bytes(value, *oid, format, false).expect("admission bound");
                let length = if matches!(value, Value::Null) {
                    0
                } else if format == 0 {
                    value_to_text(value, *oid).len()
                } else {
                    value_to_binary(value, *oid)
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
