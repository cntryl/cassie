use super::super::TypedBatch;
use crate::config::CassieRuntimeLimits;
use crate::runtime::QueryExecutionControls;
use crate::types::{DataType, Value};
use std::time::Instant;

#[test]
fn should_preserve_first_distinct_occurrences_in_selected_numeric_tuples() {
    // Arrange
    let controls =
        QueryExecutionControls::from_limits(&CassieRuntimeLimits::default(), Instant::now());
    let values = vec![
        Value::Float64(-0.0),
        Value::Float64(0.0),
        Value::Null,
        Value::Float64(1.0),
        Value::Null,
    ];
    let batch = TypedBatch::from_columns(
        &controls,
        &[("n".into(), DataType::Float)],
        &[values],
        5,
        Some(&[4, 1, 0, 3, 2]),
    )
    .expect("selected source");
    // Act
    let output = batch.distinct(&controls, &[0]).expect("typed distinct");
    drop(batch);
    // Assert
    assert_eq!(output.len(), 3);
    assert_eq!(output.value(0, 0).expect("NULL"), Value::Null);
    assert_eq!(output.value(0, 1).expect("zero"), Value::Float64(0.0));
    assert_eq!(output.value(0, 2).expect("one"), Value::Float64(1.0));
    assert!(controls.current_query_memory_bytes() > 0);
    drop(output);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_decline_distinct_key_state_without_releasing_source_leases() {
    // Arrange
    let limits = CassieRuntimeLimits::default();
    let controls = QueryExecutionControls::from_limits(&limits, Instant::now());
    let batch = TypedBatch::from_columns(
        &controls,
        &[("n".into(), DataType::BigInt)],
        &[vec![Value::Int64(1), Value::Int64(2)]],
        2,
        None,
    )
    .expect("source");
    let pressure = controls
        .reserve_query_memory(
            limits.query_memory_budget_bytes - controls.current_query_memory_bytes() - 1,
        )
        .expect("budget pressure");
    let before = controls.current_query_memory_bytes();
    // Act
    let result = batch.distinct(&controls, &[0]);
    // Assert
    assert!(matches!(
        result.map_err(crate::app::CassieError::from),
        Err(crate::app::CassieError::ResourceLimit(_))
    ));
    assert_eq!(controls.current_query_memory_bytes(), before);
    assert_eq!(batch.value(0, 1).expect("source intact"), Value::Int64(2));
    drop(pressure);
    drop(batch);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_match_set_multiplicity_with_exact_numeric_equivalence() {
    // Arrange
    use crate::sql::ast::SetOperator;
    let controls =
        QueryExecutionControls::from_limits(&CassieRuntimeLimits::default(), Instant::now());
    let left = TypedBatch::from_columns(
        &controls,
        &[("n".into(), DataType::BigInt)],
        &[vec![
            Value::Int64(0),
            Value::Int64(9_007_199_254_740_993),
            Value::Null,
            Value::Int64(0),
        ]],
        4,
        None,
    )
    .expect("integers");
    let right = TypedBatch::from_columns(
        &controls,
        &[("n".into(), DataType::Float)],
        &[vec![
            Value::Float64(-0.0),
            Value::Float64(9_007_199_254_740_992.0),
            Value::Null,
        ]],
        3,
        None,
    )
    .expect("floats");
    // Act
    let union =
        super::super::relational::set_selection(&controls, &left, &right, SetOperator::Union)
            .expect("union");
    let intersect =
        super::super::relational::set_selection(&controls, &left, &right, SetOperator::Intersect)
            .expect("intersect");
    let except =
        super::super::relational::set_selection(&controls, &left, &right, SetOperator::Except)
            .expect("except");
    let all =
        super::super::relational::set_selection(&controls, &left, &right, SetOperator::UnionAll)
            .expect("all");
    // Assert
    assert_eq!(union.get().len(), 4);
    assert_eq!(intersect.get(), &[(false, 2), (false, 0)]);
    assert_eq!(except.get(), &[(false, 1)]);
    assert_eq!(all.get().len(), 7);
    drop((union, intersect, except, all, left, right));
    assert_eq!(controls.current_query_memory_bytes(), 0);
}
