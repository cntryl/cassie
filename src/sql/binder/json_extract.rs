//! Resolve the finite JSON extraction signatures before scalar type erasure.
use crate::app::CassieError;
use crate::sql::ast::{BinaryOp, CastIdentity, Expr};
use crate::types::DataType;

pub(super) fn qualify(
    left: &mut Expr,
    op: &mut BinaryOp,
    right: &mut Expr,
    left_type: Option<&DataType>,
    right_type: Option<&DataType>,
) -> Result<(), CassieError> {
    let BinaryOp::JsonExtract { path, jsonb, .. } = op else {
        return Ok(());
    };
    *jsonb = left_type == Some(&DataType::Jsonb) || matches!(left, Expr::StringLiteral(_));
    if !matches!(left, Expr::StringLiteral(_) | Expr::Null)
        && left_type
            .is_some_and(|kind| !matches!(kind, DataType::Json | DataType::Jsonb | DataType::Null))
    {
        return Err(incompatible_operands());
    }
    if matches!(left, Expr::StringLiteral(_)) {
        cast_literal(left, DataType::Jsonb);
    }
    if matches!(right, Expr::Null) {
        return Ok(());
    }
    if *path {
        if matches!(right, Expr::StringLiteral(_)) {
            cast_literal(right, DataType::Array(Box::new(DataType::Text)));
        } else if right_type.is_some_and(|kind| {
            !matches!(kind, DataType::Null)
                && !matches!(kind, DataType::Array(element) if **element == DataType::Text)
        }) {
            return Err(incompatible_operands());
        }
    } else if right_type.is_some_and(|kind| {
        !matches!(
            kind,
            DataType::Text
                | DataType::Varchar { .. }
                | DataType::SmallInt
                | DataType::Int
                | DataType::Null
        )
    }) {
        return Err(incompatible_operands());
    }
    Ok(())
}

fn cast_literal(expression: &mut Expr, data_type: DataType) {
    *expression = Expr::Cast {
        expr: Box::new(std::mem::replace(expression, Expr::Null)),
        data_type,
        source_type: None,
        identity: CastIdentity::Scalar,
    };
}

fn incompatible_operands() -> CassieError {
    CassieError::Execution(
        "operator does not exist: JSON extraction with incompatible operands".into(),
    )
}
