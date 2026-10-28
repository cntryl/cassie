//! Resolved operator metadata is part of expression identity.
use super::super::{BinaryOp, Expr};
use crate::types::{IntegerOperation, IntegerWidth};

#[test]
fn should_preserve_array_subscript_child_order_during_mapping() {
    // Arrange
    let original = Expr::ArraySubscript {
        array: Box::new(Expr::Param(0)),
        subscripts: vec![
            super::super::ArraySubscript::Index(Expr::Param(1)),
            super::super::ArraySubscript::Slice {
                lower: Some(Expr::Param(2)),
                upper: Some(Expr::Param(3)),
            },
        ],
    };
    let mut visited = Vec::new();

    // Act
    let mapped = original.map_children(|child| {
        if let Expr::Param(index) = child {
            visited.push(*index);
        }
        child.clone()
    });

    // Assert
    assert_eq!(visited, [0, 1, 2, 3]);
    assert!(original.structurally_eq(&mapped));
}

#[test]
fn should_distinguish_resolved_operator_signatures_given_equal_operands_when_expression_identity_is_compared(
) {
    // Arrange
    let expression = |operation, width| Expr::Binary {
        left: Box::new(Expr::IntegerLiteral(1)),
        op: BinaryOp::Integer { operation, width },
        right: Box::new(Expr::IntegerLiteral(2)),
    };
    let add = expression(IntegerOperation::Add, IntegerWidth::I16);
    let wider = expression(IntegerOperation::Add, IntegerWidth::I32);
    let subtract = expression(IntegerOperation::Sub, IntegerWidth::I16);
    // Act
    let observations = [
        add.structurally_eq(&wider),
        add.structurally_eq(&subtract),
        add.structurally_eq(&add.clone()),
    ];
    // Assert
    assert_eq!(observations, [false, false, true]);
}

#[test]
fn should_preserve_negation_width_given_child_mapping_when_identity_is_compared() {
    // Arrange
    let original = Expr::IntegerNegate {
        expr: Box::new(Expr::IntegerLiteral(1)),
        width: IntegerWidth::I16,
    };
    let wider = Expr::IntegerNegate {
        expr: Box::new(Expr::IntegerLiteral(1)),
        width: IntegerWidth::I32,
    };
    // Act
    let mapped = original.map_children(Clone::clone);
    let mut mutable = mapped.clone();
    let result: Result<(), ()> = mutable.try_visit_children_mut(|child| {
        *child = Expr::IntegerLiteral(2);
        Ok(())
    });
    // Assert
    assert_eq!(result, Ok(()));
    assert!(original.structurally_eq(&mapped));
    assert!(!original.structurally_eq(&wider));
    assert!(!original.structurally_eq(&mutable));
    let mut children = 0;
    let _: std::ops::ControlFlow<()> = original.try_for_each_child(|_| {
        children += 1;
        std::ops::ControlFlow::Continue(())
    });
    assert_eq!(children, 1);
}

#[test]
fn should_preserve_row_operator_identity_given_child_mapping_when_expression_shapes_are_compared() {
    // Arrange
    let original = Expr::RowCompare(Box::new(super::super::RowComparison {
        left: vec![Expr::Param(0)],
        op: super::super::RowComparisonOp::Eq,
        right: vec![Expr::Param(1)],
    }));
    let mut distinct = original.clone();
    if let Expr::RowCompare(comparison) = &mut distinct {
        comparison.op = super::super::RowComparisonOp::IsNotDistinctFrom;
    }
    // Act
    let mapped = original.map_children(Clone::clone);
    let mut fields = Vec::new();
    original.for_each_child(|field| fields.push(field.clone()));
    // Assert
    assert!(original.structurally_eq(&mapped));
    assert!(!original.structurally_eq(&distinct));
    assert_eq!(fields.len(), 2);
    assert!(fields[0].structurally_eq(&Expr::Param(0)));
    assert!(fields[1].structurally_eq(&Expr::Param(1)));
}

#[test]
fn should_distinguish_numeric_tokens_given_equal_quoted_digits_when_ast_leaves_are_mapped() {
    // Arrange
    let numeric = Expr::NumericLiteral("9223372036854775808".into());
    let quoted = Expr::StringLiteral("9223372036854775808".into());
    // Act
    let mapped = numeric.map_children(Clone::clone);
    let encoded = serde_json::to_string(&mapped).expect("encode numeric AST");
    let decoded: Expr = serde_json::from_str(&encoded).expect("decode numeric AST");
    // Assert
    assert!(numeric.structurally_eq(&mapped));
    assert!(numeric.structurally_eq(&decoded));
    assert!(!numeric.structurally_eq(&quoted));
}
