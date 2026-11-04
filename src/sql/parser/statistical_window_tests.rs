//! Window FILTER retains one predicate independently of paired arguments.
use super::*;

#[test]
fn should_retain_one_filter_expression_given_a_paired_window_aggregate() {
    // Arrange
    let sql = "SELECT corr(y,x) FILTER(WHERE keep) OVER (ORDER BY id) FROM pairs";

    // Act
    let parsed = parse_statement(sql).expect("filtered regression window");
    let QueryStatement::Select(select) = parsed.statement else {
        panic!("SELECT");
    };
    let SelectItem::WindowFunction { function, .. } = &select.projection[0] else {
        panic!("window call");
    };

    // Assert
    assert_eq!(function.args.len(), 2);
    assert!(matches!(function.filter.as_deref(), Some(Expr::Column(name)) if name == "keep"));
    assert!(function
        .args
        .iter()
        .all(|argument| matches!(argument, Expr::Column(_))));
}

#[test]
fn should_decode_legacy_unfiltered_window_calls_without_a_filter_field() {
    // Arrange
    let legacy = r#"{"name":"var_pop","args":[],"partition_by":[],"order_by":[],"frame":null}"#;

    // Act
    let call: crate::sql::ast::WindowFunctionCall =
        serde_json::from_str(legacy).expect("legacy call");
    let encoded = serde_json::to_value(&call).expect("window encoding");

    // Assert
    assert!(call.filter.is_none());
    assert!(encoded.get("filter").is_none());
}
