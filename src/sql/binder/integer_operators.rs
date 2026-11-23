//! Select the shared checked integer kernel from known operand domains.
use crate::app::CassieError;
use crate::sql::ast::BinaryOp;
use crate::types::{DataType, IntegerOperation, IntegerWidth};

pub(super) fn resolve(
    op: &mut BinaryOp,
    left: Option<&DataType>,
    right: Option<&DataType>,
) -> Result<bool, CassieError> {
    if matches!(op, BinaryOp::Modulo)
        && [left, right].into_iter().flatten().any(|kind| {
            matches!(
                kind,
                DataType::Float | DataType::Extended(crate::types::extended::ExtendedType::Real)
            )
        })
    {
        return Err(crate::app::DiagnosticError::new(
            "42883",
            format!(
                "operator does not exist: {} % {}",
                diagnostic_type_name(left),
                diagnostic_type_name(right),
            ),
        )
        .into());
    }
    if [left, right]
        .into_iter()
        .flatten()
        .any(crate::types::extended::operations::handles)
    {
        return Ok(true);
    }
    if matches!(op, BinaryOp::Concat | BinaryOp::ByteaConcat) {
        return super::string_operators::resolve(op, left, right).map(|()| true);
    }
    let (operation, selected) = match op {
        BinaryOp::Add => (IntegerOperation::Add, None),
        BinaryOp::Sub => (IntegerOperation::Sub, None),
        BinaryOp::Mul => (IntegerOperation::Mul, None),
        BinaryOp::Div => (IntegerOperation::Div, None),
        BinaryOp::Modulo => (IntegerOperation::Rem, None),
        BinaryOp::Bitwise { operation } => (*operation, None),
        BinaryOp::Integer { operation, width } => (*operation, Some(*width)),
        _ => return Ok(true),
    };
    let shift = matches!(operation, IntegerOperation::Shl | IntegerOperation::Shr);
    if shift
        && right.is_some_and(|kind| {
            !matches!(kind, DataType::SmallInt | DataType::Int | DataType::Null)
        })
    {
        return Err(CassieError::Execution(
            "operator does not exist: integer shift with non-integer count".into(),
        ));
    }
    let width = if shift {
        left.and_then(IntegerWidth::from_data_type)
            .or_else(|| (left == Some(&DataType::Null)).then_some(IntegerWidth::I32))
    } else {
        match (left, right) {
            (Some(DataType::Null), Some(right)) => IntegerWidth::from_data_type(right),
            (Some(left), Some(DataType::Null)) => IntegerWidth::from_data_type(left),
            (Some(left), Some(right)) => IntegerWidth::from_data_type(left)
                .zip(IntegerWidth::from_data_type(right))
                .map(|(left, right)| left.max(right)),
            _ => None,
        }
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

/// PostgreSQL spellings used in operator diagnostics, rather than Cassie's
/// internal carrier or SQL type-function names.
fn diagnostic_type_name(data_type: Option<&DataType>) -> String {
    let Some(data_type) = data_type else {
        return "unknown".to_owned();
    };
    match data_type {
        DataType::Null => "unknown".to_owned(),
        DataType::SmallInt => "smallint".to_owned(),
        DataType::Int => "integer".to_owned(),
        DataType::BigInt => "bigint".to_owned(),
        DataType::Float => "double precision".to_owned(),
        DataType::Char { .. } => "character".to_owned(),
        DataType::Varchar { .. } => "character varying".to_owned(),
        DataType::Array(element) => format!("{}[]", diagnostic_type_name(Some(element))),
        DataType::Extended(kind) => kind.type_name(),
        other => other.type_name(),
    }
}
