use std::time::Instant;

use super::{decode, DecodeError, Progress};
use crate::app::CassieError;
use crate::config::CassieRuntimeLimits;
use crate::runtime::{QueryCancellationHandle, QueryExecutionControls};

fn controls(limit: usize) -> QueryExecutionControls {
    QueryExecutionControls::from_limits(
        &CassieRuntimeLimits {
            query_memory_budget_bytes: limit,
            ..CassieRuntimeLimits::default()
        },
        Instant::now(),
    )
}

fn array(dimensions: &[(i32, i32)], has_null: i32, fields: &[Option<&[u8]>]) -> Vec<u8> {
    let mut bytes = Vec::new();
    for value in [
        i32::try_from(dimensions.len()).expect("dimensions"),
        has_null,
        3802,
    ] {
        bytes.extend_from_slice(&value.to_be_bytes());
    }
    for (length, lower) in dimensions {
        bytes.extend_from_slice(&length.to_be_bytes());
        bytes.extend_from_slice(&lower.to_be_bytes());
    }
    for field in fields {
        if let Some(field) = field {
            bytes.extend_from_slice(&i32::try_from(field.len()).expect("length").to_be_bytes());
            bytes.extend_from_slice(field);
        } else {
            bytes.extend_from_slice(&(-1_i32).to_be_bytes());
        }
    }
    bytes
}

#[test]
fn should_keep_validated_ranges_with_select_existing_sql_or_compact_fallback() {
    // Arrange
    let controls = controls(4096);
    let normal = array(&[(2, 1)], 1, &[Some(b"\x011"), None]);
    let fallback = array(&[(1, i32::MAX)], 0, &[Some(b"\x01{\"z\":1}")]);
    // Act
    let first = decode(&normal, &controls, |_| Ok::<_, ()>(false), |_| {}).expect("normal");
    let second = decode(&fallback, &controls, |_| Ok::<_, ()>(false), |_| {}).expect("fallback");
    // Assert
    assert!(!first.compact_fallback());
    assert_eq!(first.dimensions().collect::<Vec<_>>(), vec![(2, 1)]);
    assert!(matches!(
        first.element(0),
        Some(super::Element::Value(value)) if value == b"\x011"
    ));
    assert!(matches!(first.element(1), Some(super::Element::SqlNull)));
    assert!(second.compact_fallback());
    assert_eq!(second.dimensions().collect::<Vec<_>>(), vec![(1, i32::MAX)]);
    drop((first, second));
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_preserve_element_error_before_later_trailer_with_shape_errors() {
    // Arrange
    let controls = controls(4096);
    let mut bytes = array(&[(1, i32::MAX)], 0, &[Some(b"bad")]);
    bytes.push(99);
    let mut calls = 0;
    // Act
    let result = decode(
        &bytes,
        &controls,
        |_| {
            calls += 1;
            Err::<bool, _>("element")
        },
        |_| {},
    );
    // Assert
    assert!(matches!(result, Err(DecodeError::Element("element"))));
    assert_eq!(calls, 1);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_distinguish_sql_null_document_null_with_late_envelope_errors() {
    // Arrange
    let controls = controls(4096);
    let sql_null = array(&[(1, 1)], 1, &[None]);
    let missing_flag = array(&[(1, 1)], 0, &[None]);
    let json_null = array(&[(1, 1)], 0, &[Some(b"\x01null")]);
    let zero_dimension = array(&[(0, 1)], 0, &[]);
    // Act / Assert
    // Assert
    assert!(decode(&sql_null, &controls, |_| Ok::<_, ()>(false), |_| {}).is_ok());
    assert!(matches!(
        decode(&missing_flag, &controls, |_| Ok::<_, ()>(true), |_| {}),
        Err(DecodeError::InvalidArray)
    ));
    assert!(matches!(
        decode(&json_null, &controls, |_| Ok::<_, ()>(true), |_| {}),
        Err(DecodeError::DocumentNull)
    ));
    assert!(matches!(
        decode(&zero_dimension, &controls, |_| Ok::<_, ()>(false), |_| {}),
        Err(DecodeError::InvalidArray)
    ));
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_admit_range_directory_exactly_with_cancel_after_real_element_validation() {
    // Arrange
    let bytes = array(&[(1, 1)], 0, &[Some(b"\x011")]);
    let exact = std::mem::size_of::<Option<std::ops::Range<usize>>>();
    let admitted = controls(exact);
    let denied = controls(exact - 1);
    let cancellation = QueryCancellationHandle::new();
    let canceled = QueryExecutionControls::with_cancellation(
        &CassieRuntimeLimits::default(),
        Instant::now(),
        cancellation.clone(),
    );
    let mut called = false;
    // Act / Assert
    let owner = decode(&bytes, &admitted, |_| Ok::<_, ()>(false), |_| {}).expect("exact");
    // Assert
    assert!(matches!(
        decode(&bytes, &denied, |_| Ok::<_, ()>(false), |_| {}),
        Err(DecodeError::Control(CassieError::ResourceLimit(_)))
    ));
    let result = decode(
        &bytes,
        &canceled,
        |_| Ok::<_, ()>(false),
        |progress| {
            if matches!(progress, Progress::Element) {
                called = true;
                cancellation.cancel();
            }
        },
    );
    assert!(called);
    assert!(matches!(
        result,
        Err(DecodeError::Control(CassieError::QueryCanceled))
    ));
    drop(owner);
    assert_eq!(admitted.current_query_memory_bytes(), 0);
    assert_eq!(denied.current_query_memory_bytes(), 0);
    assert_eq!(canceled.current_query_memory_bytes(), 0);
}
