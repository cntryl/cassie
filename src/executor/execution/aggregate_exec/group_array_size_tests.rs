//! Frame-move ownership probe for aggregate SQL Array size counting.
use super::*;
use crate::config::CassieRuntimeLimits;
use crate::runtime::QueryCancellationHandle;
use std::time::Instant;

#[test]
fn should_release_array_count_stack_after_actual_move_cancellation() {
    // Arrange
    let cancellation = QueryCancellationHandle::new();
    let controls = QueryExecutionControls::with_cancellation(
        &CassieRuntimeLimits::default(),
        Instant::now(),
        cancellation.clone(),
    );
    let array = SqlArray::from_elements(vec![Value::Null]).expect("array");
    let mut stack = Stack::new(&controls).expect("stack");
    stack.push(Frame::Array(&array)).expect("first");
    stack.push(Frame::Array(&array)).expect("second");
    assert_eq!(stack.frames.len(), stack.frames.capacity());
    let original = controls.current_query_memory_bytes();
    let mut moved = 0;

    // Act
    let result = stack.push_observing(Frame::Array(&array), || {
        moved += 1;
        cancellation.cancel();
    });

    // Assert
    assert_eq!(moved, 1);
    assert!(matches!(
        result,
        Err(QueryError::Cassie(CassieError::QueryCancelled))
    ));
    assert_eq!(controls.current_query_memory_bytes(), original);
    drop(stack);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}
