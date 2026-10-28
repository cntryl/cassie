//! Bounded JSON formatting and finite TEXT[] mutations.
use super::{QueryError, Value};
use crate::executor::retained_memory::{add, mul};
use crate::sql::functions::{self, json::JsonFunction, FunctionId};
use crate::types::{extended::TypeInputError, DataType};
use crate::{app::CassieError, runtime::QueryExecutionControls};

#[path = "json_nodes.rs"]
mod nodes;
#[path = "json_mutation_paths.rs"]
mod paths;

pub(super) fn evaluate(
    name: &str,
    args: &[Value],
    controls: Option<&QueryExecutionControls>,
) -> Option<Result<Value, QueryError>> {
    let metadata = functions::function(name)?;
    let FunctionId::Json(kind) = metadata.id else {
        return None;
    };
    if !matches!(
        kind,
        JsonFunction::Set
            | JsonFunction::SetLax
            | JsonFunction::Insert
            | JsonFunction::StripNulls(_)
            | JsonFunction::Pretty
    ) {
        return None;
    }
    if !metadata.arity.matches(args.len()) {
        return Some(Err(CassieError::Execution(format!(
            "function {name}(invalid arity) does not exist"
        ))
        .into()));
    }
    Some(run(kind, args, controls))
}

fn run(
    kind: JsonFunction,
    args: &[Value],
    controls: Option<&QueryExecutionControls>,
) -> Result<Value, QueryError> {
    check(controls)?;
    let lax = kind == JsonFunction::SetLax;
    if args
        .iter()
        .enumerate()
        .any(|(index, value)| matches!(value, Value::Null) && (!lax || matches!(index, 0 | 1 | 3)))
    {
        return Ok(Value::Null);
    }
    if lax
        && args
            .get(4)
            .is_some_and(|value| matches!(value, Value::Null))
    {
        return Err(treatment_error());
    }
    let mut bound = 0;
    let mut output = 0;
    for (index, value) in args.iter().enumerate() {
        let bytes = match value {
            Value::String(text) => text.len(),
            Value::Json(value) => crate::executor::retained_memory::serialized_json_bytes(value)?,
            _ => 64,
        };
        bound = add(bound, bytes)?;
        let native_document = index == 0 && kind != JsonFunction::StripNulls(false)
            || index == 2
                && matches!(
                    kind,
                    JsonFunction::Set | JsonFunction::SetLax | JsonFunction::Insert
                );
        let encoded = if native_document {
            match value {
                Value::String(text) => {
                    crate::types::jsonb::input_text_len(text).map_err(CassieError::TypeInput)?
                }
                Value::Json(value) => {
                    crate::types::jsonb::text_len(value).map_err(CassieError::TypeInput)?
                }
                _ => bytes,
            }
        } else {
            bytes
        };
        output = add(output, encoded)?;
    }
    let scratch = add(
        mul(
            crate::runtime::accounted::json::serialized_decode_bytes(bound)?,
            2,
        )?,
        mul(output.max(8), 8)?,
    )?;
    let _memory = controls
        .map(|controls| controls.reserve_query_memory(scratch))
        .transpose()?;
    match kind {
        JsonFunction::Pretty => {
            let source = super::json_functions::json_text(&args[0])?;
            let canonical =
                crate::types::jsonb::canonical(&source).map_err(CassieError::TypeInput)?;
            let node = nodes::Node::parse(&canonical)?;
            Ok(Value::String(node.render(true, controls)?))
        }
        JsonFunction::StripNulls(binary) => {
            let source = super::json_functions::json_text(&args[0])?;
            let mut node = nodes::Node::parse(&source)?;
            node.strip_nulls(flag(args.get(1), false)?, controls)?;
            let document = node.render(false, controls)?;
            if binary {
                canonical(&document, controls)
            } else {
                Ok(Value::String(document))
            }
        }
        JsonFunction::Set | JsonFunction::SetLax | JsonFunction::Insert => {
            mutate(kind, args, controls)
        }
        _ => unreachable!("finite JSON function dispatch"),
    }
}

fn mutate(
    kind: JsonFunction,
    args: &[Value],
    controls: Option<&QueryExecutionControls>,
) -> Result<Value, QueryError> {
    let mut document = parse(&args[0])?;
    let path_text = super::json_functions::json_text(&args[1])?;
    let path = crate::types::array::parse_text_array(&path_text, &DataType::Text)
        .map_err(|error| CassieError::ArrayInput(error.to_string()))?;
    let path = path.as_array().ok_or_else(|| {
        CassieError::InvalidParameterValue("JSON path must be a text array".into())
    })?;
    let option = flag(args.get(3), kind != JsonFunction::Insert)?;
    let (replacement, mode) = if kind == JsonFunction::SetLax && matches!(args[2], Value::Null) {
        match args.get(4) {
            None => (Some(serde_json::Value::Null), paths::Mode::Set(option)),
            Some(Value::String(treatment)) if treatment == "use_json_null" => {
                (Some(serde_json::Value::Null), paths::Mode::Set(option))
            }
            Some(Value::String(treatment)) if treatment == "delete_key" => {
                (None, paths::Mode::Delete)
            }
            Some(Value::String(treatment)) if treatment == "return_target" => {
                return formatted(&document, controls)
            }
            Some(Value::String(treatment)) if treatment == "raise_exception" => {
                return Err(CassieError::TypeInput(TypeInputError::new(
                    "22004",
                    "JSON value must not be null".into(),
                ))
                .into())
            }
            _ => return Err(treatment_error()),
        }
    } else {
        let replacement = Some(parse(&args[2])?);
        (
            replacement,
            if kind == JsonFunction::Insert {
                paths::Mode::Insert(option)
            } else {
                paths::Mode::Set(option)
            },
        )
    };
    paths::apply(&mut document, path, replacement, mode, controls)?;
    formatted(&document, controls)
}

fn parse(value: &Value) -> Result<serde_json::Value, QueryError> {
    match value {
        Value::Json(value) => Ok(value.clone()),
        value => serde_json::from_str(&super::json_functions::json_text(value)?)
            .map_err(|error| QueryError::General(format!("invalid JSON document: {error}"))),
    }
}
fn flag(value: Option<&Value>, default: bool) -> Result<bool, QueryError> {
    match value {
        None => Ok(default),
        Some(Value::Bool(value)) => Ok(*value),
        _ => Err(
            CassieError::InvalidParameterValue("JSON function flag must be boolean".into()).into(),
        ),
    }
}
fn canonical(
    document: &str,
    controls: Option<&QueryExecutionControls>,
) -> Result<Value, QueryError> {
    check(controls)?;
    let document = crate::types::jsonb::canonical(document).map_err(CassieError::TypeInput)?;
    check(controls)?;
    Ok(Value::String(document))
}
fn formatted(
    document: &serde_json::Value,
    controls: Option<&QueryExecutionControls>,
) -> Result<Value, QueryError> {
    check(controls)?;
    let document = crate::types::jsonb::format(document).map_err(CassieError::TypeInput)?;
    check(controls)?;
    Ok(Value::String(document))
}
fn treatment_error() -> QueryError {
    CassieError::InvalidParameterValue(
        "null_value_treatment must be delete_key, return_target, use_json_null, or raise_exception"
            .into(),
    )
    .into()
}
fn check(controls: Option<&QueryExecutionControls>) -> Result<(), QueryError> {
    controls.map_or(Ok(()), |controls| {
        controls.check_interruption().map_err(Into::into)
    })
}

#[cfg(test)]
#[path = "json_mutation_tests.rs"]
mod tests;
