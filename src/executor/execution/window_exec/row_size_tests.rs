//! Test-first scalar window row-size and owner witnesses.
use super::*;
use crate::config::CassieRuntimeLimits;
use crate::runtime::QueryCancellationHandle;
use crate::types::extended::numeric::parse_text;
use crate::types::typed::TypedValue;
use std::time::Instant;

fn controls(budget: usize) -> QueryExecutionControls {
    QueryExecutionControls::from_limits(
        &CassieRuntimeLimits {
            query_memory_budget_bytes: budget,
            query_timeout_ms: 0,
            ..CassieRuntimeLimits::default()
        },
        Instant::now(),
    )
}

fn rows() -> Vec<BatchRow> {
    let mut first = BatchRow::new(vec![
        ("escape\"\\é".into(), Value::String("a\0b\n".into())),
        ("null".into(), Value::Null),
        ("number".into(), Value::Float64(-0.0)),
        (
            "json".into(),
            Value::Json(serde_json::json!({"z":[1,true,null]})),
        ),
        (
            "numeric".into(),
            Value::Typed(TypedValue::Numeric(
                parse_text("-123.4500").expect("numeric"),
            )),
        ),
        (
            "array".into(),
            Value::Array(
                crate::types::SqlArray::from_elements(vec![Value::Int64(1), Value::Null])
                    .expect("array"),
            ),
        ),
    ]);
    first.set_data_types(vec![crate::types::DataType::Text].into());
    vec![first, BatchRow::new(vec![])]
}

#[test]
fn should_match_prior_serialized_row_accounting_for_mixed_values_with_metadata() {
    // Arrange
    let rows = rows();
    let controls = controls(10 * 1024 * 1024);
    let prior = rows
        .iter()
        .map(|row| serde_json::to_vec(row.entries()).map_or(0, |bytes| bytes.len()))
        .sum::<usize>()
        .saturating_add(batch::row_type_bytes(&rows));

    // Act
    let actual = batch_rows_bytes(&rows, &controls).expect("count");

    // Assert
    assert_eq!(actual, prior);
    assert_eq!(controls.current_query_memory_bytes(), 0);
    assert_eq!(batch_rows_bytes(&[], &controls).expect("empty"), 0);
}

#[test]
fn should_interrupt_actual_window_row_string_count_with_release_workspace() {
    // Arrange
    let cancel = QueryCancellationHandle::new();
    let controls = QueryExecutionControls::with_cancellation(
        &CassieRuntimeLimits::default(),
        Instant::now(),
        cancel.clone(),
    );
    let rows = vec![BatchRow::new(vec![(
        "text".into(),
        Value::String("é".repeat(2048)),
    )])];
    super::super::aggregate_exec::cancel_next_window_row_count(cancel.clone());

    // Act
    let result = batch_rows_bytes(&rows, &controls);

    // Assert
    assert!(
        cancel.is_cancelled(),
        "the actual string scan must be reached"
    );
    assert!(matches!(
        result,
        Err(QueryError::Cassie(crate::app::CassieError::QueryCancelled))
    ));
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_interrupt_actual_window_json_with_array_counting() {
    // Arrange
    let values = [
        Value::Json(serde_json::json!({"payload": "é".repeat(2048)})),
        Value::Array(
            crate::types::SqlArray::from_elements(vec![Value::Int64(1); 512]).expect("array"),
        ),
    ];
    for value in values {
        let cancel = QueryCancellationHandle::new();
        let controls = QueryExecutionControls::with_cancellation(
            &CassieRuntimeLimits::default(),
            Instant::now(),
            cancel.clone(),
        );
        let rows = vec![BatchRow::new(vec![("value".into(), value)])];
        super::super::aggregate_exec::cancel_next_window_row_count(cancel.clone());

        // Act
        let result = batch_rows_bytes(&rows, &controls);

        // Assert
        assert!(cancel.is_cancelled(), "nested counting must reach its hook");
        assert!(matches!(
            result,
            Err(QueryError::Cassie(crate::app::CassieError::QueryCancelled))
        ));
        assert_eq!(controls.current_query_memory_bytes(), 0);
    }
}

#[test]
fn should_interrupt_actual_window_numeric_counting() {
    // Arrange
    let cancel = QueryCancellationHandle::new();
    let controls = QueryExecutionControls::with_cancellation(
        &CassieRuntimeLimits::default(),
        Instant::now(),
        cancel.clone(),
    );
    // This bounded coefficient crosses the formatter's 256-step arithmetic hook.
    let value = parse_text(&"7".repeat(512)).expect("numeric");
    let rows = vec![BatchRow::new(vec![(
        "value".into(),
        Value::Typed(TypedValue::Numeric(value)),
    )])];
    crate::executor::numeric_controlled_text::cancel_next_arithmetic(cancel.clone());

    // Act
    let result = batch_rows_bytes(&rows, &controls);

    // Assert
    assert!(
        cancel.is_cancelled(),
        "numeric digit work must reach the hook"
    );
    assert!(matches!(
        result,
        Err(QueryError::Cassie(crate::app::CassieError::QueryCancelled))
    ));
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_admit_exact_transient_count_workspace_with_release_it() {
    // Arrange
    let rows = rows();
    let baseline = controls(10 * 1024 * 1024);
    let expected = batch_rows_bytes(&rows, &baseline).expect("baseline");
    let peak = baseline.peak_query_memory_bytes();
    assert!(peak > 0);

    // Act
    let results = [(peak, true), (peak - 1, false)].map(|(budget, succeeds)| {
        let controls = controls(budget);
        let result = batch_rows_bytes(&rows, &controls);
        let matches = result.as_ref().is_ok_and(|actual| *actual == expected) == succeeds;
        (matches, controls.current_query_memory_bytes())
    });

    // Assert
    assert_eq!(baseline.current_query_memory_bytes(), 0);
    assert_eq!(results, [(true, 0), (true, 0)]);
}

#[test]
fn should_abort_scalar_window_before_output_when_row_counting_is_cancelled() {
    // Arrange
    let parsed = crate::sql::parse_statement("SELECT sum(n) OVER ()").expect("window SQL");
    let crate::sql::ast::QueryStatement::Select(select) = parsed.statement else {
        panic!("SELECT");
    };
    let row = BatchRow::new(vec![
        ("n".into(), Value::Int64(1)),
        ("payload".into(), Value::String("é".repeat(2048))),
    ]);
    let cancel = QueryCancellationHandle::new();
    let controls = QueryExecutionControls::with_cancellation(
        &CassieRuntimeLimits::default(),
        Instant::now(),
        cancel.clone(),
    );
    let functions = HashMap::new();
    super::super::aggregate_exec::cancel_next_window_row_count(cancel.clone());

    // Act
    let result = apply_window_functions(
        vec![vec![row]],
        &select.projection,
        &[],
        None,
        &functions,
        None,
        &controls,
    );

    // Assert
    assert!(cancel.is_cancelled(), "the input row scan must be reached");
    assert!(matches!(
        result,
        Err(QueryError::Cassie(crate::app::CassieError::QueryCancelled))
    ));
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_reject_window_row_accounting_overflow() {
    // Arrange
    let current = usize::MAX;
    let next = 1;

    // Act
    let result = add_window_row_bytes(current, next);

    // Assert
    assert!(matches!(
        result,
        Err(QueryError::Cassie(crate::app::CassieError::ResourceLimit(
            _
        )))
    ));
}

#[test]
fn should_leave_non_window_projection_outside_the_row_count_path() {
    // Arrange
    let parsed = crate::sql::parse_statement("SELECT n").expect("ordinary projection");
    let crate::sql::ast::QueryStatement::Select(select) = parsed.statement else {
        panic!("SELECT");
    };
    let row = BatchRow::new(vec![
        ("n".into(), Value::Int64(1)),
        ("payload".into(), Value::String("é".repeat(2048))),
    ]);
    let cancel = QueryCancellationHandle::new();
    let controls = QueryExecutionControls::with_cancellation(
        &CassieRuntimeLimits::default(),
        Instant::now(),
        cancel.clone(),
    );
    let functions = HashMap::new();
    super::super::aggregate_exec::cancel_next_window_row_count(cancel.clone());

    // Act
    let result = apply_window_functions(
        vec![vec![row]],
        &select.projection,
        &[],
        None,
        &functions,
        None,
        &controls,
    );
    super::super::aggregate_exec::clear_next_window_row_count();

    // Assert
    assert!(!cancel.is_cancelled());
    let output = result.expect("ordinary projection");
    assert_eq!(output[0][0].get("n"), Some(&Value::Int64(1)));
    drop(output);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}
