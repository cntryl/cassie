//! Wire output and input for named enum values (`enum_out`, `enum_send`,
//! `enum_recv`) and enum arrays (#827).
//!
//! An enum has a user-range type OID, so no builtin codec applies. A column's
//! [`EnumColumn`] identifies it, and both formats carry the UTF-8 label.
use std::io;

use crate::executor::EnumColumn;
use crate::types::{DataType, Value};

use super::{invalid_data, read_i32};

/// The label a result cell stands for: a typed enum value or its rendered text.
fn label_of(value: &Value) -> io::Result<&str> {
    match value {
        Value::Typed(typed) => typed
            .as_enum()
            .map(crate::types::EnumValue::label)
            .ok_or_else(|| invalid_data("enum")),
        Value::String(label) => Ok(label),
        Value::Json(serde_json::Value::String(label)) => Ok(label),
        _ => Err(invalid_data("enum")),
    }
}

/// Encodes one non-NULL enum or enum-array cell in the requested format.
///
/// # Errors
///
/// Returns an error for an unsupported format code or a cell that is not an
/// enum label (or array of labels).
pub(in crate::pgwire::connection) fn encode(
    value: &Value,
    column: EnumColumn,
    format: i16,
) -> io::Result<Vec<u8>> {
    match (format, column.is_array) {
        (0, false) | (1, false) => label_of(value).map(|label| label.as_bytes().to_vec()),
        (0, true) => text_array(value).map(String::into_bytes),
        (1, true) => binary_array(value, column.element_oid),
        _ => Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "unsupported result format",
        )),
    }
}

/// PostgreSQL array text output (`array_out`) over labels.
fn text_array(value: &Value) -> io::Result<String> {
    match value {
        Value::Array(array) => crate::types::array_value::format_text(array, &DataType::Text),
        Value::Json(array) => crate::types::array::format_text_array(array, &DataType::Text),
        _ => None,
    }
    .ok_or_else(|| invalid_data("enum array"))
}

/// PostgreSQL `array_send` with the enum type as the element type.
fn binary_array(value: &Value, element_oid: i64) -> io::Result<Vec<u8>> {
    let element_oid = i32::try_from(element_oid).map_err(|_| invalid_data("enum array"))?;
    let (dimensions, elements): (Vec<(i32, i32)>, Vec<&Value>) = match value {
        Value::Array(array) => (
            array
                .dimensions()
                .iter()
                .map(|dimension| (dimension.length, dimension.lower_bound))
                .collect(),
            array.elements().iter().collect(),
        ),
        _ => return json_binary_array(value, element_oid),
    };
    encode_array(&dimensions, &elements, element_oid)
}

fn json_binary_array(value: &Value, element_oid: i32) -> io::Result<Vec<u8>> {
    let Value::Json(serde_json::Value::Array(values)) = value else {
        return Err(invalid_data("enum array"));
    };
    let dimensions = if values.is_empty() {
        Vec::new()
    } else {
        vec![(
            i32::try_from(values.len()).map_err(|_| invalid_data("enum array"))?,
            1,
        )]
    };
    let elements = values
        .iter()
        .map(|element| {
            if element.is_null() {
                None
            } else {
                Some(element)
            }
        })
        .collect::<Vec<_>>();
    let mut encoded = array_header(&dimensions, elements.iter().any(Option::is_none), element_oid);
    for element in elements {
        match element {
            None => encoded.extend_from_slice(&(-1_i32).to_be_bytes()),
            Some(serde_json::Value::String(label)) => push_element(&mut encoded, label.as_bytes())?,
            Some(_) => return Err(invalid_data("enum array")),
        }
    }
    Ok(encoded)
}

fn encode_array(
    dimensions: &[(i32, i32)],
    elements: &[&Value],
    element_oid: i32,
) -> io::Result<Vec<u8>> {
    let has_null = elements.iter().any(|element| element.is_null());
    let mut encoded = array_header(dimensions, has_null, element_oid);
    for element in elements {
        if element.is_null() {
            encoded.extend_from_slice(&(-1_i32).to_be_bytes());
        } else {
            push_element(&mut encoded, label_of(element)?.as_bytes())?;
        }
    }
    Ok(encoded)
}

/// `ndim`, `has_null`, element OID and one `(length, lower bound)` pair per
/// dimension. PostgreSQL writes an empty array with zero dimensions.
fn array_header(dimensions: &[(i32, i32)], has_null: bool, element_oid: i32) -> Vec<u8> {
    let mut encoded = Vec::new();
    let count = i32::try_from(dimensions.len()).unwrap_or(i32::MAX);
    encoded.extend_from_slice(&count.to_be_bytes());
    encoded.extend_from_slice(&i32::from(has_null).to_be_bytes());
    encoded.extend_from_slice(&element_oid.to_be_bytes());
    for (length, lower_bound) in dimensions {
        encoded.extend_from_slice(&length.to_be_bytes());
        encoded.extend_from_slice(&lower_bound.to_be_bytes());
    }
    encoded
}

fn push_element(encoded: &mut Vec<u8>, bytes: &[u8]) -> io::Result<()> {
    let length = i32::try_from(bytes.len()).map_err(|_| invalid_data("enum array"))?;
    encoded.extend_from_slice(&length.to_be_bytes());
    encoded.extend_from_slice(bytes);
    Ok(())
}

/// Binary `enum_recv` / `array_recv`: the label text of a scalar parameter or
/// an array of label text. The caller resolves labels against the enum type,
/// which owns `22P02` and `55P04`.
///
/// # Errors
///
/// Returns an error for malformed UTF-8, a malformed array header, or an
/// array whose element OID is not `column.element_oid`.
pub(in crate::pgwire::connection) fn decode_labels(
    parameter: &[u8],
    column: EnumColumn,
) -> io::Result<Value> {
    if !column.is_array {
        return std::str::from_utf8(parameter)
            .map(|label| Value::String(label.to_string()))
            .map_err(|_| invalid_data("enum"));
    }
    let mut cursor = 0;
    let ndim = read_i32(parameter, &mut cursor, "enum array")?;
    let has_null = read_i32(parameter, &mut cursor, "enum array")?;
    let element_oid = i64::from(read_i32(parameter, &mut cursor, "enum array")?);
    if !(0..=6).contains(&ndim) || !matches!(has_null, 0 | 1) || element_oid != column.element_oid
    {
        return Err(invalid_data("enum array"));
    }
    if ndim == 0 {
        if has_null != 0 || cursor != parameter.len() {
            return Err(invalid_data("enum array"));
        }
        return Ok(Value::Array(crate::types::SqlArray::empty()));
    }
    let mut shape = Vec::new();
    let mut count = 1_usize;
    for _ in 0..ndim {
        let length = read_i32(parameter, &mut cursor, "enum array")?;
        let lower_bound = read_i32(parameter, &mut cursor, "enum array")?;
        count = count
            .checked_mul(usize::try_from(length).map_err(|_| invalid_data("enum array"))?)
            .ok_or_else(|| invalid_data("enum array"))?;
        shape.push(crate::types::ArrayDimension {
            length,
            lower_bound,
        });
    }
    if parameter.len().saturating_sub(cursor) < count.saturating_mul(4) {
        return Err(invalid_data("enum array"));
    }
    let mut values = Vec::with_capacity(count);
    for _ in 0..count {
        let length = read_i32(parameter, &mut cursor, "enum array")?;
        if length == -1 {
            if has_null == 0 {
                return Err(invalid_data("enum array"));
            }
            values.push(Value::Null);
            continue;
        }
        let end = cursor
            .checked_add(usize::try_from(length).map_err(|_| invalid_data("enum array"))?)
            .ok_or_else(|| invalid_data("enum array"))?;
        let bytes = parameter
            .get(cursor..end)
            .ok_or_else(|| invalid_data("enum array"))?;
        cursor = end;
        let label = std::str::from_utf8(bytes).map_err(|_| invalid_data("enum array"))?;
        values.push(Value::String(label.to_string()));
    }
    if cursor != parameter.len() {
        return Err(invalid_data("enum array"));
    }
    crate::types::SqlArray::new(shape, values)
        .map(Value::Array)
        .map_err(|_| invalid_data("enum array"))
}

#[cfg(test)]
#[path = "enum_value_tests.rs"]
mod tests;
