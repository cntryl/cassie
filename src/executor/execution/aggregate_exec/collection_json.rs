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

#[cfg(test)]
#[path = "output_row_control_tests.rs"]
mod output_row_control_tests;

/// The value must drop before the reservation covering its allocation.
#[derive(Debug)]
pub(super) struct AggregateOutput {
    value: Value,
    memory: Option<QueryMemoryReservation>,
    copy_mode: OutputCopyMode,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum OutputCopyMode {
    Ordinary,
    NullArray,
}

impl AggregateOutput {
    pub(super) fn from_value(value: Value) -> Self {
        Self {
            value,
            memory: None,
            copy_mode: OutputCopyMode::Ordinary,
        }
    }

    pub(super) fn from_owned(value: Value, memory: QueryMemoryReservation) -> Self {
        let memory = (memory.bytes() != 0).then_some(memory);
        Self {
            value,
            memory,
            copy_mode: OutputCopyMode::Ordinary,
        }
    }

    pub(super) fn into_value(self) -> Value {
        self.value
    }
}

struct PendingNullArray {
    values: Vec<serde_json::Value>,
    memory: QueryMemoryReservation,
}

struct PendingModeString {
    value: String,
    memory: QueryMemoryReservation,
}

pub(super) fn mode_string_output(
    text: &str,
    source: &QueryMemoryReservation,
    controls: &QueryExecutionControls,
    mut observe: impl FnMut(),
) -> Result<AggregateOutput, QueryError> {
    if !source.belongs_to(controls) {
        return Err(CassieError::ResourceLimit(
            "mode source uses a different query tracker".into(),
        )
        .into());
    }
    let mut pending = PendingModeString {
        value: String::new(),
        memory: source.try_sibling(text.len())?,
    };
    pending.value = copy_output_string(
        text,
        text.len(),
        controls,
        OutputProgress::ValueCopy,
        &mut |_| observe(),
    )?;
    let PendingModeString { value, memory } = pending;
    Ok(AggregateOutput {
        value: Value::String(value),
        memory: Some(memory),
        copy_mode: OutputCopyMode::Ordinary,
    })
}

pub(super) fn null_array_output(
    count: usize,
    source: &QueryMemoryReservation,
    controls: &QueryExecutionControls,
    mut observe: impl FnMut(usize),
) -> Result<AggregateOutput, QueryError> {
    if !source.belongs_to(controls) {
        return Err(CassieError::ResourceLimit(
            "percentile source uses a different query tracker".into(),
        )
        .into());
    }
    super::check_timeout(controls)?;
    let (values, memory) = crate::runtime::owned_memory::try_reserve_vec(count, source)?;
    let mut pending = PendingNullArray { values, memory };
    for index in 0..count {
        super::check_timeout(controls)?;
        pending.values.push(serde_json::Value::Null);
        observe(index);
        super::check_timeout(controls)?;
    }
    super::check_timeout(controls)?;
    let PendingNullArray { values, memory } = pending;
    Ok(AggregateOutput {
        value: Value::Json(serde_json::Value::Array(values)),
        memory: Some(memory),
        copy_mode: OutputCopyMode::NullArray,
    })
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
        output: AggregateOutput,
        names: &[String],
        after_copy: impl FnMut(),
    ) -> Result<(), QueryError> {
        self.append_with_progress(output, names, after_copy, &mut |_| {})
    }

    fn append_with_progress(
        &mut self,
        mut output: AggregateOutput,
        names: &[String],
        mut after_copy: impl FnMut(),
        progress: &mut impl FnMut(OutputProgress),
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
            push_output_mode_observing(
                &mut self.values,
                &mut self.memory,
                &self.controls,
                name,
                &output.value,
                output.copy_mode,
                progress,
            )?;
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
    Ok(AggregateOutput {
        value,
        memory,
        copy_mode: OutputCopyMode::Ordinary,
    })
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

#[derive(Clone, Copy)]
enum OutputProgress {
    DirectoryMove,
    NameCopy,
    ValueCopy,
}

#[cfg(test)]
fn push_output_observing(
    values: &mut Vec<(String, Value)>,
    memory: &mut Option<QueryMemoryReservation>,
    controls: &QueryExecutionControls,
    name: &str,
    value: &Value,
    progress: &mut impl FnMut(OutputProgress),
) -> Result<(), QueryError> {
    push_output_mode_observing(
        values,
        memory,
        controls,
        name,
        value,
        OutputCopyMode::Ordinary,
        progress,
    )
}

fn push_output_mode_observing(
    values: &mut Vec<(String, Value)>,
    memory: &mut Option<QueryMemoryReservation>,
    controls: &QueryExecutionControls,
    name: &str,
    value: &Value,
    mode: OutputCopyMode,
    progress: &mut impl FnMut(OutputProgress),
) -> Result<(), QueryError> {
    let Some(memory) = memory.as_mut() else {
        if mode == OutputCopyMode::NullArray {
            return Err(output_overflow().into());
        }
        values.push((name.to_owned(), value.clone()));
        return Ok(());
    };
    controls.check_interruption()?;
    let old_capacity = values.capacity();
    if values.len() == old_capacity {
        let capacity = values.len().checked_add(1).ok_or_else(output_overflow)?;
        let backing = mul(capacity, std::mem::size_of::<(String, Value)>())?;
        struct Replacement {
            values: Vec<(String, Value)>,
            memory: QueryMemoryReservation,
        }
        let mut replacement = Replacement {
            values: Vec::new(),
            memory: memory.try_sibling(backing)?,
        };
        replacement
            .values
            .try_reserve_exact(capacity)
            .map_err(|error| crate::runtime::owned_memory::allocation_error(&error))?;
        if replacement.values.capacity() > capacity {
            return Err(output_overflow().into());
        }
        while let Some(item) = values.pop() {
            controls.check_interruption()?;
            replacement.values.push(item);
            progress(OutputProgress::DirectoryMove);
            controls.check_interruption()?;
        }
        let len = replacement.values.len();
        for index in 0..len / 2 {
            controls.check_interruption()?;
            replacement.values.swap(index, len - index - 1);
        }
        controls.check_interruption()?;
        let old = std::mem::replace(values, replacement.values);
        drop(old);
        memory.absorb(replacement.memory);
        let old_bytes = mul(old_capacity, std::mem::size_of::<(String, Value)>())?;
        let retained = memory
            .bytes()
            .checked_sub(old_bytes)
            .ok_or_else(output_overflow)?;
        memory.shrink_to(retained);
    }
    let value_bytes = match (mode, value) {
        (OutputCopyMode::NullArray, Value::Json(serde_json::Value::Array(values))) => {
            mul(values.len(), std::mem::size_of::<serde_json::Value>())?
        }
        (OutputCopyMode::NullArray, _) => return Err(output_overflow().into()),
        (OutputCopyMode::Ordinary, _) => {
            crate::executor::retained_memory::value_clone_bytes(value)?
        }
    };
    memory.try_grow(add(name.len(), value_bytes)?)?;
    let name = copy_output_string(
        name,
        name.len(),
        controls,
        OutputProgress::NameCopy,
        progress,
    )?;
    let value = match (mode, value) {
        (OutputCopyMode::NullArray, Value::Json(serde_json::Value::Array(values))) => Value::Json(
            serde_json::Value::Array(copy_null_array(values, controls, progress)?),
        ),
        (OutputCopyMode::Ordinary, Value::String(text)) => Value::String(copy_output_string(
            text,
            text.capacity(),
            controls,
            OutputProgress::ValueCopy,
            progress,
        )?),
        (OutputCopyMode::Ordinary, _) => value.clone(),
        (OutputCopyMode::NullArray, _) => return Err(output_overflow().into()),
    };
    values.push((name, value));
    Ok(())
}

fn copy_null_array(
    values: &[serde_json::Value],
    controls: &QueryExecutionControls,
    progress: &mut impl FnMut(OutputProgress),
) -> Result<Vec<serde_json::Value>, QueryError> {
    let mut output = Vec::new();
    output
        .try_reserve_exact(values.len())
        .map_err(|error| crate::runtime::owned_memory::allocation_error(&error))?;
    if output.capacity() > values.len() {
        return Err(output_overflow().into());
    }
    for value in values {
        controls.check_interruption()?;
        if !value.is_null() {
            return Err(output_overflow().into());
        }
        output.push(serde_json::Value::Null);
        progress(OutputProgress::ValueCopy);
        controls.check_interruption()?;
    }
    Ok(output)
}

fn copy_output_string(
    text: &str,
    admitted: usize,
    controls: &QueryExecutionControls,
    phase: OutputProgress,
    progress: &mut impl FnMut(OutputProgress),
) -> Result<String, QueryError> {
    controls.check_interruption()?;
    let mut output = String::new();
    output
        .try_reserve_exact(text.len())
        .map_err(|error| crate::runtime::owned_memory::allocation_error(&error))?;
    if output.capacity() > admitted {
        return Err(output_overflow().into());
    }
    let mut start = 0;
    while start < text.len() {
        controls.check_interruption()?;
        let mut end = start.saturating_add(1024).min(text.len());
        while !text.is_char_boundary(end) {
            end -= 1;
        }
        output.push_str(&text[start..end]);
        progress(phase);
        controls.check_interruption()?;
        start = end;
    }
    controls.check_interruption()?;
    Ok(output)
}

fn output_overflow() -> CassieError {
    CassieError::ResourceLimit("aggregate output exceeded its admitted capacity".into())
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
