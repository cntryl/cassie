use super::OwnedNumericLiteralError;
use crate::config::CassieRuntimeLimits;
use crate::runtime::QueryExecutionControls;
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
fn should_hold_numeric_syntax_diagnostic_until_its_owner_drops() {
    // Arrange
    let controls = controls(1_048_576);
    let input = format!("{}é!", "1".repeat(4095));

    // Act
    let diagnostic = OwnedNumericLiteralError::syntax(&input, &controls).expect("admitted error");

    // Assert
    assert_eq!(diagnostic.sqlstate(), "22P02");
    assert_eq!(
        diagnostic.message(),
        format!("invalid input syntax for type numeric: \"{input}\"")
    );
    assert!(controls.current_query_memory_bytes() >= diagnostic.message().len());
    drop(diagnostic);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_reject_numeric_diagnostic_when_its_owner_exceeds_budget() {
    // Arrange
    let controls = controls(1);

    // Act
    let result = OwnedNumericLiteralError::syntax("invalid", &controls);

    // Assert
    assert!(matches!(
        result,
        Err(crate::app::CassieError::ResourceLimit(_))
    ));
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_hold_numeric_range_diagnostic_with_preserve_its_exact_message() {
    // Arrange
    let controls = controls(4096);

    // Act
    let diagnostic = OwnedNumericLiteralError::range(&controls).expect("admitted error");

    // Assert
    assert_eq!(diagnostic.sqlstate(), "22003");
    assert_eq!(diagnostic.message(), "value overflows numeric format");
    assert!(controls.current_query_memory_bytes() >= diagnostic.message().len());
    drop(diagnostic);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_classify_owned_numeric_errors_like_existing_type_input_errors() {
    // Arrange
    let controls = controls(4096);
    let error = crate::app::CassieError::OwnedNumericLiteral(
        OwnedNumericLiteralError::range(&controls).expect("admitted error"),
    );

    // Act
    let class = crate::runtime::error_class(&error);

    // Assert
    assert_eq!(class, "execution");
}

#[test]
fn should_release_owned_numeric_diagnostic_when_canceled_during_copy() {
    // Arrange
    let cancellation = crate::runtime::QueryCancellationHandle::new();
    let controls = QueryExecutionControls::with_cancellation(
        &CassieRuntimeLimits::default(),
        Instant::now(),
        cancellation.clone(),
    );
    let input = "a".repeat(4096);
    let mut observed = false;

    // Act
    let result = OwnedNumericLiteralError::syntax_observing(&input, &controls, || {
        observed = true;
        cancellation.cancel();
    });

    // Assert
    assert!(observed);
    assert!(matches!(
        result,
        Err(crate::app::CassieError::QueryCanceled)
    ));
    assert_eq!(controls.current_query_memory_bytes(), 0);
}
