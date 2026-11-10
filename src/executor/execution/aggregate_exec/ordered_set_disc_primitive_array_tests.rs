//! Scalar discrete percentile primitive SQL Array ownership witnesses.
use super::*;
use crate::config::CassieRuntimeLimits;
use crate::executor::execution::aggregate_exec::collection_json::OutputRow;
use crate::runtime::QueryCancellationHandle;
use crate::types::{ArrayDimension, SqlArray};
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
fn plan() -> crate::planner::logical::LogicalPlan {
    let catalog = crate::catalog::Catalog::new();
    catalog.register_collection(
        "events",
        vec![
            (
                "value".into(),
                crate::types::DataType::Array(Box::new(crate::types::DataType::Int)),
            ),
            ("keep".into(), crate::types::DataType::Boolean),
        ],
    );
    let parsed = crate::sql::parse_statement(
        "SELECT percentile_disc($1) WITHIN GROUP (ORDER BY value) FILTER (WHERE keep) FROM events",
    )
    .expect("parse");
    crate::executor::execution::build_logical_plan(&catalog, &parsed).expect("plan")
}
fn row(value: Value, keep: bool) -> BatchRow {
    BatchRow::new(vec![
        ("value".into(), value),
        ("keep".into(), Value::Bool(keep)),
    ])
}
fn array(values: Vec<Value>, lower_bound: i32) -> Value {
    Value::Array(
        SqlArray::new(
            vec![ArrayDimension {
                length: i32::try_from(values.len()).expect("length"),
                lower_bound,
            }],
            values,
        )
        .expect("array"),
    )
}

#[test]
fn should_retain_selected_primitive_array_through_aliases() {
    // Arrange
    let plan = plan();
    let specs = super::super::aggregate_specs(&plan);
    let controls = controls(1_000_000);
    let functions = std::collections::HashMap::new();
    let params = [Value::Float64(0.0)];
    let context = AggregateExecutionContext {
        plan: &plan,
        params: &params,
        search_context: None,
        user_functions: &functions,
        session: None,
        controls: &controls,
        after_partition_row: None,
    };
    let expected = array(vec![Value::Int64(1), Value::Null, Value::Int64(3)], -2);
    let rows = [
        row(expected.clone(), true),
        row(array(vec![Value::Int64(4)], 1), false),
    ];

    // Act
    let result = evaluate_percentile_owned_observed(&specs[0].function, &rows, &context, |_, _| {})
        .expect("result");
    let mut output = OutputRow::new(Vec::new(), &[], &controls).expect("row");
    output
        .append(result, &["pct".into(), "alias".into()])
        .expect("aliases");
    let output = output.finish();

    // Assert
    assert_eq!(output.get("pct"), Some(&expected));
    assert_eq!(output.get("alias"), Some(&expected));
    assert!(output.query_memory().is_some());
    assert!(controls.current_query_memory_bytes() > 0);
    drop(output);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_retain_zero_byte_empty_primitive_array_owner() {
    // Arrange
    let plan = plan();
    let specs = super::super::aggregate_specs(&plan);
    let controls = controls(1_000_000);
    let functions = std::collections::HashMap::new();
    let params = [Value::Float64(0.5)];
    let context = AggregateExecutionContext {
        plan: &plan,
        params: &params,
        search_context: None,
        user_functions: &functions,
        session: None,
        controls: &controls,
        after_partition_row: None,
    };
    let rows = [row(Value::Array(SqlArray::empty()), true)];

    // Act
    let result = evaluate_percentile_owned_observed(&specs[0].function, &rows, &context, |_, _| {})
        .expect("result");
    let mut output = OutputRow::new(Vec::new(), &[], &controls).expect("row");
    output.append(result, &["pct".into()]).expect("empty array");
    let output = output.finish();

    // Assert
    assert_eq!(output.get("pct"), Some(&Value::Array(SqlArray::empty())));
    assert!(output.query_memory().is_some());
    drop(output);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_cancel_during_selected_primitive_array_copy() {
    // Arrange
    let plan = plan();
    let specs = super::super::aggregate_specs(&plan);
    for dimension in [true, false] {
        let cancel = QueryCancellationHandle::new();
        let controls = QueryExecutionControls::with_cancellation(
            &CassieRuntimeLimits::default(),
            Instant::now(),
            cancel.clone(),
        );
        let functions = std::collections::HashMap::new();
        let params = [Value::Float64(0.5)];
        let context = AggregateExecutionContext {
            plan: &plan,
            params: &params,
            search_context: None,
            user_functions: &functions,
            session: None,
            controls: &controls,
            after_partition_row: None,
        };
        let rows = [row(array(vec![Value::Int64(1), Value::Int64(2)], -2), true)];
        let mut seen = false;

        // Act
        let result = evaluate_percentile_owned_observed(
            &specs[0].function,
            &rows,
            &context,
            |progress, _| {
                if matches!(
                    (dimension, progress),
                    (true, FractionProgress::PrimitiveDimension)
                        | (false, FractionProgress::PrimitiveElement)
                ) {
                    seen = true;
                    cancel.cancel();
                }
            },
        );

        // Assert
        assert!(seen);
        assert!(matches!(
            result,
            Err(QueryError::Cassie(CassieError::QueryCanceled))
        ));
        assert_eq!(controls.current_query_memory_bytes(), 0);
    }
}

#[test]
fn should_preserve_invalid_with_null_fraction_before_array_output() {
    // Arrange
    let plan = plan();
    let specs = super::super::aggregate_specs(&plan);
    let controls = controls(1_000_000);
    let functions = std::collections::HashMap::new();
    let rows = [row(array(vec![Value::Int64(1)], 1), true)];
    for fraction in [Value::Null, Value::Float64(2.0)] {
        let params = [fraction.clone()];
        let context = AggregateExecutionContext {
            plan: &plan,
            params: &params,
            search_context: None,
            user_functions: &functions,
            session: None,
            controls: &controls,
            after_partition_row: None,
        };
        let mut copied = false;

        // Act
        let result = evaluate_percentile_owned_observed(
            &specs[0].function,
            &rows,
            &context,
            |progress, _| {
                copied |= matches!(
                    progress,
                    FractionProgress::PrimitiveDimension | FractionProgress::PrimitiveElement
                );
            },
        );

        // Assert
        assert!(!copied);
        if matches!(fraction, Value::Null) {
            assert_eq!(result.expect("null").into_value(), Value::Null);
        } else {
            assert!(matches!(
                result,
                Err(QueryError::Cassie(CassieError::TypeInput(_)))
            ));
        }
        assert_eq!(controls.current_query_memory_bytes(), 0);
    }
}

fn run_with_budget(budget: usize) -> (bool, usize) {
    let plan = plan();
    let specs = super::super::aggregate_specs(&plan);
    let controls = controls(budget);
    let functions = std::collections::HashMap::new();
    let params = [Value::Float64(0.5)];
    let context = AggregateExecutionContext {
        plan: &plan,
        params: &params,
        search_context: None,
        user_functions: &functions,
        session: None,
        controls: &controls,
        after_partition_row: None,
    };
    let rows = [row(
        array(vec![Value::Int64(1), Value::Null, Value::Int64(2)], -1),
        true,
    )];
    let mut copied = false;
    let result =
        evaluate_percentile_owned_observed(&specs[0].function, &rows, &context, |phase, _| {
            copied |= matches!(phase, FractionProgress::PrimitiveElement);
        });
    let success = match result {
        Ok(result) => {
            let mut output = OutputRow::new(Vec::new(), &[], &controls).expect("row");
            output
                .append(result, &["pct".into(), "alias".into()])
                .is_ok()
        }
        Err(_) => false,
    };
    if success {
        assert!(copied);
    }
    assert_eq!(controls.current_query_memory_bytes(), 0);
    (success, controls.peak_query_memory_bytes())
}

#[test]
fn should_admit_array_result_with_aliases_at_measured_peak() {
    // Arrange
    let (success, peak) = run_with_budget(1_000_000);
    // Assert
    assert!(success && peak > 0);

    // Act / Assert
    assert!(run_with_budget(peak).0);
    assert!(!run_with_budget(peak - 1).0);
}
