use super::*;
use super::super::collection_json as aggregate_json;
use crate::runtime::QueryCancellationHandle;

#[test]
fn should_retain_any_value_reservation_through_serial_output_aliases() {
    // Arrange
    let plan =
        aggregate_plan("SELECT ANY_VALUE(value) AS first, ANY_VALUE(value) AS second FROM events");
    let specs = aggregate_specs(&plan);
    let controls = controls_with_budget(16_384);
    let functions = HashMap::new();
    let context = aggregate_context(&plan, &functions, &controls);
    let rows = vec![BatchRow::new(vec![(
        "value".into(),
        Value::String("payload".repeat(32)),
    )])];
    // Act
    let batches = aggregate_query_batches_serial(rows, &specs, &context).expect("aggregate");
    let row = batches.iter().flatten().next().expect("row").clone();
    drop(batches);
    // Assert
    assert!(row.query_memory().is_some());
    assert!(row.operator_memory().is_some());
    assert!(controls.current_query_memory_bytes() > 0);
    drop(row);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_keep_empty_accumulator_without_a_new_query_owner() {
    // Arrange
    let plan = aggregate_plan("SELECT ANY_VALUE(value) FROM events");
    let controls = controls_with_budget(8192);
    let functions = HashMap::new();
    let context = aggregate_context(&plan, &functions, &controls);
    // Act
    let result =
        AggregateAccumulator::evaluate_owned(aggregate_call(&plan), &[], &context).expect("empty");
    // Assert
    assert_eq!(result.into_value(), Value::Null);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_release_owner_after_cancellation_during_alias_append() {
    // Arrange
    let cancel = QueryCancellationHandle::new();
    let controls = QueryExecutionControls::with_cancellation(
        &CassieRuntimeLimits::default(),
        Instant::now(),
        cancel.clone(),
    );
    let memory = controls
        .reserve_query_memory(128)
        .expect("accumulator owner");
    let output =
        aggregate_json::AggregateOutput::from_owned(Value::String("value".into()), memory);
    let mut row = aggregate_json::OutputRow::new(Vec::new(), &[], &controls).expect("row");
    let mut appended = 0;
    // Act
    let result = row.append_with_observer(output, &["first".into(), "second".into()], || {
        appended += 1;
        cancel.cancel();
    });
    // Assert
    assert_eq!(appended, 1);
    assert!(result.is_err());
    drop(row);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_reject_foreign_accumulator_reservation_before_absorbing() {
    // Arrange
    let controls = controls_with_budget(8192);
    let foreign = controls_with_budget(8192);
    let memory = foreign.reserve_query_memory(128).expect("foreign owner");
    let output =
        aggregate_json::AggregateOutput::from_owned(Value::String("value".into()), memory);
    let mut row = aggregate_json::OutputRow::new(Vec::new(), &[], &controls).expect("row");
    // Act
    let result = row.append(output, &["first".into()]);
    // Assert
    assert!(result.is_err());
    drop(row);
    assert_eq!(controls.current_query_memory_bytes(), 0);
    assert_eq!(foreign.current_query_memory_bytes(), 0);
}

#[test]
fn should_admit_accumulator_and_output_overlap_at_exact_budget() {
    // Arrange
    let baseline = controls_with_budget(8192);
    let memory = baseline.reserve_query_memory(128).expect("accumulator");
    let output =
        aggregate_json::AggregateOutput::from_owned(Value::String("value".into()), memory);
    let mut row = aggregate_json::OutputRow::new(Vec::new(), &[], &baseline).expect("row");
    row.append(output, &["first".into()]).expect("measure");
    let peak = baseline.peak_query_memory_bytes();
    drop(row);
    assert_eq!(baseline.current_query_memory_bytes(), 0);
    for (budget, succeeds) in [(peak, true), (peak - 1, false)] {
        let controls = controls_with_budget(budget);
        let memory = controls.reserve_query_memory(128).expect("accumulator");
        let output =
            aggregate_json::AggregateOutput::from_owned(Value::String("value".into()), memory);
        let mut row = aggregate_json::OutputRow::new(Vec::new(), &[], &controls).expect("row");
        // Act
        let result = row.append(output, &["first".into()]);
        // Assert
        assert_eq!(result.is_ok(), succeeds);
        drop(row);
        assert_eq!(controls.current_query_memory_bytes(), 0);
    }
}

#[test]
fn should_preserve_existing_json_output_reservation() {
    // Arrange
    let controls = controls_with_budget(8192);
    let plan = aggregate_plan("SELECT JSON_AGG(value) FROM events");
    let specs = aggregate_specs(&plan);
    let mut row = aggregate_json::OutputRow::new(Vec::new(), &specs, &controls).expect("JSON row");
    let before = controls.current_query_memory_bytes();
    let memory = controls.reserve_query_memory(128).expect("accumulator");
    let output =
        aggregate_json::AggregateOutput::from_owned(Value::String("value".into()), memory);
    // Act
    row.append(output, &["alias".into()]).expect("append");
    let row = row.finish();
    // Assert
    assert!(row.query_memory().is_some());
    assert!(controls.current_query_memory_bytes() >= before + 128);
    drop(row);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}
