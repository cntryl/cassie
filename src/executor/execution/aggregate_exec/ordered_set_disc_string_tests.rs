//! Scalar discrete percentile String result ownership witnesses.
use super::*;
use crate::config::CassieRuntimeLimits;
use crate::executor::execution::aggregate_exec::collection_json::{
    percentile_string_output, OutputRow,
};
use crate::runtime::QueryCancellationHandle;
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

fn plan(direction: &str) -> crate::planner::logical::LogicalPlan {
    let catalog = crate::catalog::Catalog::new();
    catalog.register_collection(
        "events",
        vec![
            ("value".into(), crate::types::DataType::Text),
            ("keep".into(), crate::types::DataType::Boolean),
        ],
    );
    let sql = format!("SELECT percentile_disc($1) WITHIN GROUP (ORDER BY value {direction}) FILTER (WHERE keep) FROM events");
    let parsed = crate::sql::parse_statement(&sql).expect("parse");
    crate::executor::execution::build_logical_plan(&catalog, &parsed).expect("plan")
}

fn row(value: Value, keep: bool) -> BatchRow {
    BatchRow::new(vec![
        ("value".into(), value),
        ("keep".into(), Value::Bool(keep)),
    ])
}

fn select(rows: &[BatchRow], direction: &str, fraction: Value) -> Value {
    let plan = plan(direction);
    let specs = super::super::aggregate_specs(&plan);
    let controls = controls(1_000_000);
    let functions = std::collections::HashMap::new();
    let params = [fraction];
    let context = AggregateExecutionContext {
        plan: &plan,
        params: &params,
        search_context: None,
        user_functions: &functions,
        session: None,
        controls: &controls,
        after_partition_row: None,
    };
    let result = evaluate_percentile_owned_observed(&specs[0].function, rows, &context, |_, _| {})
        .expect("percentile")
        .into_value();
    assert_eq!(controls.current_query_memory_bytes(), 0);
    result
}

#[test]
fn should_preserve_discrete_string_rank_filter_with_null_semantics() {
    // Arrange
    let rows = [
        row(Value::String("c".into()), true),
        row(Value::String("a".into()), true),
        row(Value::String("b".into()), false),
        row(Value::Null, true),
    ];

    // Act / Assert
    // Assert
    assert_eq!(
        select(&rows, "ASC", Value::Float64(0.0)),
        Value::String("a".into())
    );
    assert_eq!(
        select(&rows, "ASC", Value::Float64(0.5)),
        Value::String("a".into())
    );
    assert_eq!(
        select(&rows, "ASC", Value::Float64(1.0)),
        Value::String("c".into())
    );
    assert_eq!(
        select(&rows, "DESC", Value::Float64(0.5)),
        Value::String("c".into())
    );
    assert_eq!(select(&rows, "ASC", Value::Null), Value::Null);
    assert_eq!(select(&rows[2..], "ASC", Value::Float64(0.5)), Value::Null);
    let numbers = [row(Value::Int64(1), true), row(Value::Int64(2), true)];
    assert_eq!(
        select(&numbers, "ASC", Value::Float64(1.0)),
        Value::Int64(2)
    );
    let four = ["a", "b", "c", "d"].map(|value| row(Value::String(value.into()), true));
    assert_eq!(
        select(&four, "ASC", Value::Float64(0.5)),
        Value::String("b".into())
    );
    assert_eq!(
        select(&four, "ASC", Value::Float64(0.50001)),
        Value::String("c".into())
    );
    let fractions = Value::Array(
        crate::types::SqlArray::from_elements(vec![Value::Float64(0.0), Value::Float64(1.0)])
            .expect("fractions"),
    );
    assert_eq!(
        select(&four, "ASC", fractions),
        Value::Json(serde_json::json!(["a", "d"]))
    );
    let original = [
        row(Value::String("2020-01-01T00:00:00Z".into()), true),
        row(Value::String("2020-01-01T00:00:00.000000Z".into()), true),
    ];
    assert_eq!(
        select(&original, "ASC", Value::Float64(1.0)),
        original[1].get("value").unwrap().clone()
    );
}

#[test]
fn should_retain_discrete_string_through_aliases_with_last_row_clone() {
    // Arrange
    let plan = plan("ASC");
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
    let rows = [row(Value::String("λ-value".into()), true)];

    // Act
    let result = evaluate_percentile_owned_observed(&specs[0].function, &rows, &context, |_, _| {})
        .expect("result");
    let mut output = OutputRow::new(Vec::new(), &[], &controls).expect("row");
    output
        .append(result, &["pct".into(), "alias".into()])
        .expect("aliases");
    let output = output.finish();
    let last = output.clone();
    drop(output);

    // Assert
    assert_eq!(last.get("pct"), Some(&Value::String("λ-value".into())));
    assert_eq!(last.get("alias"), Some(&Value::String("λ-value".into())));
    assert!(last.query_memory().is_some());
    assert!(controls.current_query_memory_bytes() > 0);
    drop(last);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_own_an_empty_discrete_string_result() {
    // Arrange
    let plan = plan("ASC");
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
    let rows = [row(Value::String(String::new()), true)];
    let result = evaluate_percentile_owned_observed(&specs[0].function, &rows, &context, |_, _| {})
        .expect("result");

    // Act
    let mut output = OutputRow::new(Vec::new(), &[], &controls).expect("row");
    output
        .append(result, &["pct".into()])
        .expect("empty result");
    let output = output.finish();

    // Assert
    assert_eq!(output.get("pct"), Some(&Value::String(String::new())));
    assert!(output.query_memory().is_some());
    drop(output);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_require_row_admission_for_an_empty_discrete_string() {
    // Arrange
    let controls = controls(0);
    let source = controls.reserve_query_memory(0).expect("source");
    let result = percentile_string_output("", &source, &controls, || {}).expect("empty");
    let mut row = OutputRow::new(Vec::new(), &[], &controls).expect("row");

    // Act
    let result = row.append(result, &["pct".into()]);

    // Assert
    assert!(matches!(
        result,
        Err(QueryError::Cassie(CassieError::ResourceLimit(_)))
    ));
    drop(row);
    drop(source);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_reject_a_discrete_string_source_from_another_tracker() {
    // Arrange
    let first = controls(4096);
    let second = controls(4096);
    let source = first.reserve_query_memory(64).expect("source");

    // Act
    let result = percentile_string_output("value", &source, &second, || {});

    // Assert
    assert!(
        matches!(result, Err(QueryError::Cassie(CassieError::ResourceLimit(ref message)))
        if message == "percentile source uses a different query tracker")
    );
    assert_eq!(first.current_query_memory_bytes(), 64);
    assert_eq!(second.current_query_memory_bytes(), 0);
    drop(source);
    assert_eq!(first.current_query_memory_bytes(), 0);
}

#[test]
fn should_cancel_during_discrete_string_copy_with_sorted_with_fraction_owners() {
    // Arrange
    let plan = plan("ASC");
    let specs = super::super::aggregate_specs(&plan);
    let cancellation = QueryCancellationHandle::new();
    let controls = QueryExecutionControls::with_cancellation(
        &CassieRuntimeLimits::default(),
        Instant::now(),
        cancellation.clone(),
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
    let text = "λ".repeat(2048);
    let rows = [row(Value::String(text.clone()), true)];
    let mut seen = false;

    // Act
    let result =
        evaluate_percentile_owned_observed(&specs[0].function, &rows, &context, |progress, _| {
            if matches!(progress, FractionProgress::StringChunk) {
                seen = true;
                assert!(
                    controls.current_query_memory_bytes()
                        >= 2 * text.len() + size_of::<Option<f64>>()
                );
                cancellation.cancel();
            }
        });

    // Assert
    assert!(seen);
    assert!(matches!(
        result,
        Err(QueryError::Cassie(CassieError::QueryCanceled))
    ));
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

fn run_with_budget(budget: usize) -> (bool, usize) {
    let plan = plan("ASC");
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
    let rows = [row(Value::String("λ".repeat(512)), true)];
    let mut copied = false;
    let result =
        evaluate_percentile_owned_observed(&specs[0].function, &rows, &context, |progress, _| {
            if matches!(progress, FractionProgress::StringChunk) {
                copied = true;
            }
        });
    let succeeded = result.is_ok();
    if succeeded {
        assert!(copied);
    }
    drop(result);
    assert_eq!(controls.current_query_memory_bytes(), 0);
    (succeeded, controls.peak_query_memory_bytes())
}

#[test]
fn should_admit_discrete_string_copy_at_measured_overlapping_budget() {
    // Arrange
    let (succeeded, peak) = run_with_budget(1_000_000);
    // Assert
    assert!(succeeded && peak > 0);

    // Act / Assert
    assert!(run_with_budget(peak).0);
    assert!(!run_with_budget(peak - 1).0);
}

#[test]
fn should_reject_invalid_fraction_before_selecting_string_output() {
    // Arrange
    let plan = plan("ASC");
    let specs = super::super::aggregate_specs(&plan);
    let controls = controls(1_000_000);
    let functions = std::collections::HashMap::new();
    let params = [Value::Float64(2.0)];
    let context = AggregateExecutionContext {
        plan: &plan,
        params: &params,
        search_context: None,
        user_functions: &functions,
        session: None,
        controls: &controls,
        after_partition_row: None,
    };
    let rows = [row(Value::String("selected".into()), true)];
    let mut copied = false;

    // Act
    let result =
        evaluate_percentile_owned_observed(&specs[0].function, &rows, &context, |progress, _| {
            copied |= matches!(progress, FractionProgress::StringChunk);
        });

    // Assert
    assert!(matches!(
        result,
        Err(QueryError::Cassie(CassieError::TypeInput(_)))
    ));
    assert!(!copied);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}
