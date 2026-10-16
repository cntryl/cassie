//! Resolve concrete primitive prefix operators without guessing an integer width.
use crate::app::CassieError;
use crate::sql::ast::{Expr, UnaryNumericOp};
use crate::types::{DataType, IntegerWidth};

pub(super) fn resolve(
    expression: &mut Expr,
    data_type: Option<&DataType>,
) -> Result<(), CassieError> {
    let Expr::UnaryNumeric { op, expr } = expression else {
        return Ok(());
    };
    if is_unknown_operand(expr) {
        if *op == UnaryNumericOp::Minus {
            return Err(CassieError::Execution(
                "operator is not unique: - unknown".into(),
            ));
        }
        crate::sql::literal_inputs::validate(
            expr,
            &DataType::Float,
            crate::sql::ast::CastIdentity::Scalar,
        )?;
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
    if width.is_none() && !matches!(data_type, DataType::Float | DataType::Null) {
        return Err(CassieError::Execution(format!(
            "operator does not exist: {op:?} {data_type:?}"
        )));
    }
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
    if is_unknown_operand(operand) {
        (op == UnaryNumericOp::Plus).then_some(DataType::Float)
    } else {
        inferred
    }
}
