use std::time::Instant;

use crate::app::CassieError;
use crate::config::CassieRuntimeLimits;
use crate::runtime::{QueryCancellationHandle, QueryExecutionControls};

fn frame(values: &[f32]) -> Vec<u8> {
    let mut bytes = u16::try_from(values.len())
        .expect("dimensions")
        .to_be_bytes()
        .to_vec();
    bytes.extend_from_slice(&0_i16.to_be_bytes());
    for value in values {
        bytes.extend_from_slice(&value.to_be_bytes());
    }
    bytes
}

#[test]
fn should_match_binary_vector_display_with_error_precedence() {
    // Arrange
    let controls =
        QueryExecutionControls::from_limits(&CassieRuntimeLimits::default(), Instant::now());
    let values = [
        0.0_f32,
        -0.0,
        0.1,
        -1.0,
        f32::from_bits(1),
        f32::MIN_POSITIVE,
        f32::MAX,
        f32::MIN,
    ];
    let bytes = frame(&values);
    let original =
        super::super::super::super::codecs::binary_to_value_with_vector(&bytes, 25, true)
            .expect("original");
    let expected = super::super::super::super::codecs::value_to_text(&original, 25);
    // Act
    let (actual, memory) = super::decode(&bytes, 0, &controls, Some(values.len())).expect("decode");
    // Assert
    assert_eq!(actual, expected);
    drop((actual, memory));
    assert_eq!(controls.current_query_memory_bytes(), 0);
    let mut reserved = frame(&[1.0]);
    reserved[3] = 1;
    for bytes in [
        vec![],
        vec![0; 3],
        frame(&[1.0, 2.0]),
        reserved,
        frame(&[f32::NAN]),
        frame(&[f32::INFINITY]),
        {
            let mut b = frame(&[1.0]);
            b.push(0);
            b
        },
    ] {
        let result = super::decode(&bytes, 2, &controls, Some(1));
        assert!(
            matches!(result, Err(CassieError::Parse(message)) if message == "COPY binary field 3 is invalid: invalid binary vector value")
        );
    }
}

#[test]
fn should_enforce_declared_vector_dimensions_but_infer_bare_vector_dimensions() {
    // Arrange
    let bytes = frame(&[1.0, -2.5, 3.0]);
    let controls =
        QueryExecutionControls::from_limits(&CassieRuntimeLimits::default(), Instant::now());

    // Act
    let (inferred, memory) = super::decode(&bytes, 0, &controls, None).expect("bare vector");
    drop((inferred, memory));
    let rejected = super::decode(&bytes, 0, &controls, Some(2));

    // Assert
    assert!(matches!(
        rejected,
        Err(CassieError::Parse(message))
            if message == "COPY binary field 1 is invalid: invalid binary vector value"
    ));
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_enforce_binary_vector_overlap_budget_with_cancellation() {
    // Arrange
    let bytes = frame(&[0.0, 1.0, -0.5]);
    let limits = CassieRuntimeLimits {
        query_memory_budget_bytes: 26,
        ..CassieRuntimeLimits::default()
    };
    let controls = QueryExecutionControls::from_limits(&limits, Instant::now());
    let input = controls.reserve_query_memory(bytes.len()).expect("input");
    // Act
    let (actual, memory) = super::decode(&bytes, 0, &controls, None).expect("decode");
    // Assert
    assert_eq!(actual, "[0,1,-0.5]");
    assert_eq!(controls.current_query_memory_bytes(), 26);
    drop((actual, memory));
    drop(input);
    assert_eq!(controls.current_query_memory_bytes(), 0);
    let limits = CassieRuntimeLimits {
        query_memory_budget_bytes: 25,
        ..CassieRuntimeLimits::default()
    };
    let denied = QueryExecutionControls::from_limits(&limits, Instant::now());
    let input = denied.reserve_query_memory(bytes.len()).expect("input");
    assert!(super::decode(&bytes, 0, &denied, None).is_err());
    assert_eq!(denied.current_query_memory_bytes(), bytes.len());
    drop(input);
    assert_eq!(denied.current_query_memory_bytes(), 0);
    let cancellation = QueryCancellationHandle::new();
    let canceled = QueryExecutionControls::with_cancellation(
        &CassieRuntimeLimits::default(),
        Instant::now(),
        cancellation.clone(),
    );
    cancellation.cancel();
    assert!(matches!(
        super::decode(&bytes, 0, &canceled, None),
        Err(CassieError::QueryCanceled)
    ));
}

#[test]
fn should_release_binary_vector_output_on_synchronized_emission_cancellation() {
    // Arrange
    let bytes = frame(&[1.0; 257]);
    let cancellation = QueryCancellationHandle::new();
    let controls = QueryExecutionControls::with_cancellation(
        &CassieRuntimeLimits::default(),
        Instant::now(),
        cancellation.clone(),
    );
    // Act
    let result = super::decode_observed(&bytes, 0, &controls, None, |phase| {
        if phase {
            cancellation.cancel();
        }
    });
    // Assert
    assert!(matches!(result, Err(CassieError::QueryCanceled)));
    assert!(controls.peak_query_memory_bytes() > 0);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_interrupt_binary_vector_validation_before_output_allocation() {
    // Arrange
    let bytes = frame(&[1.0; 257]);
    let cancellation = QueryCancellationHandle::new();
    let controls = QueryExecutionControls::with_cancellation(
        &CassieRuntimeLimits::default(),
        Instant::now(),
        cancellation.clone(),
    );
    // Act
    let result = super::decode_observed(&bytes, 0, &controls, None, |emitting| {
        if !emitting {
            cancellation.cancel();
        }
    });
    // Assert
    assert!(matches!(result, Err(CassieError::QueryCanceled)));
    assert_eq!(controls.peak_query_memory_bytes(), 0);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}
