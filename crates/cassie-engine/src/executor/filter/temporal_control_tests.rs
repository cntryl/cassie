use crate::app::CassieError;
use crate::config::CassieRuntimeLimits;
use crate::executor::filter::QueryError;
use crate::runtime::{QueryCancellationHandle, QueryExecutionControls};
use crate::types::extended::temporal::TemporalSession;
use crate::types::Value;

#[test]
fn should_propagate_query_cancellation_through_temporal_function_input_coercion() {
    // Arrange
    let cancellation = QueryCancellationHandle::new();
    let controls = QueryExecutionControls::with_cancellation(
        &CassieRuntimeLimits::default(),
        std::time::Instant::now(),
        cancellation.clone(),
    );
    cancellation.cancel();
    let args = [
        Value::String("year".into()),
        Value::String("2024-01-15 12:00:00 UTC".into()),
    ];

    // Act
    let result = super::dispatch(
        "date_part",
        &args,
        &TemporalSession::default(),
        Some(&controls),
    );

    // Assert
    assert!(matches!(
        result,
        Err(QueryError::Cassie(CassieError::QueryCanceled))
    ));
    assert_eq!(controls.current_query_memory_bytes(), 0);
}
