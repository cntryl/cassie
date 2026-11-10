use super::*;
use crate::runtime::QueryCancellationHandle;
use std::sync::atomic::{AtomicUsize, Ordering};

#[test]
fn should_release_retained_accumulator_key_after_later_update_cancellation() {
    // Arrange
    let plan = aggregate_plan("SELECT MAX(value) FROM events");
    let cancellation = QueryCancellationHandle::new();
    let controls = QueryExecutionControls::with_cancellation(
        &CassieRuntimeLimits::default(),
        Instant::now(),
        cancellation.clone(),
    );
    let functions = HashMap::new();
    let mut context = aggregate_context(&plan, &functions, &controls);
    let updates = AtomicUsize::new(0);
    let stop = |_| {
        if updates.fetch_add(1, Ordering::SeqCst) == 1 {
            assert!(controls.current_query_memory_bytes() > 0);
            cancellation.cancel();
        }
    };
    context.after_partition_row = Some(&stop);
    let array_row = |value| {
        let mut row = BatchRow::new(vec![(
            "value".into(),
            Value::Json(serde_json::json!([value])),
        )]);
        row.set_data_types(
            vec![crate::types::DataType::Array(Box::new(
                crate::types::DataType::BigInt,
            ))]
            .into(),
        );
        row
    };
    let rows = [array_row(1), array_row(2)];
    // Act
    let result = AggregateAccumulator::evaluate(aggregate_call(&plan), &rows, &context);
    // Assert
    assert_eq!(updates.load(Ordering::SeqCst), 2);
    assert!(matches!(result, Err(QueryError::General(ref message)) if message == "query canceled"));
    assert_eq!(controls.current_query_memory_bytes(), 0);
}
