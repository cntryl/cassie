//! Options expansion with admitted argument scratch and retained output ownership.
use super::{Batch, BatchRow, Expr, QueryError, SourceExecutionEnv, Value};
use crate::executor::retained_memory::{add, lookup_bytes, mul, value_clone_bytes};
use std::mem::size_of;
use std::sync::Arc;

pub(super) fn execute(
    env: &SourceExecutionEnv<'_>,
    expression: &Expr,
    outer: Option<&BatchRow>,
) -> Result<Vec<Batch>, QueryError> {
    env.controls.check_interruption()?;
    let empty = BatchRow::from_projected_values(Vec::new());
    let row = outer.unwrap_or(&empty);
    let _scratch = env.controls.reserve_query_memory(add(
        scratch_bytes(expression, env.params, row)?,
        super::filter::cast_scratch_bytes(expression)?,
    )?)?;
    let value = super::filter::eval_scalar(
        row,
        expression,
        env.params,
        None,
        env.user_functions,
        None,
        env.session,
    )?;
    let super::filter::ScalarValue::Json(encoded) = value else {
        return if matches!(value, super::filter::ScalarValue::Null) {
            Ok(Vec::new())
        } else {
            Err(crate::app::CassieError::Execution(
                "function pg_options_to_table(...) does not exist".into(),
            )
            .into())
        };
    };
    let serde_json::Value::Array(values) = serde_json::from_str::<serde_json::Value>(&encoded)
        .map_err(|error| QueryError::General(format!("invalid logical option array: {error}")))?
    else {
        return Err(crate::app::CassieError::Execution(
            "function pg_options_to_table(...) does not exist".into(),
        )
        .into());
    };
    let mut bytes = mul(
        values.len(),
        size_of::<BatchRow>() + 2 * size_of::<(String, Value)>() + 23 + lookup_bytes(2, 23)?,
    )?;
    for value in &values {
        env.controls.check_interruption()?;
        match value {
            serde_json::Value::Null => return Err(crate::app::CassieError::NullArrayElement.into()),
            serde_json::Value::String(text) => bytes = add(bytes, text.len())?,
            _ => {
                return Err(crate::app::CassieError::Unsupported(
                    "higher-dimensional option arrays remain pending under #793".into(),
                )
                .into())
            }
        }
    }
    let memory = Arc::new(env.controls.reserve_query_memory(bytes)?);
    let mut output = Vec::with_capacity(values.len());
    for value in &values {
        env.controls.check_interruption()?;
        let text = value.as_str().expect("validated text array");
        let (name, value) = text
            .split_once('=')
            .map_or((text, Value::Null), |(name, value)| {
                (name, Value::String(value.to_owned()))
            });
        output.push(
            BatchRow::from_projected_values(vec![
                ("option_name".into(), Value::String(name.to_owned())),
                ("option_value".into(), value),
            ])
            .with_query_memory(Some(Arc::clone(&memory))),
        );
    }
    Ok(vec![output])
}

fn scratch_bytes(expression: &Expr, params: &[Value], row: &BatchRow) -> Result<usize, QueryError> {
    match expression {
        Expr::Column(name) => Ok(add(
            mul(
                value_clone_bytes(row.get(name).unwrap_or(&Value::Null))?,
                32,
            )?,
            1024,
        )?),
        Expr::Cast { expr, .. } => Ok(add(1024, scratch_bytes(expr, params, row)?)?),
        _ => super::source_values::scratch_bytes(expression, params),
    }
}
