//! Discrete percentile String/null fraction-array ownership witnesses.
use super::*;
use crate::config::CassieRuntimeLimits;
use crate::executor::execution::aggregate_exec::collection_json::OutputRow;
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

fn plan(name: &str, direction: &str) -> crate::planner::logical::LogicalPlan {
    let catalog = crate::catalog::Catalog::new();
    catalog.register_collection(
        "events",
        vec![
            ("value".into(), crate::types::DataType::Text),
            ("keep".into(), crate::types::DataType::Boolean),
        ],
    );
    let statement = crate::sql::parse_statement(&format!("SELECT {name}($1) WITHIN GROUP (ORDER BY value {direction}) FILTER (WHERE keep) FROM events")).expect("parse");
    crate::executor::execution::build_logical_plan(&catalog, &statement).expect("plan")
}

fn row(value: Value, keep: bool) -> BatchRow {
    BatchRow::new(vec![
        ("value".into(), value),
        ("keep".into(), Value::Bool(keep)),
    ])
}

fn fractions(values: Vec<Value>) -> [Value; 1] {
    [Value::Array(
        crate::types::SqlArray::from_elements(values).expect("fractions"),
    )]
}

fn run(name: &str, direction: &str, rows: &[BatchRow], params: &[Value]) -> Value {
    let plan = plan(name, direction);
    let specs = super::super::aggregate_specs(&plan);
    let controls = controls(1_000_000);
    let functions = std::collections::HashMap::new();
    let context = AggregateExecutionContext {
        plan: &plan,
        params,
        parameter_type_oids: &[],
        search_context: None,
        user_functions: &functions,
        session: None,
        controls: &controls,
        after_partition_row: None,
    };
    let output = evaluate_percentile_owned_observed(&specs[0].function, rows, &context, |_, _| {})
        .expect("percentile")
        .into_value();
    assert_eq!(controls.current_query_memory_bytes(), 0);
    output
}

#[test]
fn should_preserve_discrete_string_array_order_filter_with_original_values() {
    // Arrange
    let rows = [
        row(Value::String("c".into()), true),
        row(Value::String("a".into()), true),
        row(Value::String("b".into()), false),
        row(Value::Null, true),
    ];
    let params = fractions(vec![Value::Float64(0.0), Value::Null, Value::Float64(1.0)]);

    // Act / Assert
    // Assert
    assert_eq!(
        run("percentile_disc", "ASC", &rows, &params),
        Value::Json(serde_json::json!(["a", null, "c"]))
    );
    assert_eq!(
        run("percentile_disc", "DESC", &rows, &params),
        Value::Json(serde_json::json!(["c", null, "a"]))
    );
    assert_eq!(
        run("percentile_disc", "ASC", &rows[2..], &params),
        Value::Null
    );
    let equal = [
        row(Value::String("2020-01-01T00:00:00Z".into()), true),
        row(Value::String("2020-01-01T00:00:00.000000Z".into()), true),
    ];
    assert_eq!(
        run("percentile_disc", "ASC", &equal, &params),
        Value::Json(serde_json::json!([
            "2020-01-01T00:00:00Z",
            null,
            "2020-01-01T00:00:00.000000Z"
        ]))
    );
}

#[test]
fn should_preserve_discrete_array_fallback_with_null_fraction_paths() {
    // Arrange
    let rows = [row(Value::Int64(1), true), row(Value::Int64(2), true)];
    let mixed = [
        row(Value::Int64(1), true),
        row(Value::String("x".into()), true),
    ];
    let params = fractions(vec![Value::Float64(0.0), Value::Float64(1.0)]);
    let nulls = fractions(vec![Value::Null, Value::Null]);
    let empty = fractions(vec![]);

    // Act / Assert
    // Assert
    assert_eq!(
        run("percentile_disc", "ASC", &rows, &params),
        Value::Json(serde_json::json!([1, 2]))
    );
    assert_eq!(
        run("percentile_disc", "ASC", &mixed, &params),
        Value::Json(serde_json::json!([1, "x"]))
    );
    assert_eq!(
        run("percentile_cont", "ASC", &rows, &params),
        Value::Json(serde_json::json!([1.0, 2.0]))
    );
    assert_eq!(
        run("percentile_disc", "ASC", &mixed, &nulls),
        Value::Json(serde_json::json!([null, null]))
    );
    assert_eq!(
        run("percentile_disc", "ASC", &mixed, &empty),
        Value::Json(serde_json::json!([]))
    );
}

#[test]
fn should_retain_discrete_string_array_through_aliases_with_last_row_clone() {
    // Arrange
    let plan = plan("percentile_disc", "ASC");
    let specs = super::super::aggregate_specs(&plan);
    let controls = controls(1_000_000);
    let functions = std::collections::HashMap::new();
    let params = fractions(vec![Value::Float64(0.0), Value::Null, Value::Float64(1.0)]);
    let context = AggregateExecutionContext {
        plan: &plan,
        params: &params,
        parameter_type_oids: &[],
        search_context: None,
        user_functions: &functions,
        session: None,
        controls: &controls,
        after_partition_row: None,
    };
    let rows = [
        row(Value::String(String::new()), true),
        row(Value::String("λ-value".into()), true),
    ];

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
    let expected = Value::Json(serde_json::json!(["", null, "λ-value"]));
    assert_eq!(last.get("pct"), Some(&expected));
    assert_eq!(last.get("alias"), Some(&expected));
    assert!(last.query_memory().is_some());
    assert!(controls.current_query_memory_bytes() > 0);
    drop(last);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_cancel_actual_string_array_copy_with_all_owners_live() {
    // Arrange
    let plan = plan("percentile_disc", "ASC");
    let specs = super::super::aggregate_specs(&plan);
    let cancellation = QueryCancellationHandle::new();
    let controls = QueryExecutionControls::with_cancellation(
        &CassieRuntimeLimits::default(),
        Instant::now(),
        cancellation.clone(),
    );
    let functions = std::collections::HashMap::new();
    let params = fractions(vec![Value::Float64(0.5), Value::Null]);
    let context = AggregateExecutionContext {
        plan: &plan,
        params: &params,
        parameter_type_oids: &[],
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
            if matches!(progress, FractionProgress::ArrayStringChunk) {
                seen = true;
                assert!(
                    controls.current_query_memory_bytes()
                        >= 2 * text.len()
                            + 2 * size_of::<Option<f64>>()
                            + 2 * size_of::<serde_json::Value>()
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
    let plan = plan("percentile_disc", "ASC");
    let specs = super::super::aggregate_specs(&plan);
    let controls = controls(budget);
    let functions = std::collections::HashMap::new();
    let params = fractions(vec![Value::Float64(0.0), Value::Null, Value::Float64(1.0)]);
    let context = AggregateExecutionContext {
        plan: &plan,
        params: &params,
        parameter_type_oids: &[],
        search_context: None,
        user_functions: &functions,
        session: None,
        controls: &controls,
        after_partition_row: None,
    };
    let rows = [row(Value::String("λ".repeat(512)), true)];
    let result = evaluate_percentile_owned_observed(&specs[0].function, &rows, &context, |_, _| {});
    let success = match result {
        Ok(result) => {
            let mut output = OutputRow::new(Vec::new(), &[], &controls).expect("row");
            output
                .append(result, &["pct".into(), "alias".into()])
                .is_ok()
        }
        Err(_) => false,
    };
    assert_eq!(controls.current_query_memory_bytes(), 0);
    (success, controls.peak_query_memory_bytes())
}

#[test]
fn should_admit_string_array_result_with_aliases_at_measured_peak() {
    // Arrange
    let (success, peak) = run_with_budget(1_000_000);
    // Assert
    assert!(success && peak > 0);

    // Act / Assert
    assert!(run_with_budget(peak).0);
    assert!(!run_with_budget(peak - 1).0);
}
