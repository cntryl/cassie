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

#[test]
fn should_return_expression_relative_offsets_when_index_is_queried_on_recursive_slice() {
    // Arrange
    let source = "1 OPERATOR(pg_catalog.+) 2 OPERATOR(pg_catalog.*) 3";
    let index = super::Index::scan(source).unwrap();
    let first_operator_end = source.find(") 2").unwrap() + 1;
    let suffix = &source[first_operator_end..];
    // Act
    let span = index
        .last_binary(suffix)
        .expect("the suffix has a binary qualified operator");
    // Assert
    assert_eq!(&suffix[span.start..span.end], "OPERATOR(pg_catalog.*)");
    assert_eq!(span.start, suffix.find("OPERATOR").unwrap());
}

#[test]
fn should_reclassify_operator_adjacency_given_recursive_slice() {
    // Arrange
    let source = "1 OPERATOR(pg_catalog.+)OPERATOR(pg_catalog.-) 2";
    let index = super::Index::scan(source).unwrap();
    let second_operator_start = source.find("OPERATOR(pg_catalog.-)").unwrap();
    // Act
    let full = index
        .last_binary(source)
        .expect("the first operator has a left operand");
    let suffix = &source[second_operator_start..];
    let suffix_span = index.last_binary(suffix);
    // Assert
    assert_eq!(&source[full.start..full.end], "OPERATOR(pg_catalog.+)");
    assert!(suffix_span.is_none());
}

#[test]
fn should_rebase_offsets_at_each_recursive_left_slice() {
    // Arrange
    let source = "OPERATOR(pg_catalog.-) 1 OPERATOR(pg_catalog.+) 2 OPERATOR(pg_catalog.*) 3";
    let index = super::Index::scan(source).unwrap();
    // Act
    let outer = index.last_binary(source).expect("outer binary operator");
    let left = &source[..outer.start];
    let middle = index.last_binary(left).expect("left binary operator");
    let prefix_end = source.find(") 1").unwrap() + 1;
    let inner = &left[prefix_end..];
    let inner_span = index
        .last_binary(inner)
        .expect("nested slice binary operator");
    // Assert
    assert_eq!(&source[outer.start..outer.end], "OPERATOR(pg_catalog.*)");
    assert_eq!(&left[middle.start..middle.end], "OPERATOR(pg_catalog.+)");
    assert_eq!(
        &inner[inner_span.start..inner_span.end],
        "OPERATOR(pg_catalog.+)"
    );
    assert_eq!(inner_span.start, inner.find("OPERATOR").unwrap());
}
