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
fn should_skip_operator_crossing_slice_end_given_recursive_range_lookup() {
    // Arrange
    let source = "1 OPERATOR(pg_catalog.+) 2 OPERATOR(pg_catalog.*) 3";
    let index = super::Index::scan(source).unwrap();
    let second_operator_start = source.find("OPERATOR(pg_catalog.*)").unwrap();
    let partial_end = second_operator_start + "OPERATOR".len();
    let prefix = &source[..partial_end];

    // Act
    let span = index
        .last_binary(prefix)
        .expect("the first complete operator fits in the slice");

    // Assert
    assert_eq!(&prefix[span.start..span.end], "OPERATOR(pg_catalog.+)");
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
fn should_select_prior_binary_when_slice_ends_after_adjacent_qualified_operator() {
    // Arrange
    let source = "1 OPERATOR(pg_catalog.+)OPERATOR(pg_catalog.-) 2 OPERATOR(pg_catalog.*) 3";
    let index = super::Index::scan(source).unwrap();

    // Act
    let outer = index.last_binary(source).expect("outer multiplication");
    let left = &source[..outer.start];
    let inner = index
        .last_binary(left)
        .expect("addition remains binary before the adjacent prefix");

    // Assert
    assert_eq!(&source[outer.start..outer.end], "OPERATOR(pg_catalog.*)");
    assert_eq!(&left[inner.start..inner.end], "OPERATOR(pg_catalog.+)");
}

#[test]
fn should_keep_binary_eligibility_when_comments_separate_qualified_operators() {
    // Arrange
    let sources = [
        "1 OPERATOR(pg_catalog.+) 2 /* comment */ OPERATOR(pg_catalog.*) 3",
        "1 OPERATOR(pg_catalog.+) 2 -- comment\n OPERATOR(pg_catalog.*) 3",
    ];

    // Act
    let spans = sources.map(|source| {
        let index = super::Index::scan(source).unwrap();
        index
            .last_binary(source)
            .expect("the second qualified operator is binary")
    });

    // Assert
    for (source, span) in sources.iter().zip(spans) {
        assert_eq!(&source[span.start..span.end], "OPERATOR(pg_catalog.*)");
    }
}

#[test]
fn should_ignore_nested_case_operators_when_indexing_outer_expression() {
    // Arrange
    let source = "CASE WHEN 1 OPERATOR(pg_catalog.+) 2 = 3 \
                  THEN 4 OPERATOR(pg_catalog.*) 5 \
                  ELSE 6 END OPERATOR(pg_catalog.+) 7";
    let index = super::Index::scan(source).unwrap();

    // Act
    let span = index
        .last_binary(source)
        .expect("outer qualified operator remains visible");

    // Assert
    assert_eq!(&source[span.start..span.end], "OPERATOR(pg_catalog.+)");
    assert_eq!(source[..span.start].matches("OPERATOR(").count(), 2);
}

#[test]
fn should_keep_byte_offsets_for_multibyte_left_operands() {
    // Arrange
    let source = "café OPERATOR(pg_catalog.+) 2";
    let index = super::Index::scan(source).unwrap();

    // Act
    let span = index
        .last_binary(source)
        .expect("the qualified operator follows a multibyte identifier");

    // Assert
    assert_eq!(&source[..span.start], "café ");
    assert_eq!(&source[span.start..span.end], "OPERATOR(pg_catalog.+)");
}

#[test]
fn should_parse_adjacent_qualified_and_unqualified_prefix_operators() {
    // Arrange
    let sql = "OPERATOR(pg_catalog.~)~1";

    // Act
    let parsed = crate::sql::parser::parse_expression(sql);

    // Assert
    assert!(parsed.is_ok(), "qualified prefix chain: {parsed:?}");
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
    let prefix_expression_end = source.find(") 1").unwrap() + ") 1".len();
    let prefix_expression = &source[..prefix_expression_end];
    let prefix_binary = index.last_binary(prefix_expression);
    // Assert
    assert_eq!(&source[outer.start..outer.end], "OPERATOR(pg_catalog.*)");
    assert_eq!(&left[middle.start..middle.end], "OPERATOR(pg_catalog.+)");
    assert_eq!(
        &inner[inner_span.start..inner_span.end],
        "OPERATOR(pg_catalog.+)"
    );
    assert_eq!(inner_span.start, inner.find("OPERATOR").unwrap());
    assert!(prefix_binary.is_none(), "the qualified minus is a prefix");
}
