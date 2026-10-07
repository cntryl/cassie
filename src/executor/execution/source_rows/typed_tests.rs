use super::*;
use crate::config::CassieRuntimeLimits;
use std::time::Instant;

fn controls() -> QueryExecutionControls {
    QueryExecutionControls::from_limits(&CassieRuntimeLimits::default(), Instant::now())
}

fn rows(values: &[Value]) -> Vec<BatchRow> {
    values
        .iter()
        .cloned()
        .map(|value| BatchRow::new(vec![("n".into(), value)]))
        .collect()
}

#[test]
fn should_retain_typed_distinct_output_state_across_every_input_split() {
    // Arrange
    let values = vec![
        Value::Null,
        Value::Int64(7),
        Value::Null,
        Value::Int64(7),
        Value::Int64(9),
    ];
    for split in 0..=values.len() {
        let controls = controls();
        let batches = vec![rows(&values[..split]), rows(&values[split..])];
        // Act
        let output = distinct_batches(batches, &controls).expect("distinct output");
        assert_eq!(
            crate::executor::typed_batch::relational_diagnostics::last_path(),
            Some(("distinct", "native_primitive_keys"))
        );
        // Assert
        assert!(output
            .iter()
            .flatten()
            .all(|row| row.operator_memory().is_some()));
        assert_eq!(
            output
                .iter()
                .flatten()
                .map(|row| row.entries()[0].1.clone())
                .collect::<Vec<_>>(),
            vec![Value::Null, Value::Int64(7), Value::Int64(9)]
        );
        assert!(controls.current_query_memory_bytes() > 0);
        drop(output);
        assert_eq!(controls.current_query_memory_bytes(), 0);
    }
}

#[test]
fn should_retain_selected_set_output_and_exact_cross_branch_equality() {
    // Arrange
    for (operator, expected) in [
        ("UNION", 4),
        ("UNION ALL", 7),
        ("INTERSECT", 2),
        ("EXCEPT", 1),
    ] {
        let controls = controls();
        let statement =
            crate::sql::parse_statement(&format!("SELECT 1 {operator} SELECT 1")).expect("set SQL");
        let plan = crate::executor::execution::build_logical_plan(
            &crate::catalog::Catalog::new(),
            &statement,
        )
        .expect("set plan");
        let left = rows(&[
            Value::Int64(0),
            Value::Int64(9_007_199_254_740_993),
            Value::Null,
            Value::Int64(0),
        ]);
        let right = rows(&[
            Value::Float64(-0.0),
            Value::Float64(9_007_199_254_740_992.0),
            Value::Null,
        ]);
        // Act
        let output = apply_set_operation(
            left,
            right,
            &["n".into()],
            plan.set.as_ref().expect("set"),
            &controls,
        )
        .expect("set output");
        // Assert
        assert_eq!(output.len(), expected);
        assert_eq!(
            crate::executor::typed_batch::relational_diagnostics::last_path(),
            Some(("set", "native_primitive_keys"))
        );
        assert!(output.iter().all(|row| row.operator_memory().is_some()));
        assert!(output.iter().all(|row| row.entries()[0].0 == "n"));
        assert!(controls.current_query_memory_bytes() > 0);
        drop(output);
        assert_eq!(controls.current_query_memory_bytes(), 0);
    }
}

#[test]
fn should_keep_rich_distinct_adapter_state_admitted_until_output_drops() {
    // Arrange
    let controls = controls();
    let values = vec![
        Value::Null,
        Value::Json(serde_json::json!([1, null])),
        Value::Json(serde_json::json!([1, null])),
    ];
    // Act
    let output = distinct_batches(vec![rows(&values)], &controls).expect("rich fallback");
    assert_eq!(
        crate::executor::typed_batch::relational_diagnostics::last_path(),
        Some(("distinct", "bounded_semantic_keys"))
    );
    // Assert
    assert_eq!(output[0].len(), 2);
    assert_eq!(output[0][1].entries()[0].1, values[1]);
    assert!(output
        .iter()
        .flatten()
        .all(|row| row.operator_memory().is_some()));
    assert!(controls.current_query_memory_bytes() > 0);
    drop(output);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_reject_cancelled_and_denied_distinct_flatten_before_input_handoff() {
    // Arrange
    for cancelled in [false, true] {
        let limits = CassieRuntimeLimits {
            query_memory_budget_bytes: 1,
            ..CassieRuntimeLimits::default()
        };
        let cancellation = crate::runtime::QueryCancellationHandle::new();
        let controls = QueryExecutionControls::with_cancellation(
            &limits,
            Instant::now(),
            cancellation.clone(),
        );
        let input = vec![rows(&[Value::Int64(1), Value::Int64(2)])];
        if cancelled {
            cancellation.cancel();
        }
        let before = crate::executor::typed_batch::relational_diagnostics::input_handoffs();
        let completed = crate::executor::typed_batch::relational_diagnostics::last_path();
        // Act
        let result = distinct_batches(input, &controls);
        // Assert
        assert!(result.is_err());
        assert_eq!(
            crate::executor::typed_batch::relational_diagnostics::input_handoffs(),
            before,
            "denied/cancelled input must not allocate flattened backing before admission"
        );
        assert_eq!(
            crate::executor::typed_batch::relational_diagnostics::last_path(),
            completed
        );
        assert_eq!(controls.current_query_memory_bytes(), 0);
    }
}

#[test]
fn should_admit_long_set_exported_names_before_rekeying_leased_rows() {
    // Arrange
    let limits = CassieRuntimeLimits {
        query_memory_budget_bytes: 4096,
        ..CassieRuntimeLimits::default()
    };
    let controls = QueryExecutionControls::from_limits(&limits, Instant::now());
    let origin = std::sync::Arc::new(controls.reserve_query_memory(512).expect("source"));
    let right = vec![BatchRow::new(vec![("r".into(), Value::Int64(7))])
        .with_query_memory(Some(std::sync::Arc::clone(&origin)))];
    let names = vec!["n".repeat(4096)];
    let statement = crate::sql::parse_statement("SELECT 1 UNION ALL SELECT 1").expect("set");
    let plan =
        crate::executor::execution::build_logical_plan(&crate::catalog::Catalog::new(), &statement)
            .expect("plan");
    let completed = crate::executor::typed_batch::relational_diagnostics::last_path();
    // Act
    let result = apply_set_operation(
        Vec::new(),
        right,
        &names,
        plan.set.as_ref().expect("set"),
        &controls,
    );
    // Assert
    assert!(matches!(
        result.map_err(crate::app::CassieError::from),
        Err(crate::app::CassieError::ResourceLimit(_))
    ));
    assert_eq!(
        crate::executor::typed_batch::relational_diagnostics::last_path(),
        completed
    );
    assert_eq!(controls.current_query_memory_bytes(), 512);
    drop(origin);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_preserve_all_null_distinct_output_descriptor_provenance() {
    // Arrange
    for declared in [false, true] {
        let controls = controls();
        let mut input = rows(&[Value::Null, Value::Null, Value::Null]);
        let types = std::sync::Arc::new(vec![DataType::BigInt]);
        if declared {
            for row in &mut input {
                row.set_data_types(std::sync::Arc::clone(&types));
            }
        }
        // Act
        let output = distinct_batches(vec![input], &controls).expect("all-NULL output");
        // Assert
        assert_eq!(output[0].len(), 1);
        assert_eq!(output[0][0].entries()[0].1, Value::Null);
        assert_eq!(
            crate::executor::typed_batch::relational_diagnostics::last_path(),
            Some(("distinct", "native_primitive_keys"))
        );
        if declared {
            assert_eq!(output[0][0].data_types(), &[DataType::BigInt]);
            assert!(std::sync::Arc::ptr_eq(
                &output[0][0]
                    .shared_data_types()
                    .expect("declared descriptor"),
                &types
            ));
        } else {
            assert!(output[0][0].data_types().is_empty());
        }
        drop(output);
        assert_eq!(controls.current_query_memory_bytes(), 0);
    }
}
