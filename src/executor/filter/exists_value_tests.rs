//! The existing scalar evaluator owns selected branch evaluation and carriers.
use super::*;
use crate::executor::batch::BatchRow;
use std::cell::Cell;

#[test]
fn should_skip_unselected_exists_callbacks() {
    // Arrange
    let row = BatchRow::new(vec![]);
    let functions = HashMap::new();
    let calls = Cell::new(0);
    let resolver = |_: &Expr| {
        calls.set(calls.get() + 1);
        Ok(true)
    };
    let expressions = [
        "CASE WHEN true THEN false ELSE EXISTS(SELECT 1) END",
        "COALESCE(true,EXISTS(SELECT 1))",
    ]
    .map(|sql| crate::sql::parser::parse_expression(sql).expect("selected expression"));

    // Act
    let results = expressions
        .iter()
        .map(|expr| {
            evaluate_resolving_exists(
                &row,
                expr,
                ExistsValueContext {
                    params: &[],
                    search: None,
                    functions: &functions,
                    session: None,
                    resolver: &resolver,
                },
            )
        })
        .collect::<Result<Vec<_>, _>>();

    // Assert
    assert_eq!(
        results.expect("lazy scalar evaluation"),
        [Value::Bool(false), Value::Bool(true)]
    );
    assert_eq!(calls.get(), 0);
}

#[test]
fn should_invoke_reached_exists_callbacks_once() {
    // Arrange
    let row = BatchRow::new(vec![]);
    let functions = HashMap::new();
    let calls = Cell::new(0);
    let resolver = |_: &Expr| {
        calls.set(calls.get() + 1);
        Ok(true)
    };
    let expression = crate::sql::parser::parse_expression(
        "CASE WHEN EXISTS(SELECT 1) THEN COALESCE(NULL,EXISTS(SELECT 2),EXISTS(SELECT 3)) ELSE EXISTS(SELECT 4) END"
    ).expect("selected expression");

    // Act
    let result = evaluate_resolving_exists(
        &row,
        &expression,
        ExistsValueContext {
            params: &[],
            search: None,
            functions: &functions,
            session: None,
            resolver: &resolver,
        },
    );

    // Assert
    assert_eq!(
        result.expect("once-only nested selection"),
        Value::Bool(true)
    );
    assert_eq!(calls.get(), 2);
}

#[test]
fn should_retain_selected_coalesce_parameter_carrier() {
    // Arrange
    let row = BatchRow::new(vec![]);
    let functions = HashMap::new();
    let calls = Cell::new(0);
    let resolver = |_: &Expr| {
        calls.set(calls.get() + 1);
        Ok(true)
    };
    let expression = crate::sql::parser::parse_expression("COALESCE($1,EXISTS(SELECT 1))")
        .expect("selected expression");
    let params = [Value::Json(serde_json::json!([]))];

    // Act
    let result = evaluate_resolving_exists(
        &row,
        &expression,
        ExistsValueContext {
            params: &params,
            search: None,
            functions: &functions,
            session: None,
            resolver: &resolver,
        },
    );

    // Assert
    assert_eq!(result.expect("existing rich parameter carrier"), params[0]);
    assert_eq!(calls.get(), 0);
}

#[test]
fn should_resolve_selected_function_arguments_once() {
    // Arrange
    let row = BatchRow::new(vec![]);
    let functions = HashMap::from([(
        "gate".into(),
        FunctionMeta {
            name: "gate".into(),
            args: vec![crate::catalog::FunctionArgMeta {
                name: "x".into(),
                data_type: crate::types::DataType::Boolean,
            }],
            return_type: crate::types::DataType::Boolean,
            volatility: crate::catalog::Volatility::Immutable,
            body: "x".into(),
        },
    )]);
    let calls = Cell::new(0);
    let resolver = |_: &Expr| {
        calls.set(calls.get() + 1);
        Ok(true)
    };
    let expression = crate::sql::parser::parse_expression("gate(EXISTS(SELECT 1))")
        .expect("selected expression");

    // Act
    let result = evaluate_resolving_exists(
        &row,
        &expression,
        ExistsValueContext {
            params: &[],
            search: None,
            functions: &functions,
            session: None,
            resolver: &resolver,
        },
    );

    // Assert
    assert_eq!(
        result.expect("selected function argument"),
        Value::Bool(true)
    );
    assert_eq!(calls.get(), 1);
}

#[test]
fn should_preserve_unclassified_function_body_errors() {
    // Arrange
    let row = BatchRow::new(vec![]);
    let functions = HashMap::from([(
        "gate".into(),
        FunctionMeta {
            name: "gate".into(),
            args: vec![],
            return_type: crate::types::DataType::Boolean,
            volatility: crate::catalog::Volatility::Immutable,
            body: "EXISTS(SELECT 1)".into(),
        },
    )]);
    let calls = Cell::new(0);
    let resolver = |_: &Expr| {
        calls.set(calls.get() + 1);
        Ok(true)
    };
    let expression =
        crate::sql::parser::parse_expression("gate()").expect("existing function call");

    // Act
    let result = evaluate_resolving_exists(
        &row,
        &expression,
        ExistsValueContext {
            params: &[],
            search: None,
            functions: &functions,
            session: None,
            resolver: &resolver,
        },
    );

    // Assert
    assert!(matches!(result, Err(QueryError::General(ref message))
        if message == "EXISTS predicate was not resolved before filtering"));
    assert_eq!(calls.get(), 0);
}
