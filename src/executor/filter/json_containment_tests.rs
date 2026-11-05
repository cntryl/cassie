//! Containment owns every live parser frame before allocation.

use super::*;
use crate::config::CassieRuntimeLimits;
use crate::runtime::{QueryCancellationHandle, QueryExecutionControls};
use std::time::Instant;

fn controls(bytes: usize) -> QueryExecutionControls {
    QueryExecutionControls::from_limits(
        &CassieRuntimeLimits {
            query_memory_budget_bytes: bytes,
            ..CassieRuntimeLimits::default()
        },
        Instant::now(),
    )
}

fn contains() -> BinaryOp {
    BinaryOp::Typed {
        operator: TypedOperator::ContainsOrEqual,
    }
}

#[test]
fn should_admit_containment_parser_frames_before_decoding() {
    // Arrange
    let document = ScalarValue::Json(r#"{"a":[1,2,3],"b":{"k":"value"}}"#.into());
    let required = ScalarValue::Json(r#"{"b":{"k":"value"}}"#.into());

    for (budget, accepted) in [(128, false), (128 * 1024, true)] {
        let controls = controls(budget);

        // Act
        let result = evaluate_with_controls(&contains(), &document, &required, Some(&controls));

        // Assert
        assert_eq!(result.is_ok(), accepted);
        if accepted {
            assert!(matches!(result, Ok(ScalarValue::Bool(true))));
        } else {
            assert!(matches!(
                result,
                Err(QueryError::Cassie(CassieError::ResourceLimit(_)))
            ));
        }
        assert_eq!(controls.current_query_memory_bytes(), 0);
    }
}

#[test]
fn should_bound_expanded_numeric_coefficients_before_comparing_them() {
    // Arrange
    let number = ScalarValue::Json("1e400".into());

    for (budget, accepted) in [(4096, false), (64 * 1024, true)] {
        let controls = controls(budget);

        // Act
        let result = evaluate_with_controls(&contains(), &number, &number, Some(&controls));

        // Assert
        assert_eq!(result.is_ok(), accepted);
        if accepted {
            assert!(matches!(result, Ok(ScalarValue::Bool(true))));
        } else {
            assert!(matches!(
                result,
                Err(QueryError::Cassie(CassieError::ResourceLimit(_)))
            ));
        }
        assert_eq!(controls.current_query_memory_bytes(), 0);
    }
}

#[test]
fn should_preserve_unvisited_numeric_values_in_legacy_json() {
    // Arrange
    let document = ScalarValue::Json(r#"{"unvisited":1e999999}"#.into());
    let required = ScalarValue::Json("{}".into());
    let controls = controls(128 * 1024);

    // Act
    let result = evaluate_with_controls(&contains(), &document, &required, Some(&controls));

    // Assert
    assert!(matches!(result, Ok(ScalarValue::Bool(true))));
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_preserve_deep_raw_json_with_iterative_owned_frames() {
    // Arrange
    let text = format!("{}1{}", "[".repeat(180), "]".repeat(180));
    let document = ScalarValue::Json(text);

    for (budget, accepted) in [(256 * 1024, false), (32 * 1024 * 1024, true)] {
        let controls = controls(budget);

        // Act
        let result = evaluate_with_controls(&contains(), &document, &document, Some(&controls));

        // Assert
        assert_eq!(result.is_ok(), accepted);
        if accepted {
            assert!(matches!(result, Ok(ScalarValue::Bool(true))));
        } else {
            assert!(matches!(
                result,
                Err(QueryError::Cassie(CassieError::ResourceLimit(_)))
            ));
        }
        assert_eq!(controls.current_query_memory_bytes(), 0);
    }
}

#[test]
fn should_release_containment_frames_after_actual_scalar_cancellation() {
    // Arrange
    let document = ScalarValue::Json("[1,2]".into());
    for (required, first_comparison) in [("[1]", true), ("[9]", false)] {
        let cancellation = QueryCancellationHandle::new();
        let controls = QueryExecutionControls::with_cancellation(
            &CassieRuntimeLimits::default(),
            Instant::now(),
            cancellation.clone(),
        );
        let required = ScalarValue::Json(required.into());
        let mut observations = Vec::new();

        // Act
        let result = evaluate_observing(
            &contains(),
            &document,
            &required,
            Some(&controls),
            |matched| {
                observations.push((matched, controls.current_query_memory_bytes()));
                cancellation.cancel();
            },
        );

        // Assert
        assert!(matches!(
            result,
            Err(QueryError::Cassie(CassieError::QueryCancelled))
        ));
        assert_eq!(observations.len(), 1);
        assert_eq!(observations[0].0, first_comparison);
        assert!(observations[0].1 > 0);
        assert_eq!(controls.current_query_memory_bytes(), 0);
    }
}

#[test]
fn should_return_strict_null_without_allocating_containment_frames() {
    // Arrange
    let controls = controls(0);
    let document = ScalarValue::Json("{}".into());

    // Act
    let result =
        evaluate_with_controls(&contains(), &document, &ScalarValue::Null, Some(&controls));

    // Assert
    assert!(matches!(result, Ok(ScalarValue::Null)));
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_release_deep_containment_frames_after_leaf_cancellation() {
    // Arrange
    let document = ScalarValue::Json(format!("{}1{}", "[".repeat(80), "]".repeat(80)));
    let cancellation = QueryCancellationHandle::new();
    let controls = QueryExecutionControls::with_cancellation(
        &CassieRuntimeLimits::default(),
        Instant::now(),
        cancellation.clone(),
    );
    let mut observed = None;

    // Act
    let result = evaluate_observing(
        &contains(),
        &document,
        &document,
        Some(&controls),
        |matched| {
            observed = Some((matched, controls.current_query_memory_bytes()));
            cancellation.cancel();
        },
    );

    // Assert
    assert!(matches!(
        result,
        Err(QueryError::Cassie(CassieError::QueryCancelled))
    ));
    let (matched, retained) = observed.expect("actual deep leaf comparison");
    assert!(matched);
    assert!(
        retained > 1024 * 1024,
        "multiple parent workspaces remain owned"
    );
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_reverse_controlled_containment_for_the_contained_by_operator() {
    // Arrange
    let document = ScalarValue::Json("[1,2]".into());
    let required = ScalarValue::Json("[2]".into());
    let op = BinaryOp::Typed {
        operator: TypedOperator::ContainedOrEqual,
    };
    let controls = controls(128 * 1024);

    // Act
    let result = evaluate_with_controls(&op, &required, &document, Some(&controls));

    // Assert
    assert!(matches!(result, Ok(ScalarValue::Bool(true))));
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_enforce_containment_controls_during_constant_evaluation() {
    // Arrange
    let document = ScalarValue::Json(r#"{"k":1}"#.into());
    let controls = controls(128);

    // Act
    let result = super::super::temporal::binary_or_scalar(
        &document,
        &contains(),
        &document,
        Some(&controls),
    );

    // Assert
    assert!(matches!(
        result,
        Err(QueryError::Cassie(CassieError::ResourceLimit(_)))
    ));
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_enforce_containment_controls_during_row_evaluation() {
    // Arrange
    let document = crate::types::Value::Json(serde_json::json!({"k":1}));
    let row: Vec<(String, crate::types::Value)> = vec![
        ("left".into(), document.clone()),
        ("right".into(), document),
    ];
    let controls = controls(128);
    let functions = std::collections::HashMap::new();
    let context = super::super::EvalContext {
        params: &[],
        search_context: None,
        user_functions: &functions,
        local_args: None,
        session: None,
        exists: None,
        controls: Some(&controls),
        memory: None,
    };

    // Act
    let result = super::super::binary_expr::eval_binary_expr(
        &row,
        &crate::sql::ast::Expr::Column("left".into()),
        &contains(),
        &crate::sql::ast::Expr::Column("right".into()),
        context,
    );

    // Assert
    assert!(matches!(
        result,
        Err(QueryError::Cassie(CassieError::ResourceLimit(_)))
    ));
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_preserve_visited_legacy_numeric_range_errors() {
    // Arrange
    let document = ScalarValue::Json("1e200000".into());
    let controls = controls(4 * 1024 * 1024);

    // Act
    let result = evaluate_with_controls(&contains(), &document, &document, Some(&controls));

    // Assert
    assert!(matches!(result, Err(QueryError::General(_))));
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_preserve_zero_with_an_expanding_legacy_numeric_exponent() {
    // Arrange
    let document = ScalarValue::Json("0e200000".into());
    let zero = ScalarValue::Json("0".into());
    let controls = controls(4 * 1024 * 1024);

    // Act
    let result = evaluate_with_controls(&contains(), &document, &zero, Some(&controls));

    // Assert
    assert!(matches!(result, Ok(ScalarValue::Bool(true))));
    assert_eq!(controls.current_query_memory_bytes(), 0);
}
