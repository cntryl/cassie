//! Finite variadic TEXT paths over native JSON document identities.
use super::super::{QueryError, ScalarValue, Value};
use crate::executor::retained_memory::{add, mul, serialized_json_bytes};
use crate::sql::functions::{self, json::JsonFunction, FunctionId};
use crate::{app::CassieError, runtime::QueryExecutionControls};

pub(super) fn evaluate(
    name: &str,
    args: &[Value],
    controls: Option<&QueryExecutionControls>,
) -> Option<Result<Value, QueryError>> {
    match functions::function(name)?.id {
        FunctionId::Json(JsonFunction::ExtractPath { binary, as_text }) => {
            Some(extract(args, binary, as_text, controls))
        }
        FunctionId::Json(JsonFunction::PathMatch) => Some(path_match(args, controls)),
        FunctionId::Json(
            kind @ (JsonFunction::PathExists
            | JsonFunction::PathQueryArray
            | JsonFunction::PathQueryFirst
            | JsonFunction::PathMatchTz
            | JsonFunction::PathExistsTz
            | JsonFunction::PathQueryArrayTz
            | JsonFunction::PathQueryFirstTz),
        ) => Some(path_function(kind, args, controls)),
        FunctionId::Json(JsonFunction::SqlExists) => Some(sql_json_exists(args, controls)),
        _ => None,
    }
}

fn path_function(
    kind: JsonFunction,
    args: &[Value],
    controls: Option<&QueryExecutionControls>,
) -> Result<Value, QueryError> {
    if !(2..=4).contains(&args.len()) {
        return Err(QueryError::General(
            "invalid JSONPATH function arity".into(),
        ));
    }
    if args.iter().any(Value::is_null) {
        return Ok(Value::Null);
    }
    let silent = match args.get(3) {
        Some(Value::Bool(value)) => *value,
        Some(_) => {
            return Err(CassieError::InvalidParameterValue(
                "JSONPATH silent argument must be boolean".into(),
            )
            .into())
        }
        None => false,
    };
    if let Some(controls) = controls {
        controls.check_interruption()?;
    }
    let input_bytes = args.iter().try_fold(0, |total, value| {
        add(
            total,
            match value {
                Value::String(text) => text.len(),
                Value::Json(value) => serialized_json_bytes(value)?,
                _ => 16,
            },
        )
    })?;
    let decoded_bytes = crate::runtime::accounted::json::serialized_decode_bytes(input_bytes)?;
    let estimated_bytes = add(mul(decoded_bytes, 2)?, mul(input_bytes.max(8), 8)?)?;
    let mut reservation = controls
        .map(|controls| controls.reserve_query_memory(estimated_bytes))
        .transpose()?;
    let document = document_arg(&args[0])?;
    let original_path = string_arg(&args[1], "JSONPATH")?;
    let variables = match args.get(2) {
        Some(value) => document_arg(value)?,
        None => serde_json::Value::Object(serde_json::Map::new()),
    };
    if !variables.is_object() {
        return Err(CassieError::InvalidParameterValue(
            "JSONPATH vars argument must be a JSON object".into(),
        )
        .into());
    }
    let path_text = substitute_variables(original_path, &variables)?;
    let path = sql_json_path::JsonPath::new(&path_text)
        .map_err(|error| CassieError::Parse(error.to_string()))?;
    let timezone = timezone_offset(controls)?;
    let evaluated = evaluate_jsonpath_function(kind, &path, &document, &variables, timezone);
    let result = match evaluated {
        Ok(value) => value,
        Err(error) if silent && error.can_silent() => silent_result(kind),
        Err(error) => return Err(path_error(&error)),
    };
    let output_bytes = match &result {
        PathResult::Exists(_) | PathResult::Match(_) => 1,
        PathResult::Array(values) => values
            .iter()
            .try_fold(0, |total, value| add(total, serialized_json_bytes(value)?))?,
        PathResult::First(Some(value)) => serialized_json_bytes(value)?,
        PathResult::First(None) => 0,
    };
    if let Some(reservation) = &mut reservation {
        reservation.try_grow(output_bytes.saturating_sub(estimated_bytes))?;
    }
    if let Some(controls) = controls {
        controls.check_interruption()?;
    }
    match result {
        PathResult::Exists(value) => Ok(Value::Bool(value)),
        PathResult::Array(values) => Ok(Value::Json(serde_json::Value::Array(values))),
        PathResult::First(value) => Ok(value.map_or(Value::Null, Value::Json)),
        PathResult::Match(value) => Ok(value.map_or(Value::Null, Value::Bool)),
    }
}

fn sql_json_exists(
    args: &[Value],
    controls: Option<&QueryExecutionControls>,
) -> Result<Value, QueryError> {
    let [document, path, policy] = args else {
        return Err(QueryError::General("invalid JSON_EXISTS arity".into()));
    };
    if args.iter().any(Value::is_null) {
        return Ok(Value::Null);
    }
    let policy = super::text_arg("JSON_EXISTS ON ERROR", policy)?
        .ok_or_else(|| CassieError::InvalidParameterValue("missing ON ERROR policy".into()))?;
    match path_function(
        JsonFunction::PathExists,
        &[document.clone(), path.clone()],
        controls,
    ) {
        Ok(value) => Ok(value),
        Err(error) => match policy.to_ascii_lowercase().as_str() {
            "false" => Ok(Value::Bool(false)),
            "true" => Ok(Value::Bool(true)),
            "unknown" => Ok(Value::Null),
            "error" => Err(error),
            _ => Err(CassieError::InvalidParameterValue(
                "JSON_EXISTS ON ERROR policy must be TRUE, FALSE, UNKNOWN, or ERROR".into(),
            )
            .into()),
        },
    }
}

fn evaluate_jsonpath_function(
    kind: JsonFunction,
    path: &sql_json_path::JsonPath,
    document: &serde_json::Value,
    variables: &serde_json::Value,
    timezone: chrono::FixedOffset,
) -> Result<PathResult, sql_json_path::EvalError> {
    match kind {
        JsonFunction::PathExists => path
            .exists_with_vars(document, variables)
            .map(PathResult::Exists),
        JsonFunction::PathExistsTz => path
            .query_with_vars_tz(document, variables, timezone)
            .map(|items| PathResult::Exists(!items.is_empty())),
        JsonFunction::PathQueryArray => path
            .query_with_vars(document, variables)
            .map(|items| PathResult::Array(items.into_iter().map(json_item).collect())),
        JsonFunction::PathQueryArrayTz => path
            .query_with_vars_tz(document, variables, timezone)
            .map(|items| PathResult::Array(items.into_iter().map(json_item).collect())),
        JsonFunction::PathQueryFirst => path
            .query_first_with_vars(document, variables)
            .map(|item| PathResult::First(item.map(json_item))),
        JsonFunction::PathQueryFirstTz => path
            .query_with_vars_tz(document, variables, timezone)
            .map(|items| PathResult::First(items.into_iter().next().map(json_item))),
        JsonFunction::PathMatchTz => {
            path.query_with_vars_tz(document, variables, timezone)
                .map(|items| {
                    let result = if let [value] = items.as_slice() {
                        serde_json::from_str::<serde_json::Value>(&value.to_string())
                            .ok()
                            .and_then(|value| value.as_bool())
                    } else {
                        None
                    };
                    PathResult::Match(result)
                })
        }
        _ => unreachable!("path function dispatch selects a path function"),
    }
}

fn silent_result(kind: JsonFunction) -> PathResult {
    match kind {
        JsonFunction::PathExists | JsonFunction::PathExistsTz => PathResult::Exists(false),
        JsonFunction::PathQueryArray | JsonFunction::PathQueryArrayTz => {
            PathResult::Array(Vec::new())
        }
        JsonFunction::PathQueryFirst | JsonFunction::PathQueryFirstTz => PathResult::First(None),
        JsonFunction::PathMatchTz => PathResult::Match(Some(false)),
        _ => unreachable!("path function dispatch selects a path function"),
    }
}

enum PathResult {
    Exists(bool),
    Match(Option<bool>),
    Array(Vec<serde_json::Value>),
    First(Option<serde_json::Value>),
}

fn json_item(value: sql_json_path::json::Cow<'_, serde_json::Value>) -> serde_json::Value {
    value.into_owned()
}

fn document_arg(value: &Value) -> Result<serde_json::Value, QueryError> {
    match value {
        Value::Json(value) => Ok(value.clone()),
        Value::String(value) => serde_json::from_str(value)
            .map_err(|error| CassieError::Execution(error.to_string()).into()),
        _ => Err(CassieError::Execution("JSONPATH functions require JSON values".into()).into()),
    }
}

fn string_arg<'a>(value: &'a Value, type_name: &str) -> Result<&'a str, QueryError> {
    match value {
        Value::String(value) => Ok(value),
        _ => Err(CassieError::Execution(format!("JSONPATH function requires {type_name}")).into()),
    }
}

fn path_error(error: &sql_json_path::EvalError) -> QueryError {
    CassieError::Execution(error.to_string()).into()
}

fn timezone_offset(
    controls: Option<&QueryExecutionControls>,
) -> Result<chrono::FixedOffset, QueryError> {
    let east = controls
        .and_then(QueryExecutionControls::temporal_session)
        .map_or(0, |session| session.zone.offset_at(session.statement_start));
    chrono::FixedOffset::east_opt(east).ok_or_else(|| {
        CassieError::InvalidParameterValue("JSONPATH timezone offset is out of range".into()).into()
    })
}

fn substitute_variables(path: &str, variables: &serde_json::Value) -> Result<String, QueryError> {
    let mut output = String::with_capacity(path.len());
    let mut chars = path.chars().peekable();
    let mut quoted = false;
    while let Some(ch) = chars.next() {
        if ch == '"' {
            quoted = !quoted;
            output.push(ch);
            continue;
        }
        if ch == '\\' && quoted {
            output.push(ch);
            if let Some(escaped) = chars.next() {
                output.push(escaped);
            }
            continue;
        }
        if ch == '$' && !quoted && chars.peek().is_some_and(|next| is_variable_start(*next)) {
            let mut name = String::new();
            while chars.peek().is_some_and(|next| is_variable_part(*next)) {
                name.push(chars.next().expect("peeked JSONPATH variable character"));
            }
            let value = variables
                .as_object()
                .and_then(|object| object.get(&name))
                .ok_or_else(|| {
                    CassieError::InvalidParameterValue(format!(
                        "could not find JSONPATH variable \"{name}\""
                    ))
                })?;
            output.push_str(
                &serde_json::to_string(value)
                    .map_err(|error| CassieError::Execution(error.to_string()))?,
            );
            continue;
        }
        output.push(ch);
    }
    Ok(output)
}

fn is_variable_start(ch: char) -> bool {
    ch == '_' || ch.is_ascii_alphabetic()
}

fn is_variable_part(ch: char) -> bool {
    ch == '_' || ch.is_ascii_alphanumeric()
}

fn path_match(
    args: &[Value],
    controls: Option<&QueryExecutionControls>,
) -> Result<Value, QueryError> {
    if args.len() != 2 {
        return Err(QueryError::General("invalid jsonb_path_match arity".into()));
    }
    if args.iter().any(|value| matches!(value, Value::Null)) {
        return Ok(Value::Null);
    }
    if let Some(controls) = controls {
        controls.check_interruption()?;
    }
    let left = super::super::scalar_from_value(&args[0]);
    let right = super::super::scalar_from_value(&args[1]);
    match evaluate_path(&left, &right, false)? {
        ScalarValue::Bool(value) => Ok(Value::Bool(value)),
        ScalarValue::Null => Ok(Value::Null),
        _ => unreachable!("JSONPATH match returns boolean or NULL"),
    }
}

fn evaluate_path(
    left: &ScalarValue,
    right: &ScalarValue,
    exists: bool,
) -> Result<ScalarValue, QueryError> {
    if matches!(left, ScalarValue::Null) || matches!(right, ScalarValue::Null) {
        return Ok(ScalarValue::Null);
    }
    let document_text = left.as_str().ok_or_else(|| {
        CassieError::Execution("JSONPATH operator requires a JSON document".into())
    })?;
    let path_text = right.as_str().ok_or_else(|| {
        CassieError::Execution("JSONPATH operator requires a JSONPATH value".into())
    })?;
    let document: serde_json::Value = serde_json::from_str(document_text)
        .map_err(|error| CassieError::Execution(error.to_string()))?;
    let path = sql_json_path::JsonPath::new(path_text)
        .map_err(|error| CassieError::Parse(error.to_string()))?;
    if exists {
        return path
            .exists(&document)
            .map(ScalarValue::Bool)
            .map_err(|error| CassieError::Execution(error.to_string()).into());
    }
    let result = path
        .query(&document)
        .map_err(|error| CassieError::Execution(error.to_string()))?;
    match result.as_slice() {
        [value] => match serde_json::from_str::<serde_json::Value>(&value.to_string())
            .map_err(|error| CassieError::Execution(error.to_string()))?
        {
            serde_json::Value::Bool(value) => Ok(ScalarValue::Bool(value)),
            _ => Ok(ScalarValue::Null),
        },
        _ => Ok(ScalarValue::Null),
    }
}

fn extract(
    args: &[Value],
    binary: bool,
    as_text: bool,
    controls: Option<&QueryExecutionControls>,
) -> Result<Value, QueryError> {
    if let Some(controls) = controls {
        controls.check_interruption()?;
    }
    if args.len() < 2 {
        return Err(
            CassieError::Execution("function json_extract_path() does not exist".into()).into(),
        );
    }
    if args.iter().any(|value| matches!(value, Value::Null)) {
        return Ok(Value::Null);
    }
    let mut bytes = 0;
    for value in args {
        bytes = add(
            bytes,
            match value {
                Value::String(text) => text.len(),
                Value::Json(value) => serialized_json_bytes(value)?,
                _ => 64,
            },
        )?;
    }
    let expanded = match &args[0] {
        Value::String(text) if binary => {
            crate::types::jsonb::input_text_len(text).map_err(CassieError::TypeInput)?
        }
        Value::Json(value) if binary => {
            crate::types::jsonb::text_len(value).map_err(CassieError::TypeInput)?
        }
        _ => bytes,
    };
    let scratch = add(
        mul(
            crate::runtime::accounted::json::serialized_decode_bytes(bytes)?,
            2,
        )?,
        mul(expanded.max(8), 8)?,
    )?;
    let _memory = controls
        .map(|controls| controls.reserve_query_memory(scratch))
        .transpose()?;
    let document = super::json_functions::json_text(&args[0])?;
    let segments = args[1..].iter().map(|value| match value {
        Value::String(text) => Some(text.as_str()),
        _ => None,
    });
    let result =
        super::super::json_extract::evaluate_path(&document, segments, as_text, binary, controls)?;
    if let Some(controls) = controls {
        controls.check_interruption()?;
    }
    match result {
        ScalarValue::Null => Ok(Value::Null),
        ScalarValue::Str(text) | ScalarValue::Json(text) => Ok(Value::String(text)),
        _ => unreachable!("JSON extraction yields documents, text or NULL"),
    }
}

#[cfg(test)]
mod tests {
    use super::{evaluate, Value};
    use crate::{
        config::CassieRuntimeLimits,
        runtime::{QueryCancellationHandle, QueryExecutionControls},
    };
    use std::time::Instant;

    #[test]
    fn should_admit_finite_json_path_scratch_given_small_budgets_when_extracting_documents() {
        // Arrange
        let input = [
            Value::String(format!("{{\"a\":\"{}\"}}", "x".repeat(80))),
            Value::String("a".into()),
        ];
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
        let ample = controls(131_072);
        // Act
        let denied = evaluate("jsonb_extract_path", &input, Some(&scarce)).expect("finite path");
        let accepted = evaluate("jsonb_extract_path", &input, Some(&ample)).expect("finite path");
        // Assert
        assert!(denied
            .expect_err("decode exceeds budget")
            .to_string()
            .contains("memory budget exceeded"));
        assert_eq!(
            accepted.expect("admitted extraction"),
            Value::String(format!("\"{}\"", "x".repeat(80)))
        );
        assert_eq!(scarce.current_query_memory_bytes(), 0);
        assert_eq!(ample.current_query_memory_bytes(), 0);
    }

    #[test]
    fn should_release_finite_json_path_scratch_given_parse_errors_and_cancellation_when_queries_stop(
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
        let input = [Value::String("invalid".into()), Value::String("a".into())];
        // Act
        let invalid = evaluate("jsonb_extract_path", &input, Some(&controls)).expect("finite path");
        let interrupted =
            evaluate("jsonb_extract_path", &input, Some(&cancelled)).expect("finite path");
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

#[cfg(test)]
mod expansion_tests {
    use super::{evaluate, Value};
    use crate::{config::CassieRuntimeLimits, runtime::QueryExecutionControls};
    use std::time::Instant;

    #[test]
    fn should_admit_expanded_json_numbers_given_compact_exponents_when_extracting_paths() {
        // Arrange
        let input = [
            Value::String("{\"a\":1e1000}".into()),
            Value::String("a".into()),
        ];
        let controls = |budget| {
            QueryExecutionControls::from_limits(
                &CassieRuntimeLimits {
                    query_memory_budget_bytes: budget,
                    ..CassieRuntimeLimits::default()
                },
                Instant::now(),
            )
        };
        let scarce = controls(8192);
        let ample = controls(131_072);
        // Act
        let denied = evaluate("jsonb_extract_path", &input, Some(&scarce)).expect("finite path");
        let accepted = evaluate("jsonb_extract_path", &input, Some(&ample)).expect("finite path");
        // Assert
        assert!(denied
            .expect_err("expanded scratch exceeds budget")
            .to_string()
            .contains("memory budget exceeded"));
        assert_eq!(
            accepted.expect("admitted expanded number"),
            Value::String(format!("1{}", "0".repeat(1000)))
        );
        assert_eq!(scarce.current_query_memory_bytes(), 0);
        assert_eq!(ample.current_query_memory_bytes(), 0);
    }
}
