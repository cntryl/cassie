//! Source-aware scalar casts, shared by runtime and semantic constants.
use super::{cast_scalar, DataType, QueryError, ScalarValue};

pub(super) fn scalar(
    value: &ScalarValue,
    target: &DataType,
    source: Option<&DataType>,
    controls: Option<&crate::runtime::QueryExecutionControls>,
) -> Result<ScalarValue, QueryError> {
    if let Some(value) = super::character_arrays::cast(value, target, source, controls, None, true)
    {
        return value;
    }
    if let Some(value) = text_cast(value, target, source, controls)? {
        return Ok(value);
    }
    if matches!(target, DataType::Array(_)) && source == Some(target) {
        return Ok(value.clone());
    }
    if let Some(value) = bytea_integer_cast(value, target, source)? {
        return Ok(value);
    }
    if let Some(value) = integer_text_cast(value, target, controls) {
        return value;
    }
    if let Some(value) = special_scalar_cast(value, target, source, controls)? {
        return Ok(value);
    }
    if let Some(value) = formatted_text_cast(value, target, source, controls)? {
        return Ok(value);
    }
    cast_scalar(value, target, controls)
}

fn text_cast(
    value: &ScalarValue,
    target: &DataType,
    source: Option<&DataType>,
    controls: Option<&crate::runtime::QueryExecutionControls>,
) -> Result<Option<ScalarValue>, QueryError> {
    if source == Some(&DataType::Jsonb)
        && matches!(target, DataType::Text | DataType::Varchar { .. })
    {
        return jsonb_text(value).map(Some);
    }
    if matches!(target, DataType::Text | DataType::Varchar { .. }) {
        if let Some(formatted) = format_array_text(value, source, controls) {
            return Ok(Some(formatted));
        }
        if matches!(value, ScalarValue::Json(_)) || source == Some(&DataType::Json) {
            return json_text(value).map(Some);
        }
    }
    Ok(None)
}

fn bytea_integer_cast(
    value: &ScalarValue,
    target: &DataType,
    source: Option<&DataType>,
) -> Result<Option<ScalarValue>, QueryError> {
    if *target == DataType::Bytea {
        if let Some(width) = source.and_then(crate::types::IntegerWidth::from_data_type) {
            return super::bytea_casts::encode(value, width).map(Some);
        }
    }
    if source == Some(&DataType::Bytea) {
        if let Some(width) = crate::types::IntegerWidth::from_data_type(target) {
            return super::bytea_casts::decode(value, width).map(Some);
        }
    }
    Ok(None)
}

fn integer_text_cast(
    value: &ScalarValue,
    target: &DataType,
    controls: Option<&crate::runtime::QueryExecutionControls>,
) -> Option<Result<ScalarValue, QueryError>> {
    if let (ScalarValue::Str(input), Some(width)) =
        (value, crate::types::IntegerWidth::from_data_type(target))
    {
        Some(super::integers::controlled_input(input, width, controls))
    } else {
        None
    }
}

fn special_scalar_cast(
    value: &ScalarValue,
    target: &DataType,
    source: Option<&DataType>,
    controls: Option<&crate::runtime::QueryExecutionControls>,
) -> Result<Option<ScalarValue>, QueryError> {
    // Explicit typmod casts truncate; assignment and function coercion still
    // use cast_scalar's bounded validation. Format the source before applying
    // the width so CHAR blanks and temporal spellings keep their SQL identity.
    if let DataType::Char { length } = target {
        if length.is_none() && matches!(source, Some(DataType::Char { .. })) {
            return Ok(Some(value.clone()));
        }
        let text = scalar(value, &DataType::Text, source, controls)?;
        return character(&text, *length, controls).map(Some);
    }
    if let DataType::Varchar {
        length: Some(width),
    } = target
    {
        return varchar(value, *width, source, controls).map(Some);
    }
    if *target == DataType::Date && source == Some(&DataType::Timestamp) {
        if let ScalarValue::Str(text) = value {
            return crate::types::temporal::parse_timestamp(text)
                .map(|timestamp| {
                    ScalarValue::Str(crate::types::temporal::format_date(timestamp.date()))
                })
                .map_err(QueryError::General)
                .map(Some);
        }
    }
    if matches!(target, DataType::Text | DataType::Varchar { .. })
        && matches!(source, Some(DataType::Char { .. }))
    {
        if let ScalarValue::Str(text) = value {
            return cast_scalar(
                &ScalarValue::Str(crate::types::char_text::canonical_char_text(text).to_owned()),
                target,
                controls,
            )
            .map(Some);
        }
    }
    if *target == DataType::Boolean {
        if let ScalarValue::Str(input) = value {
            return crate::types::boolean::parse_text(input)
                .map(ScalarValue::Bool)
                .ok_or_else(|| {
                    QueryError::Cassie(crate::app::CassieError::from(
                        crate::types::BooleanInputError,
                    ))
                })
                .map(Some);
        }
    }
    if *target == DataType::Float {
        if let Some(value) = float_input(value) {
            return value.map(Some);
        }
    }
    Ok(None)
}

fn formatted_text_cast(
    value: &ScalarValue,
    target: &DataType,
    source: Option<&DataType>,
    controls: Option<&crate::runtime::QueryExecutionControls>,
) -> Result<Option<ScalarValue>, QueryError> {
    if matches!(target, DataType::Text | DataType::Varchar { .. }) {
        if source == Some(&DataType::Float) {
            if let ScalarValue::Float(value) = value {
                return cast_scalar(
                    &ScalarValue::Str(crate::types::float_text::format(*value)),
                    target,
                    controls,
                )
                .map(Some);
            }
        }
        if let ScalarValue::Str(text) = value {
            let formatted = match source {
                Some(DataType::Timestamp) => Some(crate::types::temporal::timestamp_text(text)),
                Some(DataType::Time) => Some(crate::types::temporal::time_text(text)),
                _ => None,
            };
            if let Some(formatted) = formatted {
                return cast_scalar(
                    &ScalarValue::Str(formatted.map_err(QueryError::General)?),
                    target,
                    controls,
                )
                .map(Some);
            }
        }
    }
    Ok(None)
}

fn float_input(value: &ScalarValue) -> Option<Result<ScalarValue, QueryError>> {
    match value {
        ScalarValue::Float(value) => Some(Ok(ScalarValue::Float(*value))),
        ScalarValue::Str(text) => Some(
            crate::types::float_input::parse(text)
                .map(ScalarValue::Float)
                .map_err(|error| QueryError::Cassie(crate::app::CassieError::FloatInput(error))),
        ),
        _ => None,
    }
}

fn jsonb_text(value: &ScalarValue) -> Result<ScalarValue, QueryError> {
    let document = super::json_concat::as_json_document(value)?;
    let ScalarValue::Json(text) = document else {
        return Ok(ScalarValue::Null);
    };
    crate::types::jsonb::canonical(&text)
        .map(ScalarValue::Str)
        .map_err(|error| crate::app::CassieError::TypeInput(error).into())
}

fn json_text(value: &ScalarValue) -> Result<ScalarValue, QueryError> {
    let text = match value {
        ScalarValue::Json(text) | ScalarValue::Str(text) => text,
        ScalarValue::Null => return Ok(ScalarValue::Null),
        _ => return Err(QueryError::General("cannot cast value to JSON text".into())),
    };
    serde_json::from_str::<serde_json::Value>(text)
        .map_err(|error| QueryError::General(error.to_string()))?;
    Ok(ScalarValue::Str(text.clone()))
}

fn format_array_text(
    value: &ScalarValue,
    source: Option<&DataType>,
    controls: Option<&crate::runtime::QueryExecutionControls>,
) -> Option<ScalarValue> {
    let Some(DataType::Array(element_type)) = source else {
        return None;
    };
    match value {
        ScalarValue::Json(input) => {
            let array = serde_json::from_str::<serde_json::Value>(input).ok()?;
            crate::types::array::format_text_array(&array, element_type).map(ScalarValue::Str)
        }
        ScalarValue::Array(array) => crate::types::array_value::format_text_in(
            array,
            element_type,
            &super::temporal::session_of(controls),
        )
        .map(ScalarValue::Str),
        _ => None,
    }
}

fn varchar(
    value: &ScalarValue,
    width: u32,
    source: Option<&DataType>,
    controls: Option<&crate::runtime::QueryExecutionControls>,
) -> Result<ScalarValue, QueryError> {
    let value = scalar(value, &DataType::Text, source, controls)?;
    Ok(match value {
        ScalarValue::Str(mut text) => {
            let width = usize::try_from(width).unwrap_or(usize::MAX);
            if let Some((boundary, _)) = text.char_indices().nth(width) {
                text.truncate(boundary);
            }
            ScalarValue::Str(text)
        }
        value => value,
    })
}

/// Conversions from exact integer NUMERIC tokens stay in the folding/runtime
/// phase. They must never acquire unknown-string input semantics.
pub(super) fn numeric_literal(
    input: &str,
    target: &DataType,
    identity: crate::sql::ast::CastIdentity,
    controls: Option<&crate::runtime::QueryExecutionControls>,
) -> Result<ScalarValue, QueryError> {
    if identity != crate::sql::ast::CastIdentity::Scalar
        || matches!(
            target,
            DataType::Extended(crate::types::extended::ExtendedType::Numeric(_))
        )
    {
        let value = crate::types::extended::numeric::parse_text(input)
            .map_err(|error| QueryError::Cassie(crate::app::CassieError::TypeInput(error)))?;
        return super::cast_scalar(
            &ScalarValue::Typed(crate::types::typed::TypedValue::Numeric(value)),
            target,
            controls,
        );
    }
    if let Some(width) = crate::types::IntegerWidth::from_data_type(target) {
        return super::integers::controlled_input(input, width, controls);
    }
    if *target == DataType::Float {
        return crate::types::float_input::parse(input)
            .map(ScalarValue::Float)
            .map_err(|error| QueryError::Cassie(crate::app::CassieError::FloatInput(error)));
    }
    match target {
        DataType::Text | DataType::Varchar { length: None } => {
            return Ok(ScalarValue::Str(input.into()))
        }
        DataType::Varchar {
            length: Some(length),
        } => {
            return Ok(ScalarValue::Str(
                input
                    .chars()
                    .take(usize::try_from(*length).unwrap_or(usize::MAX))
                    .collect(),
            ))
        }
        _ => {}
    }
    let value = crate::types::extended::numeric::parse_text(input)
        .map_err(|error| QueryError::Cassie(crate::app::CassieError::TypeInput(error)))?;
    super::cast_scalar(
        &ScalarValue::Typed(crate::types::typed::TypedValue::Numeric(value)),
        target,
        controls,
    )
}

/// SQL explicit character casts apply a character-counted typmod. An absent
/// typmod is the unbounded bpchar input/result identity, not SQL CHAR's default 1.
pub(super) fn character(
    value: &ScalarValue,
    width: Option<u32>,
    controls: Option<&crate::runtime::QueryExecutionControls>,
) -> Result<ScalarValue, QueryError> {
    let Some(text) = super::cast_to_text(value) else {
        return Ok(ScalarValue::Null);
    };
    if width.is_none() {
        return Ok(ScalarValue::Str(text));
    }
    let plan = crate::types::char_text::conversion_plan(&text, width, true, true)
        .map_err(crate::app::CassieError::TypeInput)?;
    let mut result = String::new();
    result
        .try_reserve_exact(plan.output_bytes)
        .map_err(|error| {
            crate::app::CassieError::ResourceLimit(format!(
                "unable to allocate character output: {error}"
            ))
        })?;
    if result.capacity() > plan.output_bytes {
        return Err(crate::app::CassieError::ResourceLimit(
            "allocator exceeded admitted character output capacity".into(),
        )
        .into());
    }
    result.push_str(&text[..plan.prefix_bytes]);
    let mut remaining = plan.padding;
    while remaining > 0 {
        if let Some(controls) = controls {
            controls.check_interruption()?;
        }
        let count = remaining.min(4096);
        result.extend(std::iter::repeat_n(' ', count));
        remaining -= count;
    }
    Ok(ScalarValue::Str(result))
}

/// Assignment/input VARCHAR keeps its bounded validation rather than explicit truncation.
pub(super) fn bounded_varchar(
    value: &ScalarValue,
    max_length: Option<u32>,
) -> Result<ScalarValue, QueryError> {
    let value = super::cast_to_text(value)
        .filter(|value| {
            max_length.is_none_or(|max_length| {
                usize::try_from(max_length)
                    .is_ok_and(|max_length| value.chars().count() <= max_length)
            })
        })
        .ok_or_else(|| QueryError::General("cannot cast value to VARCHAR".to_string()))?;
    Ok(ScalarValue::Str(value))
}

/// Upper bound for simultaneously retained expanding cast intermediates.
pub(crate) fn cast_scratch_bytes(expr: &crate::sql::ast::Expr) -> Result<usize, QueryError> {
    use crate::executor::retained_memory::{add, mul};
    let mut bytes = match expr {
        crate::sql::ast::Expr::Cast {
            data_type: DataType::Char {
                length: Some(width),
            },
            ..
        } => mul(
            usize::try_from(*width)
                .map_err(|_| QueryError::General("CHAR width overflow".into()))?,
            8,
        )?,
        crate::sql::ast::Expr::Cast {
            data_type: DataType::Bytea,
            source_type: Some(source),
            ..
        } => super::bytea_casts::scratch_bytes(source),
        _ => 0,
    };
    expr.try_visit_children(|child| {
        bytes = add(bytes, cast_scratch_bytes(child)?)?;
        Ok::<_, QueryError>(())
    })?;
    Ok(bytes)
}

/// Text input can expand twofold into the canonical hex carrier. Reserve
/// both evaluator/result copies before conversion; bound parameters already
/// reserve their reached copies through the lazy constant resolver.
pub(super) fn bytea_literal_scratch(expr: &crate::sql::ast::Expr) -> Result<usize, QueryError> {
    use crate::executor::retained_memory::{add, mul};
    let crate::sql::ast::Expr::Cast {
        expr,
        data_type: DataType::Bytea,
        ..
    } = expr
    else {
        return Ok(0);
    };
    let mut input = expr.as_ref();
    while let crate::sql::ast::Expr::Cast { expr, .. } = input {
        input = expr;
    }
    if let crate::sql::ast::Expr::StringLiteral(text) = input {
        Ok(mul(add(2, mul(text.len(), 2)?)?, 2)?)
    } else {
        Ok(0)
    }
}

#[cfg(test)]
#[path = "casts_character_tests.rs"]
mod character_tests;
