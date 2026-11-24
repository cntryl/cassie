//! Evaluation for the selected JSON/JSONB SQL/JSON path operators.
use super::{BinaryOp, QueryError, ScalarValue};
use crate::app::CassieError;

pub(super) fn evaluate(
    left: &ScalarValue,
    op: &BinaryOp,
    right: &ScalarValue,
    controls: Option<&crate::runtime::QueryExecutionControls>,
) -> Option<Result<ScalarValue, QueryError>> {
    let exists = match op {
        BinaryOp::JsonPathExists => true,
        BinaryOp::JsonPathMatch => false,
        _ => return None,
    };
    Some(evaluate_path(left, right, exists, controls))
}

fn evaluate_path(
    left: &ScalarValue,
    right: &ScalarValue,
    exists: bool,
    controls: Option<&crate::runtime::QueryExecutionControls>,
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
    let document = crate::runtime::accounted::json_arena::parse_value_with_optional_controls(
        document_text.as_bytes(),
        controls,
    )
    .map_err(|error| match error {
        crate::runtime::accounted::json_arena::ValueParseError::Invalid => CassieError::Execution(
            crate::runtime::accounted::json_arena::syntax_error(document_text.as_bytes()),
        ),
        crate::runtime::accounted::json_arena::ValueParseError::Controlled(error) => error,
    })?;
    let path = sql_json_path::JsonPath::new(path_text)
        .map_err(|error| CassieError::Parse(error.to_string()))?;
    if exists {
        return match path.exists(document.get()) {
            Ok(value) => Ok(ScalarValue::Bool(value)),
            Err(error) if error.can_silent() => Ok(ScalarValue::Bool(false)),
            Err(error) => Err(jsonpath_runtime_error(&error)),
        };
    }
    let values = match path.query(document.get()) {
        Ok(values) => values,
        Err(error) if error.can_silent() => return Ok(ScalarValue::Bool(false)),
        Err(error) => return Err(jsonpath_runtime_error(&error)),
    };
    match values.as_slice() {
        [value] => Ok(value
            .as_ref()
            .as_bool()
            .map_or(ScalarValue::Null, ScalarValue::Bool)),
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
