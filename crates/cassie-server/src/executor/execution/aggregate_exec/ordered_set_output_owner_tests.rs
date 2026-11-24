//! Sorted-state and selected null-array output ownership witnesses.
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

#[test]
fn should_retain_sorted_values_until_their_owner_drops() {
    // Arrange
    let catalog = crate::catalog::Catalog::new();
    catalog.register_collection(
        "events",
        vec![("value".into(), crate::types::DataType::Float)],
    );
    let parsed = crate::sql::parse_statement(
        "SELECT percentile_cont(0.5) WITHIN GROUP (ORDER BY value) FROM events",
    )
    .expect("parse");
    let plan = crate::executor::execution::build_logical_plan(&catalog, &parsed).expect("plan");
    let specs = super::super::aggregate_specs(&plan);
    let controls = controls(1_000_000);
    let functions = std::collections::HashMap::new();
    let context = AggregateExecutionContext {
        plan: &plan,
        params: &[],
        parameter_type_oids: &[],
        search_context: None,
        user_functions: &functions,
        session: None,
        controls: &controls,
        after_partition_row: None,
    };
    let rows =
        [3.0, 1.0, 2.0].map(|value| BatchRow::new(vec![("value".into(), Value::Float64(value))]));

    // Act
    let sorted = collect_sorted_values_using(
        &specs[0].function,
        &rows,
        &specs[0].function.order_by[0],
        &context,
        |values| try_sort_by(values, &controls, compare_values),
    )
    .expect("sorted");

    // Assert
    assert_eq!(
        sorted.values,
        [
            Value::Float64(1.0),
            Value::Float64(2.0),
            Value::Float64(3.0)
        ]
    );
    assert!(sorted.memory.bytes() > 0);
    assert!(sorted.memory.belongs_to(&controls));
    assert!(controls.current_query_memory_bytes() >= sorted.memory.bytes());
    drop(sorted);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_retain_null_percentile_output_through_every_row_alias() {
    // Arrange
    let catalog = crate::catalog::Catalog::new();
    catalog.register_collection(
        "events",
        vec![("value".into(), crate::types::DataType::Float)],
    );
    let parsed = crate::sql::parse_statement(
        "SELECT percentile_cont($1) WITHIN GROUP (ORDER BY value) FROM events",
    )
    .expect("parse");
    let plan = crate::executor::execution::build_logical_plan(&catalog, &parsed).expect("plan");
    let specs = super::super::aggregate_specs(&plan);
    let params = [Value::Array(
        crate::types::SqlArray::from_elements(vec![Value::Null, Value::Null]).expect("fractions"),
    )];
    let controls = controls(1_000_000);
    let functions = std::collections::HashMap::new();
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
    let rows = [BatchRow::new(vec![("value".into(), Value::Float64(3.0))])];
    let names = vec!["pct".to_owned(), "pct_alias".to_owned()];

    // Act
    let result = evaluate_percentile_owned_observed(&specs[0].function, &rows, &context, |_, _| {})
        .expect("percentile");
    let mut output = OutputRow::new(Vec::new(), &[], &controls).expect("output row");
    output.append(result, &names).expect("copy every alias");
    let output = output.finish();

    // Assert
    let expected = Value::Json(serde_json::json!([null, null]));
    assert_eq!(output.get("pct"), Some(&expected));
    assert_eq!(output.get("pct_alias"), Some(&expected));
    assert!(output.query_memory().is_some());
    assert!(controls.current_query_memory_bytes() > 0);
    drop(output);
    assert_eq!(controls.current_query_memory_bytes(), 0);
    let no_rows = evaluate_percentile_owned_observed(&specs[0].function, &[], &context, |_, _| {})
        .expect("no rows");
    assert_eq!(no_rows.into_value(), Value::Null);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_preserve_scalar_fraction_result_policy() {
    // Arrange
    let catalog = crate::catalog::Catalog::new();
    catalog.register_collection(
        "events",
        vec![("value".into(), crate::types::DataType::Float)],
    );
    let parsed = crate::sql::parse_statement(
        "SELECT percentile_cont($1) WITHIN GROUP (ORDER BY value) FROM events",
    )
    .expect("parse");
    let plan = crate::executor::execution::build_logical_plan(&catalog, &parsed).expect("plan");
    let specs = super::super::aggregate_specs(&plan);
    let params = [Value::Float64(0.5)];
    let controls = controls(1_000_000);
    let functions = std::collections::HashMap::new();
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
    let rows = [BatchRow::new(vec![("value".into(), Value::Float64(3.0))])];

    // Act
    let output = evaluate_percentile_owned_observed(&specs[0].function, &rows, &context, |_, _| {})
        .expect("scalar percentile");

    // Assert
    assert_eq!(output.into_value(), Value::Float64(3.0));
    assert_eq!(controls.current_query_memory_bytes(), 0);

    let array_params = [Value::Array(
        crate::types::SqlArray::from_elements(vec![Value::Float64(0.5)]).expect("fractions"),
    )];
    let array_context = AggregateExecutionContext {
        params: &array_params,
        ..context
    };
    let array =
        evaluate_percentile_owned_observed(&specs[0].function, &rows, &array_context, |_, _| {})
            .expect("non-null array percentile");
    assert_eq!(array.into_value(), Value::Json(serde_json::json!([3.0])));
}

#[test]
fn should_admit_null_result_array_at_exact_overlapping_budget() {
    // Arrange
    let source_bytes = 64;
    let output_bytes = 3 * size_of::<serde_json::Value>();

    // Act / Assert
    for (budget, succeeds) in [
        (source_bytes + output_bytes, true),
        (source_bytes + output_bytes - 1, false),
    ] {
        let controls = controls(budget);
        let source = controls.reserve_query_memory(source_bytes).expect("source");
        let result = null_array_output(3, &source, &controls, |_| {});
        // Assert
        assert_eq!(result.is_ok(), succeeds);
        if let Ok(output) = result {
            assert_eq!(
                controls.current_query_memory_bytes(),
                source_bytes + output_bytes
            );
            drop(output);
        }
        assert_eq!(controls.current_query_memory_bytes(), source_bytes);
        drop(source);
        assert_eq!(controls.current_query_memory_bytes(), 0);
    }
}

#[test]
fn should_reject_foreign_null_result_owner_before_admission() {
    // Arrange
    let source_controls = controls(1024);
    let other_controls = controls(1024);
    let source = source_controls.reserve_query_memory(64).expect("source");

    // Act
    let result = null_array_output(2, &source, &other_controls, |_| {});

    // Assert
    assert!(matches!(
        result,
        Err(QueryError::Cassie(CassieError::ResourceLimit(_)))
    ));
    assert_eq!(source_controls.current_query_memory_bytes(), 64);
    assert_eq!(other_controls.current_query_memory_bytes(), 0);
    drop(source);
    assert_eq!(source_controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_cancel_during_null_result_construction_with_release_output() {
    // Arrange
    let cancellation = QueryCancellationHandle::new();
    let controls = QueryExecutionControls::with_cancellation(
        &CassieRuntimeLimits::default(),
        Instant::now(),
        cancellation.clone(),
    );
    let source = controls.reserve_query_memory(64).expect("source");
    let mut seen = false;

    // Act
    let result = null_array_output(3, &source, &controls, |index| {
        if index == 1 {
            seen = true;
            cancellation.cancel();
        }
    });

    // Assert
    assert!(seen);
    assert!(matches!(result, Err(QueryError::General(ref message)) if message == "query canceled"));
    assert_eq!(controls.current_query_memory_bytes(), 64);
    drop(source);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_admit_empty_null_array_row_owner_before_alias_copy() {
    // Arrange
    let controls = controls(0);
    let source = controls.reserve_query_memory(0).expect("source");
    let output = null_array_output(0, &source, &controls, |_| {}).expect("empty array");
    let mut row = OutputRow::new(Vec::new(), &[], &controls).expect("empty row");

    // Act
    let result = row.append(output, &["pct".to_owned()]);

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
fn should_retain_empty_null_array_through_aliases() {
    // Arrange
    let controls = controls(4096);
    let source = controls.reserve_query_memory(0).expect("source");
    let output = null_array_output(0, &source, &controls, |_| {}).expect("empty array");
    let mut row = OutputRow::new(Vec::new(), &[], &controls).expect("row");

    // Act
    row.append(output, &["pct".into(), "alias".into()])
        .expect("aliases");
    let row = row.finish();

    // Assert
    let expected = Value::Json(serde_json::json!([]));
    assert_eq!(row.get("pct"), Some(&expected));
    assert_eq!(row.get("alias"), Some(&expected));
    assert!(row.query_memory().is_some());
    drop(row);
    drop(source);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_retain_null_array_for_continuous_with_discrete_filtered_percentiles() {
    // Arrange
    let catalog = crate::catalog::Catalog::new();
    catalog.register_collection(
        "events",
        vec![
            ("value".into(), crate::types::DataType::Float),
            ("keep".into(), crate::types::DataType::Boolean),
        ],
    );
    let params = [Value::Array(
        crate::types::SqlArray::from_elements(vec![Value::Null, Value::Null]).expect("fractions"),
    )];
    let rows = [
        BatchRow::new(vec![
            ("value".into(), Value::Float64(1.0)),
            ("keep".into(), Value::Bool(false)),
        ]),
        BatchRow::new(vec![
            ("value".into(), Value::Float64(3.0)),
            ("keep".into(), Value::Bool(true)),
        ]),
    ];
    let functions = std::collections::HashMap::new();
    for name in ["percentile_cont", "percentile_disc"] {
        let parsed = crate::sql::parse_statement(&format!(
            "SELECT {name}($1) WITHIN GROUP (ORDER BY value) FILTER (WHERE keep) FROM events"
        ))
        .expect("parse");
        let plan = crate::executor::execution::build_logical_plan(&catalog, &parsed).expect("plan");
        let specs = super::super::aggregate_specs(&plan);
        let controls = controls(1_000_000);
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

        // Act
        let output =
            evaluate_percentile_owned_observed(&specs[0].function, &rows, &context, |_, _| {})
                .expect("filtered percentile");
        let mut row = OutputRow::new(Vec::new(), &[], &controls).expect("row");
        row.append(output, &["pct".into(), "alias".into()])
            .expect("aliases");
        let row = row.finish();

        // Assert
        assert_eq!(
            row.get("pct"),
            Some(&Value::Json(serde_json::json!([null, null])))
        );
        assert_eq!(
            row.get("alias"),
            Some(&Value::Json(serde_json::json!([null, null])))
        );
        assert!(row.query_memory().is_some());
        drop(row);
        assert_eq!(controls.current_query_memory_bytes(), 0);
        let filtered_empty =
            evaluate_percentile_owned_observed(&specs[0].function, &rows[..1], &context, |_, _| {})
                .expect("no selected rows");
        assert_eq!(filtered_empty.into_value(), Value::Null);
        assert_eq!(controls.current_query_memory_bytes(), 0);
    }
}

#[test]
fn should_cancel_actual_percentile_null_output_while_source_leases_overlap() {
    // Arrange
    let catalog = crate::catalog::Catalog::new();
    catalog.register_collection(
        "events",
        vec![("value".into(), crate::types::DataType::Float)],
    );
    let parsed = crate::sql::parse_statement(
        "SELECT percentile_cont($1) WITHIN GROUP (ORDER BY value) FROM events",
    )
    .expect("parse");
    let plan = crate::executor::execution::build_logical_plan(&catalog, &parsed).expect("plan");
    let specs = super::super::aggregate_specs(&plan);
    let params = [Value::Array(
        crate::types::SqlArray::from_elements(vec![Value::Null, Value::Null, Value::Null])
            .expect("fractions"),
    )];
    let rows = [BatchRow::new(vec![("value".into(), Value::Float64(1.0))])];
    let cancellation = QueryCancellationHandle::new();
    let controls = QueryExecutionControls::with_cancellation(
        &CassieRuntimeLimits::default(),
        Instant::now(),
        cancellation.clone(),
    );
    let functions = std::collections::HashMap::new();
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
    let mut seen = false;

    // Act
    let result = evaluate_percentile_owned_observed(
        &specs[0].function,
        &rows,
        &context,
        |progress, index| {
            if matches!(progress, FractionProgress::ResultElement) && index == 0 {
                seen = true;
                let minimum = 3 * size_of::<Option<f64>>()
                    + size_of::<Value>()
                    + 3 * size_of::<serde_json::Value>();
                assert!(controls.current_query_memory_bytes() >= minimum);
                cancellation.cancel();
            }
        },
    );

    // Assert
    assert!(seen);
    assert!(matches!(result, Err(QueryError::General(ref message)) if message == "query canceled"));
    assert_eq!(controls.current_query_memory_bytes(), 0);
}
