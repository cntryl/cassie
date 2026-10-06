use std::io;

use super::{invalid_data, invalid_value, unsupported_codec};
use crate::types::schema::vector_dimensions_for_oid;
use crate::types::Value;
use crate::vector::text::visit_components;

pub(super) fn encode_vector(value: Value, type_oid: i64) -> io::Result<Vec<u8>> {
    let dimensions =
        vector_dimensions_for_oid(type_oid).ok_or_else(|| unsupported_codec(type_oid))?;
    if let Value::String(text) = &value {
        return encode_text_vector(text, dimensions);
    }
    let length = match &value {
        Value::Vector(vector) => vector.values.len(),
        Value::Json(serde_json::Value::Array(values)) => values.len(),
        _ => return invalid_value("vector"),
    };
    if length != dimensions || dimensions > i16::MAX as usize {
        return Err(invalid_data("vector"));
    }
    let mut encoded = vector_output_buffer(dimensions)?;
    match value {
        Value::Vector(vector) => {
            for component in vector.values {
                if !component.is_finite() {
                    return Err(invalid_data("vector"));
                }
                encoded.extend_from_slice(&component.to_be_bytes());
            }
        }
        Value::Json(serde_json::Value::Array(values)) => {
            for component in values {
                let component = component
                    .as_f64()
                    .and_then(crate::vector::f64_to_finite_f32)
                    .ok_or_else(|| invalid_data("vector"))?;
                encoded.extend_from_slice(&component.to_be_bytes());
            }
        }
        _ => unreachable!("validated vector carrier"),
    }
    Ok(encoded)
}

pub(super) fn decode_vector(parameter: &[u8], type_oid: i64) -> io::Result<Value> {
    if parameter.len() < 4 {
        return Err(invalid_data("vector"));
    }
    let dimensions = usize::from(u16::from_be_bytes([parameter[0], parameter[1]]));
    let reserved = i16::from_be_bytes([parameter[2], parameter[3]]);
    let expected =
        vector_dimensions_for_oid(type_oid).ok_or_else(|| unsupported_codec(type_oid))?;
    if dimensions != expected || reserved != 0 || parameter.len() != 4 + dimensions * 4 {
        return Err(invalid_data("vector"));
    }
    let mut values = Vec::with_capacity(dimensions);
    for bytes in parameter[4..].as_chunks::<4>().0 {
        let value = f32::from_be_bytes(*bytes);
        if !value.is_finite() {
            return Err(invalid_data("vector"));
        }
        values.push(value);
    }
    Ok(Value::Vector(crate::types::Vector::new(values)))
}

// Text Bind retains its original String carrier. Validate the entire known-width
// sequence before allocating a result buffer, then replay the immutable text into
// that bounded buffer. Neither pass builds a JSON array or a temporary vector.
fn encode_text_vector(text: &str, dimensions: usize) -> io::Result<Vec<u8>> {
    visit_components(text, dimensions, |_| {}).map_err(|_| invalid_data("vector"))?;
    let mut encoded = vector_output_buffer(dimensions)?;
    visit_components(text, dimensions, |component| {
        encoded.extend_from_slice(&component.to_be_bytes());
    })
    .map_err(|_| invalid_data("vector"))?;
    Ok(encoded)
}

fn vector_output_buffer(dimensions: usize) -> io::Result<Vec<u8>> {
    let header = i16::try_from(dimensions).map_err(|_| invalid_data("vector"))?;
    let bytes = dimensions
        .checked_mul(4)
        .and_then(|bytes| bytes.checked_add(4))
        .ok_or_else(|| invalid_data("vector"))?;
    let mut encoded = Vec::with_capacity(bytes);
    encoded.extend_from_slice(&header.to_be_bytes());
    encoded.extend_from_slice(&0_i16.to_be_bytes());
    Ok(encoded)
}
