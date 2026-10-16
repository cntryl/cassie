//! Source-width-aware PostgreSQL integer/BYTEA casts.
use super::{QueryError, ScalarValue};
use crate::types::{DataType, IntegerByteaError, IntegerWidth};

const HEX: &[u8; 16] = b"0123456789abcdef";

const fn byte_width(width: IntegerWidth) -> usize {
    match width {
        IntegerWidth::I16 => 2,
        IntegerWidth::I32 => 4,
        IntegerWidth::I64 => 8,
    }
}

pub(super) fn scratch_bytes(source: &DataType) -> usize {
    IntegerWidth::from_data_type(source).map_or(0, |width| 2 * (2 + 2 * byte_width(width)))
}

pub(super) fn encode(value: &ScalarValue, width: IntegerWidth) -> Result<ScalarValue, QueryError> {
    let number = match value {
        ScalarValue::Null => return Ok(ScalarValue::Null),
        ScalarValue::Int(number) => *number,
        _ => {
            return Err(QueryError::General(
                "integer BYTEA cast requires an integer".into(),
            ))
        }
    };
    if !width.contains(number) {
        return Err(QueryError::General(width.range_message().into()));
    }
    let bytes = number.to_be_bytes();
    let bytes = &bytes[8 - byte_width(width)..];
    let mut output = String::with_capacity(2 + 2 * bytes.len());
    output.push_str("\\x");
    for byte in bytes {
        output.push(char::from(HEX[usize::from(byte >> 4)]));
        output.push(char::from(HEX[usize::from(byte & 15)]));
    }
    Ok(ScalarValue::Str(output))
}

pub(super) fn decode(value: &ScalarValue, width: IntegerWidth) -> Result<ScalarValue, QueryError> {
    let text = match value {
        ScalarValue::Null => return Ok(ScalarValue::Null),
        ScalarValue::Str(text) => text,
        _ => return Err(invalid_bytea()),
    };
    let digits = text.strip_prefix("\\x").ok_or_else(invalid_bytea)?;
    if !digits.len().is_multiple_of(2) {
        return Err(invalid_bytea());
    }
    let byte_count = digits.len() / 2;
    if byte_count > byte_width(width) {
        return Err(QueryError::Cassie(
            IntegerByteaError::new(width, byte_count).into(),
        ));
    }
    // Short inputs are zero-extended, not sign-extended. Signedness is applied
    // only after decoding into the target integer's complete width.
    let mut bytes = [0; 8];
    for (slot, pair) in bytes[8 - byte_count..]
        .iter_mut()
        .zip(digits.as_bytes().as_chunks::<2>().0)
    {
        *slot = (hex_digit(pair[0]).ok_or_else(invalid_bytea)? << 4)
            | hex_digit(pair[1]).ok_or_else(invalid_bytea)?;
    }
    let value = match width {
        IntegerWidth::I16 => i64::from(i16::from_be_bytes([bytes[6], bytes[7]])),
        IntegerWidth::I32 => {
            i64::from(i32::from_be_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]))
        }
        IntegerWidth::I64 => i64::from_be_bytes(bytes),
    };
    Ok(ScalarValue::Int(value))
}

const fn hex_digit(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

fn invalid_bytea() -> QueryError {
    QueryError::General("cannot cast value to BYTEA".into())
}
