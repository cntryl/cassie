//! Result types of operators and functions over network, bit-string and
//! `pg_lsn` operands, so described result OIDs match PostgreSQL.
use std::collections::HashMap;

use crate::sql::ast::{BinaryOp, Expr, FunctionCall};
use crate::types::extended::operations;
use crate::types::{DataType, Schema};

type Functions = HashMap<String, crate::catalog::FunctionMeta>;

/// The result type of `left op right` when an operand is an owned family.
pub(super) fn binary_type(
    left: &Expr,
    op: &BinaryOp,
    right: &Expr,
    schema: &Schema,
    functions: &Functions,
    parameters: &[i32],
) -> Option<DataType> {
    let operator = op.typed_operator()?;
    let left = super::inference::infer_expr_type(left, schema, functions, parameters)?;
    let right = super::inference::infer_expr_type(right, schema, functions, parameters)?;
    if !operations::handles(&left) && !operations::handles(&right) {
        return None;
    }
    operations::operator_type(operator, &left, &right)
}

/// The result type of a function call over owned family arguments.
pub(super) fn function_type(
    function: &FunctionCall,
    schema: &Schema,
    functions: &Functions,
    parameters: &[i32],
) -> Option<DataType> {
    if let Some(kind) = crate::sql::functions::hashing::return_type(&function.name) {
        return Some(kind);
    }
    let arguments: Vec<DataType> = function
        .args
        .iter()
        .map(|argument| {
            super::inference::infer_expr_type(argument, schema, functions, parameters)
                .unwrap_or(DataType::Null)
        })
        .collect();
    if !arguments.iter().any(operations::handles) && !operations::is_family_function(&function.name)
    {
        return None;
    }
    operations::function_type(&function.name, &arguments)
}
