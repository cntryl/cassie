use super::parse;
use crate::sql::ast::{Expr, UnaryNumericOp};

#[test]
fn should_fold_spaced_minimum_literals_given_prefix_signs_when_width_is_selected_later() {
    // Arrange
    let sql = "- 9223372036854775808";
    // Act
    let expression = parse(sql).expect("signed literal");
    // Assert
    assert!(matches!(expression, Expr::IntegerLiteral(i64::MIN)));
}
#[test]
fn should_preserve_cast_precedence_given_numeric_prefixes_when_syntax_is_parsed() {
    // Arrange
    let sql = "-(7::SMALLINT)::BIGINT";
    // Act
    let expression = parse(sql).expect("unary cast");
    // Assert
    assert!(
        matches!(expression, Expr::UnaryNumeric { op: UnaryNumericOp::Minus, expr } if matches!(*expr, Expr::Cast {data_type: crate::types::DataType::BigInt,..}))
    );
}
#[test]
fn should_bound_prefix_chains_given_excessive_signs_when_unary_syntax_is_parsed() {
    // Arrange
    let sql = format!("{}7", "+ ".repeat(129));
    // Act
    let error = parse(&sql).expect_err("prefix bound");
    // Assert
    assert_eq!(error.kind(), crate::sql::SqlErrorKind::ResourceLimit);
}
#[test]
fn should_bound_combined_depth_given_casts_below_signs_when_unary_syntax_is_parsed() {
    // Arrange
    let sql = format!("{}(7{})", "+ ".repeat(65), "::INT".repeat(65));
    // Act
    let error = parse(&sql).expect_err("combined bound");
    // Assert
    assert_eq!(error.kind(), crate::sql::SqlErrorKind::ResourceLimit);
}
#[test]
fn should_preserve_prefix_identity_given_child_mapping_when_parameter_paths_are_rebuilt() {
    // Arrange
    let original = Expr::UnaryNumeric {
        op: UnaryNumericOp::Minus,
        expr: Box::new(Expr::Param(0)),
    };
    // Act
    let mapped = original.map_children(|_| Expr::Param(2));
    // Assert
    assert!(
        matches!(mapped, Expr::UnaryNumeric {op: UnaryNumericOp::Minus, expr} if matches!(*expr, Expr::Param(2)))
    );
    let parsed = crate::sql::parse_statement("SELECT -($1)").expect("parameter syntax");
    assert_eq!(crate::sql::parameter_count(&parsed), 1);
}
#[test]
fn should_distinguish_prefix_identity_given_equal_operands_when_expression_shapes_are_compared() {
    // Arrange
    let minus = Expr::UnaryNumeric {
        op: UnaryNumericOp::Minus,
        expr: Box::new(Expr::Param(0)),
    };
    let plus = Expr::UnaryNumeric {
        op: UnaryNumericOp::Plus,
        expr: Box::new(Expr::Param(0)),
    };
    // Act
    let equal = minus.structurally_eq(&plus);
    // Assert
    assert!(!equal);
    assert!(minus.structurally_eq(&minus.map_children(Clone::clone)));
}
