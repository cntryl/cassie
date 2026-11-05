//! Serialized index ASTs cannot carry a silently ignored window FILTER.
use super::*;

#[test]
fn should_reject_window_sublinks_before_serialized_index_expression_publication() {
    // Arrange
    let expression = crate::sql::parser::parse_expression(
        "(SELECT var_pop(n) FILTER(WHERE keep) OVER() FROM records)",
    )
    .expect("window scalar sublink");
    let encoded = serde_json::to_string(&expression).expect("index AST");

    // Act
    let result = encoded_expression(&encoded);

    // Assert
    assert!(
        matches!(result, Err(CassieError::Unsupported(message)) if message == "index expressions cannot contain subqueries")
    );
}

#[test]
fn should_retain_filtered_window_sql_in_raw_query_definition_admission() {
    // Arrange
    let query = "SELECT corr(y,x) FILTER(WHERE keep) OVER() FROM records";

    // Act
    let result = query_sql(query);

    // Assert
    assert!(result.is_ok());
}
