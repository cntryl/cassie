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
    append_command_complete_frame, append_data_row_frame_in, append_portal_suspended_frame,
    append_row_description_frame, select_command_tag,
};

#[cfg(test)]
#[path = "result_frames/tests.rs"]
mod tests;

/// Buffers precede their lease so cancellation drops owned allocations before admission.
pub(super) struct QueryResultFrames<'a> {
    bytes: Vec<u8>,
    command: Cow<'a, str>,
    result: Option<&'a QueryResult>,
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
        let returned_rows =
            (!result.columns().is_empty() || result.row_count() != 0).then_some(rows.len());
        let command = select_command_tag(result.command(), returned_rows);
        Ok(Self {
            bytes,
            command,
            result: Some(result),
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
            result: None,
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

    /// Writes the `DataRow` for result row `row_index`. A binary `reg*` column
    /// sends the OID its rendered name stands for.
    pub(super) fn row(
        &mut self,
        row_index: usize,
        row: &[Value],
        columns: &[ColumnMeta],
        formats: &[i16],
    ) -> io::Result<()> {
        self.bytes.clear();
        let postgres_array_text_columns = self
            .result
            .map_or(&[][..], QueryResult::postgres_array_text_columns);
        let temporal = self.result.map_or_else(
            || crate::types::extended::temporal::default_session(),
            QueryResult::temporal_context,
        );
        let binary_object_columns = self.result.map(|result| {
            (0..row.len())
                .filter(|column| super::writers::result_format_for_index(formats, *column) == 1)
                .filter_map(|column| {
                    result
                        .object_oid(row_index, column)
                        .map(|oid| (column, Value::Int64(i64::from(oid))))
                })
                .collect::<Vec<_>>()
        });
        match binary_object_columns {
            Some(replacements) if !replacements.is_empty() => {
                let mut row = row.to_vec();
                for (column, value) in replacements {
                    row[column] = value;
                }
                append_data_row_frame_in(
                    &mut self.bytes,
                    &row,
                    columns,
                    formats,
                    postgres_array_text_columns,
                    temporal,
                )
            }
            _ => append_data_row_frame_in(
                &mut self.bytes,
                row,
                columns,
                formats,
                postgres_array_text_columns,
                temporal,
            ),
        }
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
        .map_err(|error| crate::runtime::owned_memory::allocation_error(&error))?;
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
    let postgres_array_text_columns = result.postgres_array_text_columns();
    for row in rows {
        let mut payload = add(2, mul(row.len(), 4)?)?;
        let mut cell_scratch = 0;
        for (index, value) in row.iter().enumerate() {
            let oid = result
                .columns()
                .get(index)
                .map_or(25, |column| column.type_oid);
            let atttypmod = result
                .columns()
                .get(index)
                .map_or(-1, |column| column.atttypmod);
            let format = match formats.len() {
                0 => 0,
                1 => formats[0],
                _ => formats.get(index).copied().unwrap_or(0),
            };
            let postgres_array_text = postgres_array_text_columns.binary_search(&index).is_ok();
            let mut cell = cell_bytes(value, oid, atttypmod, format, postgres_array_text)?;
            if format == 0 && array_element_oid(oid).is_some() {
                let mut extra = 0;
                if let Value::Array(array) = value {
                    if array
                        .dimensions()
                        .iter()
                        .any(|dimension| dimension.lower_bound != 1)
                    {
                        extra = add(extra, mul(array.dimensions().len(), 32)?)?;
                    }
                }
                if array_element_oid(oid) == Some(1184) {
                    let count = match value {
                        Value::Array(array) => array.len(),
                        Value::Json(serde_json::Value::Array(array)) => array.len(),
                        _ => 0,
                    };
                    extra = add(
                        extra,
                        mul(count, result.temporal_context().zone.name().len())?,
                    )?;
                }
                cell.encoded = add(cell.encoded, mul(extra, 2)?)?;
                cell.scratch = add(cell.scratch, mul(extra, 8)?)?;
            }
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

fn cell_bytes(
    value: &Value,
    oid: i64,
    atttypmod: i32,
    format: i16,
    postgres_array_text: bool,
) -> Result<CellBytes, CassieError> {
    if matches!(value, Value::Null) {
        return Ok(CellBytes {
            encoded: 0,
            scratch: 0,
        });
    }
    if format == 0 && (postgres_array_text || array_element_oid(oid).is_some()) {
        return postgres_array_text_bytes(value, oid);
    }
    if format == 1 && oid == 1042 && atttypmod >= 4 {
        return bpchar_cell_bytes(value, atttypmod);
    }
    if oid == 3802 {
        return jsonb_cell_bytes(value, format);
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
    if let Some(bytes) = array_cell_bytes(value, oid)? {
        return Ok(bytes);
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

fn bpchar_cell_bytes(value: &Value, atttypmod: i32) -> Result<CellBytes, CassieError> {
    let Value::String(text) = value else {
        return text_bytes(value);
    };
    let width = usize::try_from(atttypmod - 4).map_err(|_| {
        CassieError::ResourceLimit("bpchar typmod exceeds the supported range".into())
    })?;
    let padding = width.saturating_sub(text.chars().count());
    let encoded = add(text.len(), padding)?;
    Ok(CellBytes {
        encoded,
        scratch: add(encoded, mul(text.len().max(8), 3)?)?,
    })
}

fn jsonb_cell_bytes(value: &Value, format: i16) -> Result<CellBytes, CassieError> {
    let (encoded, scratch) = match value {
        Value::Json(value) => {
            let encoded = crate::types::jsonb::text_len(value).map_err(CassieError::TypeInput)?;
            (
                encoded,
                add(
                    mul(encoded.max(8), 8)?,
                    crate::runtime::accounted::json::retained_bytes(value)?,
                )?,
            )
        }
        Value::String(text) => {
            let encoded =
                crate::types::jsonb::input_text_len(text).map_err(CassieError::TypeInput)?;
            (
                encoded,
                add(
                    mul(encoded.max(8), 8)?,
                    crate::runtime::accounted::json::serialized_decode_bytes(text.len())?,
                )?,
            )
        }
        _ => {
            let bytes = text_bytes(value)?;
            (bytes.encoded, bytes.scratch)
        }
    };
    Ok(CellBytes {
        encoded: add(encoded, usize::from(format == 1))?,
        scratch,
    })
}

fn array_cell_bytes(value: &Value, oid: i64) -> Result<Option<CellBytes>, CassieError> {
    let Some(element_oid) = array_element_oid(oid) else {
        return Ok(None);
    };
    let (count, dimension_count) = match value {
        Value::Json(serde_json::Value::Array(values)) => {
            (values.len(), usize::from(!values.is_empty()))
        }
        Value::Array(array) => (array.len(), array.dimensions().len()),
        _ => return Ok(None),
    };
    let mut encoded = add(12, mul(dimension_count, 8)?)?;
    let mut element_scratch = 0;
    for index in 0..count {
        let cell = match value {
            Value::Json(serde_json::Value::Array(values)) => {
                array_element_bytes(&values[index], element_oid)?
            }
            Value::Array(array) => {
                native_array_element_bytes(&array.elements()[index], element_oid)?
            }
            _ => unreachable!("native or JSON array"),
        };
        encoded = add(encoded, add(4, cell.encoded)?)?;
        element_scratch = element_scratch.max(cell.scratch);
    }
    Ok(Some(CellBytes {
        encoded,
        scratch: add(mul(encoded.max(8), 3)?, element_scratch)?,
    }))
}

fn native_array_element_bytes(value: &Value, oid: i64) -> Result<CellBytes, CassieError> {
    if value.is_null() {
        return Ok(CellBytes {
            encoded: 0,
            scratch: 0,
        });
    }
    if let Value::Json(value) = value {
        return array_element_bytes(value, oid);
    }
    // Native scalar receivers have bounded framing in addition to their text
    // magnitude (numeric digits, bit length, network header, temporal width).
    let text = text_bytes(value)?;
    let encoded = if matches!(oid, 25 | 1042 | 1043 | 705) {
        text.encoded
    } else {
        add(text.encoded, 32)?
    };
    Ok(CellBytes {
        encoded,
        scratch: add(text.scratch, mul(encoded.max(128), 8)?)?,
    })
}

fn array_element_oid(array_oid: i64) -> Option<i64> {
    if (34_000..44_000).contains(&array_oid) {
        return Some(array_oid - 34_000);
    }
    let array_oid = u32::try_from(array_oid).ok()?;
    crate::catalog::builtin_identity::builtin_array_element_oid(array_oid).map(i64::from)
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
        Value::Typed(value) => {
            let encoded = crate::executor::native_result_text_bound(
                value,
                crate::types::extended::temporal::default_session(),
            )?;
            return Ok(CellBytes {
                encoded,
                scratch: mul(encoded.max(128), 4)?,
            });
        }
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
        Value::Array(array) => {
            return native_array_json_bytes(array, None);
        }
    };
    Ok(CellBytes {
        encoded,
        scratch: mul(encoded.max(8), 3)?,
    })
}

fn postgres_array_text_bytes(value: &Value, oid: i64) -> Result<CellBytes, CassieError> {
    let (element_count, json_bytes) = match value {
        Value::Json(array) => (
            array.as_array().map_or(0, Vec::len),
            serialized_json_bytes(array)?,
        ),
        Value::Array(array) => (
            array.len(),
            native_array_json_bytes(array, array_element_oid(oid))?.encoded,
        ),
        _ => return text_bytes(value),
    };
    // Array quoting can expand each element; also admit the temporary String
    // directory used while joining PostgreSQL's text representation.
    let encoded = add(add(mul(json_bytes, 4)?, mul(element_count, 2)?)?, 2)?;
    let scratch = add(
        mul(encoded.max(128), 3)?,
        mul(element_count, size_of::<String>())?,
    )?;
    Ok(CellBytes { encoded, scratch })
}

fn native_array_json_bytes(
    array: &crate::types::SqlArray,
    element_oid: Option<i64>,
) -> Result<CellBytes, CassieError> {
    // Rank is bounded by six. This covers every nested bracket/comma and the
    // escaped JSON representation without cloning or rendering any element.
    let encoded =
        array
            .elements()
            .iter()
            .try_fold(mul(add(array.len(), 1)?, 14)?, |bytes, value| {
                let encoded = match (element_oid, value) {
                    (Some(3802), Value::Json(json)) => {
                        crate::types::jsonb::text_len(json).map_err(CassieError::TypeInput)?
                    }
                    _ => text_bytes(value)?.encoded,
                };
                add(bytes, add(mul(encoded, 6)?, 2)?)
            })?;
    Ok(CellBytes {
        encoded,
        scratch: add(mul(encoded.max(128), 8)?, mul(array.len(), 128)?)?,
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
    if oid == 3802 {
        let encoded = add(
            crate::types::jsonb::text_len(value).map_err(CassieError::TypeInput)?,
            1,
        )?;
        return Ok(CellBytes {
            encoded,
            scratch: add(
                mul(encoded.max(8), 8)?,
                crate::runtime::accounted::json::retained_bytes(value)?,
            )?,
        });
    }
    if oid == 114 || value.is_object() {
        return json_bytes(value);
    }
    match value {
        serde_json::Value::Bool(value) => cell_bytes(&Value::Bool(*value), oid, -1, 1, false),
        serde_json::Value::Number(value) => {
            let value = value
                .as_i64()
                .map(Value::Int64)
                .or_else(|| value.as_f64().map(Value::Float64))
                .unwrap_or(Value::Null);
            cell_bytes(&value, oid, -1, 1, false)
        }
        serde_json::Value::String(text) => {
            let encoded = if matches!(oid, 25 | 1042 | 1043 | 705) {
                text.len()
            } else {
                add(text.len().max(16), 32)?
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
