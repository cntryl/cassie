//! Result types of concatenation, CASE branches and date/time expressions.
use super::{infer_expr_type, BinaryOp, DataType, Expr, FunctionCall, HashMap, Schema};

/// The result type of a date/time builtin, unless a user function shadows it.
pub(super) fn temporal_function_type(
    function: &FunctionCall,
    source_schema: &Schema,
    user_functions: &HashMap<String, crate::catalog::FunctionMeta>,
    parameter_types: &[i32],
) -> Option<DataType> {
    let name = function.name.to_ascii_lowercase();
    if user_functions.contains_key(&name) {
        return None;
    }
    let arg_type = |index: usize| {
        function.args.get(index).and_then(|argument| {
            infer_expr_type(argument, source_schema, user_functions, parameter_types)
        })
    };
    crate::sql::functions::temporal_return_type(&name, &function.args, &arg_type)
}

/// The result type of date/time arithmetic (`date + int`, `timestamp - timestamp`, ...).
pub(super) fn temporal_arithmetic_type(
    (left, op, right): (&Expr, &BinaryOp, &Expr),
    source_schema: &Schema,
    user_functions: &HashMap<String, crate::catalog::FunctionMeta>,
    parameter_types: &[i32],
) -> Option<DataType> {
    let left_type = infer_expr_type(left, source_schema, user_functions, parameter_types);
    let right_type = infer_expr_type(right, source_schema, user_functions, parameter_types);
    crate::sql::functions::temporal_binary_type(op, left_type.as_ref(), right_type.as_ref())
}

pub(super) fn infer_concatenation_type(
    left: &Expr,
    right: &Expr,
    source_schema: &Schema,
    user_functions: &HashMap<String, crate::catalog::FunctionMeta>,
    parameter_types: &[i32],
) -> Option<DataType> {
    let operands = [
        infer_expr_type(left, source_schema, user_functions, parameter_types),
        infer_expr_type(right, source_schema, user_functions, parameter_types),
    ];
    if operands
        .iter()
        .all(|kind| matches!(kind, Some(DataType::Null) | None))
        && (super::super::validation::expr_contains_parameters(left)
            || super::super::validation::expr_contains_parameters(right))
    {
        None
    } else if operands.contains(&Some(DataType::Bytea))
        && operands
            .iter()
            .all(|kind| matches!(kind, Some(DataType::Bytea | DataType::Null) | None))
    {
        Some(DataType::Bytea)
    } else {
        Some(DataType::Text)
    }
}

pub(crate) fn common_case_type(left: DataType, right: DataType) -> Option<DataType> {
    if left == DataType::Null {
        return Some(right);
    }
    if right == DataType::Null {
        return Some(left);
    }
    if left == right {
        return Some(left);
    }
    if let Some(common) = crate::types::extended::numeric_types::common(&left, &right) {
        return Some(common);
    }
    if matches!(
        left,
        DataType::Text | DataType::Char { .. } | DataType::Varchar { .. }
    ) && matches!(
        right,
        DataType::Text | DataType::Char { .. } | DataType::Varchar { .. }
    ) {
        return Some(DataType::Text);
    }
    if matches!(
        left,
        DataType::SmallInt | DataType::Int | DataType::BigInt | DataType::Float
    ) && matches!(
        right,
        DataType::SmallInt | DataType::Int | DataType::BigInt | DataType::Float
    ) {
        if left == DataType::Float || right == DataType::Float {
            return Some(DataType::Float);
        }
        if left == DataType::BigInt || right == DataType::BigInt {
            return Some(DataType::BigInt);
        }
        if left == DataType::Int || right == DataType::Int {
            return Some(DataType::Int);
        }
        return Some(DataType::SmallInt);
    }
    None
}
