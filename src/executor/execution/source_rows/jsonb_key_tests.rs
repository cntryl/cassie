use super::*;
use crate::config::CassieRuntimeLimits;
use std::sync::Arc;
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
fn row(text: &str) -> BatchRow {
    BatchRow::new(vec![(
        "payload".into(),
        Value::Json(serde_json::from_str(text).expect("document")),
    )])
    .with_optional_data_types(Some(Arc::new(vec![DataType::Jsonb])))
}
fn run(controls: &QueryExecutionControls) -> Result<Vec<BatchRow>, QueryError> {
    sort_union_all_rows(vec![row("10")], vec![row("2.00")], controls)
}

#[test]
fn should_admit_decorated_jsonb_union_rows_at_the_measured_boundary() {
    // Arrange
    let ample = controls(1_048_576);
    let measured = run(&ample).expect("measure admitted UNION ALL");
    let peak = ample.peak_query_memory_bytes();
    drop(measured);
    let exact = controls(peak);
    let scarce = controls(peak - 1);
    // Act
    let admitted = run(&exact);
    let denied = run(&scarce);
    // Assert
    let rows = admitted.expect("exact decorated-key boundary");
    assert_eq!(
        rows[0].get("payload"),
        Some(&Value::Json(serde_json::from_str("2.00").expect("number")))
    );
    assert!(matches!(
        denied,
        Err(QueryError::Cassie(crate::app::CassieError::ResourceLimit(
            _
        )))
    ));
    drop(rows);
    assert_eq!(exact.current_query_memory_bytes(), 0);
    assert_eq!(scarce.current_query_memory_bytes(), 0);
}

#[test]
fn should_release_decorated_jsonb_union_keys_after_a_late_numeric_error() {
    // Arrange
    let controls = controls(1_048_576);
    // Act
    let result = sort_union_all_rows(vec![row("1")], vec![row("1e-16384")], &controls);
    // Assert
    assert!(
        matches!(result,Err(QueryError::Cassie(crate::app::CassieError::TypeInput(ref error))) if error.sqlstate=="22003")
    );
    assert_eq!(controls.current_query_memory_bytes(), 0);
}
