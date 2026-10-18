use crate::sql::parser::{parse_statement, SqlErrorKind};

#[test]
fn should_bound_qualified_prefix_given_excessive_chain_when_expression_is_parsed() {
    // Arrange
    let sql = format!(
        "SELECT {}1",
        "OPERATOR(pg_catalog.~) ".repeat(super::depth::MAX_POSTFIX_DEPTH + 1)
    );
    // Act
    let error = parse_statement(&sql).expect_err("bounded qualified prefix");
    // Assert
    assert_eq!(error.kind(), SqlErrorKind::ResourceLimit);
}

#[test]
fn should_bound_qualified_binary_given_excessive_chain_when_expression_is_parsed() {
    // Arrange
    let sql = format!(
        "SELECT 1{}",
        " OPERATOR(pg_catalog.+) 1".repeat(super::depth::MAX_POSTFIX_DEPTH + 1)
    );
    // Act
    let error = parse_statement(&sql).expect_err("bounded qualified binary");
    // Assert
    assert_eq!(error.kind(), SqlErrorKind::ResourceLimit);
}

#[test]
fn should_reject_unknown_namespace_given_qualified_symbol_when_builtin_fallback_is_unavailable() {
    // Arrange
    let statements = [
        "SELECT 1 OPERATOR(public.+) 2",
        "SELECT 1 OPERATOR(\"PG_CATALOG\".+) 2",
    ];
    // Act
    let errors = statements.map(|sql| parse_statement(sql).expect_err("namespace remains pending"));
    // Assert
    assert!(errors
        .iter()
        .all(|error| error.kind() == SqlErrorKind::Unsupported));
}

#[test]
fn should_preserve_column_named_operator_given_arithmetic_when_lookahead_is_not_a_qualified_token()
{
    // Arrange
    let source = "1 + operator + 3";
    let expected = "(1 + operator) + 3";
    // Act
    let parsed = crate::sql::parser::parse_expression(source).unwrap();
    let grouped = crate::sql::parser::parse_expression(expected).unwrap();
    // Assert
    assert!(parsed.structurally_eq(&grouped));
}
