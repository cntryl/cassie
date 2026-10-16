//! Select the shared checked integer kernel from known operand domains.
use crate::app::CassieError;
use crate::sql::ast::BinaryOp;
use crate::types::{DataType, IntegerOperation, IntegerWidth};

pub(super) fn resolve(
    op: &mut BinaryOp,
    left: Option<&DataType>,
    right: Option<&DataType>,
) -> Result<bool, CassieError> {
    let (operation, selected) = match op {
        BinaryOp::Add => (IntegerOperation::Add, None),
        BinaryOp::Sub => (IntegerOperation::Sub, None),
        BinaryOp::Mul => (IntegerOperation::Mul, None),
        BinaryOp::Div => (IntegerOperation::Div, None),
        BinaryOp::Integer { operation, width } => (*operation, Some(*width)),
        _ => return Ok(true),
    };
    let width = match (left, right) {
        (Some(DataType::Null), Some(right)) => IntegerWidth::from_data_type(right),
        (Some(left), Some(DataType::Null)) => IntegerWidth::from_data_type(left),
        (Some(left), Some(right)) => IntegerWidth::from_data_type(left)
            .zip(IntegerWidth::from_data_type(right))
            .map(|(left, right)| left.max(right)),
        _ => None,
    };
    let Some(width) = width else {
        // A previously selected signature survives an unresolved analysis
        // pass; later concrete operands still have to match that signature.
        if selected.is_some() {
            if left.is_none()
                || right.is_none()
                || (left == Some(&DataType::Null) && right == Some(&DataType::Null))
            {
                return Ok(true);
            }
            return Err(CassieError::Planner(
                "resolved integer operator does not match its operand types".into(),
            ));
        }
        return Ok(false);
    };
    if selected.is_some_and(|selected| selected != width) {
        return Err(CassieError::Planner(
            "resolved integer operator does not match its operand types".into(),
        ));
    }
    *op = BinaryOp::Integer { operation, width };
    Ok(true)
}
