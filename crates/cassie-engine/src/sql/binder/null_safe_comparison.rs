//! Resolve the finite primitive equality signature before NULL evaluation.
use super::{BinaryOp, CassieError, DataType, Expr, ResultTypes};

pub(super) fn qualify(
    op: &BinaryOp,
    left: &mut Expr,
    right: &mut Expr,
    types: &ResultTypes,
) -> Result<(), CassieError> {
    if !matches!(op, BinaryOp::IsDistinctFrom | BinaryOp::IsNotDistinctFrom) {
        return Ok(());
    }
    let known = |expression: &Expr| {
        if matches!(expression, Expr::StringLiteral(_) | Expr::Null) {
            None
        } else {
            types.expression_type(expression).filter(primitive)
        }
    };
    let left_type = known(left);
    let right_type = known(right);
    if let Some(kind) = &right_type {
        coerce_unknown(left, kind);
    }
    if let Some(kind) = &left_type {
        coerce_unknown(right, kind);
    }
    if let (Some(left), Some(right)) = (left_type, right_type) {
        if super::super::validation::data_type_family(&left)
            != super::super::validation::data_type_family(&right)
        {
            return Err(CassieError::Execution(format!(
                "operator does not exist: {} = {}",
                left.type_name(),
                right.type_name()
            )));
        }
    }
    Ok(())
}

fn primitive(kind: &DataType) -> bool {
    matches!(
        kind,
        DataType::SmallInt
            | DataType::Int
            | DataType::BigInt
            | DataType::Float
            | DataType::Boolean
            | DataType::Text
            | DataType::Char { .. }
            | DataType::Varchar { .. }
    )
}

fn coerce_unknown(expression: &mut Expr, kind: &DataType) {
    // Text and character signatures keep their existing trailing-blank policy.
    if !matches!(expression, Expr::StringLiteral(_))
        || matches!(
            kind,
            DataType::Text | DataType::Char { .. } | DataType::Varchar { .. }
        )
    {
        return;
    }
    *expression = Expr::Cast {
        expr: Box::new(std::mem::replace(expression, Expr::Null)),
        data_type: kind.clone(),
        source_type: Some(DataType::Text),
        identity: crate::sql::ast::CastIdentity::Scalar,
    };
}
