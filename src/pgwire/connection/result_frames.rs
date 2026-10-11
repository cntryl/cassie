//! Admission for transient query-result encoding, including buffers held across writes.

use std::borrow::Cow;
use std::io;
use std::mem::size_of;

use crate::app::CassieError;
use crate::executor::retained_memory::{add, mul, serialized_json_bytes};
use crate::executor::{ColumnMeta, QueryResult};
use crate::runtime::{QueryExecutionControls, QueryMemoryReservation};
use crate::types::schema::vector_dimensions_for_oid;
use crate::types::Value;

use super::writers::{
    append_command_complete_frame, append_data_row_frame, append_portal_suspended_frame,
    append_row_description_frame, select_command_tag,
};

#[cfg(test)]
#[path = "result_frames/tests.rs"]
mod tests;

/// Buffers precede their lease so cancellation drops owned allocations before admission.
pub(super) struct QueryResultFrames<'a> {
    bytes: Vec<u8>,
    command: Cow<'a, str>,
    _memory: QueryMemoryReservation,
}

impl<'a> QueryResultFrames<'a> {
    pub(super) fn try_new(
        result: &'a QueryResult,
        rows: &[Vec<Value>],
        formats: &[i16],
    ) -> Result<Self, CassieError> {
        let (scratch, capacity) = scratch_bytes(result, rows, formats)?;
        let memory = result.admission().try_sibling(scratch)?;
        let bytes = frame_buffer(capacity)?;
        // The SELECT tag can allocate; admit it together with the frame buffers first.
        let command = select_command_tag(result.command(), rows.len());
        Ok(Self {
            bytes,
            command,
            _memory: memory,
        })
    }

    pub(super) fn try_command(
        command: &'a str,
        controls: &QueryExecutionControls,
    ) -> Result<Self, CassieError> {
        controls.check_interruption()?;
        let payload = add(command.len(), 1)?;
        let capacity = add(payload, 5)?;
        let scratch = add(mul(capacity.max(8), 3)?, mul(payload.max(8), 3)?)?;
        let memory = controls.reserve_query_memory(scratch)?;
        let bytes = frame_buffer(capacity)?;
        Ok(Self {
            bytes,
            command: Cow::Borrowed(command),
            _memory: memory,
        })
    }

    pub(super) fn description(
        &mut self,
        columns: &[ColumnMeta],
        formats: &[i16],
    ) -> io::Result<()> {
        self.bytes.clear();
        append_row_description_frame(&mut self.bytes, columns, formats)
    }

    pub(super) fn row(
        &mut self,
        row: &[Value],
        columns: &[ColumnMeta],
        formats: &[i16],
    ) -> io::Result<()> {
        self.bytes.clear();
        append_data_row_frame(&mut self.bytes, row, columns, formats)
    }

    pub(super) fn completion(&mut self, suspended: bool) -> io::Result<()> {
        self.bytes.clear();
        if suspended {
            append_portal_suspended_frame(&mut self.bytes)
        } else {
            append_command_complete_frame(&mut self.bytes, &self.command)
        }
    }

    pub(super) fn bytes(&self) -> &[u8] {
        &self.bytes
    }
}

fn frame_buffer(capacity: usize) -> Result<Vec<u8>, CassieError> {
    let mut bytes = Vec::new();
    bytes
        .try_reserve_exact(capacity)
        .map_err(crate::runtime::owned_memory::allocation_error)?;
    if bytes.capacity() > capacity {
        return Err(CassieError::ResourceLimit(
            "allocator exceeded admitted pgwire frame capacity".to_owned(),
        ));
    }
    Ok(bytes)
}

fn scratch_bytes(
    result: &QueryResult,
    rows: &[Vec<Value>],
    formats: &[i16],
) -> Result<(usize, usize), CassieError> {
    let description = result.columns().iter().try_fold(2usize, |bytes, column| {
        add(bytes, add(column.name.len(), 19)?)
    })?;
    // usize's decimal form is at most 20 digits on the supported targets.
    let command = result.command().len().max(27);
    let mut largest_payload = description.max(add(command, 1)?);
    let mut largest_encoder = mul(largest_payload.max(8), 3)?;
    for row in rows {
        let mut payload = add(2, mul(row.len(), 4)?)?;
        let mut cell_scratch = 0;
        for (index, value) in row.iter().enumerate() {
            let oid = result
                .columns()
                .get(index)
                .map_or(25, |column| column.type_oid);
            let format = match formats.len() {
                0 => 0,
                1 => formats[0],
                _ => formats.get(index).copied().unwrap_or(0),
            };
            let cell = cell_bytes(value, oid, format)?;
            payload = add(payload, cell.encoded)?;
            cell_scratch = cell_scratch.max(cell.scratch);
        }
        largest_payload = largest_payload.max(payload);
        largest_encoder = largest_encoder.max(add(mul(payload.max(8), 3)?, cell_scratch)?);
    }
    // Vec growth may overlap the old buffer and its doubled replacement.
    // Include the largest retained frame, the active encoder/payload and tag.
    let capacity = add(largest_payload, 5)?;
    let scratch = add(
        mul(capacity.max(8), 3)?,
        add(largest_encoder, mul(command.max(8), 3)?)?,
    )?;
    Ok((scratch, capacity))
}

struct CellBytes {
    encoded: usize,
    scratch: usize,
}

fn cell_bytes(value: &Value, oid: i64, format: i16) -> Result<CellBytes, CassieError> {
    if matches!(value, Value::Null) {
        return Ok(CellBytes {
            encoded: 0,
            scratch: 0,
        });
    }
    if format == 0 || matches!(oid, 25 | 1042 | 1043 | 705) {
        return text_bytes(value);
    }
    if let Some(dimensions) = vector_dimensions_for_oid(oid) {
        let encoded = add(4, mul(dimensions, 4)?)?;
        return Ok(CellBytes {
            encoded,
            scratch: encoded,
        });
    }
    if (34_000..44_000).contains(&oid) {
        if let Value::Json(serde_json::Value::Array(values)) = value {
            let mut encoded = if values.is_empty() { 12 } else { 20 };
            let mut element_scratch = 0;
            for value in values {
                let cell = array_element_bytes(value, oid - 34_000)?;
                encoded = add(encoded, add(4, cell.encoded)?)?;
                element_scratch = element_scratch.max(cell.scratch);
            }
            return Ok(CellBytes {
                encoded,
                scratch: add(mul(encoded.max(8), 3)?, element_scratch)?,
            });
        }
    }
    if oid == 114 {
        return match value {
            Value::Json(value) => json_bytes(value),
            _ => text_bytes(value),
        };
    }
    let encoded = match oid {
        16 => 1,
        21 => 2,
        23 | 1082 => 4,
        2950 => 16,
        17 => match value {
            Value::String(text) => text.len() / 2,
            _ => 0,
        },
        _ => 8,
    };
    let scratch = match value {
        // Bool lowercasing and the timestamp fallback each create a text copy.
        Value::String(text) => add(encoded, mul(text.len().max(8), 3)?)?,
        // Float-to-int8 validation formats a fixed decimal before parsing.
        Value::Float64(_) if oid == 20 => add(encoded, 3 * 328)?,
        _ => encoded,
    };
    Ok(CellBytes { encoded, scratch })
}

fn text_bytes(value: &Value) -> Result<CellBytes, CassieError> {
    let encoded = match value {
        Value::Null => 4,
        Value::Bool(_) => 5,
        Value::Int64(_) => 20,
        // Includes the sign and decimal point of the smallest negative subnormal.
        Value::Float64(_) => 328,
        Value::String(text) => {
            return Ok(CellBytes {
                encoded: text.len(),
                scratch: text.len(),
            })
        }
        Value::Json(value) => return json_bytes(value),
        Value::Vector(vector) => {
            let encoded = add(2, mul(vector.values.len(), 65)?)?;
            // f32 output is under 64 bytes. Count all intermediate strings,
            // their directory, join output and final bracketed formatting.
            let scratch = add(
                mul(encoded.max(8), 6)?,
                mul(vector.values.len(), 128 + size_of::<String>())?,
            )?;
            return Ok(CellBytes { encoded, scratch });
        }
    };
    Ok(CellBytes {
        encoded,
        scratch: mul(encoded.max(8), 3)?,
    })
}

fn json_bytes(value: &serde_json::Value) -> Result<CellBytes, CassieError> {
    let encoded = serialized_json_bytes(value)?;
    // serde_json::to_string starts a 128-byte buffer and grows geometrically.
    Ok(CellBytes {
        encoded,
        scratch: mul(encoded.max(128), 3)?,
    })
}

fn array_element_bytes(value: &serde_json::Value, oid: i64) -> Result<CellBytes, CassieError> {
    if value.is_null() {
        return Ok(CellBytes {
            encoded: 0,
            scratch: 0,
        });
    }
    if oid == 114 || value.is_object() {
        return json_bytes(value);
    }
    match value {
        serde_json::Value::Bool(value) => cell_bytes(&Value::Bool(*value), oid, 1),
        serde_json::Value::Number(value) => {
            let value = value
                .as_i64()
                .map(Value::Int64)
                .or_else(|| value.as_f64().map(Value::Float64))
                .unwrap_or(Value::Null);
            cell_bytes(&value, oid, 1)
        }
        serde_json::Value::String(text) => {
            let encoded = if matches!(oid, 25 | 1042 | 1043 | 705) {
                text.len()
            } else {
                text.len().max(16)
            };
            Ok(CellBytes {
                encoded,
                scratch: add(encoded, mul(text.len().max(8), 3)?)?,
            })
        }
        _ => Ok(CellBytes {
            encoded: 0,
            scratch: 0,
        }),
    }
}
