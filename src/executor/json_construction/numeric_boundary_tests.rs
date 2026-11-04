use super::*;
use crate::config::CassieRuntimeLimits;
use std::time::Instant;

fn controls(budget: usize) -> QueryExecutionControls {
    QueryExecutionControls::from_limits(
        &CassieRuntimeLimits {
            query_memory_budget_bytes: budget,
            ..CassieRuntimeLimits::default()
        },
        Instant::now(),
    )
}
fn build(
    input: &str,
    controls: &QueryExecutionControls,
) -> Result<(Value, Option<QueryMemoryReservation>), QueryError> {
    let input = Value::String(input.into());
    let bytes = bound(&input, Some(&DataType::Jsonb), true)?;
    let owner = reserve(Some(controls), bytes)?;
    let document = encode(&input, Some(&DataType::Jsonb), Some(controls))?;
    Ok((finish(document, true, Some(controls))?, owner))
}

#[test]
fn should_admit_expanded_jsonb_numeric_output_at_its_measured_boundary() {
    // Arrange
    let ample = controls(64 * 1024 * 1024);
    let measured = build("0.01e131073", &ample).expect("measure expanded canonical number");
    let peak = ample.peak_query_memory_bytes();
    drop(measured);
    let exact = controls(peak);
    let scarce = controls(peak - 1);
    // Act
    let admitted = build("0.01e131073", &exact);
    let denied = build("0.01e131073", &scarce);
    // Assert
    let output = admitted.expect("exact pre-admitted boundary");
    assert!(matches!(&output.0,Value::String(text) if text.len()==131072));
    assert!(matches!(
        denied,
        Err(QueryError::Cassie(CassieError::ResourceLimit(_)))
    ));
    drop(output);
    assert_eq!(exact.current_query_memory_bytes(), 0);
    assert_eq!(scarce.current_query_memory_bytes(), 0);
}

#[test]
fn should_reject_true_jsonb_numeric_range_before_requesting_output_memory() {
    // Arrange
    let controls = controls(1);
    // Act
    let result = build("0.01e131074", &controls);
    // Assert
    assert!(
        matches!(result,Err(QueryError::Cassie(CassieError::TypeInput(ref error))) if error.sqlstate=="22003")
    );
    assert_eq!(controls.peak_query_memory_bytes(), 0);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}
