//! Focused logical CHAR-array padding and retained-owner regressions.
use super::*;
use crate::types::{ArrayDimension, SqlArray};
use std::time::Instant;

fn kind(width: u32) -> DataType {
    DataType::Array(Box::new(DataType::Char {
        length: Some(width),
    }))
}

fn native() -> Value {
    Value::Array(
        SqlArray::new(
            vec![ArrayDimension {
                length: 3,
                lower_bound: -2,
            }],
            vec![
                Value::String("é".into()),
                Value::Null,
                Value::String("x".into()),
            ],
        )
        .expect("canonical native array"),
    )
}

#[test]
fn should_admit_all_logical_character_padding_before_hydrating_any_element() {
    // Arrange
    let original = native();
    let target = kind(32);
    let sizing = QueryExecutionControls::from_limits(
        &crate::config::CassieRuntimeLimits::default(),
        Instant::now(),
    );
    let bytes = padding_bytes(&original, &target, &sizing).expect("padding bound");

    // Act
    for (budget, accepted) in [(bytes - 1, false), (bytes, true)] {
        let controls = QueryExecutionControls::from_limits(
            &crate::config::CassieRuntimeLimits {
                query_memory_budget_bytes: budget,
                ..crate::config::CassieRuntimeLimits::default()
            },
            Instant::now(),
        );
        let mut memory = controls.reserve_query_memory(0).expect("scratch owner");
        let mut value = original.clone();
        HYDRATED.with(|count| count.set(0));
        let outcome = hydrate_value(&mut value, &target, &controls, &mut memory);
        // Assert
        assert_eq!(outcome.is_ok(), accepted);
        assert_eq!(
            HYDRATED.with(std::cell::Cell::get),
            if accepted { 2 } else { 0 }
        );
        if accepted {
            let Value::Array(array) = value else {
                panic!("native result");
            };
            assert_eq!(array.dimensions()[0].lower_bound, -2);
            assert_eq!(
                array.elements()[0],
                Value::String(format!("é{}", " ".repeat(31)))
            );
            assert_eq!(array.elements()[1], Value::Null);
        } else {
            assert!(matches!(outcome, Err(CassieError::ResourceLimit(_))));
            assert_eq!(value, original);
        }
        drop(memory);
        assert_eq!(controls.current_query_memory_bytes(), 0);
    }
}

#[test]
fn should_retain_the_logical_character_row_memory_until_release() {
    // Arrange
    let controls = QueryExecutionControls::from_limits(
        &crate::config::CassieRuntimeLimits::default(),
        Instant::now(),
    );
    let origin = Arc::new(controls.reserve_query_memory(41).expect("origin owner"));
    let weak = Arc::downgrade(&origin);
    let row = BatchRow::from_projected_values(vec![("items".into(), native())])
        .with_optional_data_types(Some(Arc::new(vec![kind(3)])))
        .with_query_memory(Some(origin));

    // Act
    let row = hydrate_row(row, &controls).expect("logical row");
    let retained = controls.current_query_memory_bytes();
    let value = row.get("items").expect("array field");
    let Value::Array(array) = value else {
        panic!("native result");
    };

    // Assert
    assert_eq!(
        array.elements(),
        &[
            Value::String("é  ".into()),
            Value::Null,
            Value::String("x  ".into())
        ]
    );
    assert!(retained > 41);
    assert!(weak.upgrade().is_some());
    drop(row);
    assert!(weak.upgrade().is_none());
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_cancel_logical_character_padding_at_the_owned_chunk_boundary() {
    // Arrange
    let cancellation = crate::runtime::QueryCancellationHandle::new();
    let controls = QueryExecutionControls::with_cancellation(
        &crate::config::CassieRuntimeLimits::default(),
        Instant::now(),
        cancellation.clone(),
    );
    let mut memory = controls.reserve_query_memory(0).expect("scratch owner");
    let mut value = native();
    AFTER_CHUNK.with(|slot| *slot.borrow_mut() = Some(cancellation));

    // Act
    let result = hydrate_value(&mut value, &kind(8192), &controls, &mut memory);
    drop(memory);

    // Assert
    assert!(matches!(result, Err(CassieError::QueryCancelled)));
    assert_eq!(value, native());
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_leave_canonical_stored_array_bytes_unchanged_when_logical_rows_are_hydrated() {
    // Arrange
    let Value::Array(array) = native() else {
        unreachable!()
    };
    let stored = array.to_storage_json();
    let original = stored.clone();
    let controls = QueryExecutionControls::from_limits(
        &crate::config::CassieRuntimeLimits::default(),
        Instant::now(),
    );
    let mut value = super::super::json_to_typed_value(&stored, &kind(3));
    let mut memory = controls.reserve_query_memory(0).expect("logical owner");

    // Act
    hydrate_value(&mut value, &kind(3), &controls, &mut memory).expect("logical padding");

    // Assert
    assert_eq!(stored, original);
    let Value::Array(logical) = value else {
        panic!("native result");
    };
    assert_eq!(logical.elements()[0], Value::String("é  ".into()));
    assert_eq!(array.elements()[0], Value::String("é".into()));
}

#[test]
fn should_deny_stored_character_array_expansion_before_allocating_large_padding() {
    // Arrange
    let controls = QueryExecutionControls::from_limits(
        &crate::config::CassieRuntimeLimits {
            query_memory_budget_bytes: 4096,
            ..crate::config::CassieRuntimeLimits::default()
        },
        Instant::now(),
    );
    let mut value = native();
    let mut memory = controls.reserve_query_memory(0).expect("logical owner");
    HYDRATED.with(|count| count.set(0));

    // Act
    let outcome = hydrate_value(&mut value, &kind(1_000_000), &controls, &mut memory);
    let hydrated = HYDRATED.with(std::cell::Cell::get);
    drop(memory);

    // Assert
    assert!(matches!(outcome, Err(CassieError::ResourceLimit(_))));
    assert_eq!(hydrated, 0);
    assert_eq!(value, native());
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_admit_character_array_envelope_conversion_before_native_decoding() {
    // Arrange
    let Value::Array(array) = native() else {
        unreachable!()
    };
    let input = Value::Json(array.to_storage_json());
    let target = kind(3);
    let sizing = QueryExecutionControls::from_limits(
        &crate::config::CassieRuntimeLimits::default(),
        Instant::now(),
    );
    let bytes = padding_bytes(&input, &target, &sizing).expect("native envelope bound");

    // Act
    for (budget, accepted) in [(bytes - 1, false), (bytes, true)] {
        let controls = QueryExecutionControls::from_limits(
            &crate::config::CassieRuntimeLimits {
                query_memory_budget_bytes: budget,
                ..crate::config::CassieRuntimeLimits::default()
            },
            Instant::now(),
        );
        let mut value = input.clone();
        let mut memory = controls.reserve_query_memory(0).expect("native scratch");
        DECODED.with(|count| count.set(0));
        let result = hydrate_value(&mut value, &target, &controls, &mut memory);
        // Assert
        assert_eq!(result.is_ok(), accepted);
        assert_eq!(DECODED.with(std::cell::Cell::get), usize::from(accepted));
        if accepted {
            let Value::Array(array) = value else {
                panic!("native image array");
            };
            assert_eq!(array.dimensions()[0].lower_bound, -2);
            assert_eq!(array.elements()[0], Value::String("é  ".into()));
            assert_eq!(array.elements()[1], Value::Null);
        } else {
            assert!(matches!(result, Err(CassieError::ResourceLimit(_))));
        }
        drop(memory);
        assert_eq!(controls.current_query_memory_bytes(), 0);
    }
}
