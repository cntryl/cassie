//! Evaluation for the selected JSON/JSONB SQL/JSON path operators.
use super::{BinaryOp, QueryError, ScalarValue};
use crate::app::CassieError;

pub(super) fn evaluate(
    left: &ScalarValue,
    op: &BinaryOp,
    right: &ScalarValue,
) -> Option<Result<ScalarValue, QueryError>> {
    let exists = match op {
        BinaryOp::JsonPathExists => true,
        BinaryOp::JsonPathMatch => false,
        _ => return None,
    };
    Some(evaluate_path(left, right, exists))
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
        return match path.exists(&document) {
            Ok(value) => Ok(ScalarValue::Bool(value)),
            Err(error) if error.can_silent() => Ok(ScalarValue::Bool(false)),
            Err(error) => Err(jsonpath_runtime_error(&error)),
        };
    }
    let values = match path.query(&document) {
        Ok(values) => values,
        Err(error) if error.can_silent() => return Ok(ScalarValue::Bool(false)),
        Err(error) => return Err(jsonpath_runtime_error(&error)),
    };
    match values.as_slice() {
        [value] => {
            let value: serde_json::Value = serde_json::from_str(&value.to_string())
                .map_err(|error| CassieError::Execution(error.to_string()))?;
            match value {
                serde_json::Value::Bool(value) => Ok(ScalarValue::Bool(value)),
                _ => Ok(ScalarValue::Null),
            }
        }
        _ => Ok(ScalarValue::Null),
    }
}

fn jsonpath_runtime_error(error: &sql_json_path::EvalError) -> QueryError {
    let sqlstate = if error.is_structural() {
        "22039"
    } else {
        "2203B"
    };
    CassieError::from(crate::app::DiagnosticError::new(
        sqlstate,
        error.to_string(),
    ))
    .into()
}
