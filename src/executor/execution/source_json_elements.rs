//! Bounded `json[b]_array_elements[_text]` and `json[b]_each[_text]` FROM sources.
//!
//! Text input reserves its decode bound (`serialized_decode_bytes` plus the raw span copy)
//! before parsing and holds it until the rows exist; the complete retained output is then
//! admitted before any row is built, so a malformed document or a resource failure never
//! publishes a partial result.
use std::fmt;
use std::mem::size_of;
use std::sync::Arc;

use serde::de::{Deserialize, Deserializer, MapAccess, Visitor};
use serde_json::value::RawValue;

use super::{BatchRow, QueryError, SourceExecution, SourceExecutionEnv, Value};
use crate::app::{CassieError, DiagnosticError};
use crate::runtime::accounted::json_arena::{self, ValueParseError};
use crate::sql::ast::FunctionCall;
use crate::sql::table_sources::{self, JsonElementSource};
use crate::types::extended::TypeInputError;
use crate::types::DataType;

/// Object members in input order, retaining duplicate keys (`json` semantics).
struct OrderedObject(Vec<(String, Box<RawValue>)>);

impl<'de> Deserialize<'de> for OrderedObject {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct Members;

        impl<'de> Visitor<'de> for Members {
            type Value = OrderedObject;

            fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str("a JSON object")
            }

            fn visit_map<M: MapAccess<'de>>(self, mut map: M) -> Result<Self::Value, M::Error> {
                let mut members = Vec::new();
                while let Some(member) = map.next_entry::<String, Box<RawValue>>()? {
                    members.push(member);
                }
                Ok(OrderedObject(members))
            }
        }

        deserializer.deserialize_map(Members)
    }
}

enum Element {
    Raw(Box<RawValue>),
    Tree(serde_json::Value),
}

struct Entry {
    key: Option<String>,
    value: Element,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Root {
    Array,
    Object,
    Scalar,
}

pub(super) fn execute(
    env: &SourceExecutionEnv<'_>,
    function: &FunctionCall,
    ordinal: bool,
    outer: Option<&BatchRow>,
    row_budget: Option<usize>,
    ctes: &super::CteContext,
) -> SourceExecution {
    env.controls.check_interruption()?;
    let source = table_sources::json_element_source(&function.name).ok_or_else(|| {
        QueryError::General("JSON element expansion requires a selected function".into())
    })?;
    if row_budget == Some(0) {
        return Ok((Vec::new(), Vec::new()));
    }
    let argument = argument_value(env, function, outer, ctes)?;
    // Admit the decode bound and the raw span copy before any parse allocates.
    let _decode = match &argument {
        Value::String(text) => Some(
            env.controls.reserve_query_memory(
                crate::runtime::accounted::json::serialized_decode_bytes(text.len())?
                    .saturating_add(text.len()),
            )?,
        ),
        _ => None,
    };
    let entries = decode(env, source, argument)?;
    let take = entries.len().min(row_budget.unwrap_or(usize::MAX));
    let columns = table_sources::json_element_columns(source, ordinal);
    let types = Arc::new(
        columns
            .iter()
            .map(|(_, data_type)| data_type.clone())
            .collect::<Vec<DataType>>(),
    );
    // Admit the complete retained expansion before any row is built.
    let mut memory = env.controls.reserve_query_memory(0)?;
    let mut total = size_of::<Vec<BatchRow>>();
    for entry in entries.iter().take(take) {
        env.controls.check_interruption()?;
        total = total.saturating_add(entry_bytes(source, entry, columns.len())?);
    }
    memory.try_grow(total)?;
    let mut rows = Vec::new();
    rows.try_reserve_exact(take).map_err(|error| {
        CassieError::ResourceLimit(format!("unable to retain JSON expansion rows: {error}"))
    })?;
    for (index, entry) in entries.into_iter().take(take).enumerate() {
        env.controls.check_interruption()?;
        let Entry { key, value } = entry;
        let mut values = Vec::with_capacity(columns.len());
        let mut names = columns.iter().map(|(name, _)| name.clone());
        let mut next_name = || names.next().unwrap_or_default();
        if source.each {
            values.push((next_name(), key.map_or(Value::Null, Value::String)));
        }
        values.push((next_name(), element_value(source, value)?));
        if ordinal {
            let position = i64::try_from(index + 1).map_err(|_| {
                CassieError::ResourceLimit("JSON expansion ordinality overflow".into())
            })?;
            values.push((next_name(), Value::Int64(position)));
        }
        rows.push(
            BatchRow::from_projected_values(values)
                .with_optional_data_types(Some(Arc::clone(&types))),
        );
    }
    env.controls.check_interruption()?;
    let memory = Arc::new(memory);
    for row in &mut rows {
        row.attach_operator_memory(env.controls, Arc::clone(&memory))?;
    }
    Ok((
        super::batch::chunk_rows(rows, super::batch::DEFAULT_BATCH_SIZE),
        Vec::new(),
    ))
}

fn argument_value(
    env: &SourceExecutionEnv<'_>,
    function: &FunctionCall,
    outer: Option<&BatchRow>,
    ctes: &super::CteContext,
) -> Result<Value, QueryError> {
    let expression = function.args.first().ok_or_else(|| {
        QueryError::General("JSON element expansion requires one argument".into())
    })?;
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

fn invalid_input(text: &str) -> CassieError {
    let detail = json_arena::syntax_error(text.as_bytes());
    if detail.starts_with("recursion limit exceeded") {
        // The shared parser mirrors serde's 128-level bound; surface it as a depth error.
        return CassieError::from(DiagnosticError::new(
            "54001",
            "JSON document exceeds the supported expansion nesting depth".to_owned(),
        ));
    }
    CassieError::TypeInput(TypeInputError::new(
        "22P02",
        format!("invalid input syntax for type json: {detail}"),
    ))
}

fn parse_error(error: ValueParseError, text: &str) -> QueryError {
    match error {
        ValueParseError::Invalid => invalid_input(text).into(),
        ValueParseError::Controlled(error) => error.into(),
    }
}

fn wrong_input(source: JsonElementSource) -> QueryError {
    CassieError::from(DiagnosticError::new(
        "42883",
        format!("function {}(...) does not exist", source.function_name()),
    ))
    .into()
}

fn decode(
    env: &SourceExecutionEnv<'_>,
    source: JsonElementSource,
    argument: Value,
) -> Result<Vec<Entry>, QueryError> {
    if argument.is_null() {
        return Ok(Vec::new());
    }
    if source.binary {
        let document = match argument {
            Value::Json(document) => document,
            Value::String(text) => {
                json_arena::parse_value_in_reserved_memory(text.as_bytes(), env.controls)
                    .map_err(|error| parse_error(error, &text))?
            }
            _ => return Err(wrong_input(source)),
        };
        if contains_nul(&document) {
            return Err(nul_error());
        }
        return binary_entries(source, document);
    }
    let text = match argument {
        Value::String(text) => text,
        Value::Json(document) => document.to_string(),
        _ => return Err(wrong_input(source)),
    };
    text_entries(env, source, &text)
}

fn binary_entries(
    source: JsonElementSource,
    document: serde_json::Value,
) -> Result<Vec<Entry>, QueryError> {
    match document {
        serde_json::Value::Array(elements) if !source.each => Ok(elements
            .into_iter()
            .map(|value| Entry {
                key: None,
                value: Element::Tree(value),
            })
            .collect()),
        serde_json::Value::Object(members) if source.each => {
            let mut members = members.into_iter().collect::<Vec<_>>();
            // JSONB orders object keys by UTF-8 length, then bytes.
            members.sort_by(|(left, _), (right, _)| {
                left.len()
                    .cmp(&right.len())
                    .then_with(|| left.as_bytes().cmp(right.as_bytes()))
            });
            Ok(members
                .into_iter()
                .map(|(key, value)| Entry {
                    key: Some(key),
                    value: Element::Tree(value),
                })
                .collect())
        }
        other => Err(shape_error(
            source,
            match other {
                serde_json::Value::Array(_) => Root::Array,
                serde_json::Value::Object(_) => Root::Object,
                _ => Root::Scalar,
            },
        )),
    }
}

fn text_entries(
    env: &SourceExecutionEnv<'_>,
    source: JsonElementSource,
    text: &str,
) -> Result<Vec<Entry>, QueryError> {
    let root = {
        let document = json_arena::parse_value_in_reserved_memory(text.as_bytes(), env.controls)
            .map_err(|error| parse_error(error, text))?;
        match document {
            serde_json::Value::Array(_) => Root::Array,
            serde_json::Value::Object(_) => Root::Object,
            _ => Root::Scalar,
        }
    };
    let expected = if source.each {
        Root::Object
    } else {
        Root::Array
    };
    if root != expected {
        return Err(shape_error(source, root));
    }
    // The text is valid, so a failure here is only serde's recursion bound (128 levels).
    let nesting = |_: serde_json::Error| {
        QueryError::from(CassieError::from(DiagnosticError::new(
            "54001",
            "JSON document exceeds the supported expansion nesting depth".to_owned(),
        )))
    };
    if source.each {
        let object = serde_json::from_str::<OrderedObject>(text).map_err(nesting)?;
        Ok(object
            .0
            .into_iter()
            .map(|(key, value)| Entry {
                key: Some(key),
                value: Element::Raw(value),
            })
            .collect())
    } else {
        let elements = serde_json::from_str::<Vec<Box<RawValue>>>(text).map_err(nesting)?;
        Ok(elements
            .into_iter()
            .map(|value| Entry {
                key: None,
                value: Element::Raw(value),
            })
            .collect())
    }
}

/// PostgreSQL 18 wording, which differs between the json and jsonb families.
fn shape_error(source: JsonElementSource, root: Root) -> QueryError {
    let name = source.function_name();
    let message = match (source.binary, source.each, root) {
        (true, false, Root::Object) => "cannot extract elements from an object".to_owned(),
        (true, false, _) => "cannot extract elements from a scalar".to_owned(),
        (true, true, _) => format!("cannot call {name} on a non-object"),
        (false, false, Root::Scalar) => format!("cannot call {name} on a scalar"),
        (false, false, _) => format!("cannot call {name} on a non-array"),
        (false, true, Root::Array) => "cannot deconstruct an array as an object".to_owned(),
        (false, true, _) => "cannot deconstruct a scalar".to_owned(),
    };
    CassieError::InvalidParameterValue(message).into()
}

fn entry_bytes(
    source: JsonElementSource,
    entry: &Entry,
    columns: usize,
) -> Result<usize, QueryError> {
    let value = match &entry.value {
        Element::Raw(raw) => raw.get().len(),
        Element::Tree(value) if source.text => crate::types::jsonb::text_len(value)
            .map_err(|error| QueryError::from(CassieError::TypeInput(error)))?,
        Element::Tree(value) => crate::runtime::accounted::json::retained_bytes(value)?,
    };
    let key = entry.key.as_ref().map_or(0, String::capacity);
    let slots = columns.saturating_mul(size_of::<(String, Value)>() + 32);
    Ok(size_of::<BatchRow>()
        .saturating_add(slots)
        .saturating_add(key)
        .saturating_add(value)
        .saturating_add(64))
}

fn element_value(source: JsonElementSource, element: Element) -> Result<Value, QueryError> {
    match (element, source.text) {
        (Element::Tree(value), false) => Ok(Value::Json(value)),
        (Element::Raw(raw), false) => Ok(Value::String(raw.get().to_owned())),
        (Element::Tree(serde_json::Value::Null), true) => Ok(Value::Null),
        (Element::Tree(serde_json::Value::String(text)), true) => Ok(Value::String(text)),
        (Element::Tree(value), true) => crate::types::jsonb::format(&value)
            .map(Value::String)
            .map_err(|error| CassieError::TypeInput(error).into()),
        (Element::Raw(raw), true) => raw_text(&raw),
    }
}

/// `json` text extraction: `null` is SQL NULL, strings are unescaped and every
/// other token keeps its input spelling.
fn raw_text(raw: &RawValue) -> Result<Value, QueryError> {
    let token = raw.get().trim();
    match token.as_bytes().first() {
        Some(b'n') => Ok(Value::Null),
        Some(b'"') => {
            let text = serde_json::from_str::<String>(token).map_err(|error| {
                QueryError::from(CassieError::TypeInput(TypeInputError::new(
                    "22P02",
                    format!("invalid input syntax for type json: {error}"),
                )))
            })?;
            if text.contains('\0') {
                return Err(nul_error());
            }
            Ok(Value::String(text))
        }
        _ => Ok(Value::String(token.to_owned())),
    }
}

fn nul_error() -> QueryError {
    CassieError::TypeInput(TypeInputError::new(
        "22P05",
        "unsupported Unicode escape sequence: \\u0000 cannot be converted to text".into(),
    ))
    .into()
}

/// JSONB cannot hold U+0000 in any string or key.
fn contains_nul(document: &serde_json::Value) -> bool {
    let mut pending = vec![document];
    while let Some(value) = pending.pop() {
        match value {
            serde_json::Value::String(text) if text.contains('\0') => return true,
            serde_json::Value::Array(values) => pending.extend(values),
            serde_json::Value::Object(members) => {
                for (key, value) in members {
                    if key.contains('\0') {
                        return true;
                    }
                    pending.push(value);
                }
            }
            _ => {}
        }
    }
    false
}
