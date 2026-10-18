//! Source-aware scalar casts, shared by runtime and semantic constants.
use super::{cast_scalar, DataType, QueryError, ScalarValue};

pub(super) fn scalar(
    value: &ScalarValue,
    target: &DataType,
    source: Option<&DataType>,
    controls: Option<&crate::runtime::QueryExecutionControls>,
) -> Result<ScalarValue, QueryError> {
    if matches!(target,DataType::Array(element) if **element == DataType::Text)
        && source == Some(target)
    {
        return Ok(value.clone());
    }
    if *target == DataType::Bytea {
        if let Some(width) = source.and_then(crate::types::IntegerWidth::from_data_type) {
            return super::bytea_casts::encode(value, width);
        }
    }
    if source == Some(&DataType::Bytea) {
        if let Some(width) = crate::types::IntegerWidth::from_data_type(target) {
            return super::bytea_casts::decode(value, width);
        }
    }
    if let (ScalarValue::Str(input), Some(width)) =
        (value, crate::types::IntegerWidth::from_data_type(target))
    {
        return super::integers::controlled_input(input, width, controls);
    }
    // Explicit typmod casts truncate; assignment and function coercion still
    // use cast_scalar's bounded validation. Format the source before applying
    // the width so CHAR blanks and temporal spellings keep their SQL identity.
    if let DataType::Varchar {
        length: Some(width),
    } = target
    {
        return varchar(value, *width, source, controls);
    }
    if *target == DataType::Date && source == Some(&DataType::Timestamp) {
        if let ScalarValue::Str(text) = value {
            return crate::types::temporal::parse_timestamp(text)
                .map(|timestamp| {
                    ScalarValue::Str(crate::types::temporal::format_date(timestamp.date()))
                })
                .map_err(QueryError::General);
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
            );
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
                });
        }
    }
    if *target == DataType::Float {
        match value {
            ScalarValue::Float(value) => return Ok(ScalarValue::Float(*value)),
            ScalarValue::Str(text) => {
                return crate::types::float_input::parse(text)
                    .map(ScalarValue::Float)
                    .map_err(|error| {
                        QueryError::Cassie(crate::app::CassieError::FloatInput(error))
                    })
            }
            _ => {}
        }
    }
    if matches!(target, DataType::Text | DataType::Varchar { .. }) {
        if source == Some(&DataType::Float) {
            if let ScalarValue::Float(value) = value {
                return cast_scalar(
                    &ScalarValue::Str(crate::types::float_text::format(*value)),
                    target,
                    controls,
                );
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
                );
            }
        }
    }
    cast_scalar(value, target, controls)
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
) -> Result<ScalarValue, QueryError> {
    let Some(text) = super::cast_to_text(value) else {
        return Ok(ScalarValue::Null);
    };
    let Some(width) = width else {
        return Ok(ScalarValue::Str(text));
    };
    let width =
        usize::try_from(width).map_err(|_| QueryError::General("CHAR width overflow".into()))?;
    let mut result: String = text.chars().take(width).collect();
    result.extend(std::iter::repeat_n(
        ' ',
        width.saturating_sub(result.chars().count()),
    ));
    Ok(ScalarValue::Str(result))
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
