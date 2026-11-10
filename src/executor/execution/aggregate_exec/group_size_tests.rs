//! Differential and interruption probes for scalar aggregate-size accounting.
use super::*;
use crate::config::CassieRuntimeLimits;
use crate::runtime::QueryCancellationHandle;
use std::time::Instant;

fn typed_numeric() -> Value {
    Value::Typed(crate::types::typed::TypedValue::Numeric(
        crate::types::extended::numeric::parse_text("1.25").expect("numeric"),
    ))
}

#[test]
fn should_match_serde_entry_size_for_scalar_values_and_escaping() {
    // Arrange
    let controls =
        QueryExecutionControls::from_limits(&CassieRuntimeLimits::default(), Instant::now());
    let controls_text: String = (0_u8..=31).map(char::from).collect();
    let values = vec![
        (
            "a\"\\Ω".into(),
            Value::String(format!("{controls_text}λ😀")),
        ),
        (String::new(), Value::Null),
        ("b".into(), Value::Bool(false)),
        ("true".into(), Value::Bool(true)),
        ("min".into(), Value::Int64(i64::MIN)),
        ("max".into(), Value::Int64(i64::MAX)),
        ("negzero".into(), Value::Float64(-0.0)),
        ("poszero".into(), Value::Float64(0.0)),
        ("subnormal".into(), Value::Float64(f64::from_bits(1))),
        ("maxfloat".into(), Value::Float64(f64::MAX)),
        ("minfloat".into(), Value::Float64(f64::MIN)),
        ("finite".into(), Value::Float64(1.234_567_890_123_456_7e-200)),
        ("nan".into(), Value::Float64(f64::NAN)),
        ("infinity".into(), Value::Float64(f64::INFINITY)),
        (
            "negative infinity".into(),
            Value::Float64(f64::NEG_INFINITY),
        ),
    ];

    // Act / Assert
    assert_eq!(
        json_bytes_controlled(&values, &controls).expect("controlled"),
        json_bytes(&values)
    );
    assert_eq!(
        json_bytes_controlled(&[], &controls).expect("empty"),
        json_bytes(&[] as &[(String, Value)])
    );
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_match_serde_for_mixed_scalar_and_numeric_entries() {
    // Arrange
    let controls =
        QueryExecutionControls::from_limits(&CassieRuntimeLimits::default(), Instant::now());
    let values = vec![
        ("scalar".into(), Value::String("x".into())),
        ("typed".into(), typed_numeric()),
    ];
    // Act / Assert
    assert_eq!(
        json_bytes_controlled(&values, &controls).expect("fallback"),
        json_bytes(&values)
    );
}

#[test]
fn should_cancel_during_eligibility_scan_before_numeric_entry() {
    // Arrange
    let cancellation = QueryCancellationHandle::new();
    let controls = QueryExecutionControls::with_cancellation(
        &CassieRuntimeLimits::default(),
        Instant::now(),
        cancellation.clone(),
    );
    let values = vec![
        ("scalar".into(), Value::Bool(true)),
        ("typed".into(), typed_numeric()),
    ];
    let mut observed = 0;

    // Act
    let result = json_bytes_with_observers(
        &values,
        &controls,
        || {
            observed += 1;
            cancellation.cancel();
        },
        || {},
    );

    // Assert
    assert_eq!(observed, 1);
    assert!(matches!(
        result,
        Err(QueryError::Cassie(crate::app::CassieError::QueryCancelled))
    ));
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_interrupt_during_actual_group_string_size_scan() {
    // Arrange
    let cancellation = QueryCancellationHandle::new();
    let controls = QueryExecutionControls::with_cancellation(
        &CassieRuntimeLimits::default(),
        Instant::now(),
        cancellation.clone(),
    );
    let values = vec![("field".into(), Value::String("λ".repeat(8192)))];
    let mut scanned = false;
    // Act
    let result = json_bytes_controlled_observing(&values, &controls, || {
        scanned = true;
        cancellation.cancel();
    });
    // Assert
    assert!(scanned);
    assert!(matches!(
        result,
        Err(QueryError::Cassie(crate::app::CassieError::QueryCancelled))
    ));
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_count_selected_entries_without_a_scratch_allocation() {
    // Arrange
    let controls = QueryExecutionControls::from_limits(
        &CassieRuntimeLimits {
            query_memory_budget_bytes: 0,
            ..CassieRuntimeLimits::default()
        },
        Instant::now(),
    );
    let values = vec![("field".into(), Value::String("x".repeat(8192)))];
    // Act
    let result = json_bytes_controlled(&values, &controls);
    // Assert
    assert_eq!(result.expect("no scratch budget"), json_bytes(&values));
    assert_eq!(controls.peak_query_memory_bytes(), 0);
}

#[test]
fn should_count_nested_json_entries_with_admitted_frame_budget() {
    // Arrange
    let values = vec![(
        "document".into(),
        Value::Json(serde_json::json!({"z":[1, null, "λ\n"], "a":{"n":123}})),
    )];
    let baseline =
        QueryExecutionControls::from_limits(&CassieRuntimeLimits::default(), Instant::now());
    let expected = json_bytes(&values);
    assert_eq!(
        json_bytes_controlled(&values, &baseline).expect("measure"),
        expected
    );
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
        let result = json_bytes_controlled(&values, &controls);
        // Assert
        assert_eq!(result.is_ok(), accepted);
        if let Ok(size) = result {
            assert_eq!(size, expected);
        }
        assert_eq!(controls.current_query_memory_bytes(), 0);
    }
}

#[test]
fn should_match_serde_size_for_vector_float_boundaries_without_scratch() {
    // Arrange
    let controls = QueryExecutionControls::from_limits(
        &CassieRuntimeLimits {
            query_memory_budget_bytes: 0,
            ..CassieRuntimeLimits::default()
        },
        Instant::now(),
    );
    let values = vec![
        (
            "empty".into(),
            Value::Vector(crate::types::Vector::new(vec![])),
        ),
        (
            "values".into(),
            Value::Vector(crate::types::Vector::new(vec![
                0.0,
                -0.0,
                f32::MIN,
                f32::MAX,
                f32::from_bits(1),
                f32::NAN,
                f32::INFINITY,
                f32::NEG_INFINITY,
            ])),
        ),
    ];
    // Act / Assert
    assert_eq!(
        json_bytes_controlled(&values, &controls).expect("count"),
        json_bytes(&values)
    );
    assert_eq!(controls.peak_query_memory_bytes(), 0);
}

#[test]
fn should_cancel_during_actual_vector_element_count() {
    // Arrange
    let cancellation = QueryCancellationHandle::new();
    let controls = QueryExecutionControls::with_cancellation(
        &CassieRuntimeLimits::default(),
        Instant::now(),
        cancellation.clone(),
    );
    let values = vec![(
        "vector".into(),
        Value::Vector(crate::types::Vector::new(vec![1.25; 8192])),
    )];
    let mut counted = false;
    // Act
    let result = json_bytes_controlled_observing(&values, &controls, || {
        counted = true;
        cancellation.cancel();
    });
    // Assert
    assert!(counted);
    assert!(matches!(
        result,
        Err(QueryError::Cassie(crate::app::CassieError::QueryCancelled))
    ));
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_match_serde_for_shaped_nested_arrays_and_numeric_values() {
    // Arrange
    let controls =
        QueryExecutionControls::from_limits(&CassieRuntimeLimits::default(), Instant::now());
    let nested =
        crate::types::SqlArray::from_elements(vec![Value::String("Ω\n".into()), Value::Null])
            .expect("nested");
    let shaped = crate::types::SqlArray::new(
        vec![crate::types::ArrayDimension {
            length: 2,
            lower_bound: -7,
        }],
        vec![
            Value::Array(nested),
            Value::Json(serde_json::json!({"v":[1, true]})),
        ],
    )
    .expect("shaped");
    let supported = vec![("array".into(), Value::Array(shaped))];
    let numeric = vec![(
        "array".into(),
        Value::Array(crate::types::SqlArray::from_elements(vec![typed_numeric()]).expect("array")),
    )];

    // Act / Assert
    for values in [&supported, &numeric] {
        assert_eq!(
            json_bytes_controlled(values, &controls).expect("size"),
            json_bytes(values)
        );
    }
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_cancel_during_nested_array_element_count() {
    // Arrange
    let cancellation = QueryCancellationHandle::new();
    let controls = QueryExecutionControls::with_cancellation(
        &CassieRuntimeLimits::default(),
        Instant::now(),
        cancellation.clone(),
    );
    let array = crate::types::SqlArray::from_elements(vec![Value::Int64(1); 8192]).expect("array");
    let values = vec![("array".into(), Value::Array(array))];
    let mut counted = false;
    // Act
    let result = json_bytes_controlled_observing(&values, &controls, || {
        counted = true;
        cancellation.cancel();
    });
    // Assert
    assert!(counted);
    assert!(matches!(
        result,
        Err(QueryError::Cassie(crate::app::CassieError::QueryCancelled))
    ));
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_enforce_nested_array_counting_frame_budget() {
    // Arrange
    let mut value = Value::Int64(1);
    for _ in 0..40 {
        value = Value::Array(crate::types::SqlArray::from_elements(vec![value]).expect("array"));
    }
    let values = vec![("array".into(), value)];
    let baseline =
        QueryExecutionControls::from_limits(&CassieRuntimeLimits::default(), Instant::now());
    let expected = json_bytes(&values);
    assert_eq!(
        json_bytes_controlled(&values, &baseline).expect("measure"),
        expected
    );
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
        let result = json_bytes_controlled(&values, &controls);
        // Assert
        assert_eq!(result.is_ok(), accepted);
        if let Ok(size) = result {
            assert_eq!(size, expected);
        }
        assert_eq!(controls.current_query_memory_bytes(), 0);
    }
}

#[test]
fn should_match_serde_for_all_fixed_width_typed_values_without_scratch() {
    // Arrange
    use crate::types::extended::{
        network::NetAddr,
        temporal::{Interval, TimeTz},
    };
    use crate::types::typed::TypedValue;
    let controls = QueryExecutionControls::from_limits(
        &CassieRuntimeLimits {
            query_memory_budget_bytes: 0,
            ..CassieRuntimeLimits::default()
        },
        Instant::now(),
    );
    let network = NetAddr {
        family: 4,
        bits: 32,
        addr: [255; 16],
    };
    let typed = vec![
        TypedValue::Real(f32::NAN),
        TypedValue::Real(f32::INFINITY),
        TypedValue::Inet(network),
        TypedValue::Cidr(network),
        TypedValue::Macaddr([255; 6]),
        TypedValue::Macaddr8([255; 8]),
        TypedValue::PgLsn(u64::MAX),
        TypedValue::Date(i32::MIN),
        TypedValue::Time(i64::MAX),
        TypedValue::TimeTz(TimeTz {
            time: i64::MIN,
            zone: i32::MAX,
        }),
        TypedValue::Timestamp(i64::MIN),
        TypedValue::TimestampTz(i64::MAX),
        TypedValue::Interval(Interval {
            months: i32::MIN,
            days: i32::MAX,
            micros: i64::MIN,
        }),
    ];
    let values = typed
        .into_iter()
        .enumerate()
        .map(|(index, value)| (index.to_string(), Value::Typed(value)))
        .collect::<Vec<_>>();

    // Act / Assert
    assert_eq!(
        json_bytes_controlled(&values, &controls).expect("count"),
        json_bytes(&values)
    );
    assert_eq!(controls.peak_query_memory_bytes(), 0);
}

#[test]
fn should_count_fixed_typed_values_inside_arrays() {
    // Arrange
    let controls =
        QueryExecutionControls::from_limits(&CassieRuntimeLimits::default(), Instant::now());
    let array = crate::types::SqlArray::from_elements(vec![Value::Typed(
        crate::types::typed::TypedValue::Date(i32::MIN),
    )])
    .expect("array");
    let values = vec![("array".into(), Value::Array(array))];
    // Act / Assert
    assert_eq!(
        json_bytes_controlled(&values, &controls).expect("count"),
        json_bytes(&values)
    );
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_count_bit_strings_with_stored_padding_and_nested_arrays() {
    // Arrange
    use crate::types::extended::bit_string::BitString;
    use crate::types::typed::TypedValue;
    let controls =
        QueryExecutionControls::from_limits(&CassieRuntimeLimits::default(), Instant::now());
    let unusual: BitString =
        serde_json::from_str(r#"{"len":9,"bytes":[255,127,42]}"#).expect("stored bits");
    let nested =
        crate::types::SqlArray::from_elements(vec![Value::Typed(TypedValue::Bit(unusual.clone()))])
            .expect("array");
    let values = vec![
        (
            "empty".into(),
            Value::Typed(TypedValue::Bit(BitString::from_bits([]))),
        ),
        ("padded".into(), Value::Typed(TypedValue::VarBit(unusual))),
        ("nested".into(), Value::Array(nested)),
    ];
    // Act / Assert
    assert_eq!(
        json_bytes_controlled(&values, &controls).expect("count"),
        json_bytes(&values)
    );
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_count_flat_bit_values_with_zero_scratch_budget() {
    // Arrange
    use crate::types::extended::bit_string::BitString;
    use crate::types::typed::TypedValue;
    let controls = QueryExecutionControls::from_limits(
        &CassieRuntimeLimits {
            query_memory_budget_bytes: 0,
            ..CassieRuntimeLimits::default()
        },
        Instant::now(),
    );
    let bits = BitString::from_bits((0..4097).map(|index| index % 2 == 0));
    let values = vec![("bits".into(), Value::Typed(TypedValue::Bit(bits)))];
    // Act / Assert
    assert_eq!(
        json_bytes_controlled(&values, &controls).expect("count"),
        json_bytes(&values)
    );
    assert_eq!(controls.peak_query_memory_bytes(), 0);
}

#[test]
fn should_cancel_during_actual_stored_bit_byte_count() {
    // Arrange
    use crate::types::extended::bit_string::BitString;
    use crate::types::typed::TypedValue;
    let cancellation = QueryCancellationHandle::new();
    let controls = QueryExecutionControls::with_cancellation(
        &CassieRuntimeLimits::default(),
        Instant::now(),
        cancellation.clone(),
    );
    let bits = BitString::from_bits((0..8192).map(|index| index % 3 == 0));
    let values = vec![("bits".into(), Value::Typed(TypedValue::VarBit(bits)))];
    let mut counted = false;
    // Act
    let result = json_bytes_controlled_observing(&values, &controls, || {
        counted = true;
        cancellation.cancel();
    });
    // Assert
    assert!(counted);
    assert!(matches!(
        result,
        Err(QueryError::Cassie(crate::app::CassieError::QueryCancelled))
    ));
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_match_serde_for_numeric_group_values_and_nested_arrays() {
    // Arrange
    use crate::types::extended::numeric::parse_text;
    use crate::types::typed::TypedValue;
    let controls =
        QueryExecutionControls::from_limits(&CassieRuntimeLimits::default(), Instant::now());
    let values = [
        "0",
        "-12345678901234567890.00100",
        "0e-16383",
        "NaN",
        "Infinity",
        "-Infinity",
    ]
    .into_iter()
    .enumerate()
    .map(|(index, value)| {
        (
            index.to_string(),
            Value::Typed(TypedValue::Numeric(parse_text(value).expect("numeric"))),
        )
    })
    .collect::<Vec<_>>();
    let nested = vec![(
        "array".into(),
        Value::Array(crate::types::SqlArray::from_elements(vec![typed_numeric()]).expect("array")),
    )];

    // Act / Assert
    for values in [&values, &nested] {
        assert_eq!(
            json_bytes_controlled(values, &controls).expect("count"),
            json_bytes(values)
        );
        assert_eq!(controls.current_query_memory_bytes(), 0);
    }
}

#[test]
fn should_enforce_numeric_group_count_budget_boundary() {
    // Arrange
    let values = vec![("numeric".into(), typed_numeric())];
    let baseline =
        QueryExecutionControls::from_limits(&CassieRuntimeLimits::default(), Instant::now());
    let expected = json_bytes(&values);
    assert_eq!(
        json_bytes_controlled(&values, &baseline).expect("measure"),
        expected
    );
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
        let result = json_bytes_controlled(&values, &controls);
        // Assert
        assert_eq!(result.is_ok(), accepted);
        if let Ok(size) = result {
            assert_eq!(size, expected);
        }
        assert_eq!(controls.current_query_memory_bytes(), 0);
    }
}

#[test]
fn should_cancel_during_actual_numeric_group_limb_arithmetic() {
    // Arrange
    let cancellation = QueryCancellationHandle::new();
    let controls = QueryExecutionControls::with_cancellation(
        &CassieRuntimeLimits::default(),
        Instant::now(),
        cancellation.clone(),
    );
    let numeric = crate::types::extended::numeric::parse_text(&"7".repeat(1000)).expect("numeric");
    let values = vec![(
        "numeric".into(),
        Value::Typed(crate::types::typed::TypedValue::Numeric(numeric)),
    )];
    crate::executor::numeric_controlled_text::cancel_next_arithmetic(cancellation);
    // Act
    let result = json_bytes_controlled(&values, &controls);
    // Assert
    assert!(matches!(
        result,
        Err(QueryError::Cassie(crate::app::CassieError::QueryCancelled))
    ));
    assert_eq!(controls.current_query_memory_bytes(), 0);
}
