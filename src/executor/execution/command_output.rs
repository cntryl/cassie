//! Result construction for commands under their existing request memory controls.
use std::fmt::{self, Write};
use std::mem::size_of;

use super::{ColumnMeta, DataType, QueryError, QueryExecutionControls, QueryResult, Value};
use crate::executor::retained_memory::{add, mul};
use crate::runtime::accounted::Accounted;

pub(super) enum Cell<'a> {
    Text(&'a str),
    OptionalText(Option<&'a str>),
    Joined(&'a [String]),
    Int(i64),
    Bool(bool),
}

pub(super) fn empty(
    command: &str,
    controls: &QueryExecutionControls,
) -> Result<QueryResult, QueryError> {
    let mut result = QueryResult::try_builder(controls)?;
    result.try_set_command(command)?;
    Ok(result.finish()?)
}

pub(super) fn counted(
    command: &str,
    count: impl fmt::Display,
    controls: &QueryExecutionControls,
) -> Result<QueryResult, QueryError> {
    let mut result = QueryResult::try_builder(controls)?;
    set_counted(&mut result, command, count)?;
    Ok(result.finish()?)
}

pub(super) fn set_counted(
    result: &mut super::QueryResultBuilder<'_>,
    command: &str,
    count: impl fmt::Display,
) -> Result<(), QueryError> {
    let mut text = CommandText {
        bytes: [0; 128],
        length: 0,
    };
    write!(text, "{command} {count}")
        .map_err(|_| QueryError::General("command tag exceeds its fixed buffer".to_owned()))?;
    result.try_set_command(text.as_str())?;
    Ok(())
}

pub(super) fn columns<'a>(
    command: &str,
    columns: &[(&str, DataType)],
    controls: &'a QueryExecutionControls,
) -> Result<super::QueryResultBuilder<'a>, QueryError> {
    let mut result = QueryResult::try_builder(controls)?;
    result.try_set_command(command)?;
    for (name, data_type) in columns {
        let scratch_bytes = add(
            size_of::<ColumnMeta>(),
            add(name.len(), type_name_scratch_bytes(data_type)?)?,
        )?;
        let column = Accounted::try_new(controls, scratch_bytes, || {
            ColumnMeta::from_data_type(*name, data_type)
        })?;
        result.try_push_column(column.get())?;
    }
    Ok(result)
}

pub(super) fn push_row(
    result: &mut super::QueryResultBuilder<'_>,
    cells: &[Cell<'_>],
    controls: &QueryExecutionControls,
) -> Result<(), QueryError> {
    let bytes = cells
        .iter()
        .try_fold(mul(cells.len(), size_of::<Value>())?, |bytes, cell| {
            add(bytes, cell.heap_bytes()?)
        })?;
    let _scratch = controls.reserve_query_memory(bytes)?;
    let mut row = Vec::new();
    row.try_reserve_exact(cells.len()).map_err(|error| {
        crate::app::CassieError::ResourceLimit(format!(
            "unable to construct command result row: {error}"
        ))
    })?;
    for cell in cells {
        row.push(cell.value());
    }
    result.try_push_row(&row)?;
    Ok(())
}

impl Cell<'_> {
    fn heap_bytes(&self) -> Result<usize, crate::app::CassieError> {
        match self {
            Self::Text(text) => Ok(text.len()),
            Self::OptionalText(text) => Ok(text.map_or(0, str::len)),
            Self::Joined(parts) => parts
                .iter()
                .try_fold(parts.len().saturating_sub(1), |bytes, part| {
                    add(bytes, part.len())
                }),
            Self::Int(_) | Self::Bool(_) => Ok(0),
        }
    }

    fn value(&self) -> Value {
        match self {
            Self::Text(text) => Value::String((*text).to_owned()),
            Self::OptionalText(text) => {
                text.map_or(Value::Null, |text| Value::String(text.to_owned()))
            }
            Self::Joined(parts) => Value::String(parts.join(",")),
            Self::Int(value) => Value::Int64(*value),
            Self::Bool(value) => Value::Bool(*value),
        }
    }
}

fn type_name_scratch_bytes(data_type: &DataType) -> Result<usize, crate::app::CassieError> {
    match data_type {
        // Each array formatting step retains its input while constructing its output.
        DataType::Array(inner) => add(mul(type_name_scratch_bytes(inner)?, 3)?, 4),
        // Covers fixed names and formatted numeric widths without allocating to estimate them.
        _ => Ok(128),
    }
}

struct CommandText {
    bytes: [u8; 128],
    length: usize,
}

impl CommandText {
    fn as_str(&self) -> &str {
        std::str::from_utf8(&self.bytes[..self.length]).expect("formatted command is UTF-8")
    }
}

impl Write for CommandText {
    fn write_str(&mut self, text: &str) -> fmt::Result {
        let end = self.length.checked_add(text.len()).ok_or(fmt::Error)?;
        let output = self.bytes.get_mut(self.length..end).ok_or(fmt::Error)?;
        output.copy_from_slice(text.as_bytes());
        self.length = end;
        Ok(())
    }
}

#[cfg(test)]
#[path = "command_output_tests.rs"]
mod tests;
