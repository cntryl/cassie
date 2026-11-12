use super::*;
use crate::executor::filter::EvaluationMemory;
use crate::runtime::{QueryCancellationHandle, QueryExecutionControls};
use std::time::Instant;

fn controls_with_budget(bytes: usize) -> QueryExecutionControls {
    QueryExecutionControls::from_limits(
        &crate::config::CassieRuntimeLimits {
            query_memory_budget_bytes: bytes,
            ..crate::config::CassieRuntimeLimits::default()
        },
        Instant::now(),
    )
}

#[test]
fn should_admit_temporal_to_char_workspace_before_rendering() {
    // Arrange
    let controls = controls_with_budget(4 * 1024);
    let memory = EvaluationMemory::new(&controls).expect("expression owner");
    let session = super::super::temporal::session_of(Some(&controls));
    let args = [
        Value::Typed(TypedValue::Date(0)),
        Value::String("x".repeat(256)),
    ];

    // Act
    let result = evaluate("to_char", &args, &session, Some(&controls), Some(&memory));

    // Assert
    assert!(matches!(
        result,
        Err(QueryError::Cassie(crate::app::CassieError::ResourceLimit(
            _
        )))
    ));
    drop(result);
    drop(memory);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_admit_temporal_parse_workspace_before_to_date_parsing() {
    // Arrange
    let controls = controls_with_budget(4 * 1024);
    let memory = EvaluationMemory::new(&controls).expect("expression owner");
    let session = super::super::temporal::session_of(Some(&controls));
    let input = "x".repeat(256);
    let args = [Value::String(input.clone()), Value::String(input)];

    // Act
    let result = evaluate("to_date", &args, &session, Some(&controls), Some(&memory));

    // Assert
    assert!(matches!(
        result,
        Err(QueryError::Cassie(crate::app::CassieError::ResourceLimit(
            _
        )))
    ));
    drop(result);
    drop(memory);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_admit_temporal_parse_workspace_without_an_expression_owner() {
    // Arrange
    let controls = controls_with_budget(4 * 1024);
    let session = super::super::temporal::session_of(Some(&controls));
    let input = "x".repeat(256);
    let args = [Value::String(input.clone()), Value::String(input)];

    // Act
    let result = evaluate("to_date", &args, &session, Some(&controls), None);

    // Assert
    assert!(matches!(
        result,
        Err(QueryError::Cassie(crate::app::CassieError::ResourceLimit(
            _
        )))
    ));
    drop(result);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_release_temporal_parse_workspace_when_to_date_returns() {
    // Arrange
    let controls = controls_with_budget(32 * 1024);
    let memory = EvaluationMemory::new(&controls).expect("expression owner");
    let session = super::super::temporal::session_of(Some(&controls));
    let args = [
        Value::String("2000-01-02".into()),
        Value::String("YYYY-MM-DD".into()),
    ];

    // Act
    let result =
        evaluate("to_date", &args, &session, Some(&controls), Some(&memory)).expect("date parse");

    // Assert
    assert!(matches!(result, Value::Typed(TypedValue::Date(_))));
    assert_eq!(memory.bytes(), 0);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_retain_temporal_to_char_output_after_releasing_format_workspace() {
    // Arrange
    let controls = controls_with_budget(32 * 1024);
    let memory = EvaluationMemory::new(&controls).expect("expression owner");
    let session = super::super::temporal::session_of(Some(&controls));
    let args = [
        Value::Typed(TypedValue::Timestamp(0)),
        Value::String("YYYY".into()),
    ];

    // Act
    let result = evaluate("to_char", &args, &session, Some(&controls), Some(&memory))
        .expect("format timestamp");

    // Assert
    let Value::String(output) = result else {
        panic!("expected formatted text")
    };
    assert_eq!(output, "2000");
    assert!(memory.bytes() >= "YYYY".len() * 4 + 64);
    assert_eq!(memory.bytes(), output.capacity());
    assert_eq!(controls.current_query_memory_bytes(), memory.bytes());
}

#[test]
fn should_stop_temporal_formatting_when_its_request_is_canceled() {
    // Arrange
    let cancellation = QueryCancellationHandle::new();
    let controls = QueryExecutionControls::with_cancellation(
        &crate::config::CassieRuntimeLimits::default(),
        Instant::now(),
        cancellation.clone(),
    );
    let memory = EvaluationMemory::new(&controls).expect("expression owner");
    let session = super::super::temporal::session_of(Some(&controls));
    let args = [
        Value::Typed(TypedValue::Timestamp(0)),
        Value::String("YYYY".repeat(128)),
    ];
    cancellation.cancel();

    // Act
    let result = evaluate("to_char", &args, &session, Some(&controls), Some(&memory));

    // Assert
    assert!(matches!(
        result,
        Err(QueryError::Cassie(crate::app::CassieError::QueryCanceled))
    ));
    drop(result);
    drop(memory);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_check_cancellation_while_scanning_long_temporal_fraction_input() {
    // Arrange
    let cancellation = QueryCancellationHandle::new();
    let controls = QueryExecutionControls::with_cancellation(
        &crate::config::CassieRuntimeLimits::default(),
        Instant::now(),
        cancellation.clone(),
    );
    let session = super::super::temporal::session_of(Some(&controls));
    let input = "1".repeat(10_000);
    let mut checkpoints = 0;

    // Act
    let result =
        format::parse_with_precision_checked(&input, "FF6", session.abbreviations, &mut || {
            checkpoints += 1;
            if checkpoints == 90 {
                cancellation.cancel();
            }
            controls.check_interruption()
        });

    // Assert
    assert!(matches!(
        result,
        Err(format::ParseExecutionError::Check(
            crate::app::CassieError::QueryCanceled
        ))
    ));
    assert_eq!(checkpoints, 90);
}
