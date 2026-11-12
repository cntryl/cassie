use crate::sql::ast::{CommentOnObjectKind, IdentifierComponent, QueryStatement};
use crate::sql::{parse_statement, SqlErrorKind};

#[test]
fn should_preserve_quoted_relation_and_column_identifiers_given_column_comment() {
    // Arrange
    let sql = "COMMENT ON COLUMN \"Q Schema\".\"T.Table\".\"Odd.Name\" IS 'note'";

    // Act
    let parsed = parse_statement(sql).expect("parse selected column comment");

    // Assert
    let QueryStatement::CommentOn(statement) = parsed.statement else {
        panic!("expected COMMENT ON statement");
    };
    assert_eq!(statement.kind, CommentOnObjectKind::Column);
    assert_eq!(
        statement.object.components().map(|part| part.value()).collect::<Vec<_>>(),
        ["Q Schema", "T.Table"]
    );
    assert_eq!(
        statement.column.as_ref().map(IdentifierComponent::value),
        Some("Odd.Name")
    );
    assert_eq!(statement.description.as_deref(), Some("note"));
}

#[test]
fn should_reject_materialized_view_comment_given_unselected_target() {
    // Arrange
    let sql = "COMMENT ON MATERIALIZED VIEW selected_view IS 'note'";

    // Act
    let result = parse_statement(sql);

    // Assert
    let error = result.expect_err("materialized-view comments are outside #845");
    assert_eq!(error.kind(), SqlErrorKind::Unsupported);
}

#[test]
fn should_reject_comment_expression_given_non_string_non_null_value() {
    // Arrange
    let sql = "COMMENT ON TABLE selected_table IS 123";

    // Act
    let result = parse_statement(sql);

    // Assert
    let error = result.expect_err("COMMENT accepts only text or NULL");
    assert_eq!(error.kind(), SqlErrorKind::Unsupported);
}

#[test]
fn should_reject_parameterized_comment_value_given_utility_literal_contract() {
    // Arrange
    let sql = "COMMENT ON TABLE selected_table IS $1";

    // Act
    let result = parse_statement(sql);

    // Assert
    let error = result.expect_err("COMMENT values are fixed utility literals");
    assert_eq!(error.kind(), SqlErrorKind::Unsupported);
}
