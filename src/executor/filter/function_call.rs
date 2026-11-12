//! Scalar call evaluation preserves argument order and context-owned scratch.
#[cfg(test)]
#[path = "json_object_keys_argument_tests.rs"]
mod json_object_keys_argument_tests;

use super::{
    array_functions, datum_size, evaluate_builtin_function, evaluate_coalesce,
    evaluate_user_defined_function, json_builders, text_selection, EvalContext, FunctionCall,
    QueryError, RowAccess, Value,
};

#[cfg(test)]
thread_local! {
    static INPUT_VALUE_CLONES: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}
#[cfg(test)]
pub(in crate::executor::filter) fn record_input_value_clone(present: bool) {
    if present {
        INPUT_VALUE_CLONES.with(|count| count.set(count.get().saturating_add(1)));
    }
}
#[cfg(test)]
fn take_input_value_clone_count() -> usize {
    INPUT_VALUE_CLONES.with(|count| count.replace(0))
}

pub(in crate::executor::filter) fn evaluate_function<R: RowAccess + ?Sized>(
    function: &FunctionCall,
    row: &R,
    context: EvalContext<'_>,
) -> Result<Value, QueryError> {
    let name = function.name.to_ascii_lowercase();
    let local_name = name.rsplit('.').next().unwrap_or(&name);
    let argument_context = if context.memory.is_none()
        && matches!(
            local_name,
            "power" | "pow" | "to_char" | "to_date" | "to_timestamp"
        ) {
        EvalContext {
            memory: context.deletion_memory,
            ..context
        }
    } else {
        context
    };
    let mut argument_memory_scope = if matches!(
        local_name,
        "power" | "pow" | "to_char" | "to_date" | "to_timestamp"
    ) {
        argument_context
            .memory
            .map(|memory| memory.scope(0))
            .transpose()?
    } else {
        None
    };
    if name == crate::sql::ast::VARIADIC_ARRAY_ARGUMENT_FUNCTION {
        let [array] = function.args.as_slice() else {
            return Err(crate::app::CassieError::Execution(
                "invalid internal VARIADIC array argument".into(),
            )
            .into());
        };
        return super::super::evaluate_expr_value_with_context(row, array, context);
    }
    if crate::sql::functions::is_column_size(&name) {
        return datum_size::evaluate(function, row, context);
    }
    if name == "coalesce" {
        return evaluate_coalesce(function, row, context);
    }

    if let Some(result) = super::regex_functions::evaluate_expression(function, row, context) {
        return result;
    }
    if let Some(result) = borrowed_object_keys_argument(&name, function, row, context) {
        return result;
    }

    let args = evaluate_argument_values(function, row, argument_context, &name)?;

    if name == "width_bucket"
        && context
            .user_functions
            .get(&name)
            .is_some_and(|metadata| metadata.args.len() == function.args.len())
    {
        return evaluate_user_defined_function(&name, row, context, args);
    }

    if let Some(memory) = context.memory {
        let bytes = args.iter().try_fold(1024, |bytes, value| {
            crate::executor::retained_memory::add(
                bytes,
                crate::executor::retained_memory::mul(
                    super::super::cast_admission::value_workspace_bytes(value)?,
                    8,
                )?,
            )
        })?;
        memory.grow(bytes)?;
    }

    if name == crate::sql::ast::ARRAY_CONSTRUCTOR_FUNCTION {
        return evaluate_array_constructor(function, row, args);
    }
    if name == crate::sql::ast::ROW_CONSTRUCTOR_FUNCTION {
        return Ok(evaluate_row_constructor(args));
    }
    if name == crate::sql::ast::FIELD_ACCESS_FUNCTION {
        return evaluate_field_access(function, args);
    }
    reject_record_arguments(local_name, &args)?;

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

    let result = evaluate_builtin_function(&name, function, row, &args, context)
        .unwrap_or_else(|| evaluate_user_defined_function(&name, row, context, args))?;
    let retained_bytes = match (&result, local_name) {
        (Value::String(text), "to_char") => text.capacity(),
        (Value::Typed(value @ crate::types::typed::TypedValue::Numeric(_)), "power" | "pow") => {
            value.heap_bytes()
        }
        _ => 0,
    };
    if retained_bytes > 0 {
        if let Some(scope) = argument_memory_scope.as_mut() {
            scope.retain_existing(retained_bytes)?;
        }
    }
    Ok(result)
}

fn evaluate_argument_values<R: RowAccess + ?Sized>(
    function: &FunctionCall,
    row: &R,
    context: EvalContext<'_>,
    name: &str,
) -> Result<Vec<Value>, QueryError> {
    function
        .args
        .iter()
        .enumerate()
        .map(|(index, argument)| {
            if matches!(argument, super::Expr::StringLiteral(_)) {
                super::super::evaluation::admit_leaf(row, argument, context)?;
            }
            let value = text_selection::literal_argument(
                name,
                index,
                argument,
                &function.args,
                context.params,
            )
            .or_else(|| super::super::conditional::evaluate_argument(name, argument, row, context))
            .unwrap_or_else(|| {
                super::super::evaluate_expr_value_with_context(row, argument, context)
            })?;
            if let Some(memory) = context.memory {
                memory.grow(crate::executor::retained_memory::mul(
                    super::super::cast_admission::value_workspace_bytes(&value)?,
                    4,
                )?)?;
            }
            Ok::<Value, QueryError>(value)
        })
        .collect()
}

/// Built-ins that accept a record argument; every other function over a
/// record is undefined, as in PostgreSQL (`function abs(record) does not exist`).
const RECORD_TOLERANT_FUNCTIONS: &[&str] = &[
    "pg_typeof",
    "nullif",
    "greatest",
    "least",
    "num_nulls",
    "num_nonnulls",
    "to_json",
    "to_jsonb",
    "row_to_json",
    "json_build_object",
    "jsonb_build_object",
    "json_build_array",
    "jsonb_build_array",
];

fn reject_record_arguments(name: &str, args: &[Value]) -> Result<(), QueryError> {
    let is_record =
        |value: &Value| matches!(value, Value::Typed(typed) if typed.as_record().is_some());
    if !args.iter().any(is_record) || RECORD_TOLERANT_FUNCTIONS.contains(&name) {
        return Ok(());
    }
    let types = args
        .iter()
        .map(|value| match value {
            Value::Typed(typed) => typed.as_record().map_or_else(
                || crate::types::composite::value_data_type(value).type_name(),
                |record| record.descriptor().name.clone(),
            ),
            other => crate::types::composite::value_data_type(other).type_name(),
        })
        .collect::<Vec<_>>()
        .join(", ");
    Err(crate::app::CassieError::from(
        crate::app::DiagnosticError::new(
            "42883",
            format!("function {name}({types}) does not exist"),
        )
        .with_hint(
            "No function matches the given name and argument types. You might need to add explicit type casts.",
        ),
    )
    .into())
}

/// `ROW(a, b, ...)`: an anonymous record whose field types follow the values.
fn evaluate_row_constructor(args: Vec<Value>) -> Value {
    let fields = args
        .into_iter()
        .map(|value| {
            let data_type = crate::types::composite::value_data_type(&value);
            (value, data_type)
        })
        .collect();
    Value::Typed(crate::types::typed::TypedValue::Record(
        crate::types::RecordValue::anonymous(fields),
    ))
}

/// `(record).field`: NULL for a NULL record, the named field otherwise.
fn evaluate_field_access(function: &FunctionCall, args: Vec<Value>) -> Result<Value, QueryError> {
    let Some(super::Expr::StringLiteral(field)) = function.args.get(1) else {
        return Err(QueryError::General(
            "field selection requires a field name".into(),
        ));
    };
    match args.into_iter().next() {
        None | Some(Value::Null) => Ok(Value::Null),
        Some(Value::Typed(crate::types::typed::TypedValue::Record(record))) => {
            record.field(field).cloned().ok_or_else(|| {
                crate::app::CassieError::from(crate::app::DiagnosticError::new(
                    "42703",
                    format!(
                        "column \"{field}\" not found in data type {}",
                        record.descriptor().name
                    ),
                ))
                .into()
            })
        }
        Some(_) => Err(
            crate::app::CassieError::from(crate::app::DiagnosticError::new(
                "42809",
                format!("column notation .{field} applied to a non-composite value"),
            ))
            .into(),
        ),
    }
}

fn evaluate_array_constructor<R: RowAccess + ?Sized>(
    function: &FunctionCall,
    row: &R,
    args: Vec<Value>,
) -> Result<Value, QueryError> {
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
    crate::types::SqlArray::from_constructor_values(args, nested)
        .map(Value::Array)
        .map_err(|message| {
            if message == "number of array dimensions exceeds the maximum allowed (6)" {
                return crate::app::CassieError::ResourceLimit(message).into();
            }
            QueryError::Cassie(
                crate::app::DiagnosticError::new(
                    "2202E",
                    "multidimensional arrays must have array expressions with matching dimensions"
                        .to_string(),
                )
                .into(),
            )
        })
}

fn borrowed_object_keys_argument<R: RowAccess + ?Sized>(
    name: &str,
    function: &FunctionCall,
    row: &R,
    context: EvalContext<'_>,
) -> Option<Result<Value, QueryError>> {
    if !matches!(name, "json_object_keys" | "jsonb_object_keys") || context.memory.is_some() {
        return None;
    }
    let [argument @ (super::Expr::Column(_) | super::Expr::Param(_))] = function.args.as_slice()
    else {
        return None;
    };
    let owner = context.deletion_memory?;
    let controls = context.controls.or_else(|| {
        context
            .session
            .and_then(super::CassieSession::statement_execution_controls)
    })?;
    Some((|| {
        let argument_scratch = context
            .session
            .map(|session| session.statement_text_scratch(argument))
            .transpose()?;
        super::super::evaluation::admit_leaf(row, argument, context)?;
        let null = Value::Null;
        let value = match argument {
            super::Expr::Column(name) => {
                super::super::column_value_ref(row, name, context.local_args)
            }
            super::Expr::Param(index) => context.params.get(*index),
            _ => unreachable!("direct argument selected above"),
        }
        .unwrap_or(&null);
        drop(argument_scratch);
        super::json_functions::evaluate_with_owner(
            name,
            std::slice::from_ref(value),
            Some(controls),
            Some(owner),
        )
        .expect("selected JSON object-key builtin")
    })())
}
