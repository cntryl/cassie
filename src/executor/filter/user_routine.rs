//! SQL routine argument and body evaluation under the invoking expression context.
use super::{
    cast_scalar, eval_scalar, execution_controls, merge_local_args, scalar_from_value, EvalContext,
    HashMap, QueryError, RowAccess, Value,
};

pub(super) fn evaluate_user_defined_function<R: RowAccess + ?Sized>(
    name: &str,
    row: &R,
    context: EvalContext<'_>,
    args: Vec<Value>,
) -> Result<Value, QueryError> {
    let lookup = name.to_ascii_lowercase();
    let Some(metadata) = context
        .user_functions
        .get(name)
        .or_else(|| context.user_functions.get(&lookup))
    else {
        return Err(QueryError::General(format!(
            "unsupported function '{name}'"
        )));
    };
    if args.len() != metadata.args.len() {
        return Err(QueryError::General(format!(
            "function '{name}' expects {} args, got {}",
            metadata.args.len(),
            args.len()
        )));
    }
    if let Some(memory) = context.memory {
        memory.grow(crate::executor::retained_memory::add(
            crate::executor::retained_memory::mul(metadata.body.len(), 128)?,
            1024,
        )?)?;
    }
    let body = crate::sql::parser::parse_expression(&metadata.body).map_err(|error| {
        QueryError::General(format!("invalid function body for '{name}': {error}"))
    })?;
    crate::sql::definition_guard::expression(&body).map_err(QueryError::Cassie)?;
    let locals = metadata
        .args
        .iter()
        .zip(args)
        .map(|(arg, value)| {
            let value = cast_scalar(
                &scalar_from_value(&value),
                &arg.data_type,
                execution_controls(context),
            )?;
            Ok((arg.name.to_ascii_lowercase(), value.to_value()))
        })
        .collect::<Result<HashMap<String, Value>, QueryError>>()?;
    let merged_args = merge_local_args(context.local_args, locals);
    if context.memory.is_some() {
        return super::super::evaluate_expr_value_with_context(
            row,
            &body,
            EvalContext {
                local_args: Some(&merged_args),
                ..context
            },
        )
        .and_then(|value| {
            cast_scalar(
                &scalar_from_value(&value),
                &metadata.return_type,
                execution_controls(context),
            )
        })
        .map(|value| value.to_value());
    }
    eval_scalar(
        row,
        &body,
        context.params,
        context.search_context,
        context.user_functions,
        Some(&merged_args),
        context.session,
    )
    .and_then(|value| cast_scalar(&value, &metadata.return_type, execution_controls(context)))
    .map(|value| value.to_value())
}
