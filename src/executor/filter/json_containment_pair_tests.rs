use super::{evaluate_with_controls, walk, BinaryOp, ScalarValue, TypedOperator};
use crate::config::CassieRuntimeLimits;
use crate::runtime::{QueryCancellationHandle, QueryExecutionControls};
use std::time::Instant;

fn contains() -> BinaryOp {
    BinaryOp::Typed {
        operator: TypedOperator::ContainsOrEqual,
    }
}

#[test]
fn should_release_pair_after_cancellation_at_actual_raw_pair_handoff() {
    // Arrange
    let cancel = QueryCancellationHandle::new();
    let controls = QueryExecutionControls::with_cancellation(
        &CassieRuntimeLimits::default(),
        Instant::now(),
        cancel.clone(),
    );
    let candidate = ScalarValue::Json("[1,2]".into());
    let required = ScalarValue::Json("2".into());
    walk::cancel_after_pair_once(cancel);
    // Act
    let result = evaluate_with_controls(&contains(), &candidate, &required, Some(&controls));
    // Assert
    assert!(result.is_err());
    assert!(controls.peak_query_memory_bytes() > 0);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_preserve_array_scalar_pair_success_and_nested_decode_error() {
    // Arrange
    let controls =
        QueryExecutionControls::from_limits(&CassieRuntimeLimits::default(), Instant::now());
    let array = ScalarValue::Json("[1,2]".into());
    let scalar = ScalarValue::Json("2".into());
    let broken = ScalarValue::Json(r#"["\ud800"]"#.into());
    let need = ScalarValue::Json(r#""a""#.into());
    // Act
    let found = evaluate_with_controls(&contains(), &array, &scalar, Some(&controls));
    let error = evaluate_with_controls(&contains(), &broken, &need, Some(&controls));
    // Assert
    assert!(matches!(found, Ok(ScalarValue::Bool(true))));
    assert!(error.is_err());
    assert_eq!(controls.current_query_memory_bytes(), 0);
}
