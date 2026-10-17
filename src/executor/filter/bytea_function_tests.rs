//! Resource ownership of reached immutable BYTEA function work.
use crate::config::CassieRuntimeLimits;
use crate::runtime::QueryExecutionControls;
use crate::sql::parser::parse_expression;

#[test]
fn should_reject_bytea_function_scratch_given_tiny_budget_when_constant_is_reached() {
    // Arrange
    let expression = parse_expression(&format!("decode('{}','hex')", "ab".repeat(8192))).unwrap();
    let controls = QueryExecutionControls::from_limits(
        &CassieRuntimeLimits {
            query_memory_budget_bytes: 128,
            ..CassieRuntimeLimits::default()
        },
        std::time::Instant::now(),
    );
    // Act
    let result = super::constants::constant_scalar_with_parameters(&expression, &[], &controls)
        .map_err(crate::app::CassieError::from);
    // Assert
    assert!(
        matches!(result, Err(crate::app::CassieError::ResourceLimit(_))),
        "{result:?}"
    );
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_release_bytea_function_scratch_given_valid_constant_when_evaluation_finishes() {
    // Arrange
    let expression = parse_expression("decode('00ff','hex')").unwrap();
    let controls = QueryExecutionControls::from_limits(
        &CassieRuntimeLimits::default(),
        std::time::Instant::now(),
    );
    // Act
    let result =
        super::constants::constant_scalar_with_parameters(&expression, &[], &controls).unwrap();
    // Assert
    assert!(matches!(&result.0,Some(super::ScalarValue::Str(value)) if value == "\\x00ff"));
    assert!(controls.peak_query_memory_bytes() > 0);
    drop(result);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}
