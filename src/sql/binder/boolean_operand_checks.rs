//! Collation, BETWEEN and concatenation operand checks of boolean contexts.

use super::{
    canonicalize_boolean_pair, decimal_literals, enum_operands, validate_expression, BinaryOp,
    BindingContext, CassieError, Catalog, DataType, Expr, ResultTypes,
};

pub(super) fn validate_collation_pair(left: &Expr, right: &Expr) -> Result<(), CassieError> {
    fn explicit_collation(expression: &Expr) -> Option<&str> {
        match expression {
            Expr::Collate { collation, .. } => Some(collation.as_str()),
            _ => None,
        }
    }
    if let (Some(left), Some(right)) = (explicit_collation(left), explicit_collation(right)) {
        if left != right {
            return Err(crate::app::DiagnosticError::new(
                "42P21",
                format!("collation mismatch between explicit collations {left:?} and {right:?}"),
            )
            .into());
        }
    }
    Ok(())
}

pub(super) fn validate_between_expression(
    expression: &mut Expr,
    types: &ResultTypes,
    catalog: &Catalog,
    context: &BindingContext,
) -> Result<(), CassieError> {
    let Expr::Between {
        expr,
        low,
        high,
        symmetric,
        ..
    } = expression
    else {
        unreachable!("BETWEEN validation dispatch")
    };
    super::super::symmetric_between::validate([expr, low, high], *symmetric, types)?;
    canonicalize_boolean_pair(expr, low, types)?;
    canonicalize_boolean_pair(expr, high, types)?;
    canonicalize_boolean_pair(low, high, types)?;
    enum_operands::qualify_pair(&BinaryOp::Gte, expr, low, types)?;
    enum_operands::qualify_pair(&BinaryOp::Lte, expr, high, types)?;
    decimal_literals::narrow_pair(expr, low, types);
    decimal_literals::narrow_pair(expr, high, types);
    validate_expression(expr, types, catalog, context)?;
    validate_expression(low, types, catalog, context)?;
    validate_expression(high, types, catalog, context)?;
    Ok(())
}

pub(super) fn validate_concatenation_comparison(
    left: &mut Expr,
    right: &mut Expr,
    types: &ResultTypes,
) -> Result<(), CassieError> {
    if types.parameter_types().is_empty()
        || (!contains_concatenation(left) && !contains_concatenation(right))
    {
        return Ok(());
    }
    // An uncast SQL string is unknown until its comparison signature is known.
    let literal = if types.expression_type(left) == Some(DataType::Bytea)
        && matches!(right, Expr::StringLiteral(_))
    {
        Some(&mut *right)
    } else if types.expression_type(right) == Some(DataType::Bytea)
        && matches!(left, Expr::StringLiteral(_))
    {
        Some(&mut *left)
    } else {
        None
    };
    if let Some(literal) = literal {
        *literal = Expr::Cast {
            expr: Box::new(std::mem::replace(literal, Expr::Null)),
            data_type: DataType::Bytea,
            source_type: None,
            identity: crate::sql::ast::CastIdentity::Scalar,
        };
    }
    let left = types.expression_type(left);
    let right = types.expression_type(right);
    if let (Some(left), Some(right)) = (&left, &right) {
        if !matches!(left, DataType::Null)
            && !matches!(right, DataType::Null)
            && (left == &DataType::Bytea || right == &DataType::Bytea)
            && left != right
        {
            return Err(CassieError::Planner(format!(
                "incompatible comparison operands: {left:?} and {right:?}"
            )));
        }
    }
    let family = |kind: Option<&DataType>| {
        kind.filter(|kind| **kind != DataType::Null)
            .map(super::super::validation::data_type_family)
    };
    super::super::validation::require_compatible_families(
        family(left.as_ref()),
        family(right.as_ref()),
        "comparison",
    )
}

fn contains_concatenation(expr: &Expr) -> bool {
    matches!(
        expr,
        Expr::Binary {
            op: BinaryOp::Concat | BinaryOp::ByteaConcat,
            ..
        }
    ) || expr
        .try_visit_children(|child| {
            if contains_concatenation(child) {
                Err(())
            } else {
                Ok(())
            }
        })
        .is_err()
}
