use super::CopyPayload;
use crate::config::CassieRuntimeLimits;
use crate::runtime::{QueryCancellationHandle, QueryExecutionControls};
use std::time::Instant;

#[test]
fn should_admit_copy_payload_growth_overlapping_retained_owner_and_release() {
    // Arrange
    let limits = CassieRuntimeLimits {
        query_memory_budget_bytes: 16,
        ..CassieRuntimeLimits::default()
    };
    let controls = QueryExecutionControls::from_limits(&limits, Instant::now());
    let owner = controls.reserve_query_memory(8).expect("owner");
    let mut payload = CopyPayload::new(&controls).expect("payload");
    payload
        .append(b"12345678", &controls)
        .expect("initial buffer");
    // Act
    let growth = payload.append(b"9", &controls);
    // Assert
    assert!(
        growth.is_err(),
        "replacement must coexist with the old allocation and owner"
    );
    assert_eq!(payload.as_slice(), b"12345678");
    assert_eq!(controls.current_query_memory_bytes(), 16);
    drop(payload);
    assert_eq!(controls.current_query_memory_bytes(), 8);
    drop(owner);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_leave_copy_payload_unchanged_when_growth_is_cancelled() {
    // Arrange
    let cancellation = QueryCancellationHandle::new();
    let controls = QueryExecutionControls::with_cancellation(
        &CassieRuntimeLimits::default(),
        Instant::now(),
        cancellation.clone(),
    );
    let mut payload = CopyPayload::new(&controls).expect("payload");
    payload.append(b"a", &controls).expect("initial buffer");
    cancellation.cancel();
    // Act
    let result = payload.append(b"b", &controls);
    // Assert
    assert!(result.is_err());
    assert_eq!(payload.as_slice(), b"a");
    drop(payload);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_admit_exact_copy_failure_error_text_and_release_it() {
    // Arrange
    let controls =
        QueryExecutionControls::from_limits(&CassieRuntimeLimits::default(), Instant::now());
    // Act
    let (text, memory) = super::copy_fail_message("reason", &controls).expect("message");
    // Assert
    assert_eq!(text, "COPY failed: reason");
    assert_eq!(controls.current_query_memory_bytes(), text.capacity());
    drop((text, memory));
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_admit_latin1_expansion_while_input_owner_is_live() {
    // Arrange
    let limits = CassieRuntimeLimits {
        query_memory_budget_bytes: 5,
        ..CassieRuntimeLimits::default()
    };
    let controls = QueryExecutionControls::from_limits(&limits, Instant::now());
    let input = controls.reserve_query_memory(2).expect("input");
    // Act
    let decoded = super::decode_latin1(&[b'a', 0xff], &controls).expect("three output bytes");
    // Assert
    assert_eq!(decoded.as_slice(), "aÿ".as_bytes());
    assert_eq!(controls.current_query_memory_bytes(), 5);
    drop(decoded);
    drop(input);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_reject_latin1_expansion_before_allocating_when_over_budget() {
    // Arrange
    let limits = CassieRuntimeLimits {
        query_memory_budget_bytes: 2,
        ..CassieRuntimeLimits::default()
    };
    let controls = QueryExecutionControls::from_limits(&limits, Instant::now());
    // Act
    let decoded = super::decode_latin1(&[b'a', 0xff], &controls);
    // Assert
    assert!(decoded.is_err());
    assert_eq!(controls.current_query_memory_bytes(), 0);
}
