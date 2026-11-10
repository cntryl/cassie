//! Fraction-owner admission, ordering and cancellation witnesses.
use super::*;
use crate::config::CassieRuntimeLimits;
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
fn should_preserve_scalar_null_empty_with_both_fraction_array_carriers() {
    // Arrange
    let controls = controls(4096);
    let input = controls.reserve_query_memory(64).expect("input owner");
    let native = Value::Array(
        crate::types::SqlArray::from_elements(vec![
            Value::Float64(0.0),
            Value::Null,
            Value::Float64(0.5),
            Value::Float64(1.0),
        ])
        .expect("native array"),
    );
    let json = Value::Json(serde_json::json!([0.0, null, 0.5, 1.0]));

    // Act / Assert
    // Assert
    assert!(
        collect_fractions(&Value::Null, false, &controls, &input, |_| {})
            .expect("NULL")
            .is_none()
    );
    let scalar = collect_fractions(&Value::Float64(0.5), false, &controls, &input, |_| {})
        .expect("scalar")
        .expect("owner");
    assert!(!scalar.is_array);
    assert_eq!(scalar.values, [Some(0.5)]);
    drop(scalar);
    let empty = collect_fractions(
        &Value::Json(serde_json::json!([])),
        false,
        &controls,
        &input,
        |_| {},
    )
    .expect("empty")
    .expect("owner");
    assert!(empty.is_array);
    assert_eq!(empty.values, Vec::new());
    drop(empty);
    for value in [&native, &json] {
        let fractions = collect_fractions(value, false, &controls, &input, |_| {})
            .expect("array")
            .expect("owner");
        assert!(fractions.is_array);
        assert_eq!(fractions.values, [Some(0.0), None, Some(0.5), Some(1.0)]);
    }
    let numeric_text = Value::Json(serde_json::json!(["0.5", null]));
    let fractions = collect_fractions(&numeric_text, true, &controls, &input, |_| {})
        .expect("numeric text array")
        .expect("owner");
    assert_eq!(fractions.values, [Some(0.5), None]);
    drop(fractions);
    drop(input);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_admit_fraction_directory_with_input_owner_at_exact_budget() {
    // Arrange
    let value = Value::Json(serde_json::json!([0.0, 0.5, 1.0]));
    let input_bytes = 64;
    let output_bytes = 3 * size_of::<Option<f64>>();

    // Act / Assert
    for (budget, succeeds) in [
        (input_bytes + output_bytes, true),
        (input_bytes + output_bytes - 1, false),
    ] {
        let controls = controls(budget);
        let input = controls
            .reserve_query_memory(input_bytes)
            .expect("input owner");
        let result = collect_fractions(&value, false, &controls, &input, |_| {});
        // Assert
        assert_eq!(result.is_ok(), succeeds);
        if let Ok(Some(owner)) = result {
            assert_eq!(
                controls.current_query_memory_bytes(),
                input_bytes + output_bytes
            );
            drop(owner);
        }
        assert_eq!(controls.current_query_memory_bytes(), input_bytes);
        drop(input);
        assert_eq!(controls.current_query_memory_bytes(), 0);
    }
}

#[test]
fn should_cancel_after_an_actual_fraction_element_with_release_output() {
    // Arrange
    let cancellation = QueryCancellationHandle::new();
    let controls = QueryExecutionControls::with_cancellation(
        &CassieRuntimeLimits::default(),
        Instant::now(),
        cancellation.clone(),
    );
    let input = controls.reserve_query_memory(64).expect("input owner");
    let value = Value::Json(serde_json::json!([0.0, 0.5, 1.0]));
    let mut reached = false;

    // Act
    let result = collect_fractions(&value, false, &controls, &input, |index| {
        if index == 1 {
            reached = true;
            cancellation.cancel();
        }
    });

    // Assert
    assert!(reached);
    assert!(matches!(
        result,
        Err(QueryError::General(ref message)) if message == "query canceled"
    ));
    assert_eq!(controls.current_query_memory_bytes(), 64);
    drop(input);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_reject_foreign_fraction_tracker_before_admission() {
    // Arrange
    let owner_controls = controls(4096);
    let foreign_controls = controls(4096);
    let input = owner_controls
        .reserve_query_memory(64)
        .expect("input owner");
    let value = Value::Json(serde_json::json!([0.5]));

    // Act
    let result = collect_fractions(&value, false, &foreign_controls, &input, |_| {});

    // Assert
    assert!(matches!(
        result,
        Err(QueryError::Cassie(CassieError::ResourceLimit(_)))
    ));
    assert_eq!(owner_controls.current_query_memory_bytes(), 64);
    assert_eq!(foreign_controls.current_query_memory_bytes(), 0);
    drop(input);
    assert_eq!(owner_controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_preserve_fraction_errors_with_allow_earlier_resource_failure() {
    // Arrange
    let input_bytes = 64;
    let value = Value::Json(serde_json::json!([0.5, 1.5]));
    let output_bytes = 2 * size_of::<Option<f64>>();
    let exact = controls(input_bytes + output_bytes);
    let exact_input = exact.reserve_query_memory(input_bytes).expect("input");
    let below = controls(input_bytes + output_bytes - 1);
    let below_input = below.reserve_query_memory(input_bytes).expect("input");

    // Act
    let invalid = collect_fractions(&value, false, &exact, &exact_input, |_| {});
    let limited = collect_fractions(&value, false, &below, &below_input, |_| {});

    // Assert
    assert!(
        matches!(invalid, Err(QueryError::Cassie(CassieError::TypeInput(ref error))) if error.sqlstate == "22003")
    );
    assert!(matches!(
        limited,
        Err(QueryError::Cassie(CassieError::ResourceLimit(_)))
    ));
    assert_eq!(exact.current_query_memory_bytes(), input_bytes);
    assert_eq!(below.current_query_memory_bytes(), input_bytes);
    drop(exact_input);
    drop(below_input);
    assert_eq!(exact.current_query_memory_bytes(), 0);
    assert_eq!(below.current_query_memory_bytes(), 0);
}

#[test]
fn should_reject_first_later_malformed_with_nonfinite_fraction_elements() {
    // Arrange
    let controls = controls(4096);
    let input = controls.reserve_query_memory(64).expect("input");
    let cases = [
        Value::Json(serde_json::json!(["bad", 0.5])),
        Value::Json(serde_json::json!([0.5, "bad"])),
        Value::Array(
            crate::types::SqlArray::from_elements(vec![Value::Float64(f64::NAN)])
                .expect("native array"),
        ),
        Value::Array(
            crate::types::SqlArray::from_elements(vec![Value::Float64(f64::INFINITY)])
                .expect("native array"),
        ),
    ];

    // Act / Assert
    for (index, value) in cases.iter().enumerate() {
        let result = collect_fractions(value, false, &controls, &input, |_| {});
        if index < 2 {
            // Assert
            assert!(matches!(result, Err(QueryError::General(_))));
        } else {
            assert!(
                matches!(result, Err(QueryError::Cassie(CassieError::TypeInput(ref error))) if error.sqlstate == "22003")
            );
        }
        assert_eq!(controls.current_query_memory_bytes(), 64);
    }
    drop(input);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_admit_before_parsing_an_invalid_scalar_fraction() {
    // Arrange
    let input_bytes = 64;
    let output_bytes = size_of::<Option<f64>>();
    let exact = controls(input_bytes + output_bytes);
    let exact_input = exact.reserve_query_memory(input_bytes).expect("input");
    let below = controls(input_bytes + output_bytes - 1);
    let below_input = below.reserve_query_memory(input_bytes).expect("input");

    // Act
    let invalid = collect_fractions(&Value::Float64(1.5), false, &exact, &exact_input, |_| {});
    let limited = collect_fractions(&Value::Float64(1.5), false, &below, &below_input, |_| {});

    // Assert
    assert!(
        matches!(invalid, Err(QueryError::Cassie(CassieError::TypeInput(ref error))) if error.sqlstate == "22003")
    );
    assert!(matches!(
        limited,
        Err(QueryError::Cassie(CassieError::ResourceLimit(_)))
    ));
    drop(exact_input);
    drop(below_input);
    assert_eq!(exact.current_query_memory_bytes(), 0);
    assert_eq!(below.current_query_memory_bytes(), 0);
}

#[test]
fn should_allow_cancellation_to_precede_a_later_invalid_fraction() {
    // Arrange
    let cancellation = QueryCancellationHandle::new();
    let controls = QueryExecutionControls::with_cancellation(
        &CassieRuntimeLimits::default(),
        Instant::now(),
        cancellation.clone(),
    );
    let input = controls.reserve_query_memory(64).expect("input");
    let value = Value::Json(serde_json::json!([0.5, 1.5]));

    // Act
    let result = collect_fractions(&value, false, &controls, &input, |index| {
        if index == 0 {
            cancellation.cancel();
        }
    });

    // Assert
    assert!(matches!(
        result,
        Err(QueryError::General(ref message)) if message == "query canceled"
    ));
    assert_eq!(controls.current_query_memory_bytes(), 64);
    drop(input);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_report_invalid_fraction_before_reaching_sorted_value_errors() {
    // Arrange
    let catalog = crate::catalog::Catalog::new();
    catalog.register_collection(
        "events",
        vec![("value".to_owned(), crate::types::DataType::Text)],
    );
    let parsed = crate::sql::parse_statement(
        "SELECT percentile_cont(1.5) WITHIN GROUP (ORDER BY value::FLOAT8) FROM events",
    )
    .expect("parse");
    let plan = crate::executor::execution::build_logical_plan(&catalog, &parsed).expect("plan");
    let specs = super::super::aggregate_specs(&plan);
    let controls = controls(4096);
    let functions = std::collections::HashMap::new();
    let context = AggregateExecutionContext {
        plan: &plan,
        params: &[],
        search_context: None,
        user_functions: &functions,
        session: None,
        controls: &controls,
        after_partition_row: None,
    };
    let rows = [BatchRow::new(vec![(
        "value".to_owned(),
        Value::String("not-a-float".to_owned()),
    )])];

    // Act
    let result = evaluate_percentile(&specs[0].function, &rows, &context);

    // Assert
    assert!(
        matches!(result, Err(QueryError::Cassie(CassieError::TypeInput(ref error))) if error.sqlstate == "22003")
    );
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_retain_fraction_owner_during_actual_sort_with_release_on_cancel_or_empty() {
    // Arrange
    let catalog = crate::catalog::Catalog::new();
    catalog.register_collection(
        "events",
        vec![("value".to_owned(), crate::types::DataType::Float)],
    );
    let parsed = crate::sql::parse_statement(
        "SELECT percentile_cont($1) WITHIN GROUP (ORDER BY value) FROM events",
    )
    .expect("parse");
    let plan = crate::executor::execution::build_logical_plan(&catalog, &parsed).expect("plan");
    let specs = super::super::aggregate_specs(&plan);
    let params = [Value::Array(
        crate::types::SqlArray::from_elements(vec![
            Value::Float64(0.25),
            Value::Float64(0.5),
            Value::Float64(0.75),
        ])
        .expect("fractions"),
    )];
    let rows = [3.0, 1.0, 2.0]
        .map(|value| BatchRow::new(vec![("value".to_owned(), Value::Float64(value))]));
    let functions = std::collections::HashMap::new();
    for cancel_on_sort in [false, true] {
        let cancellation = QueryCancellationHandle::new();
        let controls = QueryExecutionControls::with_cancellation(
            &CassieRuntimeLimits::default(),
            Instant::now(),
            cancellation.clone(),
        );
        let context = AggregateExecutionContext {
            plan: &plan,
            params: &params,
            search_context: None,
            user_functions: &functions,
            session: None,
            controls: &controls,
            after_partition_row: None,
        };
        let mut observed = false;
        let input_rows = if cancel_on_sort { rows.as_slice() } else { &[] };

        // Act
        let result = evaluate_percentile_observed(
            &specs[0].function,
            input_rows,
            &context,
            |progress, bytes| {
                match progress {
                    FractionProgress::Ready if !cancel_on_sort => {}
                    FractionProgress::SortComparison if cancel_on_sort => {}
                    _ => return,
                }
                observed = true;
                assert_eq!(bytes, 3 * size_of::<Option<f64>>());
                assert!(controls.current_query_memory_bytes() >= bytes);
                if cancel_on_sort {
                    cancellation.cancel();
                }
            },
        );

        // Assert
        assert!(observed, "actual evaluation boundary reached");
        if cancel_on_sort {
            assert!(matches!(
                result,
                Err(QueryError::General(ref message)) if message == "query canceled"
            ));
        } else {
            assert!(matches!(result, Ok(Value::Null)));
        }
        assert_eq!(controls.current_query_memory_bytes(), 0);
    }
}

#[test]
fn should_cancel_actual_percentile_evaluation_during_fraction_collection() {
    // Arrange
    let catalog = crate::catalog::Catalog::new();
    catalog.register_collection(
        "events",
        vec![("value".to_owned(), crate::types::DataType::Float)],
    );
    let parsed = crate::sql::parse_statement(
        "SELECT percentile_cont($1) WITHIN GROUP (ORDER BY value) FROM events",
    )
    .expect("parse");
    let plan = crate::executor::execution::build_logical_plan(&catalog, &parsed).expect("plan");
    let specs = super::super::aggregate_specs(&plan);
    let params = [Value::Array(
        crate::types::SqlArray::from_elements(vec![Value::Float64(0.5), Value::Float64(1.5)])
            .expect("fractions"),
    )];
    let rows = [BatchRow::new(vec![(
        "value".to_owned(),
        Value::Float64(1.0),
    )])];
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
        search_context: None,
        user_functions: &functions,
        session: None,
        controls: &controls,
        after_partition_row: None,
    };
    let mut observed = false;

    // Act
    let result =
        evaluate_percentile_observed(&specs[0].function, &rows, &context, |progress, index| {
            if matches!(progress, FractionProgress::Element) && index == 0 {
                observed = true;
                cancellation.cancel();
            }
        });

    // Assert
    assert!(observed);
    assert!(matches!(
        result,
        Err(QueryError::General(ref message)) if message == "query canceled"
    ));
    assert_eq!(controls.current_query_memory_bytes(), 0);
}
