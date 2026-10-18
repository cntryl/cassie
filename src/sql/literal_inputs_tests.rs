//! Small diagnostic admission and nested binding witnesses.
use super::*;
use crate::config::CassieRuntimeLimits;
use std::time::Instant;

fn controls(budget: usize) -> QueryExecutionControls {
    QueryExecutionControls::from_limits(
        &CassieRuntimeLimits {
            query_memory_budget_bytes: budget,
            query_timeout_ms: 0,
            ..CassieRuntimeLimits::default()
        },
        Instant::now(),
    )
}

#[test]
fn should_check_cancellation_before_allocating_a_scalar_input_diagnostic() {
    // Arrange
    let cancellation = crate::runtime::QueryCancellationHandle::new();
    let controls = QueryExecutionControls::with_cancellation(
        &CassieRuntimeLimits::default(),
        Instant::now(),
        cancellation.clone(),
    );
    let expression = Expr::StringLiteral("bad".into());
    cancellation.cancel();
    // Act
    let result = validate_with_controls(
        &expression,
        &DataType::Int,
        CastIdentity::Scalar,
        Some(&controls),
    );
    // Assert
    assert!(matches!(result, Err(CassieError::QueryCancelled)));
    assert_eq!(controls.peak_query_memory_bytes(), 0);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_admit_exact_scalar_error_capacity_before_copying_literal_input() {
    // Arrange
    let input = "x".repeat(64);
    let expression = Expr::StringLiteral(input.clone());
    let expected = format!("invalid input syntax for type integer: \"{input}\"");
    let admitted = controls(expected.len());
    let denied = controls(expected.len() - 1);
    // Act
    let result = validate_with_controls(
        &expression,
        &DataType::Int,
        CastIdentity::Scalar,
        Some(&admitted),
    );
    let rejected = validate_with_controls(
        &expression,
        &DataType::Int,
        CastIdentity::Scalar,
        Some(&denied),
    );
    // Assert
    assert!(matches!(result, Err(CassieError::Execution(message)) if message == expected));
    assert!(matches!(rejected, Err(CassieError::ResourceLimit(_))));
    assert_eq!(admitted.peak_query_memory_bytes(), expected.len());
    assert_eq!(admitted.current_query_memory_bytes(), 0);
    assert_eq!(denied.peak_query_memory_bytes(), 0);
    assert_eq!(denied.current_query_memory_bytes(), 0);
}

#[test]
fn should_retain_scalar_diagnostic_admission_inside_exists_analysis() {
    // Arrange
    let input = "x".repeat(256);
    let valid = crate::sql::parse_statement(&format!("SELECT EXISTS(SELECT '{input}'::TEXT)"))
        .expect("valid AST");
    let invalid = crate::sql::parse_statement(&format!("SELECT EXISTS(SELECT '{input}'::INT)"))
        .expect("invalid AST");
    let catalog = crate::catalog::Catalog::new();
    let context = crate::sql::binder::BindingContext::default();
    let baseline = controls(1_048_576);
    crate::sql::parse_analysis::validate(&valid, &[], &catalog, &context, &baseline)
        .expect("valid analysis");
    let denied = controls(baseline.peak_query_memory_bytes() + 1);
    let admitted = controls(1_048_576);
    // Act
    let rejected = crate::sql::parse_analysis::validate(&invalid, &[], &catalog, &context, &denied);
    let result = crate::sql::parse_analysis::validate(&invalid, &[], &catalog, &context, &admitted);
    // Assert
    assert!(
        matches!(rejected, Err(CassieError::ResourceLimit(_))),
        "{rejected:?}"
    );
    assert!(matches!(result, Err(CassieError::Execution(message)) if message.contains(&input)));
    assert_eq!(denied.current_query_memory_bytes(), 0);
    assert_eq!(admitted.current_query_memory_bytes(), 0);
}

#[test]
fn should_admit_scalar_diagnostics_given_nested_source_scopes_when_cast_binding_fails() {
    // Arrange
    let input = "x".repeat(256);
    let catalog = crate::catalog::Catalog::new();
    let context = crate::sql::binder::BindingContext::default();
    let queries = [
        format!("WITH c AS (SELECT '{input}'::TYPE AS n) SELECT n FROM c"),
        format!("SELECT n FROM (VALUES ('{input}'::TYPE)) v(n)"),
        format!("SELECT n FROM (SELECT '{input}'::TYPE AS n) v"),
        format!("SELECT a.n FROM (VALUES (1)) a(n) JOIN (VALUES (1)) b(n) ON EXISTS (SELECT '{input}'::TYPE)"),
    ];
    for sql in queries {
        let valid =
            crate::sql::parse_statement(&sql.replace("::TYPE", "::TEXT")).expect("valid AST");
        let invalid =
            crate::sql::parse_statement(&sql.replace("::TYPE", "::INT")).expect("invalid AST");
        let baseline = controls(1_048_576);
        crate::sql::parse_analysis::validate(&valid, &[], &catalog, &context, &baseline)
            .expect("baseline");
        let denied = controls(baseline.peak_query_memory_bytes() + 1);
        // Act
        let result =
            crate::sql::parse_analysis::validate(&invalid, &[], &catalog, &context, &denied);
        // Assert
        assert!(
            matches!(result, Err(CassieError::ResourceLimit(_))),
            "{sql}: {result:?}"
        );
        assert_eq!(denied.current_query_memory_bytes(), 0);
    }
}

#[test]
fn should_validate_nested_cast_inputs_given_earlier_subqueries_when_binding_projection_order() {
    // Arrange
    let catalog = crate::catalog::Catalog::new();
    let context = crate::sql::binder::BindingContext::default();
    let queries = [
        "SELECT sum(EXISTS(SELECT 'bad'::INT),1)",
        "SELECT EXISTS(SELECT 'bad'::INT), missing",
    ];
    for sql in queries {
        let parsed = crate::sql::parse_statement(sql).expect("AST");
        let admitted = controls(1_048_576);
        // Act
        let result =
            crate::sql::parse_analysis::validate(&parsed, &[], &catalog, &context, &admitted);
        // Assert
        assert!(
            matches!(result, Err(CassieError::Execution(ref message)) if message == "invalid input syntax for type integer: \"bad\""),
            "{sql}: {result:?}"
        );
        assert_eq!(admitted.current_query_memory_bytes(), 0);
    }
}
