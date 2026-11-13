//! Decoded JSON documents shared by the record-returning and `JSON_TABLE` sources.
//!
//! `json` input keeps its raw text (member order, duplicate keys, spelling); `jsonb` input is a
//! canonical tree. Both are read through [`Fragment`].
use serde_json::value::RawValue;

use super::source_json_elements::{contains_nul, nul_error, parse_error, OrderedObject};
use super::{BatchRow, Expr, QueryError, SourceExecutionEnv, Value};
use crate::app::{CassieError, DiagnosticError};
use crate::runtime::accounted::json_arena;
use crate::types::extended::TypeInputError;

/// A JSON value of either input family.
pub(super) enum Fragment {
    Raw(Box<RawValue>),
    Tree(serde_json::Value),
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Kind {
    Null,
    Bool,
    Number,
    String,
    Array,
    Object,
}

fn nesting(_: serde_json::Error) -> QueryError {
    CassieError::from(DiagnosticError::new(
        "54001",
        "JSON document exceeds the supported expansion nesting depth".to_owned(),
    ))
    .into()
}

fn invalid_text(error: &serde_json::Error) -> QueryError {
    CassieError::TypeInput(TypeInputError::new(
        "22P02",
        format!("invalid input syntax for type json: {error}"),
    ))
    .into()
}

impl Fragment {
    pub(super) fn kind(&self) -> Kind {
        match self {
            Self::Raw(raw) => match raw.get().trim_start().as_bytes().first() {
                Some(b'n') => Kind::Null,
                Some(b't' | b'f') => Kind::Bool,
                Some(b'"') => Kind::String,
                Some(b'[') => Kind::Array,
                Some(b'{') => Kind::Object,
                _ => Kind::Number,
            },
            Self::Tree(value) => match value {
                serde_json::Value::Null => Kind::Null,
                serde_json::Value::Bool(_) => Kind::Bool,
                serde_json::Value::Number(_) => Kind::Number,
                serde_json::Value::String(_) => Kind::String,
                serde_json::Value::Array(_) => Kind::Array,
                serde_json::Value::Object(_) => Kind::Object,
            },
        }
    }

    /// Text of a scalar: a string unescaped, a number or boolean in its JSON spelling.
    pub(super) fn scalar_text(&self) -> Result<String, QueryError> {
        match self {
            Self::Raw(raw) => {
                let token = raw.get().trim();
                if self.kind() != Kind::String {
                    return Ok(token.to_owned());
                }
                let text =
                    serde_json::from_str::<String>(token).map_err(|error| invalid_text(&error))?;
                if text.contains('\0') {
                    return Err(nul_error());
                }
                Ok(text)
            }
            Self::Tree(serde_json::Value::String(text)) => Ok(text.clone()),
            Self::Tree(value) => crate::types::jsonb::format(value)
                .map_err(|error| CassieError::TypeInput(error).into()),
        }
    }

    /// The value as `json` text: the original spelling for `json` input, canonical `jsonb`
    /// text otherwise.
    pub(super) fn json_text(&self) -> Result<String, QueryError> {
        match self {
            Self::Raw(raw) => Ok(raw.get().trim().to_owned()),
            Self::Tree(value) => crate::types::jsonb::format(value)
                .map_err(|error| CassieError::TypeInput(error).into()),
        }
    }

    /// The value as a canonical tree.
    pub(super) fn tree(&self) -> Result<serde_json::Value, QueryError> {
        match self {
            Self::Raw(raw) => serde_json::from_str(raw.get()).map_err(nesting),
            Self::Tree(value) => Ok(value.clone()),
        }
    }

    /// The value as a canonical tree, without copying `jsonb` input.
    pub(super) fn into_tree(self) -> Result<serde_json::Value, QueryError> {
        match self {
            Self::Raw(raw) => serde_json::from_str(raw.get()).map_err(nesting),
            Self::Tree(value) => Ok(value),
        }
    }

    /// Array elements in order.
    pub(super) fn elements(self) -> Result<Vec<Self>, QueryError> {
        match self {
            Self::Raw(raw) => Ok(serde_json::from_str::<Vec<Box<RawValue>>>(raw.get())
                .map_err(nesting)?
                .into_iter()
                .map(Self::Raw)
                .collect()),
            Self::Tree(serde_json::Value::Array(values)) => {
                Ok(values.into_iter().map(Self::Tree).collect())
            }
            Self::Tree(_) => Ok(Vec::new()),
        }
    }

    /// Object members in input order; `json` input retains duplicate keys.
    pub(super) fn members(self) -> Result<Vec<(String, Self)>, QueryError> {
        match self {
            Self::Raw(raw) => Ok(serde_json::from_str::<OrderedObject>(raw.get())
                .map_err(nesting)?
                .0
                .into_iter()
                .map(|(key, value)| (key, Self::Raw(value)))
                .collect()),
            Self::Tree(serde_json::Value::Object(members)) => Ok(members
                .into_iter()
                .map(|(key, value)| (key, Self::Tree(value)))
                .collect()),
            Self::Tree(_) => Ok(Vec::new()),
        }
    }

    /// Bytes this fragment retains.
    pub(super) fn retained_bytes(&self) -> Result<usize, QueryError> {
        match self {
            Self::Raw(raw) => Ok(raw.get().len()),
            Self::Tree(value) => Ok(crate::runtime::accounted::json::retained_bytes(value)?),
        }
    }
}

/// Parses a SQL document argument. SQL NULL yields `None`. The caller reserves the decode
/// bound for text arguments before this call.
pub(super) fn decode_document(
    env: &SourceExecutionEnv<'_>,
    binary: bool,
    function_name: &str,
    argument: Value,
) -> Result<Option<Fragment>, QueryError> {
    let wrong = || {
        QueryError::from(CassieError::from(DiagnosticError::new(
            "42883",
            format!("function {function_name}(...) does not exist"),
        )))
    };
    if argument.is_null() {
        return Ok(None);
    }
    if binary {
        let document = match argument {
            Value::Json(document) => document,
            Value::String(text) => {
                json_arena::parse_value_in_reserved_memory(text.as_bytes(), env.controls)
                    .map_err(|error| parse_error(error, &text))?
            }
            _ => return Err(wrong()),
        };
        if contains_nul(&document) {
            return Err(nul_error());
        }
        return Ok(Some(Fragment::Tree(document)));
    }
    let text = match argument {
        Value::String(text) => text,
        Value::Json(document) => document.to_string(),
        _ => return Err(wrong()),
    };
    // Validate once with the accounted parser; the raw span is then read without copying.
    json_arena::parse_value_in_reserved_memory(text.as_bytes(), env.controls)
        .map_err(|error| parse_error(error, &text))?;
    let raw = serde_json::from_str::<Box<RawValue>>(&text).map_err(nesting)?;
    Ok(Some(Fragment::Raw(raw)))
}

/// Reserves the decode bound of a text argument (`serialized_decode_bytes` plus the raw copy).
pub(super) fn reserve_decode(
    env: &SourceExecutionEnv<'_>,
    argument: &Value,
) -> Result<Option<crate::runtime::QueryMemoryReservation>, QueryError> {
    match argument {
        Value::String(text) => Ok(Some(
            env.controls.reserve_query_memory(
                crate::runtime::accounted::json::serialized_decode_bytes(text.len())?
                    .saturating_add(text.len()),
            )?,
        )),
        _ => Ok(None),
    }
}

/// Evaluates one source argument against the lateral outer row, with scratch admission.
pub(super) fn evaluate_argument(
    env: &SourceExecutionEnv<'_>,
    expression: &Expr,
    outer: Option<&BatchRow>,
    ctes: &super::CteContext,
) -> Result<Value, QueryError> {
    let empty = BatchRow::from_projected_values(Vec::new());
    let row = outer.unwrap_or(&empty);
    let _scratch =
        env.controls
            .reserve_query_memory(super::source_set_function::argument_scratch(
                expression, env.params, row,
            )?)?;
    let evaluation_memory = super::filter::EvaluationMemory::new(env.controls)?;
    super::source_set_function::argument_value(env, expression, row, ctes, &evaluation_memory)
}
