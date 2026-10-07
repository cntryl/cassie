use super::*;
use crate::config::CassieRuntimeLimits;
use crate::executor::typed_batch::relational_diagnostics;
use crate::types::DataType;
use std::sync::Arc;
use std::time::Instant;

fn controls() -> QueryExecutionControls {
    QueryExecutionControls::from_limits(&CassieRuntimeLimits::default(), Instant::now())
}

fn projection(sql: &str) -> Vec<SelectItem> {
    let statement = crate::sql::parse_statement(sql).expect("window SQL");
    crate::planner::logical::plan(&crate::sql::binder::BoundStatement {
        statement,
        indexes: Vec::new(),
    })
    .expect("window projection")
    .projection
}

fn rows() -> Vec<BatchRow> {
    let types = Arc::new(vec![
        DataType::BigInt,
        DataType::BigInt,
        DataType::Array(Box::new(DataType::Int)),
    ]);
    [
        (1, Some(2)),
        (2, None),
        (1, Some(1)),
        (1, Some(2)),
        (2, Some(3)),
    ]
    .into_iter()
    .enumerate()
    .map(|(index, (partition, order))| {
        BatchRow::new(vec![
            ("p".into(), Value::Int64(partition)),
            ("n".into(), order.map_or(Value::Null, Value::Int64)),
            (
                "payload".into(),
                if index == 3 {
                    Value::Null
                } else {
                    Value::Json(serde_json::json!([index, null]))
                },
            ),
        ])
        .with_optional_data_types(Some(Arc::clone(&types)))
    })
    .collect()
}

fn scalar_oracle(projection: &[SelectItem]) -> Vec<BatchRow> {
    scalar_oracle_on(rows(), projection)
}

fn scalar_oracle_on(mut rows: Vec<BatchRow>, projection: &[SelectItem]) -> Vec<BatchRow> {
    let controls = controls();
    let functions = HashMap::new();
    let context = WindowExecutionContext {
        params: &[],
        search_context: None,
        user_functions: &functions,
        session: None,
        controls: &controls,
    };
    for (function, alias) in collect_window_functions(projection) {
        apply_single_window(&mut rows, function, alias.as_deref(), &context)
            .expect("existing scalar frame authority");
    }
    assert_eq!(controls.current_query_memory_bytes(), 0);
    rows
}

fn assert_selected(projection: &[SelectItem], path: &'static str) {
    let expected = scalar_oracle(projection);
    for split in 0..=5 {
        let controls = controls();
        let parent = Arc::new(controls.reserve_query_memory(4096).expect("source parent"));
        let mut input = rows()
            .into_iter()
            .map(|row| row.with_query_memory(Some(Arc::clone(&parent))))
            .collect::<Vec<_>>();
        let right = input.split_off(split);
        // Act
        let output = apply_window_functions(
            vec![input, right],
            projection,
            &[],
            None,
            &HashMap::new(),
            None,
            &controls,
        )
        .expect("selected windows");
        drop(parent);
        // Assert
        let actual = output.iter().flatten().collect::<Vec<_>>();
        assert_eq!(actual.len(), expected.len());
        for (actual, expected) in actual.iter().zip(&expected) {
            assert_eq!(actual.entries(), expected.entries());
            assert_eq!(actual.data_types(), expected.data_types());
            assert!(actual.operator_memory().is_some());
        }
        assert_eq!(relational_diagnostics::last_path(), Some(("window", path)));
        assert!(controls.current_query_memory_bytes() > 4096);
        drop(output);
        assert_eq!(controls.current_query_memory_bytes(), 0);
    }
}

#[test]
fn should_compute_typed_ranking_across_partitions_peers_and_input_splits() {
    // Arrange
    let projection = projection("SELECT ROW_NUMBER() OVER (PARTITION BY p ORDER BY n NULLS LAST) AS rn, RANK() OVER (PARTITION BY p ORDER BY n NULLS LAST) AS r, DENSE_RANK() OVER (PARTITION BY p ORDER BY n NULLS LAST) AS d");
    // Act
    // Assert
    assert_selected(&projection, "native_primitive_keys");
}

#[test]
fn should_preserve_selected_value_window_frames_and_scalar_backed_payloads() {
    // Arrange
    let projection = projection("SELECT FIRST_VALUE(payload) OVER (PARTITION BY p ORDER BY n ROWS BETWEEN 1 FOLLOWING AND 1 FOLLOWING) AS fv, LAST_VALUE(payload) OVER (PARTITION BY p ORDER BY n) AS lv, LAG(payload) OVER (PARTITION BY p ORDER BY n) AS lagged, LEAD(payload) OVER (PARTITION BY p ORDER BY n) AS led");
    // Act
    // Assert
    assert_selected(&projection, "native_primitive_keys_scalar_backed_payload");
}

#[test]
fn should_preserve_unknown_value_argument_descriptor_after_known_input_fields() {
    // Arrange
    let projection =
        projection("SELECT FIRST_VALUE(payload) OVER (PARTITION BY p ORDER BY n) AS fv");
    let input = rows()
        .into_iter()
        .map(|row| {
            row.with_optional_data_types(Some(Arc::new(vec![DataType::BigInt, DataType::BigInt])))
        })
        .collect::<Vec<_>>();
    let mut expected = input.clone();
    let oracle_controls = controls();
    let functions = HashMap::new();
    let context = WindowExecutionContext {
        params: &[],
        search_context: None,
        user_functions: &functions,
        session: None,
        controls: &oracle_controls,
    };
    for (function, alias) in collect_window_functions(&projection) {
        apply_single_window(&mut expected, function, alias.as_deref(), &context)
            .expect("unknown scalar descriptor authority");
    }
    let controls = controls();
    // Act
    let output = apply_window_functions(
        vec![input],
        &projection,
        &[],
        None,
        &functions,
        None,
        &controls,
    )
    .expect("selected unknown payload");
    // Assert
    for (actual, expected) in output.iter().flatten().zip(&expected) {
        assert_eq!(actual.entries(), expected.entries());
        assert_eq!(actual.data_types(), expected.data_types());
    }
    drop(output);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_match_shared_window_peers_for_mixed_numeric_keys_and_identity_ties() {
    // Arrange
    let projection = projection("SELECT ROW_NUMBER() OVER (PARTITION BY p ORDER BY n NULLS LAST) AS rn, RANK() OVER (PARTITION BY p ORDER BY n NULLS LAST) AS r, DENSE_RANK() OVER (PARTITION BY p ORDER BY n NULLS LAST) AS d, LAST_VALUE(payload) OVER (PARTITION BY p ORDER BY n NULLS LAST) AS lv");
    let input = [
        (Value::Null, Value::Int64(0), "a"),
        (Value::Null, Value::Float64(-0.0), "b"),
        (Value::Int64(1), Value::Int64(9_007_199_254_740_993), "c"),
        (
            Value::Float64(1.0),
            Value::Float64(9_007_199_254_740_992.0),
            "d",
        ),
        (
            Value::Int64(1),
            Value::Float64(9_007_199_254_740_992.0),
            "e",
        ),
    ]
    .into_iter()
    .map(|(partition, order, payload)| {
        BatchRow::new(vec![
            ("p".into(), partition),
            ("n".into(), order),
            ("payload".into(), Value::String(payload.into())),
        ])
    })
    .collect::<Vec<_>>();
    let expected = scalar_oracle_on(input.clone(), &projection);
    let controls = controls();
    // Act
    let output = apply_window_functions(
        vec![input],
        &projection,
        &[],
        None,
        &HashMap::new(),
        None,
        &controls,
    )
    .expect("shared exact peer authority");
    // Assert
    for (actual, expected) in output.iter().flatten().zip(&expected) {
        assert_eq!(actual.entries(), expected.entries());
        assert_eq!(actual.data_types(), expected.data_types());
    }
    assert_eq!(
        relational_diagnostics::last_path(),
        Some(("window", "bounded_semantic_keys_scalar_backed_payload"))
    );
    drop(output);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

fn owned_rows(
    controls: &QueryExecutionControls,
) -> (
    Vec<BatchRow>,
    Arc<crate::runtime::QueryMemoryReservation>,
    Arc<crate::runtime::QueryMemoryReservation>,
) {
    let parent = Arc::new(controls.reserve_query_memory(4096).expect("source body"));
    let prior = Arc::new(controls.reserve_query_memory(2048).expect("prior operator"));
    let rows = rows()
        .into_iter()
        .map(|row| {
            row.with_query_memory(Some(Arc::clone(&parent)))
                .retain_operator_memory(controls, Arc::clone(&prior))
                .expect("prior owner chain")
        })
        .collect();
    (rows, parent, prior)
}

#[test]
fn should_release_prior_and_source_window_owners_after_budget_denial() {
    // Arrange
    let projection = projection("SELECT FIRST_VALUE(payload) OVER (PARTITION BY p ORDER BY n ROWS BETWEEN 1 FOLLOWING AND 1 FOLLOWING) AS fv, LAST_VALUE(payload) OVER (PARTITION BY p ORDER BY n) AS lv, LAG(payload) OVER (PARTITION BY p ORDER BY n) AS lagged");
    let controls = controls();
    let (input, parent, prior) = owned_rows(&controls);
    // Act
    let output = apply_window_functions(
        vec![input],
        &projection,
        &[],
        None,
        &HashMap::new(),
        None,
        &controls,
    )
    .expect("retained window owners");
    // Assert
    let peak = controls.peak_query_memory_bytes();
    drop(output);
    assert_eq!(controls.current_query_memory_bytes(), 6144);
    drop(parent);
    drop(prior);
    assert_eq!(controls.current_query_memory_bytes(), 0);
    let limits = CassieRuntimeLimits {
        query_memory_budget_bytes: peak - 1,
        ..CassieRuntimeLimits::default()
    };
    let denied = QueryExecutionControls::from_limits(&limits, Instant::now());
    let (input, parent, prior) = owned_rows(&denied);
    let completed = relational_diagnostics::last_path();
    let result = apply_window_functions(
        vec![input],
        &projection,
        &[],
        None,
        &HashMap::new(),
        None,
        &denied,
    );
    assert!(matches!(
        result.map_err(crate::app::CassieError::from),
        Err(crate::app::CassieError::ResourceLimit(_))
    ));
    assert_eq!(relational_diagnostics::last_path(), completed);
    assert_eq!(denied.current_query_memory_bytes(), 6144);
    drop(parent);
    drop(prior);
    assert_eq!(denied.current_query_memory_bytes(), 0);
}

#[test]
fn should_preserve_unselected_window_expression_frame_and_cte_boundaries() {
    // Arrange
    for (sql, cte, path) in [
        ("SELECT FIRST_VALUE(n + 1) OVER (PARTITION BY p ORDER BY n) AS v", false, "scalar_expression_window"),
        ("SELECT LAST_VALUE(payload) OVER (PARTITION BY p ORDER BY n GROUPS BETWEEN CURRENT ROW AND CURRENT ROW EXCLUDE TIES) AS v", false, "scalar_window_frame"),
        ("SELECT RANK() OVER (PARTITION BY p ORDER BY n) AS v", true, "scalar_cte_materialization"),
    ] {
        let projection = projection(sql);
        let expected = scalar_oracle(&projection);
        let controls = controls();
        let controls = if cte { controls.for_relational_scalar_cte() } else { controls };
        // Act
        let output = apply_window_functions(vec![rows()], &projection, &[], None, &HashMap::new(), None, &controls).expect("preserved scalar boundary");
        // Assert
        for (actual, expected) in output.iter().flatten().zip(&expected) {
            assert_eq!(actual.entries(), expected.entries());
            assert_eq!(actual.data_types(), expected.data_types());
        }
        assert_eq!(relational_diagnostics::last_path(), Some(("window", path)));
        drop(output);
        assert_eq!(controls.current_query_memory_bytes(), 0);
    }
}

#[test]
fn should_cancel_selected_windows_before_transferring_prior_and_source_owners() {
    // Arrange
    let cancellation = crate::runtime::QueryCancellationHandle::new();
    let controls = QueryExecutionControls::with_cancellation(
        &CassieRuntimeLimits::default(),
        Instant::now(),
        cancellation.clone(),
    );
    let (input, parent, prior) = owned_rows(&controls);
    let projection = projection("SELECT LAST_VALUE(payload) OVER (PARTITION BY p ORDER BY n) AS v");
    let completed = relational_diagnostics::last_path();
    cancellation.cancel();
    // Act
    let result = apply_window_functions(
        vec![input],
        &projection,
        &[],
        None,
        &HashMap::new(),
        None,
        &controls,
    );
    // Assert
    assert!(matches!(
        result.map_err(crate::app::CassieError::from),
        Err(crate::app::CassieError::QueryCancelled)
    ));
    assert_eq!(relational_diagnostics::last_path(), completed);
    assert_eq!(controls.current_query_memory_bytes(), 6144);
    drop(parent);
    drop(prior);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}
