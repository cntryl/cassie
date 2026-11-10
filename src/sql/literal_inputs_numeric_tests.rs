//! Binding numeric literal diagnostics retain their admitted error owner.
use super::*;
use crate::config::CassieRuntimeLimits;
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

fn numeric_type() -> DataType {
    DataType::Extended(crate::types::extended::ExtendedType::Numeric(None))
}

#[test]
fn should_reject_numeric_syntax_during_binding_with_retain_the_error_owner() {
    // Arrange
    let controls = controls(1_048_576);
    let text = format!("{}x", "1".repeat(4096));
    let expression = Expr::StringLiteral(text.clone());
    let expected = crate::types::extended::numeric::parse_text(&text).expect_err("invalid input");

    // Act
    let error = validate_with_controls(
        &expression,
        &numeric_type(),
        CastIdentity::Numeric,
        Some(&controls),
    )
    .expect_err("invalid numeric literal");
    let retained = controls.current_query_memory_bytes();
    let descriptor = error.descriptor();

    // Assert
    assert_eq!(descriptor.sql_state, "22P02");
    assert_eq!(descriptor.message, expected.message);
    assert!(retained >= text.len());
    drop(descriptor);
    drop(error);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_preserve_numeric_range_sqlstate_during_binding() {
    // Arrange
    let controls = controls(1_048_576);
    let expression = Expr::StringLiteral("1e131072".into());

    // Act
    let error = validate_with_controls(
        &expression,
        &numeric_type(),
        CastIdentity::Numeric,
        Some(&controls),
    )
    .expect_err("out-of-range literal");

    // Assert
    let descriptor = error.descriptor();
    assert_eq!(descriptor.sql_state, "22003");
    assert_eq!(descriptor.message, "value overflows numeric format");
}

#[test]
fn should_reject_numeric_error_diagnostic_before_allocating_over_budget() {
    // Arrange
    let controls = controls(1);
    let expression = Expr::StringLiteral("bad".into());

    // Act
    let result = validate_with_controls(
        &expression,
        &numeric_type(),
        CastIdentity::Numeric,
        Some(&controls),
    );

    // Assert
    assert!(matches!(result, Err(CassieError::ResourceLimit(_))));
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_detect_invalid_numeric_cast_before_nested_or_parameter_publication() {
    // Arrange
    let controls = controls(1_048_576);
    let cast = Expr::Cast {
        expr: Box::new(Expr::StringLiteral("not_numeric".into())),
        data_type: numeric_type(),
        source_type: None,
        identity: CastIdentity::Numeric,
    };

    // Act
    let rejected = rejects_cast_with_controls(&cast, Some(&controls)).expect("preflight");

    // Assert
    assert!(rejected);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_propagate_numeric_preflight_cancellation() {
    // Arrange
    let cancellation = crate::runtime::QueryCancellationHandle::new();
    let controls = QueryExecutionControls::with_cancellation(
        &CassieRuntimeLimits::default(),
        Instant::now(),
        cancellation.clone(),
    );
    let cast = Expr::Cast {
        expr: Box::new(Expr::StringLiteral("123".into())),
        data_type: numeric_type(),
        source_type: None,
        identity: CastIdentity::Numeric,
    };
    cancellation.cancel();

    // Act
    let result = rejects_cast_with_controls(&cast, Some(&controls));

    // Assert
    assert!(matches!(result, Err(CassieError::QueryCancelled)));
}
