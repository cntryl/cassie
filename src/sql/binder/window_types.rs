use std::collections::HashMap;

use crate::catalog::FunctionMeta;
use crate::sql::ast::{FunctionCall, WindowFunctionCall};
use crate::types::{DataType, Schema};

pub(crate) fn infer_window_return_type(
    function: &WindowFunctionCall,
    source_schema: &Schema,
    user_functions: &HashMap<String, FunctionMeta>,
    parameter_types: &[i32],
) -> DataType {
    if matches!(
        function.name.to_ascii_lowercase().as_str(),
        "lag" | "lead" | "first_value" | "last_value"
    ) {
        return function
            .args
            .first()
            .and_then(|arg| {
                super::inference::infer_expr_type(
                    arg,
                    source_schema,
                    user_functions,
                    parameter_types,
                )
            })
            .unwrap_or(DataType::Text);
    }
    let aggregate = FunctionCall {
        name: function.name.clone(),
        args: function.args.clone(),
        ..FunctionCall::default()
    };
    super::inference::infer_function_return_type(
        &aggregate,
        source_schema,
        user_functions,
        parameter_types,
    )
    .unwrap_or(DataType::BigInt)
}
