use crate::sql::{parse_statement, QueryStatement, SqlErrorKind};

#[test]
fn should_preserve_default_fetch_count_given_row_syntax_when_bounds_are_parsed() {
    // Arrange
    let sql = "SELECT 7 OFFSET 1 ROW FETCH NEXT ROWS ONLY";
    // Act
    let parsed = parse_statement(sql).expect("standard bounds");
    // Assert
    let QueryStatement::Select(select) = parsed.statement else {
        panic!("SELECT");
    };
    assert!(matches!(
        select.limit,
        Some(crate::sql::ast::Expr::IntegerLiteral(1))
    ));
    assert!(matches!(
        select.offset,
        Some(crate::sql::ast::Expr::IntegerLiteral(1))
    ));
}

#[test]
fn should_infer_native_fetch_slots_given_expression_bounds_when_parameters_are_described() {
    // Arrange
    let parsed =
        parse_statement("SELECT 7 OFFSET ($1+1) ROWS FETCH NEXT $2 ROWS ONLY").expect("bounds");
    let catalog = crate::catalog::Catalog::new();
    // Act
    let oids = crate::sql::parameter_type_oids_with_catalog(&parsed, &[], &catalog);
    // Assert
    assert_eq!(crate::sql::parameter_count(&parsed), 2);
    assert_eq!(oids, vec![23, 20]);
}

#[test]
fn should_reject_excessive_fetch_operators_given_parenthesized_counts_when_the_parser_budget_is_exceeded(
) {
    // Arrange
    let bound = vec!["1"; 130].join("+");
    let sql = format!("SELECT 7 FETCH FIRST ({bound}) ROWS ONLY");
    // Act
    let error = parse_statement(&sql).expect_err("finite count budget");
    // Assert
    assert_eq!(error.kind(), SqlErrorKind::ResourceLimit);
}

#[test]
fn should_preserve_existing_bound_serialization_given_fetch_syntax_when_the_statement_is_encoded() {
    // Arrange
    let parsed = parse_statement("SELECT 7 FETCH FIRST 2 ROWS ONLY OFFSET 1 ROW").expect("bounds");
    // Act
    let QueryStatement::Select(select) = parsed.statement else {
        panic!("SELECT");
    };
    let bounds = serde_json::to_value((&select.limit, &select.offset)).expect("serialize");
    // Assert
    assert_eq!(
        bounds,
        serde_json::json!([{"IntegerLiteral":2},{"IntegerLiteral":1}])
    );
}
