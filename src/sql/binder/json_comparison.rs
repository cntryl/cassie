//! Select native JSONB comparison signatures before scalar identity is erased.
use crate::app::CassieError;
use crate::sql::ast::{BinaryOp, CastIdentity, Expr, RowComparisonOp};
use crate::types::DataType;

pub(super) fn qualify(
    left: &mut Expr,
    op: &mut BinaryOp,
    right: &mut Expr,
    left_type: Option<&DataType>,
    right_type: Option<&DataType>,
) -> Result<(), CassieError> {
    let comparison = if let BinaryOp::JsonbCompare { comparison } = op {
        *comparison
    } else {
        if left_type != Some(&DataType::Jsonb) && right_type != Some(&DataType::Jsonb) {
            return Ok(());
        }
        let Some(comparison) = RowComparisonOp::from_binary(op) else {
            return Ok(());
        };
        comparison
    };
    for (expression, kind) in [(left, left_type), (right, right_type)] {
        if let Expr::StringLiteral(literal) = expression {
            serde_json::from_str::<serde::de::IgnoredAny>(literal)
                .map_err(|_| CassieError::Planner(format!("invalid JSON literal '{literal}'")))?;
            *expression = Expr::Cast {
                expr: Box::new(std::mem::replace(expression, Expr::Null)),
                data_type: DataType::Jsonb,
                source_type: None,
                identity: CastIdentity::Scalar,
            };
        } else if kind.is_some_and(|kind| !matches!(kind, DataType::Jsonb | DataType::Null)) {
            return Err(CassieError::Execution(
                "operator does not exist: jsonb comparison with incompatible operands".into(),
            ));
        }
    }
    *op = BinaryOp::JsonbCompare { comparison };
    Ok(())
}
