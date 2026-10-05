use std::io::{self, Write};
use std::mem::size_of;

use crate::app::CassieError;
use crate::runtime::accounted::json;
use crate::types::{DataType, Value};

/// Heap retained by cloning an owned SQL value, in addition to its inline slot.
pub(crate) fn value_clone_bytes(value: &Value) -> Result<usize, CassieError> {
    match value {
        Value::String(text) => Ok(text.capacity()),
        Value::Vector(vector) => mul(vector.values.capacity(), size_of::<f32>()),
        Value::Json(value) => Ok(json::retained_bytes(value)? - size_of::<serde_json::Value>()),
        Value::Null | Value::Bool(_) | Value::Int64(_) | Value::Float64(_) => Ok(0),
    }
}

/// ARRAY types own one boxed element type at each nesting level.
pub(crate) fn data_type_clone_bytes(data_type: &DataType) -> Result<usize, CassieError> {
    match data_type {
        DataType::Array(element) => add(size_of::<DataType>(), data_type_clone_bytes(element)?),
        _ => Ok(0),
    }
}

/// Counts the exact existing JSON serializer output without constructing its output buffer.
pub(crate) fn serialized_json_bytes(value: &serde_json::Value) -> Result<usize, CassieError> {
    let mut output = CountingWriter { bytes: 0 };
    serde_json::to_writer(&mut output, value).map_err(|_| overflow())?;
    Ok(output.bytes)
}

struct CountingWriter {
    bytes: usize,
}

impl Write for CountingWriter {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.bytes = self
            .bytes
            .checked_add(bytes.len())
            .ok_or_else(|| io::Error::other("serialized JSON length overflow"))?;
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

pub(crate) fn lookup_bytes(entries: usize, names_bytes: usize) -> Result<usize, CassieError> {
    add(hash_table_bytes::<(String, usize)>(entries)?, names_bytes)
}

/// Buckets, control bytes and the aligned control/header tail of a retained hash table.
pub(crate) fn hash_table_bytes<T>(entries: usize) -> Result<usize, CassieError> {
    if entries == 0 {
        return Ok(0);
    }
    let required = add(mul(entries, 8)?, 6)? / 7;
    let buckets = required
        .max(4)
        .checked_next_power_of_two()
        .ok_or_else(overflow)?;
    add(
        mul(buckets, add(size_of::<T>(), 1)?)?,
        16 * size_of::<usize>(),
    )
}

pub(crate) fn grown_capacity(count: usize, minimum: usize) -> Result<usize, CassieError> {
    if count == 0 {
        return Ok(0);
    }
    count
        .max(minimum)
        .checked_next_power_of_two()
        .ok_or_else(overflow)
}

pub(crate) fn add(left: usize, right: usize) -> Result<usize, CassieError> {
    left.checked_add(right).ok_or_else(overflow)
}

pub(crate) fn mul(left: usize, right: usize) -> Result<usize, CassieError> {
    left.checked_mul(right).ok_or_else(overflow)
}

fn overflow() -> CassieError {
    CassieError::ResourceLimit("retained query state accounting overflow".to_owned())
}
