//! Contextual collection JSON conversion and its physical output owner.

use crate::app::CassieError;
use crate::executor::batch::BatchRow;
use crate::executor::json_construction as json;
use crate::executor::retained_memory::{add, mul};
use crate::runtime::{QueryExecutionControls, QueryMemoryReservation};
use crate::sql::ast::FunctionCall;
use crate::sql::functions::CollectionAggregate;
use crate::types::{DataType, Value};

use super::{AggregateExecutionContext, AggregateSpec, QueryError};

/// The value must drop before the reservation covering its allocation.
#[derive(Debug)]
pub(super) struct AggregateOutput {
    value: Value,
    memory: Option<QueryMemoryReservation>,
}

impl AggregateOutput {
    pub(super) fn from_value(value: Value) -> Self {
        Self {
            value,
            memory: None,
        }
    }

    pub(super) fn from_owned(value: Value, memory: QueryMemoryReservation) -> Self {
        let memory = (memory.bytes() != 0).then_some(memory);
        Self { value, memory }
    }

    pub(super) fn into_value(self) -> Value {
        self.value
    }
}

/// Physical values drop before their shared output lease on every error path.
pub(super) struct OutputRow {
    values: Vec<(String, Value)>,
    memory: Option<QueryMemoryReservation>,
    controls: QueryExecutionControls,
}

impl OutputRow {
    pub(super) fn new(
        values: Vec<(String, Value)>,
        specs: &[AggregateSpec],
        controls: &QueryExecutionControls,
    ) -> Result<Self, QueryError> {
        let memory = row_memory(&values, specs, controls)?;
        Ok(Self {
            values,
            memory,
            controls: controls.clone(),
        })
    }

    pub(super) fn append(
        &mut self,
        output: AggregateOutput,
        names: &[String],
    ) -> Result<(), QueryError> {
        self.append_with_observer(output, names, || {})
    }

    pub(super) fn append_with_observer(
        &mut self,
        mut output: AggregateOutput,
        names: &[String],
        mut after_copy: impl FnMut(),
    ) -> Result<(), QueryError> {
        if let Some(memory) = output.memory.as_ref() {
            if !memory.belongs_to(&self.controls) {
                return Err(CassieError::ResourceLimit(
                    "aggregate output owner uses a different query tracker".into(),
                )
                .into());
            }
            if self.memory.is_none() {
                let bytes = add(
                    mul(
                        self.values.capacity(),
                        std::mem::size_of::<(String, Value)>(),
                    )?,
                    add(
                        std::mem::size_of::<BatchRow>(),
                        add(
                            std::mem::size_of::<QueryMemoryReservation>(),
                            2 * std::mem::size_of::<usize>(),
                        )?,
                    )?,
                )?;
                self.memory = Some(self.controls.reserve_query_memory(bytes)?);
            }
        }
        if let Some(memory) = output.memory.take() {
            self.memory
                .as_mut()
                .expect("owner checked above")
                .absorb(memory);
        }
        for name in names {
            push_output(&mut self.values, &mut self.memory, name, &output.value)?;
            after_copy();
            super::check_timeout(&self.controls)?;
        }
        Ok(())
    }

    pub(super) fn finish(self) -> BatchRow {
        BatchRow::new(self.values).with_query_memory(self.memory.map(std::sync::Arc::new))
    }
}

#[derive(Clone, Copy)]
pub(super) struct ConversionContext<'a> {
    pub(super) value_type: Option<&'a DataType>,
    pub(super) controls: Option<&'a QueryExecutionControls>,
}

pub(super) fn finish_scoped(
    function: &FunctionCall,
    row: Option<&BatchRow>,
    context: &AggregateExecutionContext<'_>,
    kind: CollectionAggregate,
    values: Vec<(Value, Option<Value>)>,
    seen_input: bool,
) -> Result<AggregateOutput, QueryError> {
    if !seen_input {
        return Ok(AggregateOutput::from_value(Value::Null));
    }
    let index = usize::from(matches!(kind, CollectionAggregate::JsonObject { .. }));
    let expression = function.args.get(index);
    let _type_memory = row
        .zip(expression)
        .map(|(row, expression)| {
            context.controls.reserve_query_memory(json::typing_bytes(
                row,
                expression,
                context.user_functions,
            )?)
        })
        .transpose()?;
    let data_type = row.zip(expression).and_then(|(row, expression)| {
        crate::executor::array_order::expression_type(row, expression, context.user_functions)
    });
    finish_with_observer(
        kind,
        values,
        seen_input,
        ConversionContext {
            value_type: data_type.as_ref(),
            controls: Some(context.controls),
        },
        || {},
    )
}

pub(super) fn finish_unscoped(
    kind: CollectionAggregate,
    values: Vec<(Value, Option<Value>)>,
    seen_input: bool,
) -> Result<Value, QueryError> {
    finish_with_observer(
        kind,
        values,
        seen_input,
        ConversionContext {
            value_type: None,
            controls: None,
        },
        || {},
    )
    .map(AggregateOutput::into_value)
}

struct EncodedEntry {
    key: Option<String>,
    value: Option<String>,
}

pub(super) fn finish_with_observer(
    kind: CollectionAggregate,
    values: Vec<(Value, Option<Value>)>,
    seen_input: bool,
    context: ConversionContext<'_>,
    mut after_encode: impl FnMut(),
) -> Result<AggregateOutput, QueryError> {
    json::check(context.controls)?;
    if !seen_input {
        return Ok(AggregateOutput::from_value(Value::Null));
    }
    let (binary, object, strict, unique) = match kind {
        CollectionAggregate::Json { binary, strict } => (binary, false, strict, false),
        CollectionAggregate::JsonObject {
            binary,
            strict,
            unique,
        } => (binary, true, strict, unique),
        CollectionAggregate::Array => unreachable!("SQL arrays retain their own finalizer"),
    };
    let bound = output_bound(&values, &context, binary, object)?;
    let mut memory = json::reserve(context.controls, bound)?;
    let mut entries = Vec::<EncodedEntry>::new();
    entries.try_reserve_exact(values.len()).map_err(|error| {
        CassieError::ResourceLimit(format!(
            "unable to allocate JSON aggregate entries: {error}"
        ))
    })?;
    for (first, second) in values {
        json::check(context.controls)?;
        let key = object
            .then(|| object_key(&first, context.controls))
            .transpose()?;
        if unique {
            for previous in &entries {
                json::check(context.controls)?;
                if previous.key == key {
                    return Err(CassieError::from(crate::app::DiagnosticError::new(
                        "22030",
                        "duplicate JSON object key value",
                    ))
                    .into());
                }
            }
        }
        let input = if object {
            second.as_ref()
        } else {
            Some(&first)
        };
        let mut value = input
            .map(|value| json::encode(value, context.value_type, context.controls))
            .transpose()?;
        if value.is_some() {
            after_encode();
            json::check(context.controls)?;
        }
        // PostgreSQL's strict JSONB object transition also omits document null;
        // plain JSON objects and strict array aggregates retain document null.
        if binary && object && strict && value.as_deref() == Some("null") {
            value = None;
        }
        entries.push(EncodedEntry { key, value });
    }
    let document = render(&entries, object, binary, bound, context.controls)?;
    let entry_bytes = entries.iter().try_fold(
        mul(entries.capacity(), std::mem::size_of::<EncodedEntry>())?,
        |bytes, entry| {
            add(
                bytes,
                add(
                    entry.key.as_ref().map_or(0, String::capacity),
                    entry.value.as_ref().map_or(0, String::capacity),
                )?,
            )
        },
    )?;
    let value = finish_document_observing(
        document,
        binary,
        entry_bytes,
        &mut memory,
        context.controls,
        || {},
    )?;
    drop(entries);
    if let Some(memory) = memory.as_mut() {
        let retained = match &value {
            Value::String(text) => text.capacity(),
            _ => crate::executor::retained_memory::value_clone_bytes(&value)?,
        };
        memory.shrink_to(retained);
    }
    Ok(AggregateOutput { value, memory })
}

/// Preserve JSONB's document carrier when the aggregate becomes a physical row value.
/// Plain JSON must keep its original text, including duplicate members and order.
pub(super) fn finish_document_observing(
    document: String,
    binary: bool,
    entry_bytes: usize,
    memory: &mut Option<QueryMemoryReservation>,
    controls: Option<&QueryExecutionControls>,
    mut after_decode: impl FnMut(),
) -> Result<Value, QueryError> {
    let value = json::finish(document, binary, controls)?;
    if !binary {
        return Ok(value);
    }
    let Value::String(document) = value else {
        return Ok(value);
    };
    // Reuse the still-live construction workspace. Grow before parsing only if
    // actual canonical/entry capacities require more than its original bound.
    let required = add(
        crate::runtime::accounted::json::serialized_decode_bytes(document.len())?,
        add(document.capacity(), entry_bytes)?,
    )?;
    if let Some(memory) = memory.as_mut() {
        if required > memory.bytes() {
            memory.try_grow(required - memory.bytes())?;
        }
    }
    json::check(controls)?;
    let document = serde_json::from_str(&document).map_err(|error| {
        CassieError::TypeInput(crate::types::extended::TypeInputError::new(
            "22P02",
            format!("invalid JSONB collection document: {error}"),
        ))
    })?;
    after_decode();
    json::check(controls)?;
    Ok(Value::Json(document))
}

fn row_memory(
    values: &Vec<(String, Value)>,
    specs: &[AggregateSpec],
    controls: &QueryExecutionControls,
) -> Result<Option<QueryMemoryReservation>, QueryError> {
    let json = specs.iter().any(|spec| {
        crate::sql::functions::function(&spec.function.name).is_some_and(|metadata| {
            matches!(
                metadata.id,
                crate::sql::functions::FunctionId::CollectionAggregate(
                    CollectionAggregate::Json { .. } | CollectionAggregate::JsonObject { .. }
                )
            )
        })
    });
    if !json {
        return Ok(None);
    }
    let mut bytes = add(
        mul(values.capacity(), std::mem::size_of::<(String, Value)>())?,
        add(
            std::mem::size_of::<BatchRow>(),
            add(
                std::mem::size_of::<QueryMemoryReservation>(),
                2 * std::mem::size_of::<usize>(),
            )?,
        )?,
    )?;
    for (name, value) in values {
        bytes = add(
            bytes,
            add(
                name.capacity(),
                crate::executor::retained_memory::value_clone_bytes(value)?,
            )?,
        )?;
    }
    controls
        .reserve_query_memory(bytes)
        .map(Some)
        .map_err(Into::into)
}

fn push_output(
    values: &mut Vec<(String, Value)>,
    memory: &mut Option<QueryMemoryReservation>,
    name: &str,
    value: &Value,
) -> Result<(), QueryError> {
    if let Some(memory) = memory.as_mut() {
        let old_capacity = values.capacity();
        if values.len() == old_capacity {
            let capacity = values.len().checked_add(1).ok_or_else(|| {
                CassieError::ResourceLimit("aggregate output capacity overflow".into())
            })?;
            let backing = mul(capacity, std::mem::size_of::<(String, Value)>())?;
            memory.try_grow(backing)?;
            values.try_reserve_exact(1).map_err(|error| {
                CassieError::ResourceLimit(format!("unable to allocate aggregate output: {error}"))
            })?;
            let actual = mul(values.capacity(), std::mem::size_of::<(String, Value)>())?;
            if actual > backing {
                memory.try_grow(actual - backing)?;
            }
            memory.shrink_to(
                memory.bytes() - mul(old_capacity, std::mem::size_of::<(String, Value)>())?,
            );
        }
        memory.try_grow(add(
            name.len(),
            crate::executor::retained_memory::value_clone_bytes(value)?,
        )?)?;
    }
    values.push((name.to_owned(), value.clone()));
    Ok(())
}

fn output_bound(
    values: &[(Value, Option<Value>)],
    context: &ConversionContext<'_>,
    binary: bool,
    object: bool,
) -> Result<usize, QueryError> {
    let mut bytes = add(mul(values.len(), std::mem::size_of::<EncodedEntry>())?, 4)?;
    for (first, second) in values {
        json::check(context.controls)?;
        if object {
            let mut key_bytes = json::bound_controlled(first, None, false, context.controls)?;
            if let Value::Array(array) = first {
                // Object keys use SQL array text, whose nondefault bound prefixes
                // do not occur in the JSON array value's punctuation bound.
                key_bytes = add(key_bytes, add(1, mul(array.dimensions().len(), 25)?)?)?;
            }
            bytes = add(bytes, mul(key_bytes, 6)?)?;
        }
        let value = if object { second.as_ref() } else { Some(first) };
        if let Some(value) = value {
            bytes = add(
                bytes,
                json::bound_controlled(value, context.value_type, binary, context.controls)?,
            )?;
        }
        bytes = add(bytes, 8)?;
    }
    Ok(bytes)
}

fn object_key(
    value: &Value,
    controls: Option<&QueryExecutionControls>,
) -> Result<String, QueryError> {
    let text = if let Some(text) = json::scalar_key_text(value, controls) {
        text
    } else {
        match value {
            Value::Null => {
                return Err(CassieError::from(crate::app::DiagnosticError::new(
                    "22004",
                    "JSON object aggregate key must not be null",
                ))
                .into())
            }
            Value::Array(array) => crate::types::array_value::format_text(array, &DataType::Text)
                .unwrap_or_else(|| array.to_nested_json().to_string()),
            Value::Json(value) => value.to_string(),
            Value::Vector(value) => format!(
                "[{}]",
                value
                    .values
                    .iter()
                    .map(std::string::ToString::to_string)
                    .collect::<Vec<_>>()
                    .join(",")
            ),
            _ => unreachable!("scalar keys use the common SQL formatter"),
        }
    };
    serde_json::to_string(&text).map_err(|error| QueryError::General(error.to_string()))
}

fn render(
    entries: &[EncodedEntry],
    object: bool,
    binary: bool,
    bound: usize,
    controls: Option<&QueryExecutionControls>,
) -> Result<String, QueryError> {
    let mut document = String::new();
    document.try_reserve_exact(bound).map_err(|error| {
        CassieError::ResourceLimit(format!("unable to allocate JSON aggregate output: {error}"))
    })?;
    document.push(if object { '{' } else { '[' });
    if object && !binary {
        document.push(' ');
    }
    let mut first = true;
    for entry in entries {
        json::check(controls)?;
        let Some(value) = &entry.value else { continue };
        if !first {
            document.push_str(", ");
        }
        first = false;
        if let Some(key) = &entry.key {
            document.push_str(key);
            document.push_str(if binary { ": " } else { " : " });
        }
        document.push_str(value);
    }
    if object && !binary {
        document.push(' ');
    }
    document.push(if object { '}' } else { ']' });
    Ok(document)
}

#[cfg(test)]
#[path = "collection_json_key_bound_tests.rs"]
mod key_bound_tests;
