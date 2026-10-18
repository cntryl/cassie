use crate::app::CassieError;
use crate::config::CassieRuntimeLimits;
use crate::runtime::QueryExecutionControls;

#[test]
fn should_retire_scalar_literal_coercion_scratch_given_unknown_parameter_when_parse_is_normalized()
{
    // Arrange
    let mut parsed = crate::sql::parse_statement("SELECT text($1)").expect("type function");
    let controls = QueryExecutionControls::from_limits(
        &CassieRuntimeLimits::default(),
        std::time::Instant::now(),
    );
    // Act
    super::normalize(&mut parsed, &[0], &controls).expect("native type coercion");
    // Assert
    let crate::sql::ast::QueryStatement::Select(select) = parsed.statement else {
        panic!("SELECT");
    };
    assert!(
        matches!(&select.projection[0], crate::sql::ast::SelectItem::Expr { expr: crate::sql::ast::Expr::Cast { data_type: crate::types::DataType::Text, .. }, alias: Some(label) } if label == "text")
    );
    assert!(controls.peak_query_memory_bytes() >= size_of::<crate::sql::ast::Expr>());
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_preserve_scalar_literal_function_given_denied_scratch_when_parse_cannot_normalize() {
    // Arrange
    let mut parsed = crate::sql::parse_statement("SELECT text($1)").expect("type function");
    let before = serde_json::to_string(&parsed).expect("snapshot");
    let controls = QueryExecutionControls::from_limits(
        &CassieRuntimeLimits {
            query_memory_budget_bytes: 1,
            ..CassieRuntimeLimits::default()
        },
        std::time::Instant::now(),
    );
    // Act
    let result = super::normalize(&mut parsed, &[0], &controls);
    // Assert
    assert!(matches!(result, Err(CassieError::ResourceLimit(_))));
    assert_eq!(serde_json::to_string(&parsed).expect("snapshot"), before);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}
