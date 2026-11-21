//! Binary array parameter decoding (`array_recv`).
use super::{
    binary_codec_for_oid, binary_to_value, invalid_data, read_i32, temporal_error, timetz_error,
    unsupported_codec, OID_ARRAY_BASE,
};
use crate::types::Value;
use std::io;

pub(super) fn decode_array(parameter: &[u8], expected_element_oid: i64) -> io::Result<Value> {
    let mut cursor = 0;
    let dimensions = read_i32(parameter, &mut cursor, "array")?;
    let has_null = read_i32(parameter, &mut cursor, "array")?;
    let element_oid = i64::from(read_i32(parameter, &mut cursor, "array")?);
    if !(0..=6).contains(&dimensions)
        || !matches!(has_null, 0 | 1)
        || element_oid != expected_element_oid
    {
        return Err(invalid_data("array"));
    }
    binary_codec_for_oid(element_oid)?;
    if dimensions == 0 {
        // The canonical empty array: no dimension header and no elements.
        if has_null != 0 || cursor != parameter.len() {
            return Err(invalid_data("array"));
        }
        return Ok(Value::Array(crate::types::SqlArray::empty()));
    }
    let mut shape =
        Vec::with_capacity(usize::try_from(dimensions).map_err(|_| invalid_data("array"))?);
    let mut element_count = 1_usize;
    for _ in 0..dimensions {
        let length = read_i32(parameter, &mut cursor, "array")?;
        let lower_bound = read_i32(parameter, &mut cursor, "array")?;
        let length_usize = usize::try_from(length).map_err(|_| invalid_data("array"))?;
        crate::types::array_bounds::check_dimension_bounds(lower_bound, i64::from(length))
            .map_err(|_| invalid_data("array"))?;
        element_count = element_count
            .checked_mul(length_usize)
            .ok_or_else(|| invalid_data("array"))?;
        shape.push(crate::types::ArrayDimension {
            length,
            lower_bound,
        });
    }
    let minimum_payload_bytes = element_count
        .checked_mul(std::mem::size_of::<i32>())
        .ok_or_else(|| invalid_data("array"))?;
    if parameter.len().saturating_sub(cursor) < minimum_payload_bytes {
        return Err(invalid_data("array"));
    }
    let mut values = Vec::with_capacity(element_count);
    let mut sql_null_frame = false;
    let mut document_null_element = false;
    for index in 0..element_count {
        let element_length = read_i32(parameter, &mut cursor, "array")?;
        if element_length == -1 {
            sql_null_frame = true;
            values.push(Value::Null);
            continue;
        }
        let element_length = usize::try_from(element_length).map_err(|_| invalid_data("array"))?;
        let end = cursor
            .checked_add(element_length)
            .ok_or_else(|| invalid_data("array"))?;
        let bytes = parameter
            .get(cursor..end)
            .ok_or_else(|| invalid_data("array"))?;
        cursor = end;
        let value = binary_to_value(bytes, element_oid).map_err(|error| {
            temporal_error::array_error(timetz_error::array_error(error, index), index)
        })?;
        if matches!(value, Value::Json(ref document) if document.is_null()) {
            document_null_element = true;
        }
        values.push(value);
    }
    if cursor != parameter.len() || (has_null == 0 && sql_null_frame) {
        return Err(invalid_data("array"));
    }
    if document_null_element {
        return Err(unsupported_codec(OID_ARRAY_BASE + element_oid));
    }
    let array = crate::types::SqlArray::new(shape, values).map_err(|_| invalid_data("array"))?;
    Ok(Value::Array(array))
}

#[cfg(test)]
mod tests {
    use super::decode_array;
    use crate::types::Value;

    const INT4_OID: i64 = 23;

    fn one_dimensional_int4(length: i32, lower_bound: i32, elements: &[i32]) -> Vec<u8> {
        let mut bytes = Vec::new();
        for field in [1, 0, 23, length, lower_bound] {
            bytes.extend_from_slice(&field.to_be_bytes());
        }
        for element in elements {
            bytes.extend_from_slice(&4_i32.to_be_bytes());
            bytes.extend_from_slice(&element.to_be_bytes());
        }
        bytes
    }

    #[test]
    fn should_reject_binary_array_whose_upper_bound_is_int_max() {
        // Arrange
        let parameter = one_dimensional_int4(1, i32::MAX, &[7]);

        // Act
        let result = decode_array(&parameter, INT4_OID);

        // Assert
        assert!(result.is_err());
    }

    #[test]
    fn should_reject_binary_array_whose_upper_bound_overflows() {
        // Arrange
        let parameter = one_dimensional_int4(2, i32::MAX, &[7, 8]);

        // Act
        let result = decode_array(&parameter, INT4_OID);

        // Assert
        assert!(result.is_err());
    }

    #[test]
    fn should_decode_binary_array_with_custom_lower_bound() {
        // Arrange
        let parameter = one_dimensional_int4(2, 5, &[7, 8]);

        // Act
        let decoded = decode_array(&parameter, INT4_OID).ok();

        // Assert
        let Some(Value::Array(array)) = decoded else {
            panic!("binary array should decode");
        };
        assert_eq!(array.dimensions()[0].lower_bound, 5);
        assert_eq!(array.dimensions()[0].length, 2);
    }
}
