//! Differential and control probes for exact sort tie text.
use super::*;
use crate::config::CassieRuntimeLimits;
use crate::executor::batch::{row_tie_key, BatchRow};
use crate::runtime::{QueryCancellationHandle, QueryExecutionControls};
use crate::types::{ArrayDimension, SqlArray, Value};
use std::time::Instant;

#[test]
fn should_match_existing_tie_text_for_supported_value_families() {
    // Arrange
    let controls =
        QueryExecutionControls::from_limits(&CassieRuntimeLimits::default(), Instant::now());
    let bits = crate::types::extended::bit_string::BitString::from_bits([true, false, true]);
    let array = SqlArray::new(
        vec![
            ArrayDimension {
                length: 2,
                lower_bound: -1,
            },
            ArrayDimension {
                length: 2,
                lower_bound: 3,
            },
        ],
        vec![
            Value::Null,
            Value::String("a\"\nΩ".into()),
            Value::Json(serde_json::json!({"z":[1,null],"a":"λ"})),
            Value::Typed(crate::types::typed::TypedValue::VarBit(bits)),
        ],
    )
    .expect("SQL array");
    let rows = [
        BatchRow::new(vec![
            ("s".into(), Value::String("line\nΩ|quote\"".into())),
            ("null".into(), Value::Null),
            ("bool".into(), Value::Bool(true)),
        ]),
        BatchRow::new(vec![
            ("nan".into(), Value::Float64(f64::NAN)),
            ("inf".into(), Value::Float64(f64::INFINITY)),
            ("negative".into(), Value::Int64(-1)),
        ]),
        BatchRow::new(vec![
            (
                "json".into(),
                Value::Json(serde_json::json!({"a":"x\nλ","b":[true,null,1]})),
            ),
            ("array".into(), Value::Array(array)),
        ]),
    ];

    // Act
    let results = rows
        .iter()
        .map(|row| {
            let expected = row_tie_key(row);
            let actual = serialize(row, &controls).expect("controlled tie");
            (expected, actual)
        })
        .collect::<Vec<_>>();

    // Assert
    for (expected, actual) in results {
        assert_eq!(actual.text(), expected);
    }
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_preserve_wrapping_prefix_and_nested_json_for_public_edge_shapes() {
    // Arrange
    let controls =
        QueryExecutionControls::from_limits(&CassieRuntimeLimits::default(), Instant::now());
    let valid_max = SqlArray::new(
        vec![ArrayDimension {
            length: 1,
            lower_bound: i32::MAX,
        }],
        vec![Value::Int64(1)],
    )
    .expect("valid max bound");
    let valid_min = SqlArray::new(
        vec![ArrayDimension {
            length: 1,
            lower_bound: i32::MIN,
        }],
        vec![Value::Int64(1)],
    )
    .expect("valid min bound");
    let malformed: Vec<SqlArray> = [
        serde_json::json!({"dimensions":[{"length":2,"lower_bound":1},{"length":0,"lower_bound":1},{"length":-1,"lower_bound":1}],"elements":[]}),
        serde_json::json!({"dimensions":[{"length":0,"lower_bound":i32::MIN}],"elements":[]}),
        serde_json::json!({"dimensions":[{"length":-1,"lower_bound":1}],"elements":[]}),
        serde_json::json!({"dimensions":[{"length":2,"lower_bound":i32::MAX}],"elements":[{"Int64":1},{"Int64":2}]}),
    ].into_iter().map(|value| serde_json::from_value(value).expect("derived array")).collect();
    for array in std::iter::once(valid_max)
        .chain(std::iter::once(valid_min))
        .chain(malformed)
    {
        let prefix = array
            .dimensions()
            .iter()
            .map(|dim| {
                format!(
                    "[{}:{}]",
                    dim.lower_bound,
                    dim.lower_bound.wrapping_add(dim.length).wrapping_sub(1)
                )
            })
            .collect::<String>();
        let expected = format!("{prefix}{}", array.to_nested_json());
        let row = BatchRow::new(vec![("a".into(), Value::Array(array))]);
        // Act
        let actual = serialize(&row, &controls).expect("controlled tie");
        // Assert
        assert_eq!(actual.text(), expected);
        drop(actual);
        assert_eq!(controls.current_query_memory_bytes(), 0);
    }
}

#[test]
fn should_stop_stride_at_zero_before_unreachable_negative_or_overflow() {
    // Arrange
    let controls =
        QueryExecutionControls::from_limits(&CassieRuntimeLimits::default(), Instant::now());
    let zero_then_negative = [
        ArrayDimension {
            length: 0,
            lower_bound: 1,
        },
        ArrayDimension {
            length: -1,
            lower_bound: 1,
        },
    ];
    let huge_then_zero = [
        ArrayDimension {
            length: i32::MAX,
            lower_bound: 1,
        },
        ArrayDimension {
            length: i32::MAX,
            lower_bound: 1,
        },
        ArrayDimension {
            length: i32::MAX,
            lower_bound: 1,
        },
        ArrayDimension {
            length: 0,
            lower_bound: 1,
        },
    ];
    // Act / Assert
    assert_eq!(
        array_stride(&zero_then_negative, &controls).expect("zero first"),
        0
    );
    assert_eq!(
        array_stride(&huge_then_zero, &controls).expect("zero after huge dimensions"),
        0
    );
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_visit_nested_array_stride_dimensions_only_linearly() {
    // Arrange
    let controls =
        QueryExecutionControls::from_limits(&CassieRuntimeLimits::default(), Instant::now());
    let dimensions = vec![
        ArrayDimension {
            length: 1,
            lower_bound: 1
        };
        128
    ];
    let array = SqlArray::new(dimensions, vec![Value::Int64(7)]).expect("deep array");
    let expected = format!("{}{}", "[1:1]".repeat(128), array.to_nested_json());
    let row = BatchRow::new(vec![("a".into(), Value::Array(array))]);
    STRIDE_VISITS.with(|count| count.set(0));
    // Act
    let actual = serialize(&row, &controls).expect("tie");
    let visits = STRIDE_VISITS.with(std::cell::Cell::get);
    // Assert
    assert_eq!(actual.text(), expected);
    assert!(visits <= 2 * 128, "root suffix must be scanned only once");
    drop(actual);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_preserve_empty_and_rank_one_array_ties() {
    // Arrange
    let controls =
        QueryExecutionControls::from_limits(&CassieRuntimeLimits::default(), Instant::now());
    let arrays = [
        SqlArray::empty(),
        SqlArray::from_elements(vec![Value::Int64(1), Value::Int64(2)]).expect("rank one"),
    ];
    for array in arrays {
        let row = BatchRow::new(vec![("a".into(), Value::Array(array))]);
        let expected = row_tie_key(&row);
        // Act
        let actual = serialize(&row, &controls).expect("tie");
        // Assert
        assert_eq!(actual.text(), expected);
        drop(actual);
        assert_eq!(controls.current_query_memory_bytes(), 0);
    }
}

#[test]
fn should_cancel_during_tie_string_copy() {
    // Arrange
    let cancellation = QueryCancellationHandle::new();
    let controls = QueryExecutionControls::with_cancellation(
        &CassieRuntimeLimits::default(),
        Instant::now(),
        cancellation.clone(),
    );
    let row = BatchRow::new(vec![("text".into(), Value::String("x".repeat(8192)))]);
    let mut copied = 0;

    // Act
    let result = serialize_observing(&row, &controls, |bytes| {
        copied = bytes;
        cancellation.cancel();
    });

    // Assert
    assert!(matches!(
        result,
        Err(QueryError::Cassie(crate::app::CassieError::QueryCancelled))
    ));
    assert!(copied > 0 && copied < 8192);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_keep_both_tie_buffers_owned_during_growth_cancellation() {
    // Arrange
    let cancellation = QueryCancellationHandle::new();
    let controls = QueryExecutionControls::with_cancellation(
        &CassieRuntimeLimits::default(),
        Instant::now(),
        cancellation.clone(),
    );
    let input = "x".repeat(8192);
    let mut copied = 0;
    let mut retained = 0;

    // Act
    let result = growth_observing(&input, &controls, |bytes| {
        copied = bytes;
        retained = controls.current_query_memory_bytes();
        cancellation.cancel();
    });

    // Assert
    assert!(matches!(
        result,
        Err(QueryError::Cassie(crate::app::CassieError::QueryCancelled))
    ));
    assert!(copied > 0 && copied < input.len());
    assert!(retained > input.len());
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_release_replacement_stack_when_cancelled_during_a_frame_move() {
    // Arrange
    let cancellation = QueryCancellationHandle::new();
    let controls = QueryExecutionControls::with_cancellation(
        &CassieRuntimeLimits::default(),
        Instant::now(),
        cancellation.clone(),
    );
    let value = Value::Null;
    let mut stack = Stack::new(&controls).expect("stack");
    stack.push(Frame::SqlValue(&value)).expect("first frame");
    stack.push(Frame::SqlValue(&value)).expect("second frame");
    assert_eq!(stack.frames.len(), stack.frames.capacity());
    let original = controls.current_query_memory_bytes();
    let mut moved = 0;

    // Act
    let result = stack.push_observing(Frame::SqlValue(&value), || {
        moved += 1;
        cancellation.cancel();
    });

    // Assert
    assert_eq!(moved, 1);
    assert!(matches!(
        result,
        Err(QueryError::Cassie(crate::app::CassieError::QueryCancelled))
    ));
    assert_eq!(controls.current_query_memory_bytes(), original);
    drop(stack);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_preserve_tie_stack_order_after_replacement_growth() {
    // Arrange
    let controls =
        QueryExecutionControls::from_limits(&CassieRuntimeLimits::default(), Instant::now());
    let values = [Value::Int64(1), Value::Int64(2), Value::Int64(3)];
    let mut stack = Stack::new(&controls).expect("stack");
    for value in &values {
        stack.push(Frame::SqlValue(value)).expect("frame");
    }

    // Act
    let ordered = stack
        .frames
        .iter()
        .map(|frame| match frame {
            Frame::SqlValue(Value::Int64(value)) => *value,
            _ => panic!("unexpected frame"),
        })
        .collect::<Vec<_>>();

    // Assert
    assert_eq!(ordered, [1, 2, 3]);
    drop(stack);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_respect_tie_growth_budget_boundary() {
    // Arrange
    let row = BatchRow::new(vec![("text".into(), Value::String("x".repeat(8192)))]);
    let baseline =
        QueryExecutionControls::from_limits(&CassieRuntimeLimits::default(), Instant::now());
    drop(serialize(&row, &baseline).expect("measure tie growth"));
    let required = baseline.peak_query_memory_bytes();
    assert!(required > 0);

    for (budget, accepted) in [(required - 1, false), (required, true)] {
        let controls = QueryExecutionControls::from_limits(
            &CassieRuntimeLimits {
                query_memory_budget_bytes: budget,
                ..CassieRuntimeLimits::default()
            },
            Instant::now(),
        );

        // Act
        let result = serialize(&row, &controls);

        // Assert
        assert_eq!(result.is_ok(), accepted);
        drop(result);
        assert_eq!(controls.current_query_memory_bytes(), 0);
    }
}

#[test]
fn should_preserve_vector_tie_text_across_scalar_array_positions() {
    // Arrange
    let controls =
        QueryExecutionControls::from_limits(&CassieRuntimeLimits::default(), Instant::now());
    let vector = crate::types::Vector::new(vec![f32::NAN, f32::INFINITY, -0.0, 1.5]);
    let array = SqlArray::from_elements(vec![Value::Vector(vector.clone())]).expect("array");
    let row = BatchRow::new(vec![
        ("vector".into(), Value::Vector(vector)),
        ("array".into(), Value::Array(array)),
    ]);
    let expected = row_tie_key(&row);

    // Act
    let actual = serialize(&row, &controls).expect("controlled tie");

    // Assert
    assert_eq!(actual.text(), expected);
    drop(actual);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_preserve_numeric_tie_text_across_scalar_array_positions() {
    // Arrange
    let controls =
        QueryExecutionControls::from_limits(&CassieRuntimeLimits::default(), Instant::now());
    let number = crate::types::extended::numeric::parse_text("-12345678901234567890.00100")
        .expect("numeric");
    let typed = Value::Typed(crate::types::typed::TypedValue::Numeric(number));
    let array = SqlArray::from_elements(vec![typed.clone()]).expect("array");
    let row = BatchRow::new(vec![
        ("number".into(), typed),
        ("array".into(), Value::Array(array)),
    ]);
    let expected = row_tie_key(&row);

    // Act
    let actual = serialize(&row, &controls).expect("controlled tie");

    // Assert
    assert_eq!(actual.text(), expected);
    drop(actual);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}
