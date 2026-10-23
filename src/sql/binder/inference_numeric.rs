//! Result types of operators and functions over the `numeric` family.
use super::{infer_expr_type, is_integer_type, DataType, Expr, FunctionCall, HashMap, Schema};

pub(super) const NUMERIC: DataType =
    DataType::Extended(crate::types::extended::ExtendedType::Numeric(None));

/// `^`: `numeric` when an exact operand selects it, else `double precision`.
pub(super) fn power_type(
    left: &Expr,
    right: &Expr,
    source_schema: &Schema,
    user_functions: &HashMap<String, crate::catalog::FunctionMeta>,
    parameter_types: &[i32],
) -> DataType {
    numeric_family_arithmetic(
        left,
        right,
        true,
        source_schema,
        user_functions,
        parameter_types,
    )
    .unwrap_or(DataType::Float)
}

/// Result types of `power`, `avg` and the math functions, which depend on
/// whether an argument is `numeric`, `real` or `double precision`.
pub(super) fn numeric_function_type(
    function: &FunctionCall,
    source_schema: &Schema,
    user_functions: &HashMap<String, crate::catalog::FunctionMeta>,
    parameter_types: &[i32],
) -> Option<DataType> {
    use crate::sql::functions::FunctionId;
    let name = function.name.to_ascii_lowercase();
    // A declared routine of the same name and arity overrides the builtin.
    if user_functions.get(&name).is_some_and(|routine| {
        !crate::sql::functions::has_distinct_builtin_arity(
            &name,
            function.args.len(),
            routine.args.len(),
        )
    }) {
        return None;
    }
    let metadata = crate::sql::functions::function(&name)?;
    let arguments = || {
        function
            .args
            .iter()
            .map(|argument| {
                infer_expr_type(argument, source_schema, user_functions, parameter_types)
                    .unwrap_or(DataType::Null)
            })
            .collect::<Vec<_>>()
    };
    match metadata.id {
        FunctionId::Power => match arguments().as_slice() {
            [left, right] => Some(
                crate::types::extended::numeric_types::arithmetic(left, right, true)
                    .unwrap_or(DataType::Float),
            ),
            _ => None,
        },
        FunctionId::Avg => Some(
            function
                .args
                .first()
                .and_then(|expr| {
                    infer_expr_type(expr, source_schema, user_functions, parameter_types)
                })
                .map_or(DataType::Float, |data_type| {
                    crate::types::extended::numeric_types::avg(&data_type)
                }),
        ),
        id if crate::sql::functions::statistical::is_statistical_function(id) => Some(
            function
                .args
                .first()
                .and_then(|expr| {
                    infer_expr_type(expr, source_schema, user_functions, parameter_types)
                })
                .map_or(DataType::Float, |data_type| {
                    crate::types::extended::numeric_types::statistical(&data_type)
                }),
        ),
        FunctionId::Math(math) => {
            Some(crate::sql::functions::math::result_type(math, &arguments()))
        }
        _ => None,
    }
}

/// `+ - * / %` and bitwise result types: the numeric family first, then
/// Cassie's integer/float carriers.
pub(super) fn arithmetic_type(
    left: &Expr,
    right: &Expr,
    modulo: bool,
    source_schema: &Schema,
    user_functions: &HashMap<String, crate::catalog::FunctionMeta>,
    parameter_types: &[i32],
) -> DataType {
    if let Some(data_type) = numeric_family_arithmetic(
        left,
        right,
        modulo,
        source_schema,
        user_functions,
        parameter_types,
    ) {
        return data_type;
    }
    let left_type = infer_expr_type(left, source_schema, user_functions, parameter_types);
    let right_type = infer_expr_type(right, source_schema, user_functions, parameter_types);
    if left_type.as_ref().is_some_and(is_integer_type)
        && right_type.as_ref().is_some_and(is_integer_type)
    {
        DataType::BigInt
    } else {
        DataType::Float
    }
}

/// The result type when an arithmetic operand is `numeric` or `real`.
fn numeric_family_arithmetic(
    left: &Expr,
    right: &Expr,
    power_or_modulo: bool,
    source_schema: &Schema,
    user_functions: &HashMap<String, crate::catalog::FunctionMeta>,
    parameter_types: &[i32],
) -> Option<DataType> {
    let left = infer_expr_type(left, source_schema, user_functions, parameter_types)
        .unwrap_or(DataType::Null);
    let right = infer_expr_type(right, source_schema, user_functions, parameter_types)
        .unwrap_or(DataType::Null);
    crate::types::extended::numeric_types::arithmetic(&left, &right, power_or_modulo)
}
