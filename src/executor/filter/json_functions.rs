//! Bounded JSON inspection, preserving SQL NULL versus document null.
use super::super::{json_key_exists, scalar_from_value, BinaryOp, QueryError, ScalarValue, Value};
use crate::app::CassieError;
use crate::runtime::QueryExecutionControls;
use crate::sql::functions::{self, json::JsonFunction, FunctionId};
use std::borrow::Cow;

#[path = "json_unique_keys.rs"]
mod unique_keys;

pub(super) fn evaluate(
    name: &str,
    args: &[Value],
    controls: Option<&QueryExecutionControls>,
) -> Option<Result<Value, QueryError>> {
    let FunctionId::Json(kind) = functions::function(name)?.id else {
        return None;
    };
    if matches!(kind, JsonFunction::SqlInput) {
        return Some(sql_input(args, controls));
    }
    matches!(
        kind,
        JsonFunction::Typeof(_)
            | JsonFunction::ArrayLength(_)
            | JsonFunction::Exists(_)
            | JsonFunction::ObjectKeys(_)
    )
    .then(|| inspect(kind, args, controls))
}

fn sql_input(
    args: &[Value],
    controls: Option<&QueryExecutionControls>,
) -> Result<Value, QueryError> {
    let [input, unique] = args else {
        return Err(QueryError::General("invalid JSON input arity".into()));
    };
    if input.is_null() || unique.is_null() {
        return Ok(Value::Null);
    }
    let Some(input) = input.as_str() else {
        return Err(CassieError::InvalidParameterValue("JSON input requires text".into()).into());
    };
    let Value::Bool(unique) = unique else {
        return Err(CassieError::InvalidParameterValue(
            "JSON unique-key option requires BOOLEAN".into(),
        )
        .into());
    };
    let reservation = controls
        .map(|controls| controls.reserve_query_memory(input.len().saturating_mul(4)))
        .transpose()?;
    if let Some(controls) = controls {
        controls.check_interruption()?;
    }
    let parsed = serde_json::value::RawValue::from_string(input.to_owned()).map_err(|error| {
        CassieError::TypeInput(crate::types::extended::TypeInputError::new(
            "22P02",
            error.to_string(),
        ))
    })?;
    if *unique {
        unique_keys::validate(input).map_err(|error| {
            let message = if error.starts_with("duplicate JSON object key value") {
                "duplicate JSON object key value".to_owned()
            } else {
                error
            };
            CassieError::from(crate::app::DiagnosticError::new("22030", message))
        })?;
    }
    if let Some(controls) = controls {
        controls.check_interruption()?;
    }
    drop(reservation);
    Ok(Value::String(parsed.get().to_owned()))
}

fn inspect(
    kind: JsonFunction,
    args: &[Value],
    controls: Option<&QueryExecutionControls>,
) -> Result<Value, QueryError> {
    let expected = if matches!(kind, JsonFunction::Exists(_)) {
        2
    } else {
        1
    };
    if args.len() != expected {
        return Err(QueryError::General("invalid JSON inspection arity".into()));
    }
    if args.iter().any(|value| matches!(value, Value::Null)) {
        return Ok(Value::Null);
    }
    let bytes = args.iter().try_fold(0, |bytes, value| {
        let size = match value {
            Value::String(text) => text.len(),
            Value::Json(value) => crate::executor::retained_memory::serialized_json_bytes(value)?,
            _ => 64,
        };
        crate::executor::retained_memory::add(bytes, size)
    })?;
    let scratch = crate::runtime::accounted::json::serialized_decode_bytes(bytes)?;
    let _memory = controls
        .map(|controls| controls.reserve_query_memory(scratch))
        .transpose()?;
    if let Some(controls) = controls {
        controls.check_interruption()?;
    }
    let result = match kind {
        JsonFunction::BuildObject(_)
        | JsonFunction::BuildArray(_)
        | JsonFunction::ToJson(_)
        | JsonFunction::Set
        | JsonFunction::SetLax
        | JsonFunction::Insert
        | JsonFunction::StripNulls(_)
        | JsonFunction::Pretty
        | JsonFunction::ExtractPath { .. }
        | JsonFunction::PathMatch
        | JsonFunction::PathExists
        | JsonFunction::PathQueryArray
        | JsonFunction::PathQueryFirst
        | JsonFunction::PathMatchTz
        | JsonFunction::PathExistsTz
        | JsonFunction::PathQueryArrayTz
        | JsonFunction::PathQueryFirstTz => {
            unreachable!("construction uses its own evaluator")
        }
        JsonFunction::SqlExists => unreachable!("SQL/JSON EXISTS uses JSONPath evaluation"),
        JsonFunction::SqlInput => unreachable!("JSON input uses its own evaluator"),
        JsonFunction::ObjectKeys(_) => {
            let document = document(&args[0])?;
            let object = document.as_object().ok_or_else(|| {
                CassieError::InvalidParameterValue(
                    "cannot call json_object_keys on a non-object".into(),
                )
            })?;
            let mut keys = object.keys().cloned().collect::<Vec<_>>();
            keys.sort();
            Value::Json(serde_json::Value::Array(
                keys.into_iter().map(serde_json::Value::String).collect(),
            ))
        }
        JsonFunction::Typeof(_) => {
            let document = document(&args[0])?;
            let kind = match document.as_ref() {
                serde_json::Value::Null => "null",
                serde_json::Value::Bool(_) => "boolean",
                serde_json::Value::Number(_) => "number",
                serde_json::Value::String(_) => "string",
                serde_json::Value::Array(_) => "array",
                serde_json::Value::Object(_) => "object",
            };
            Value::String(kind.to_owned())
        }
        JsonFunction::ArrayLength(_) => {
            let document = document(&args[0])?;
            let array = document.as_array().ok_or_else(|| {
                CassieError::InvalidParameterValue("cannot get array length of a non-array".into())
            })?;
            let length = i32::try_from(array.len())
                .map_err(|_| CassieError::ResourceLimit("JSON array length exceeds INT4".into()))?;
            Value::Int64(i64::from(length))
        }
        JsonFunction::Exists(all) => {
            let document = json_text(&args[0])?;
            let keys = scalar_from_value(&args[1]);
            let operator = if all {
                BinaryOp::JsonbKeyExistsAll
            } else {
                BinaryOp::JsonbKeyExistsAny
            };
            match json_key_exists::evaluate(&ScalarValue::Json(document), &operator, &keys)? {
                ScalarValue::Bool(value) => Value::Bool(value),
                _ => unreachable!("non-NULL key inspection returns BOOLEAN"),
            }
        }
    };
    if let Some(controls) = controls {
        controls.check_interruption()?;
    }
    Ok(result)
}

fn document(value: &Value) -> Result<Cow<'_, serde_json::Value>, QueryError> {
    match value {
        Value::Json(value) => Ok(Cow::Borrowed(value)),
        Value::String(text) => serde_json::from_str(text).map(Cow::Owned).map_err(|error| {
            QueryError::General(format!("invalid input syntax for type json: {error}"))
        }),
        _ => serde_json::from_str(&json_text(value)?)
            .map(Cow::Owned)
            .map_err(|error| QueryError::General(error.to_string())),
    }
}

pub(super) fn json_text(value: &Value) -> Result<String, QueryError> {
    match value {
        Value::String(value) => Ok(value.clone()),
        Value::Json(value) => Ok(value.to_string()),
        Value::Bool(value) => Ok(value.to_string()),
        Value::Int64(value) => Ok(value.to_string()),
        Value::Float64(value) => {
            serde_json::to_string(value).map_err(|error| QueryError::General(error.to_string()))
        }
        _ => Err(QueryError::General("invalid JSON inspection input".into())),
    }
}

#[cfg(test)]
mod tests {
    use super::{evaluate, Value};
    use crate::{config::CassieRuntimeLimits, runtime::QueryExecutionControls};
    use std::time::Instant;

    #[test]
    fn should_admit_json_inspection_decode_given_small_memory_limits_when_parsing_documents() {
        // Arrange
        let input = Value::String(format!("{{\"k\":\"{}\"}}", "x".repeat(80)));
        let controls = |budget| {
            QueryExecutionControls::from_limits(
                &CassieRuntimeLimits {
                    query_memory_budget_bytes: budget,
                    ..CassieRuntimeLimits::default()
                },
                Instant::now(),
            )
        };
        let scarce = controls(1024);
        let ample = controls(32768);
        // Act
        let denied = evaluate("jsonb_typeof", std::slice::from_ref(&input), Some(&scarce))
            .expect("selected inspection");
        let accepted =
            evaluate("jsonb_typeof", &[input], Some(&ample)).expect("selected inspection");
        // Assert
        assert!(denied
            .expect_err("decode exceeds budget")
            .to_string()
            .contains("memory budget exceeded"));
        assert_eq!(
            accepted.expect("admitted decode"),
            Value::String("object".into())
        );
        assert_eq!(scarce.current_query_memory_bytes(), 0);
        assert_eq!(ample.current_query_memory_bytes(), 0);
    }
}

#[cfg(test)]
mod interrupted_tests {
    use super::{evaluate, Value};
    use crate::{
        config::CassieRuntimeLimits,
        runtime::{QueryCancellationHandle, QueryExecutionControls},
    };
    use std::time::Instant;

    #[test]
    fn should_release_json_inspection_admission_given_parse_errors_and_cancellation_when_queries_stop(
    ) {
        // Arrange
        let limits = CassieRuntimeLimits::default();
        let controls = QueryExecutionControls::from_limits(&limits, Instant::now());
        let cancellation = QueryCancellationHandle::new();
        let cancelled = QueryExecutionControls::with_cancellation(
            &limits,
            Instant::now(),
            cancellation.clone(),
        );
        cancellation.cancel();
        // Act
        let invalid = evaluate(
            "jsonb_typeof",
            &[Value::String("invalid".into())],
            Some(&controls),
        )
        .expect("inspection");
        let interrupted = evaluate(
            "jsonb_typeof",
            &[Value::String("{}".into())],
            Some(&cancelled),
        )
        .expect("inspection");
        // Assert
        assert!(invalid.is_err());
        assert!(interrupted
            .expect_err("cancelled query")
            .to_string()
            .contains("cancel"));
        assert_eq!(controls.current_query_memory_bytes(), 0);
        assert_eq!(cancelled.current_query_memory_bytes(), 0);
    }
}
