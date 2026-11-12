//! Bounded JSON inspection, preserving SQL NULL versus document null.
use super::super::{json_key_exists, scalar_from_value, BinaryOp, QueryError, ScalarValue, Value};
use crate::app::CassieError;
use crate::runtime::QueryExecutionControls;
use crate::sql::functions::{self, json::JsonFunction, FunctionId};
use std::borrow::Cow;

#[cfg(test)]
#[path = "json_exists_function_tests.rs"]
mod exists_function_tests;

#[cfg(test)]
#[path = "json_inspection_root_tests.rs"]
mod inspection_root_tests;

#[cfg(test)]
#[path = "json_object_keys_owned_tests.rs"]
mod object_keys_owned_tests;

#[path = "json_unique_keys.rs"]
mod unique_keys;

#[cfg(test)]
pub(super) fn evaluate(
    name: &str,
    args: &[Value],
    controls: Option<&QueryExecutionControls>,
) -> Option<Result<Value, QueryError>> {
    evaluate_with_owner(name, args, controls, None)
}

pub(super) fn evaluate_with_owner(
    name: &str,
    args: &[Value],
    controls: Option<&QueryExecutionControls>,
    owner: Option<&super::super::EvaluationMemory>,
) -> Option<Result<Value, QueryError>> {
    let FunctionId::Json(kind) = functions::function(name)?.id else {
        return None;
    };
    if matches!(kind, JsonFunction::SqlInput) {
        return Some(sql_input(args, controls));
    }
    if matches!(kind, JsonFunction::VariadicArray) {
        return Some(match args {
            [array @ (Value::Array(_) | Value::Null)] => Ok(array.clone()),
            _ => Err(
                CassieError::Execution("VARIADIC JSON path argument requires TEXT[]".into()).into(),
            ),
        });
    }
    matches!(
        kind,
        JsonFunction::Typeof(_)
            | JsonFunction::ArrayLength(_)
            | JsonFunction::Exists(_)
            | JsonFunction::ObjectKeys(_)
    )
    .then(|| inspect(kind, args, controls, owner))
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

#[cfg(test)]
thread_local! {
    static CANCEL_OBJECT_KEY_COUNT: std::cell::RefCell<Option<crate::runtime::QueryCancellationHandle>> = const { std::cell::RefCell::new(None) };
}
#[cfg(test)]
fn cancel_object_key_count_scan_once(handle: crate::runtime::QueryCancellationHandle) {
    CANCEL_OBJECT_KEY_COUNT.with(|slot| *slot.borrow_mut() = Some(handle));
}

fn selected_object_key_size(
    value: &serde_json::Value,
    controls: &QueryExecutionControls,
) -> Result<usize, CassieError> {
    #[cfg(not(test))]
    {
        crate::runtime::accounted::compact_json::count(value, controls)
    }
    #[cfg(test)]
    {
        crate::runtime::accounted::compact_json::count_observing(value, controls, |progress| {
            if matches!(
                progress,
                crate::runtime::accounted::compact_json::Progress::Scan
            ) {
                CANCEL_OBJECT_KEY_COUNT.with(|slot| {
                    if let Some(handle) = slot.borrow_mut().take() {
                        handle.cancel();
                    }
                });
            }
        })
    }
}

fn inspect(
    kind: JsonFunction,
    args: &[Value],
    controls: Option<&QueryExecutionControls>,
    owner: Option<&super::super::EvaluationMemory>,
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
            Value::Json(value) => {
                if let (
                    JsonFunction::ObjectKeys(_),
                    serde_json::Value::Object(_),
                    Some(controls),
                    Some(_),
                ) = (kind, value, controls, owner)
                {
                    selected_object_key_size(value, controls)?
                } else {
                    crate::executor::retained_memory::serialized_json_bytes(value)?
                }
            }
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
    let result = inspect_value(kind, args, controls, owner)?;
    if let Some(controls) = controls {
        controls.check_interruption()?;
    }
    Ok(result)
}

fn inspect_value(
    kind: JsonFunction,
    args: &[Value],
    controls: Option<&QueryExecutionControls>,
    owner: Option<&super::super::EvaluationMemory>,
) -> Result<Value, QueryError> {
    match kind {
        JsonFunction::BuildObject(_)
        | JsonFunction::BuildArray(_)
        | JsonFunction::ToJson(_)
        | JsonFunction::Set
        | JsonFunction::SetLax
        | JsonFunction::Insert
        | JsonFunction::StripNulls(_)
        | JsonFunction::Pretty
        | JsonFunction::ExtractPath { .. }
        | JsonFunction::VariadicArray
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
        JsonFunction::ObjectKeys(_) => inspect_object_keys(&args[0], controls, owner),
        JsonFunction::Typeof(_) => inspect_typeof(&args[0], controls),
        JsonFunction::ArrayLength(_) => inspect_array_length(&args[0], controls),
        JsonFunction::Exists(all) => inspect_exists(&args[0], &args[1], all, controls),
    }
}

fn inspect_object_keys(
    value: &Value,
    controls: Option<&QueryExecutionControls>,
    owner: Option<&super::super::EvaluationMemory>,
) -> Result<Value, QueryError> {
    if let Some(value) = controlled_object_keys(value, controls, owner)? {
        return Ok(value);
    }
    let document = document(value, controls)?;
    let object = document.as_object().ok_or_else(|| {
        CassieError::InvalidParameterValue("cannot call json_object_keys on a non-object".into())
    })?;
    let mut keys = object.keys().cloned().collect::<Vec<_>>();
    keys.sort();
    Ok(Value::Json(serde_json::Value::Array(
        keys.into_iter().map(serde_json::Value::String).collect(),
    )))
}

fn inspect_typeof(
    value: &Value,
    controls: Option<&QueryExecutionControls>,
) -> Result<Value, QueryError> {
    use crate::runtime::accounted::json_arena::InspectionRoot;
    let kind = if let Some(root) = controlled_root(value, controls)? {
        match root {
            InspectionRoot::Null => "null",
            InspectionRoot::Boolean => "boolean",
            InspectionRoot::Number => "number",
            InspectionRoot::String => "string",
            InspectionRoot::Array(_) => "array",
            InspectionRoot::Object => "object",
        }
    } else {
        let document = document(value, controls)?;
        match document.as_ref() {
            serde_json::Value::Null => "null",
            serde_json::Value::Bool(_) => "boolean",
            serde_json::Value::Number(_) => "number",
            serde_json::Value::String(_) => "string",
            serde_json::Value::Array(_) => "array",
            serde_json::Value::Object(_) => "object",
        }
    };
    Ok(Value::String(kind.to_owned()))
}

fn inspect_array_length(
    value: &Value,
    controls: Option<&QueryExecutionControls>,
) -> Result<Value, QueryError> {
    use crate::runtime::accounted::json_arena::InspectionRoot;
    let length = if let Some(root) = controlled_root(value, controls)? {
        match root {
            InspectionRoot::Array(length) => length,
            _ => {
                return Err(CassieError::InvalidParameterValue(
                    "cannot get array length of a non-array".into(),
                )
                .into());
            }
        }
    } else {
        let document = document(value, controls)?;
        document
            .as_array()
            .ok_or_else(|| {
                CassieError::InvalidParameterValue("cannot get array length of a non-array".into())
            })?
            .len()
    };
    let length = i32::try_from(length)
        .map_err(|_| CassieError::ResourceLimit("JSON array length exceeds INT4".into()))?;
    Ok(Value::Int64(i64::from(length)))
}

fn inspect_exists(
    input: &Value,
    keys: &Value,
    all: bool,
    controls: Option<&QueryExecutionControls>,
) -> Result<Value, QueryError> {
    let document = ScalarValue::Json(json_text(input)?);
    let keys = scalar_from_value(keys);
    let operator = if all {
        BinaryOp::JsonbKeyExistsAll
    } else {
        BinaryOp::JsonbKeyExistsAny
    };
    let selected = if let Some(controls) = controls {
        json_key_exists::evaluate_controlled(&document, &operator, &keys, controls)?
    } else {
        None
    };
    let result = match selected {
        Some(result) => result,
        None => json_key_exists::evaluate(&document, &operator, &keys)?,
    };
    match result {
        ScalarValue::Bool(value) => Ok(Value::Bool(value)),
        _ => unreachable!("non-NULL key inspection returns BOOLEAN"),
    }
}

struct PendingObjectKeys {
    value: Value,
    memory: Option<crate::runtime::QueryMemoryReservation>,
}

fn controlled_object_keys(
    value: &Value,
    controls: Option<&QueryExecutionControls>,
    owner: Option<&super::super::EvaluationMemory>,
) -> Result<Option<Value>, QueryError> {
    let (Some(controls), Some(owner)) = (controls, owner) else {
        return Ok(None);
    };
    match value {
        Value::String(text) => {
            let Some(document) =
                crate::runtime::accounted::json_arena::parse_existence_candidate(text, controls)?
            else {
                return Ok(None);
            };
            let Some(keys) = document.object_keys(controls)? else {
                return Err(CassieError::InvalidParameterValue(
                    "cannot call json_object_keys on a non-object".into(),
                )
                .into());
            };
            adopt_object_keys(keys, controls, owner).map(Some)
        }
        Value::Json(serde_json::Value::Object(map)) => {
            let keys =
                crate::runtime::accounted::json_arena::build_object_keys_from_map(map, controls)?;
            adopt_object_keys(keys, controls, owner).map(Some)
        }
        _ => Ok(None),
    }
}

fn adopt_object_keys(
    keys: crate::runtime::accounted::json_arena::OwnedObjectKeys,
    controls: &QueryExecutionControls,
    owner: &super::super::EvaluationMemory,
) -> Result<Value, QueryError> {
    let (value, memory) = keys.into_parts();
    let mut pending = PendingObjectKeys {
        value,
        memory: Some(memory),
    };
    if !owner.belongs_to(controls)
        || !pending
            .memory
            .as_ref()
            .expect("key memory")
            .belongs_to(controls)
    {
        return Err(CassieError::ResourceLimit(
            "JSON object keys owner uses a different query tracker".into(),
        )
        .into());
    }
    controls.check_interruption()?;
    owner.adopt(pending.memory.take().expect("key memory"));
    Ok(pending.value)
}

fn controlled_root(
    value: &Value,
    controls: Option<&QueryExecutionControls>,
) -> Result<Option<crate::runtime::accounted::json_arena::InspectionRoot>, QueryError> {
    let (Value::String(text), Some(controls)) = (value, controls) else {
        return Ok(None);
    };
    let Some(document) =
        crate::runtime::accounted::json_arena::parse_existence_candidate(text, controls)?
    else {
        return Ok(None);
    };
    document.inspection_root().map(Some).map_err(Into::into)
}

fn document<'a>(
    value: &'a Value,
    controls: Option<&QueryExecutionControls>,
) -> Result<Cow<'a, serde_json::Value>, QueryError> {
    match value {
        Value::Json(value) => Ok(Cow::Borrowed(value)),
        Value::String(text) => parse_document(text, controls)
            .map(Cow::Owned)
            .map_err(|error| match error {
                crate::runtime::accounted::json_arena::ValueParseError::Invalid => {
                    QueryError::General(format!(
                        "invalid input syntax for type json: {}",
                        crate::runtime::accounted::json_arena::syntax_error(text.as_bytes())
                    ))
                }
                crate::runtime::accounted::json_arena::ValueParseError::Controlled(error) => {
                    error.into()
                }
            }),
        _ => {
            let text = json_text(value)?;
            parse_document(&text, controls)
                .map(Cow::Owned)
                .map_err(|error| match error {
                    crate::runtime::accounted::json_arena::ValueParseError::Invalid => {
                        QueryError::General(crate::runtime::accounted::json_arena::syntax_error(
                            text.as_bytes(),
                        ))
                    }
                    crate::runtime::accounted::json_arena::ValueParseError::Controlled(error) => {
                        error.into()
                    }
                })
        }
    }
}

fn parse_document(
    text: &str,
    controls: Option<&QueryExecutionControls>,
) -> Result<serde_json::Value, crate::runtime::accounted::json_arena::ValueParseError> {
    if let Some(controls) = controls {
        return crate::runtime::accounted::json_arena::parse_value_in_reserved_memory(
            text.as_bytes(),
            controls,
        );
    }
    let parsed = crate::runtime::accounted::json_arena::parse_value_with_optional_controls(
        text.as_bytes(),
        None,
    )?;
    Ok(parsed.get().clone())
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
        let canceled = QueryExecutionControls::with_cancellation(
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
            Some(&canceled),
        )
        .expect("inspection");
        // Assert
        assert!(invalid.is_err());
        assert!(interrupted
            .expect_err("canceled query")
            .to_string()
            .contains("cancel"));
        assert_eq!(controls.current_query_memory_bytes(), 0);
        assert_eq!(canceled.current_query_memory_bytes(), 0);
    }
}
