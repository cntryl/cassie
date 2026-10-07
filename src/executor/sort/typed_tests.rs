use super::*;
use crate::config::CassieRuntimeLimits;
use crate::executor::batch::BatchRow;
use crate::executor::typed_batch::relational_diagnostics;
use std::sync::Arc;
use std::time::Instant;

fn controls() -> QueryExecutionControls {
    QueryExecutionControls::from_limits(&CassieRuntimeLimits::default(), Instant::now())
}

fn rows() -> Vec<BatchRow> {
    [Some(3.0), None, Some(-0.0), Some(3.0), Some(0.0)]
        .into_iter()
        .enumerate()
        .map(|(index, value)| {
            BatchRow::new(vec![
                ("n".into(), value.map_or(Value::Null, Value::Float64)),
                ("payload".into(), Value::String(format!("lane{index}"))),
            ])
        })
        .collect()
}

fn order() -> Vec<OrderExpr> {
    vec![OrderExpr {
        expr: Expr::Column("n".into()),
        direction: SortDirection::Asc,
        nulls: Some(NullsOrder::First),
    }]
}

fn entries(batches: &[Batch]) -> Vec<Vec<(String, Value)>> {
    batches
        .iter()
        .flatten()
        .map(|row| row.entries().to_vec())
        .collect()
}

#[test]
fn should_select_native_primitive_sort_and_retain_output_backing() {
    // Arrange
    let order = order();
    let functions = HashMap::new();
    let eval = EvalInput {
        order: &order,
        projection: &[],
        params: &[],
        search_context: None,
        user_functions: &functions,
        session: None,
    };
    let expected = maintain_top_k_kernel(rows(), &eval, 5).expect("scalar ordering oracle");
    let expected = expected
        .iter()
        .map(|row| row.entries().to_vec())
        .collect::<Vec<_>>();
    for split in 0..=5 {
        let controls = controls();
        let mut input = rows();
        let right = input.split_off(split);
        // Act
        let output =
            sort_batches_with_controls(vec![input, right], &eval, &controls).expect("native sort");
        // Assert
        assert_eq!(entries(&output), expected);
        assert_eq!(
            relational_diagnostics::last_path(),
            Some(("sort", "native_primitive_keys"))
        );
        assert!(output
            .iter()
            .flatten()
            .all(|row| row.operator_memory().is_some()));
        assert!(controls.current_query_memory_bytes() > 0);
        drop(output);
        assert_eq!(controls.current_query_memory_bytes(), 0);
    }
}

#[test]
fn should_match_native_top_k_prefix_and_preserve_large_parent_lease() {
    // Arrange
    let order = order();
    let functions = HashMap::new();
    let eval = EvalInput {
        order: &order,
        projection: &[],
        params: &[],
        search_context: None,
        user_functions: &functions,
        session: None,
    };
    for count in [1, 3, 7] {
        let controls = controls();
        let parent = Arc::new(
            controls
                .reserve_query_memory(65_536)
                .expect("large shared parent"),
        );
        let input = rows()
            .into_iter()
            .map(|row| row.with_query_memory(Some(Arc::clone(&parent))))
            .collect::<Vec<_>>();
        let expected = maintain_top_k_kernel(rows(), &eval, count).expect("scalar prefix oracle");
        // Act
        let output =
            top_k_batches_with_controls(vec![input], &eval, count, &controls).expect("native heap");
        drop(parent);
        // Assert
        assert_eq!(
            entries(&output),
            expected
                .iter()
                .map(|row| row.entries().to_vec())
                .collect::<Vec<_>>()
        );
        assert_eq!(
            relational_diagnostics::last_path(),
            Some(("top_k", "native_primitive_keys"))
        );
        assert!(output
            .iter()
            .flatten()
            .all(|row| row.operator_memory().is_some()));
        assert!(
            controls.current_query_memory_bytes() > 65_536,
            "output backing and full shared source parent remain charged"
        );
        drop(output);
        assert_eq!(controls.current_query_memory_bytes(), 0);
    }
}

#[test]
fn should_preserve_quoted_and_unquoted_passthrough_alias_order() {
    // Arrange
    for sql in [
        "SELECT n AS \"Mixed\" ORDER BY \"Mixed\" DESC NULLS LAST",
        "SELECT n AS mixed ORDER BY MIXED DESC NULLS FIRST",
    ] {
        let statement = crate::sql::parse_statement(sql).expect("alias SQL");
        let plan = crate::planner::logical::plan(&crate::sql::binder::BoundStatement {
            statement,
            indexes: Vec::new(),
        })
        .expect("alias plan");
        let functions = HashMap::new();
        let eval = EvalInput {
            order: &plan.order,
            projection: &plan.projection,
            params: &[],
            search_context: None,
            user_functions: &functions,
            session: None,
        };
        let expected = maintain_top_k_kernel(rows(), &eval, 5).expect("alias scalar oracle");
        let controls = controls();
        // Act
        let output =
            sort_batches_with_controls(vec![rows()], &eval, &controls).expect("alias native order");
        // Assert
        assert_eq!(
            entries(&output),
            expected
                .iter()
                .map(|row| row.entries().to_vec())
                .collect::<Vec<_>>()
        );
        assert_eq!(
            relational_diagnostics::last_path(),
            Some(("sort", "native_primitive_keys"))
        );
        drop(output);
        assert_eq!(controls.current_query_memory_bytes(), 0);
    }
}

#[test]
fn should_keep_mixed_exact_numeric_keys_in_admitted_semantic_order() {
    // Arrange
    let input = vec![
        Value::Null,
        Value::Int64(9_007_199_254_740_993),
        Value::Float64(9_007_199_254_740_992.0),
    ]
    .into_iter()
    .map(|value| BatchRow::new(vec![("n".into(), value)]))
    .collect::<Vec<_>>();
    let order = order();
    let functions = HashMap::new();
    let eval = EvalInput {
        order: &order,
        projection: &[],
        params: &[],
        search_context: None,
        user_functions: &functions,
        session: None,
    };
    let expected =
        maintain_top_k_kernel(input.clone(), &eval, 3).expect("exact mixed scalar order");
    let controls = controls();
    // Act
    let output =
        sort_batches_with_controls(vec![input], &eval, &controls).expect("bounded mixed adapter");
    // Assert
    assert_eq!(
        entries(&output),
        expected
            .iter()
            .map(|row| row.entries().to_vec())
            .collect::<Vec<_>>()
    );
    assert_eq!(
        relational_diagnostics::last_path(),
        Some(("sort", "bounded_semantic_keys"))
    );
    drop(output);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_deny_small_winner_with_insufficient_parent_overlap_budget() {
    // Arrange
    let limits = CassieRuntimeLimits {
        query_memory_budget_bytes: 65_536 + 128,
        ..CassieRuntimeLimits::default()
    };
    let controls = QueryExecutionControls::from_limits(&limits, Instant::now());
    let parent = Arc::new(controls.reserve_query_memory(65_536).expect("parent lease"));
    let input = rows()
        .into_iter()
        .map(|row| row.with_query_memory(Some(Arc::clone(&parent))))
        .collect::<Vec<_>>();
    let order = order();
    let functions = HashMap::new();
    let eval = EvalInput {
        order: &order,
        projection: &[],
        params: &[],
        search_context: None,
        user_functions: &functions,
        session: None,
    };
    let completed = relational_diagnostics::last_path();
    // Act
    let result = top_k_batches_with_controls(vec![input], &eval, 1, &controls);
    // Assert
    assert!(matches!(
        result.map_err(crate::app::CassieError::from),
        Err(crate::app::CassieError::ResourceLimit(_))
    ));
    assert_eq!(relational_diagnostics::last_path(), completed);
    assert_eq!(controls.current_query_memory_bytes(), 65_536);
    drop(parent);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_preserve_expression_order_in_named_scalar_boundary() {
    // Arrange
    let statement = crate::sql::parse_statement("SELECT n + 1 AS computed ORDER BY computed DESC")
        .expect("expression SQL");
    let plan = crate::planner::logical::plan(&crate::sql::binder::BoundStatement {
        statement,
        indexes: Vec::new(),
    })
    .expect("expression plan");
    let functions = HashMap::new();
    let eval = EvalInput {
        order: &plan.order,
        projection: &plan.projection,
        params: &[],
        search_context: None,
        user_functions: &functions,
        session: None,
    };
    let expected = maintain_top_k_kernel(rows(), &eval, 3).expect("expression scalar oracle");
    let controls = controls();
    // Act
    let output = top_k_batches_with_controls(vec![rows()], &eval, 3, &controls)
        .expect("expression boundary");
    // Assert
    assert_eq!(
        entries(&output),
        expected
            .iter()
            .map(|row| row.entries().to_vec())
            .collect::<Vec<_>>()
    );
    assert_eq!(
        relational_diagnostics::last_path(),
        Some(("top_k", "scalar_expression_order"))
    );
    drop(output);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

fn rich_rows() -> Vec<BatchRow> {
    let types = Arc::new(vec![crate::types::DataType::Array(Box::new(
        crate::types::DataType::Json,
    ))]);
    [
        Value::Null,
        Value::Json(serde_json::json!(["escaped\n\"\\".repeat(256), {"nested": [1, null, true]}])),
        Value::Json(serde_json::json!(["short", {"nested": [2, null, false]}])),
    ]
    .into_iter()
    .map(|value| {
        BatchRow::new(vec![("n".into(), value)]).with_optional_data_types(Some(Arc::clone(&types)))
    })
    .collect()
}

#[test]
fn should_admit_borrowed_rich_array_keys_and_reject_near_budget_overlap() {
    // Arrange
    let order = order();
    let functions = HashMap::new();
    let eval = EvalInput {
        order: &order,
        projection: &[],
        params: &[],
        search_context: None,
        user_functions: &functions,
        session: None,
    };
    let expected = maintain_top_k_kernel(rich_rows(), &eval, 3).expect("ARRAY scalar authority");
    let controls = controls();
    let input = rich_rows();
    let descriptor = input[0].shared_data_types().expect("original descriptor");
    // Act
    let output =
        sort_batches_with_controls(vec![input], &eval, &controls).expect("borrowed rich adapter");
    // Assert
    assert_eq!(
        entries(&output),
        expected
            .iter()
            .map(|row| row.entries().to_vec())
            .collect::<Vec<_>>()
    );
    assert_eq!(
        relational_diagnostics::last_path(),
        Some(("sort", "bounded_semantic_keys"))
    );
    assert!(output.iter().flatten().all(|row| Arc::ptr_eq(
        &descriptor,
        &row.shared_data_types().expect("unchanged descriptor")
    )));
    let peak = controls.peak_query_memory_bytes();
    drop(output);
    assert_eq!(controls.current_query_memory_bytes(), 0);
    let limits = CassieRuntimeLimits {
        query_memory_budget_bytes: peak - 1,
        ..CassieRuntimeLimits::default()
    };
    let denied = QueryExecutionControls::from_limits(&limits, Instant::now());
    let completed = relational_diagnostics::last_path();
    let result = sort_batches_with_controls(vec![rich_rows()], &eval, &denied);
    assert!(matches!(
        result.map_err(crate::app::CassieError::from),
        Err(crate::app::CassieError::ResourceLimit(_))
    ));
    assert_eq!(relational_diagnostics::last_path(), completed);
    assert_eq!(denied.current_query_memory_bytes(), 0);
}

#[test]
fn should_cancel_native_ordering_before_transferring_source_parent() {
    // Arrange
    let order = order();
    let functions = HashMap::new();
    let eval = EvalInput {
        order: &order,
        projection: &[],
        params: &[],
        search_context: None,
        user_functions: &functions,
        session: None,
    };
    for heap in [false, true] {
        let cancellation = crate::runtime::QueryCancellationHandle::new();
        let controls = QueryExecutionControls::with_cancellation(
            &CassieRuntimeLimits::default(),
            Instant::now(),
            cancellation.clone(),
        );
        let parent = Arc::new(controls.reserve_query_memory(128).expect("source parent"));
        let input = rows()
            .into_iter()
            .map(|row| row.with_query_memory(Some(Arc::clone(&parent))))
            .collect();
        let completed = relational_diagnostics::last_path();
        cancellation.cancel();
        // Act
        let result = if heap {
            top_k_batches_with_controls(vec![input], &eval, 1, &controls)
        } else {
            sort_batches_with_controls(vec![input], &eval, &controls)
        };
        // Assert
        assert!(matches!(
            result.map_err(crate::app::CassieError::from),
            Err(crate::app::CassieError::QueryCancelled)
        ));
        assert_eq!(relational_diagnostics::last_path(), completed);
        assert_eq!(controls.current_query_memory_bytes(), 128);
        drop(parent);
        assert_eq!(controls.current_query_memory_bytes(), 0);
    }
}
