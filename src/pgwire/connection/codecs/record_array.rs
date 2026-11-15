//! Wire text and binary output for arrays of named composite types
//! (`array_out` / `array_send` over `record_out` / `record_send`) (#1268).
use std::io;

use crate::types::composite::CompositeRef;
use crate::types::extended::temporal::TemporalSession;
use crate::types::{DataType, SqlArray, Value};

fn invalid(detail: impl std::fmt::Display) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, detail.to_string())
}

/// The array a result cell stands for, with every element a typed record. A
/// cell is the array itself or its already rendered text.
fn typed(value: &Value, descriptor: &CompositeRef) -> io::Result<SqlArray> {
    let converted = match value {
        Value::Array(array) => crate::executor::scan::composite_array_records(array, descriptor),
        Value::String(text) => crate::executor::scan::composite_array_from_text(text, descriptor),
        _ => {
            return Err(invalid(
                "composite array column holds a value that is not an array",
            ))
        }
    }
    .map_err(invalid)?;
    match converted {
        Value::Array(array) => Ok(array),
        _ => Err(invalid("composite array conversion did not yield an array")),
    }
}

/// Encodes one non-null composite-array cell in `format_code`.
///
/// # Errors
///
/// Returns an error for a cell that is not an array of `descriptor` records or
/// for an unsupported format code.
pub(in crate::pgwire::connection) fn encode(
    value: &Value,
    descriptor: &CompositeRef,
    format_code: i16,
    session: &TemporalSession,
) -> io::Result<Vec<u8>> {
    match (format_code, value) {
        (0, Value::String(text)) => Ok(text.clone().into_bytes()),
        (0, _) => {
            let array = typed(value, descriptor)?;
            crate::types::array_value::format_text_in(
                &array,
                &DataType::Composite(descriptor.clone()),
                session,
            )
            .map(String::into_bytes)
            .ok_or_else(|| invalid("composite array cannot be rendered as text"))
        }
        (1, _) => binary(&typed(value, descriptor)?, descriptor),
        _ => Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "unsupported result format",
        )),
    }
}

/// PostgreSQL `array_send`: dimension count, null flag, element type OID, one
/// `(length, lower bound)` pair per dimension, then each element as a length
/// and its `record_send` bytes (`-1` for SQL NULL).
fn binary(array: &SqlArray, descriptor: &CompositeRef) -> io::Result<Vec<u8>> {
    let element_oid =
        i32::try_from(descriptor.oid).map_err(|_| invalid("composite type OID exceeds INT4"))?;
    let dimension_count = i32::try_from(array.dimensions().len())
        .map_err(|_| invalid("array has too many dimensions"))?;
    let has_null = array.elements().iter().any(Value::is_null);
    let mut encoded = Vec::new();
    encoded.extend_from_slice(&dimension_count.to_be_bytes());
    encoded.extend_from_slice(&i32::from(has_null).to_be_bytes());
    encoded.extend_from_slice(&element_oid.to_be_bytes());
    for dimension in array.dimensions() {
        encoded.extend_from_slice(&dimension.length.to_be_bytes());
        encoded.extend_from_slice(&dimension.lower_bound.to_be_bytes());
    }
    for element in array.elements() {
        let Some(record) = super::record::as_record(element) else {
            if element.is_null() {
                encoded.extend_from_slice(&(-1_i32).to_be_bytes());
                continue;
            }
            return Err(invalid("composite array element is not a record"));
        };
        let bytes = super::record::binary(record)?;
        let length =
            i32::try_from(bytes.len()).map_err(|_| invalid("record exceeds the wire length"))?;
        encoded.extend_from_slice(&length.to_be_bytes());
        encoded.extend_from_slice(&bytes);
    }
    Ok(encoded)
}
