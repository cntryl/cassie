use super::*;
use crate::config::CassieRuntimeLimits;
use std::sync::Arc;
use std::time::Instant;

fn controls() -> QueryExecutionControls {
    QueryExecutionControls::from_limits(&CassieRuntimeLimits::default(), Instant::now())
}

#[test]
fn should_select_direct_distinct_on_second_column_across_every_split() {
    // Arrange
    for split in 0..=5 {
        let controls = controls();
        let input = [
            Value::Null,
            Value::Int64(7),
            Value::Null,
            Value::Int64(7),
            Value::Int64(9),
        ]
        .into_iter()
        .enumerate()
        .map(|(index, key)| {
            BatchRow::new(vec![
                (
                    "payload".into(),
                    Value::Int64(i64::try_from(index).expect("small index")),
                ),
                ("n".into(), key),
            ])
        })
        .collect::<Vec<_>>();
        let mut input = input;
        let right = input.split_off(split);
        // Act
        let output = distinct_on_batches(
            vec![input, right],
            &[Expr::Column("n".into())],
            &[],
            None,
            &HashMap::new(),
            None,
            &controls,
        )
        .expect("distinct on");
        // Assert
        assert_eq!(
            crate::executor::typed_batch::relational_diagnostics::last_path(),
            Some(("distinct_on", "native_primitive_keys"))
        );
        assert_eq!(
            output
                .iter()
                .flatten()
                .map(|row| row.entries()[0].1.clone())
                .collect::<Vec<_>>(),
            vec![Value::Int64(0), Value::Int64(1), Value::Int64(4)]
        );
        assert!(output
            .iter()
            .flatten()
            .all(|row| row.operator_memory().is_some()));
        drop(output);
        assert_eq!(controls.current_query_memory_bytes(), 0);
    }
}

#[test]
fn should_preserve_shared_distinct_on_rich_and_exact_numeric_equality() {
    // Arrange
    let values = [
        Value::Null,
        Value::Int64(9_007_199_254_740_993),
        Value::Float64(9_007_199_254_740_992.0),
        Value::Int64(0),
        Value::Float64(-0.0),
        Value::Json(serde_json::json!(["\\\"".repeat(128), null])),
        Value::Json(serde_json::json!(["\\\"".repeat(128), null])),
    ];
    let input = values
        .into_iter()
        .enumerate()
        .map(|(index, value)| {
            BatchRow::new(vec![
                (
                    "payload".into(),
                    Value::Int64(i64::try_from(index).expect("small index")),
                ),
                ("n".into(), value),
            ])
        })
        .collect::<Vec<_>>();
    let expected = scalar_distinct_on_batches(
        vec![input.clone()],
        &[Expr::Column("n".into())],
        &[],
        None,
        &HashMap::new(),
        None,
        &controls(),
    )
    .expect("scalar authority");
    for split in 0..=input.len() {
        let controls = controls();
        let mut input = input.clone();
        let right = input.split_off(split);
        // Act
        let output = distinct_on_batches(
            vec![input, right],
            &[Expr::Column("n".into())],
            &[],
            None,
            &HashMap::new(),
            None,
            &controls,
        )
        .expect("rich equality");
        // Assert
        assert_eq!(
            output
                .iter()
                .flatten()
                .map(BatchRow::entries)
                .collect::<Vec<_>>(),
            expected
                .iter()
                .flatten()
                .map(BatchRow::entries)
                .collect::<Vec<_>>()
        );
        assert_eq!(
            crate::executor::typed_batch::relational_diagnostics::last_path(),
            Some(("distinct_on", "bounded_semantic_keys"))
        );
        drop(output);
        assert_eq!(controls.current_query_memory_bytes(), 0);
    }
}

fn owned_rows(
    controls: &QueryExecutionControls,
) -> (
    Vec<BatchRow>,
    Arc<crate::runtime::QueryMemoryReservation>,
    Arc<crate::runtime::QueryMemoryReservation>,
) {
    let source = Arc::new(controls.reserve_query_memory(70_000).expect("huge source"));
    let prior = Arc::new(controls.reserve_query_memory(128).expect("prior operator"));
    let rows = (0..3)
        .map(|index| {
            BatchRow::new(vec![
                (
                    "payload".into(),
                    if index == 0 {
                        Value::String("p".repeat(65_500))
                    } else {
                        Value::Null
                    },
                ),
                ("n".into(), Value::Int64(7)),
            ])
            .with_query_memory(Some(Arc::clone(&source)))
            .retain_operator_memory(controls, Arc::clone(&prior))
            .expect("prior root")
        })
        .collect();
    (rows, source, prior)
}

#[test]
fn should_retain_huge_distinct_on_parent_and_prior_owners_until_winner_drops() {
    // Arrange
    let controls = controls();
    let (input, source, prior) = owned_rows(&controls);
    // Act
    let output = distinct_on_batches(
        vec![input],
        &[Expr::Column("n".into())],
        &[],
        None,
        &HashMap::new(),
        None,
        &controls,
    )
    .expect("one winner");
    // Assert
    let peak = controls.peak_query_memory_bytes();
    assert_eq!(output[0].len(), 1);
    drop(source);
    drop(prior);
    assert!(controls.current_query_memory_bytes() >= 70_128);
    drop(output);
    assert_eq!(controls.current_query_memory_bytes(), 0);
    let limits = CassieRuntimeLimits {
        query_memory_budget_bytes: peak - 1,
        ..CassieRuntimeLimits::default()
    };
    let denied = QueryExecutionControls::from_limits(&limits, Instant::now());
    let (input, source, prior) = owned_rows(&denied);
    let completed = crate::executor::typed_batch::relational_diagnostics::last_path();
    let result = distinct_on_batches(
        vec![input],
        &[Expr::Column("n".into())],
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
    assert_eq!(
        crate::executor::typed_batch::relational_diagnostics::last_path(),
        completed
    );
    assert_eq!(denied.current_query_memory_bytes(), 70_128);
    drop(source);
    drop(prior);
    assert_eq!(denied.current_query_memory_bytes(), 0);
}

#[test]
fn should_preserve_distinct_on_expression_and_cte_scalar_boundaries() {
    // Arrange
    let statement =
        crate::sql::parse_statement("SELECT DISTINCT ON (n + 1) n").expect("expression SQL");
    let plan =
        crate::executor::execution::build_logical_plan(&crate::catalog::Catalog::new(), &statement)
            .expect("plan");
    for cte in [false, true] {
        let controls = controls();
        let controls = if cte {
            controls.for_relational_scalar_cte()
        } else {
            controls
        };
        let expressions = if cte {
            vec![Expr::Column("n".into())]
        } else {
            plan.distinct_on.clone()
        };
        let input = [1, 1, 2]
            .map(|n| BatchRow::new(vec![("n".into(), Value::Int64(n))]))
            .to_vec();
        let expected = scalar_distinct_on_batches(
            vec![input.clone()],
            &expressions,
            &[],
            None,
            &HashMap::new(),
            None,
            &controls,
        )
        .expect("scalar oracle");
        // Act
        let output = distinct_on_batches(
            vec![input],
            &expressions,
            &[],
            None,
            &HashMap::new(),
            None,
            &controls,
        )
        .expect("scalar boundary");
        // Assert
        assert_eq!(
            output
                .iter()
                .flatten()
                .map(BatchRow::entries)
                .collect::<Vec<_>>(),
            expected
                .iter()
                .flatten()
                .map(BatchRow::entries)
                .collect::<Vec<_>>()
        );
        assert_eq!(
            crate::executor::typed_batch::relational_diagnostics::last_path(),
            Some((
                "distinct_on",
                if cte {
                    "scalar_cte_materialization"
                } else {
                    "scalar_expression_distinct_on"
                }
            ))
        );
        drop(output);
        drop(expected);
        assert_eq!(controls.current_query_memory_bytes(), 0);
    }
}

#[test]
fn should_control_empty_and_cancelled_distinct_on_without_operator_allocation() {
    // Arrange
    let limits = CassieRuntimeLimits {
        query_memory_budget_bytes: 0,
        ..CassieRuntimeLimits::default()
    };
    let cancellation = crate::runtime::QueryCancellationHandle::new();
    let controls =
        QueryExecutionControls::with_cancellation(&limits, Instant::now(), cancellation.clone());
    // Act
    let output = distinct_on_batches(
        vec![Vec::new()],
        &[Expr::Column("n".into())],
        &[],
        None,
        &HashMap::new(),
        None,
        &controls,
    )
    .expect("empty selected relation");
    // Assert
    assert!(output.is_empty());
    assert_eq!(controls.current_query_memory_bytes(), 0);
    let completed = crate::executor::typed_batch::relational_diagnostics::last_path();
    cancellation.cancel();
    assert!(distinct_on_batches(
        vec![Vec::new()],
        &[Expr::Column("n".into())],
        &[],
        None,
        &HashMap::new(),
        None,
        &controls
    )
    .is_err());
    assert_eq!(
        crate::executor::typed_batch::relational_diagnostics::last_path(),
        completed
    );
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_preserve_all_null_distinct_on_declared_output_descriptor() {
    // Arrange
    let controls = controls();
    let types = Arc::new(vec![DataType::Text, DataType::BigInt]);
    let mut input = ["first", "second"].map(|name| {
        BatchRow::new(vec![
            ("payload".into(), Value::String(name.into())),
            ("n".into(), Value::Null),
        ])
    });
    for row in &mut input {
        row.set_data_types(Arc::clone(&types));
    }
    // Act
    let output = distinct_on_batches(
        vec![input.to_vec()],
        &[Expr::Column("n".into())],
        &[],
        None,
        &HashMap::new(),
        None,
        &controls,
    )
    .expect("NULL keys");
    // Assert
    assert_eq!(output[0].len(), 1);
    assert_eq!(output[0][0].entries()[0].1, Value::String("first".into()));
    assert!(Arc::ptr_eq(
        &output[0][0].shared_data_types().expect("descriptor"),
        &types
    ));
    assert_eq!(
        crate::executor::typed_batch::relational_diagnostics::last_path(),
        Some(("distinct_on", "native_primitive_keys"))
    );
    drop(output);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}
