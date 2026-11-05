//! Scalar call evaluation preserves argument order and context-owned scratch.
use super::{
    array_functions, datum_size, evaluate_builtin_function, evaluate_coalesce,
    evaluate_user_defined_function, json_builders, text_selection, CassieSession, EvalContext,
    FunctionCall, QueryError, RowAccess, Value,
};

pub(in crate::executor::filter) fn evaluate_function<R: RowAccess + ?Sized>(
    function: &FunctionCall,
    row: &R,
    context: EvalContext<'_>,
) -> Result<Value, QueryError> {
    let name = function.name.to_ascii_lowercase();
    if crate::sql::functions::is_column_size(&name) {
        return datum_size::evaluate(function, row, context);
    }
    if name == "coalesce" {
        return evaluate_coalesce(function, row, context);
    }

    if let Some(result) = super::regex_functions::evaluate_expression(function, row, context) {
        return result;
    }

    let args: Vec<Value> = function
        .args
        .iter()
        .enumerate()
        .map(|(index, arg)| {
            let value =
                text_selection::literal_argument(&name, index, arg, &function.args, context.params)
                    .or_else(|| {
                        super::super::conditional::evaluate_argument(&name, arg, row, context)
                    })
                    .unwrap_or_else(|| {
                        super::super::evaluate_expr_value_with_context(row, arg, context)
                    })?;
            if let Some(memory) = context.memory {
                memory.grow(crate::executor::retained_memory::mul(
                    crate::executor::retained_memory::value_clone_bytes(&value)?,
                    4,
                )?)?;
            }
            Ok::<Value, QueryError>(value)
        })
        .collect::<Result<Vec<_>, _>>()?;
    if let Some(memory) = context.memory {
        let bytes = args.iter().try_fold(1024, |bytes, value| {
            crate::executor::retained_memory::add(
                bytes,
                crate::executor::retained_memory::mul(
                    crate::executor::retained_memory::value_clone_bytes(value)?,
                    8,
                )?,
            )
        })?;
        memory.grow(bytes)?;
    }

    if name == crate::sql::ast::ARRAY_CONSTRUCTOR_FUNCTION {
        let nested = args.iter().any(|value| matches!(value, Value::Array(_)))
            || function.args.iter().any(|argument| match argument {
                super::Expr::Cast {
                    data_type: super::DataType::Array(_),
                    ..
                } => true,
                super::Expr::Column(name) => {
                    matches!(row.column_type(name), Some(super::DataType::Array(_)))
                }
                _ => false,
            });
        return crate::types::SqlArray::from_constructor_values(args, nested)
            .map(Value::Array)
            .map_err(|message| {
                if message == "number of array dimensions exceeds the maximum allowed (6)" {
                    return crate::app::CassieError::ResourceLimit(message).into();
                }
                QueryError::Cassie(crate::app::DiagnosticError::new("2202E",
                    "multidimensional arrays must have array expressions with matching dimensions"
                        .to_string(),
                ).into())
            });
    }

    if let Some(result) = json_builders::evaluate(function, row, &args, context) {
        return result;
    }
    if let Some(result) = array_functions::evaluate(
        name.strip_prefix("pg_catalog.").unwrap_or(&name),
        &args,
        context,
    ) {
        return result;
    }

    evaluate_builtin_function(
        &name,
        function,
        row,
        &args,
        context.search_context,
        context.session,
        context.controls.or_else(|| {
            context
                .session
                .and_then(CassieSession::statement_execution_controls)
        }),
    )
    .unwrap_or_else(|| evaluate_user_defined_function(&name, row, context, args))
}
