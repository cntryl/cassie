//! Resolve concrete primitive prefix operators without guessing an integer width.
use crate::app::CassieError;
use crate::sql::ast::{Expr, UnaryNumericOp};
use crate::types::{DataType, IntegerWidth};
#[path = "unary_math.rs"]
mod math;

pub(super) fn resolve(
    expression: &mut Expr,
    data_type: Option<&DataType>,
) -> Result<(), CassieError> {
    if math::resolve(expression, data_type)? {
        return Ok(());
    }
    let Expr::UnaryNumeric { op, expr } = expression else {
        return Ok(());
    };
    validate_signature(*op, expr, data_type)?;
    if *op == UnaryNumericOp::Complement {
        return Ok(());
    }
    if is_unknown_operand(expr) {
        let Expr::UnaryNumeric { expr, .. } = std::mem::replace(expression, Expr::Null) else {
            unreachable!("numeric unary matched above")
        };
        *expression = Expr::Cast {
            expr,
            data_type: DataType::Float,
            source_type: None,
            identity: crate::sql::ast::CastIdentity::Scalar,
        };
        return Ok(());
    }
    let Some(data_type) = data_type else {
        return Ok(());
    };
    let width = IntegerWidth::from_data_type(data_type);
    if *op == UnaryNumericOp::Minus && width.is_none() {
        return Ok(());
    }
    let Expr::UnaryNumeric { expr, op } = std::mem::replace(expression, Expr::Null) else {
        unreachable!("numeric unary matched above")
    };
    *expression = if op == UnaryNumericOp::Plus {
        *expr
    } else {
        Expr::IntegerNegate {
            expr,
            width: width.expect("integer signature selected above"),
        }
    };
    Ok(())
}

/// Raw unknown constants select FLOAT8 for unary plus; unary minus is ambiguous.
pub(crate) fn is_unknown_operand(expression: &Expr) -> bool {
    matches!(expression, Expr::StringLiteral(_) | Expr::Null)
}
pub(super) fn result_type(
    op: UnaryNumericOp,
    operand: &Expr,
    inferred: Option<DataType>,
) -> Option<DataType> {
    if matches!(op, UnaryNumericOp::SquareRoot | UnaryNumericOp::CubeRoot)
        || op == UnaryNumericOp::Absolute && is_unknown_operand(operand)
    {
        return Some(DataType::Float);
    }
    if is_unknown_operand(operand) {
        (op == UnaryNumericOp::Plus).then_some(DataType::Float)
    } else if op == UnaryNumericOp::Complement {
        inferred
            .map(|kind| crate::types::extended::operations::complement_type(&kind).unwrap_or(kind))
    } else {
        inferred
    }
}

/// Check operator selection without rewriting or evaluating the operand.
pub(crate) fn validate_signature(
    op: UnaryNumericOp,
    operand: &Expr,
    data_type: Option<&DataType>,
) -> Result<(), CassieError> {
    if is_unknown_operand(operand) {
        if matches!(op, UnaryNumericOp::Minus | UnaryNumericOp::Complement) {
            return Err(CassieError::Execution(format!(
                "operator is not unique: {} unknown",
                if op == UnaryNumericOp::Minus {
                    "-"
                } else {
                    "~"
                }
            )));
        }
        return crate::sql::literal_inputs::validate(
            operand,
            &DataType::Float,
            crate::sql::ast::CastIdentity::Scalar,
        );
    }
    if let Some(data_type) = data_type {
        if op == UnaryNumericOp::Complement
            && crate::types::extended::operations::complement_type(data_type).is_some()
        {
            return Ok(());
        }
        if op == UnaryNumericOp::Complement
            && IntegerWidth::from_data_type(data_type).is_none()
            && *data_type != DataType::Null
        {
            return Err(CassieError::Execution(
                "operator does not exist: ~ non-integer".into(),
            ));
        }
        if IntegerWidth::from_data_type(data_type).is_none()
            && !matches!(data_type, DataType::Float | DataType::Null)
        {
            return Err(CassieError::Execution(format!(
                "operator does not exist: {op:?} {data_type:?}"
            )));
        }
    }
    Ok(())
}
