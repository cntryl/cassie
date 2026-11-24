use crate::ast::{FunctionCall, ParsedStatement, SubqueryExpr, SubqueryKind};

#[test]
fn should_default_function_call_without_defaulting_its_attachment() {
    // Arrange
    struct RuntimeOnly;

    // Act
    let call = FunctionCall::<RuntimeOnly>::default();

    // Assert
    assert!(call.name.is_empty());
    assert!(call.args.is_empty());
    assert_eq!(call.quantifier, crate::ast::AggregateQuantifier::Default);
    assert!(call.filter.is_none());
    assert!(call.order_by.is_empty());
}

#[test]
fn should_omit_runtime_attachment_when_syntax_ast_is_serialized() {
    // Arrange
    let query = ParsedStatement {
        raw_sql: "SELECT 1".to_string(),
        statement: crate::ast::QueryStatement::Select(Box::new(crate::ast::SelectStatement {
            source: crate::ast::QuerySource::SingleRow,
            ctes: Vec::new(),
            recursive: false,
            distinct: false,
            distinct_on: Vec::new(),
            projection: Vec::new(),
            filter: None,
            group_by: Vec::new(),
            grouping_subtotal: false,
            having: None,
            order: Vec::new(),
            limit: None,
            offset: None,
            set: None,
        })),
    };
    struct RuntimeOnly;

    let expression = SubqueryExpr::<RuntimeOnly> {
        kind: SubqueryKind::Scalar,
        query: Box::new(query),
        data_type: None,
        outcome: Some(RuntimeOnly),
    };

    // Act
    let encoded = serde_json::to_value(&expression).expect("serialize syntax AST");
    let decoded: SubqueryExpr<RuntimeOnly> = serde_json::from_value(encoded.clone())
        .expect("deserialize syntax AST without runtime attachment");

    // Assert
    assert!(encoded.get("outcome").is_none());
    assert!(decoded.outcome.is_none());
}

#[test]
fn should_rebind_nested_query_ast_given_runtime_attachment_type_when_syntax_is_parsed() {
    // Arrange
    let inner = ParsedStatement {
        raw_sql: "SELECT 1".to_string(),
        statement: crate::ast::QueryStatement::Select(Box::new(crate::ast::SelectStatement {
            source: crate::ast::QuerySource::SingleRow,
            ctes: Vec::new(),
            recursive: false,
            distinct: false,
            distinct_on: Vec::new(),
            projection: vec![crate::ast::SelectItem::Expr {
                expr: crate::ast::Expr::IntegerLiteral(1),
                alias: None,
            }],
            filter: None,
            group_by: Vec::new(),
            grouping_subtotal: false,
            having: None,
            order: Vec::new(),
            limit: None,
            offset: None,
            set: None,
        })),
    };
    let parsed: ParsedStatement = ParsedStatement {
        raw_sql: "SELECT (SELECT 1)".to_string(),
        statement: crate::ast::QueryStatement::Select(Box::new(crate::ast::SelectStatement {
            source: crate::ast::QuerySource::SingleRow,
            ctes: Vec::new(),
            recursive: false,
            distinct: false,
            distinct_on: Vec::new(),
            projection: vec![crate::ast::SelectItem::Expr {
                expr: crate::ast::Expr::Subquery(Box::new(SubqueryExpr::new(
                    SubqueryKind::Scalar,
                    inner,
                ))),
                alias: None,
            }],
            filter: None,
            group_by: Vec::new(),
            grouping_subtotal: false,
            having: None,
            order: Vec::new(),
            limit: None,
            offset: None,
            set: None,
        })),
    };
    let expected_syntax = serde_json::to_value(&parsed).expect("serialize syntax AST");

    // Act
    let mut attached: ParsedStatement<String> = crate::attachment::rebind_attachment(parsed);
    let crate::ast::QueryStatement::Select(select) = &mut attached.statement else {
        panic!("SELECT");
    };
    let crate::ast::SelectItem::Expr { expr, .. } = &mut select.projection[0] else {
        panic!("expression projection");
    };
    let crate::ast::Expr::Subquery(subquery) = expr else {
        panic!("subquery expression");
    };
    subquery.outcome = Some("runtime outcome".to_string());

    // Assert
    assert_eq!(
        serde_json::to_value(attached).expect("serialize attached AST"),
        expected_syntax
    );
}

#[test]
fn should_compare_field_constraints_without_attachment_partial_eq() {
    // Arrange
    #[derive(Debug, Clone)]
    struct DebugOnlyAttachment;

    let constraint = crate::declarations::FieldConstraint::<DebugOnlyAttachment>::new("id");
    let equivalent = constraint.clone();

    // Act
    let matches = constraint == equivalent;

    // Assert
    assert!(matches);
}
