//! JSON construction and conversion with admitted document/output scratch.
use super::{EvalContext, FunctionCall, QueryError, Value};
use crate::app::CassieError;
use crate::executor::batch::RowAccess;
use crate::executor::retained_memory::{add, mul};
use crate::sql::functions::{self, json::JsonFunction, FunctionId};
use crate::types::DataType;

#[path = "json_builder_values.rs"]
mod values;

pub(super) fn evaluate<R: RowAccess + ?Sized>(
    function: &FunctionCall,
    row: &R,
    args: &[Value],
    context: EvalContext<'_>,
) -> Option<Result<Value, QueryError>> {
    let FunctionId::Json(kind) = functions::function(&function.name)?.id else {
        return None;
    };
    match kind {
        JsonFunction::BuildObject(binary) => {
            if matches!(
                function.name.to_ascii_lowercase().as_str(),
                "json_object" | "jsonb_object"
            ) {
                Some(build_standard_object(function, row, args, context, binary))
            } else {
                Some(build(function, row, args, context, binary, true))
            }
        }
        JsonFunction::BuildArray(binary) => {
            if function.name.eq_ignore_ascii_case("json_array")
                && matches!(function.args.as_slice(), [crate::sql::ast::Expr::Subquery(subquery)] if matches!(subquery.kind, crate::sql::ast::SubqueryKind::Array))
            {
                Some(build_array_query(function, row, args, context))
            } else {
                Some(build(function, row, args, context, binary, false))
            }
        }
        JsonFunction::ToJson(binary) => Some(convert(function, row, args, context, binary)),
        _ => None,
    }
}

fn build_array_query<R: RowAccess + ?Sized>(
    function: &FunctionCall,
    row: &R,
    args: &[Value],
    context: EvalContext<'_>,
) -> Result<Value, QueryError> {
    let [Value::Array(array)] = args else {
        return Err(QueryError::General(
            "invalid JSON_ARRAY query result".into(),
        ));
    };
    let controls = super::super::temporal::context_controls(context);
    let element_type = argument_types(function, row, context)
        .into_iter()
        .next()
        .flatten()
        .and_then(|kind| match kind {
            DataType::Array(element) => Some(*element),
            _ => None,
        });
    let count = array
        .elements()
        .iter()
        .filter(|value| !value.is_null())
        .count();
    let bytes = array.elements().iter().try_fold(
        add(mul(array.elements().len(), 8)?, 2)?,
        |bytes, value| {
            if value.is_null() {
                Ok(bytes)
            } else {
                add(bytes, values::bound(value, element_type.as_ref(), false)?)
            }
        },
    )?;
    let _memory = reserve(controls, bytes)?;
    let mut elements = Vec::with_capacity(count);
    for value in array.elements().iter().filter(|value| !value.is_null()) {
        check(controls)?;
        elements.push(values::encode(value, element_type.as_ref(), controls)?);
    }
    finish(format!("[{}]", elements.join(", ")), false, controls)
}

fn build_standard_object(
    function: &FunctionCall,
    row: &(impl RowAccess + ?Sized),
    args: &[Value],
    context: EvalContext<'_>,
    binary: bool,
) -> Result<Value, QueryError> {
    let kinds = argument_types(function, row, context);
    let (keys, values) = match args {
        [pairs] => {
            let pairs = text_array(pairs, kinds.first().and_then(Option::as_ref))?;
            if !pairs.len().is_multiple_of(2) {
                return Err(CassieError::InvalidParameterValue(
                    "argument array must have even number of elements".into(),
                )
                .into());
            }
            let (pairs, _) = pairs.as_chunks::<2>();
            let (keys, values): (Vec<_>, Vec<_>) = pairs
                .iter()
                .map(|pair| (pair[0].clone(), pair[1].clone()))
                .unzip();
            (keys, values)
        }
        [key_values, value_values] => {
            let keys = text_array(key_values, kinds.first().and_then(Option::as_ref))?;
            let values = text_array(value_values, kinds.get(1).and_then(Option::as_ref))?;
            if keys.len() != values.len() {
                return Err(CassieError::InvalidParameterValue(
                    "JSON object keys and values arrays must have the same length".into(),
                )
                .into());
            }
            (keys, values)
        }
        _ => {
            return Err(CassieError::InvalidParameterValue(
                "JSON object requires one alternating or two parallel text arrays".into(),
            )
            .into());
        }
    };
    let controls = super::super::temporal::context_controls(context);
    let byte_count = keys
        .iter()
        .zip(&values)
        .try_fold(2, |bytes, (key, value)| {
            add(bytes, key.as_deref().map_or(0, str::len))
                .and_then(|bytes| add(bytes, value.as_deref().map_or(0, str::len)))
                .and_then(|bytes| add(bytes, 8))
        })?;
    let _memory = reserve(controls, byte_count)?;
    let mut elements = Vec::with_capacity(keys.len());
    for (key, value) in keys.iter().zip(&values) {
        check(controls)?;
        let (Some(key), Some(value)) = (key, value) else {
            return Err(CassieError::InvalidParameterValue(
                "JSON object keys and values must not be NULL".into(),
            )
            .into());
        };
        let key =
            serde_json::to_string(key).map_err(|error| QueryError::General(error.to_string()))?;
        let value =
            serde_json::to_string(value).map_err(|error| QueryError::General(error.to_string()))?;
        let separator = if binary { ": " } else { " : " };
        elements.push(format!("{key}{separator}{value}"));
    }
    finish(format!("{{{}}}", elements.join(", ")), binary, controls)
}

fn text_array(value: &Value, kind: Option<&DataType>) -> Result<Vec<Option<String>>, QueryError> {
    let Some(DataType::Array(element)) = kind else {
        return Err(CassieError::InvalidParameterValue(
            "JSON object requires one alternating or two parallel text arrays".into(),
        )
        .into());
    };
    if **element != DataType::Text {
        return Err(CassieError::InvalidParameterValue(
            "JSON object arrays must have TEXT elements".into(),
        )
        .into());
    }
    if let Value::Array(array) = value {
        if array.dimensions().len() > 1 {
            return Err(CassieError::InvalidParameterValue(
                "JSON object requires one-dimensional arrays".into(),
            )
            .into());
        }
        return array
            .elements()
            .iter()
            .map(|value| match value {
                Value::String(value) => Ok(Some(value.clone())),
                Value::Null => Ok(None),
                _ => Err(CassieError::InvalidParameterValue(
                    "JSON object arrays must have TEXT elements".into(),
                )
                .into()),
            })
            .collect();
    }
    let Value::String(input) = value else {
        return Err(CassieError::InvalidParameterValue(
            "JSON object requires one alternating or two parallel text arrays".into(),
        )
        .into());
    };
    let parsed = crate::types::array::parse_text_array(input, element)
        .map_err(|error| CassieError::ArrayInput(error.to_string()))?;
    let serde_json::Value::Array(values) = parsed else {
        return Err(CassieError::InvalidParameterValue(
            "JSON object requires one-dimensional arrays".into(),
        )
        .into());
    };
    values
        .into_iter()
        .map(|value| match value {
            serde_json::Value::String(value) => Ok(Some(value)),
            serde_json::Value::Null => Ok(None),
            _ => Err(CassieError::InvalidParameterValue(
                "JSON object arrays must have TEXT elements".into(),
            )
            .into()),
        })
        .collect()
}

fn build<R: RowAccess + ?Sized>(
    function: &FunctionCall,
    row: &R,
    args: &[Value],
    context: EvalContext<'_>,
    binary: bool,
    object: bool,
) -> Result<Value, QueryError> {
    if object && !args.len().is_multiple_of(2) {
        return Err(CassieError::InvalidParameterValue(
            "argument list must have even number of elements".into(),
        )
        .into());
    }
    let controls = super::super::temporal::context_controls(context);
    let _types_memory = admit_types(function, row, context, controls)?;
    let types = argument_types(function, row, context);
    let bound = args
        .iter()
        .zip(&types)
        .try_fold(add(mul(args.len(), 8)?, 2)?, |bytes, (value, kind)| {
            add(bytes, values::bound(value, kind.as_ref(), binary)?)
        })?;
    let _memory = reserve(controls, bound)?;
    let mut elements = Vec::with_capacity(if object { args.len() / 2 } else { args.len() });
    if object {
        for (pair, kinds) in args.as_chunks::<2>().0.iter().zip(types.as_chunks::<2>().0) {
            check(controls)?;
            let key = values::key(&pair[0], kinds[0].as_ref(), controls)?;
            let value = values::encode(&pair[1], kinds[1].as_ref(), controls)?;
            elements.push(format!("{key} : {value}"));
        }
    } else {
        for (value, kind) in args.iter().zip(&types) {
            check(controls)?;
            elements.push(values::encode(value, kind.as_ref(), controls)?);
        }
    }
    let document = if object {
        format!("{{{}}}", elements.join(", "))
    } else {
        format!("[{}]", elements.join(", "))
    };
    finish(document, binary, controls)
}

fn convert<R: RowAccess + ?Sized>(
    function: &FunctionCall,
    row: &R,
    args: &[Value],
    context: EvalContext<'_>,
    binary: bool,
) -> Result<Value, QueryError> {
    let [value] = args else {
        return Err(QueryError::General("invalid JSON conversion arity".into()));
    };
    if matches!(value, Value::Null) {
        return Ok(Value::Null);
    }
    let controls = super::super::temporal::context_controls(context);
    let _types_memory = admit_types(function, row, context, controls)?;
    let kind = super::super::super::array_order::expression_type(
        row,
        &function.args[0],
        context.user_functions,
    );
    let _memory = reserve(controls, values::bound(value, kind.as_ref(), binary)?)?;
    let document = values::encode(value, kind.as_ref(), controls)?;
    finish(document, binary, controls)
}

fn argument_types<R: RowAccess + ?Sized>(
    function: &FunctionCall,
    row: &R,
    context: EvalContext<'_>,
) -> Vec<Option<DataType>> {
    function
        .args
        .iter()
        .map(|expr| {
            super::super::super::array_order::expression_type(row, expr, context.user_functions)
        })
        .collect()
}

fn reserve(
    controls: Option<&crate::runtime::QueryExecutionControls>,
    bytes: usize,
) -> Result<Option<crate::runtime::QueryMemoryReservation>, QueryError> {
    check(controls)?;
    let bytes = add(
        crate::runtime::accounted::json::serialized_decode_bytes(bytes)?,
        mul(bytes.max(8), 8)?,
    )?;
    controls
        .map(|controls| controls.reserve_query_memory(bytes))
        .transpose()
        .map_err(Into::into)
}

fn finish(
    document: String,
    binary: bool,
    controls: Option<&crate::runtime::QueryExecutionControls>,
) -> Result<Value, QueryError> {
    check(controls)?;
    let document = if binary {
        crate::types::jsonb::canonical(&document).map_err(CassieError::TypeInput)?
    } else {
        document
    };
    check(controls)?;
    Ok(Value::String(document))
}

fn check(controls: Option<&crate::runtime::QueryExecutionControls>) -> Result<(), QueryError> {
    controls.map_or(Ok(()), |controls| {
        controls.check_interruption().map_err(Into::into)
    })
}

fn admit_types<R: RowAccess + ?Sized>(
    function: &FunctionCall,
    row: &R,
    context: EvalContext<'_>,
    controls: Option<&crate::runtime::QueryExecutionControls>,
) -> Result<Option<crate::runtime::QueryMemoryReservation>, QueryError> {
    check(controls)?;
    let bytes = function.args.iter().try_fold(0, |bytes, expr| {
        add(
            bytes,
            values::typing_bytes(row, expr, context.user_functions)?,
        )
    })?;
    controls
        .map(|controls| controls.reserve_query_memory(bytes))
        .transpose()
        .map_err(Into::into)
}

#[cfg(test)]
#[path = "json_builder_tests.rs"]
mod tests;
