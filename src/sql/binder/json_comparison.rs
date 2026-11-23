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
    if let Some(comparison) = RowComparisonOp::from_binary(op) {
        if left_type == Some(&DataType::Json) || right_type == Some(&DataType::Json) {
            let type_name = |expression: &Expr, kind: Option<&DataType>| {
                if matches!(expression, Expr::StringLiteral(_)) {
                    "unknown".to_owned()
                } else {
                    kind.map_or_else(|| "unknown".to_owned(), DataType::type_name)
                }
            };
            return Err(CassieError::Execution(format!(
                "operator does not exist: {} {} {}",
                type_name(left, left_type),
                comparison.symbol(),
                type_name(right, right_type),
            )));
        }
    }
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
    for (index, (expression, kind)) in [(left, left_type), (right, right_type)]
        .into_iter()
        .enumerate()
    {
        if let Expr::StringLiteral(literal) = expression {
            serde_json::from_str::<serde::de::IgnoredAny>(literal).map_err(|error| {
                crate::app::DiagnosticError::new("22P02", "invalid input syntax for type json")
                    .with_detail(error.to_string())
                    .into()
            })?;
            *expression = Expr::Cast {
                expr: Box::new(std::mem::replace(expression, Expr::Null)),
                data_type: DataType::Jsonb,
                source_type: None,
                identity: CastIdentity::Scalar,
            };
        } else if kind.is_some_and(|kind| !matches!(kind, DataType::Jsonb | DataType::Null)) {
            let type_name = kind.map_or_else(|| "unknown".to_owned(), DataType::type_name);
            let (left_name, right_name) = if index == 0 {
                (
                    type_name,
                    right_type.map_or_else(|| "unknown".to_owned(), DataType::type_name),
                )
            } else {
                (
                    left_type.map_or_else(|| "unknown".to_owned(), DataType::type_name),
                    type_name,
                )
            };
            return Err(CassieError::Execution(format!(
                "operator does not exist: {left_name} {} {right_name}",
                comparison.symbol(),
            )));
        }
    }
    *op = BinaryOp::JsonbCompare { comparison };
    Ok(())
}
