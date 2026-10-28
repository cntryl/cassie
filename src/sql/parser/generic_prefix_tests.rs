use crate::sql::parser::{parse_statement, SqlErrorKind};

#[test]
fn should_reject_compound_prefix_given_adjacent_operator_tokens_when_signatures_are_distinct() {
    // Arrange
    let expressions = ["@-7", "~-1", "@~1", "|/|/81", "|/-1", "||/-8", "-~1"];
    // Act
    let results = expressions.map(|expression| parse_statement(&format!("SELECT {expression}")));
    // Assert
    assert!(results.into_iter().all(|result| result.is_err()));
}

#[test]
fn should_bound_prefix_depth_given_excessive_chain_when_expression_is_parsed() {
    // Arrange
    let sql = format!(
        "SELECT {}1",
        "OPERATOR(pg_catalog.~) ".repeat(super::depth::MAX_POSTFIX_DEPTH + 1)
    );
    // Act
    let error = parse_statement(&sql).expect_err("prefix chain exceeds admitted depth");
    // Assert
    assert_eq!(error.kind(), SqlErrorKind::ResourceLimit);
}
