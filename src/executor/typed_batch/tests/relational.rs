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
    let output = distinct_view(&batch, &controls, &[0]).expect("typed distinct");
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
    let result = distinct_view(&batch, &controls, &[0]);
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
    let union = set_selection_fixture(&controls, &left, &right, SetOperator::Union).expect("union");
    let intersect =
        set_selection_fixture(&controls, &left, &right, SetOperator::Intersect).expect("intersect");
    let except =
        set_selection_fixture(&controls, &left, &right, SetOperator::Except).expect("except");
    let all = set_selection_fixture(&controls, &left, &right, SetOperator::UnionAll).expect("all");
    // Assert
    assert_eq!(union.get().len(), 4);
    assert_eq!(intersect.get(), &[(false, 2), (false, 0)]);
    assert_eq!(except.get(), &[(false, 1)]);
    assert_eq!(all.get().len(), 7);
    drop((union, intersect, except, all, left, right));
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_admit_float_canonicalization_scratch_before_constructing_tuple_keys() {
    // Arrange
    use crate::executor::semantic::SemanticValue;
    use std::mem::size_of;
    let limits = CassieRuntimeLimits::default();
    let controls = QueryExecutionControls::from_limits(&limits, Instant::now());
    let batch = TypedBatch::from_columns(
        &controls,
        &[("n".into(), DataType::Float)],
        &[vec![Value::Float64(-9_223_372_036_854_775_808.0)]],
        1,
        None,
    )
    .expect("FLOAT source");
    let key_backing =
        size_of::<Vec<SemanticValue>>() + size_of::<SemanticValue>() + 2 * size_of::<usize>();
    let pressure = controls
        .reserve_query_memory(
            limits.query_memory_budget_bytes
                - controls.current_query_memory_bytes()
                - key_backing
                - 63,
        )
        .expect("scratch pressure");
    let before = controls.current_query_memory_bytes();
    // Act
    let result = super::super::relational::TupleKeys::new(&controls, &batch, &[0]);
    // Assert
    assert!(matches!(
        result.map_err(crate::app::CassieError::from),
        Err(crate::app::CassieError::ResourceLimit(_))
    ));
    assert_eq!(controls.current_query_memory_bytes(), before);
    drop(pressure);
    drop(batch);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_deny_escaped_json_semantic_keys_before_serialized_key_construction() {
    // Arrange
    use crate::executor::batch::BatchRow;
    use crate::executor::semantic::SemanticValue;
    use std::mem::size_of;
    let controls =
        QueryExecutionControls::from_limits(&CassieRuntimeLimits::default(), Instant::now());
    let json = serde_json::json!({"escaped": "\\\"\n".repeat(257), "array": [1, null]});
    let serialized = serde_json::to_string(&json).expect("key oracle");
    let input = vec![BatchRow::new(vec![("j".into(), Value::Json(json.clone()))])];
    let key_bytes = size_of::<Vec<SemanticValue>>()
        + size_of::<SemanticValue>()
        + 2 * size_of::<usize>()
        + (2 * serialized.len()).max(128);
    let pressure = controls
        .reserve_query_memory(controls.query_memory_budget_bytes - key_bytes + 1)
        .expect("key pressure");
    let before = controls.current_query_memory_bytes();
    // Act
    let result = super::super::relational::TupleKeys::scalar_rows(&controls, &input);
    // Assert
    assert!(matches!(
        result.map_err(crate::app::CassieError::from),
        Err(crate::app::CassieError::ResourceLimit(_))
    ));
    assert_eq!(controls.current_query_memory_bytes(), before);
    assert_eq!(input[0].entries()[0].1, Value::Json(json));
    drop(pressure);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

// Test compositions exercise retained views and branch keys using the production primitives.
fn distinct_view(
    batch: &TypedBatch,
    controls: &QueryExecutionControls,
    columns: &[usize],
) -> Result<TypedBatch, crate::executor::QueryError> {
    let keys = super::super::relational::TupleKeys::new(controls, batch, columns)?;
    let mut positions = keys.distinct_positions(controls)?;
    for position in &mut positions {
        *position = batch.position(*position)?;
    }
    TypedBatch::from_views(
        controls,
        batch.schema(),
        batch.columns.get(),
        batch.domain,
        Some(&positions),
    )
}

fn set_selection_fixture(
    controls: &QueryExecutionControls,
    left: &TypedBatch,
    right: &TypedBatch,
    operator: crate::sql::ast::SetOperator,
) -> Result<crate::runtime::accounted::Accounted<Vec<(bool, usize)>>, crate::executor::QueryError> {
    super::super::check_controls(controls)?;
    if left.schema().len() != right.schema().len() {
        return Err(super::super::invalid("set input widths disagree"));
    }
    let width = left.schema().len();
    let _column_memory = controls.reserve_query_memory(crate::executor::retained_memory::mul(
        width,
        std::mem::size_of::<usize>(),
    )?)?;
    let columns = (0..width).collect::<Vec<_>>();
    let left_keys = super::super::relational::TupleKeys::new(controls, left, &columns)?;
    let right_keys = super::super::relational::TupleKeys::new(controls, right, &columns)?;
    super::super::relational::set_positions(controls, &left_keys, &right_keys, operator)
}
