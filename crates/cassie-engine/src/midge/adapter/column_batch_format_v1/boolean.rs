//! Primitive Plain CBC1 Boolean decode; no JSON or SQL Value lane objects.
use super::binary::invalid;
use super::chunk::{bit_is_set, parse_chunk, Codec, LogicalType};
use crate::app::CassieError;

pub(crate) struct DecodedBooleanChunk {
    pub(crate) values: Vec<Option<bool>>,
    pub(crate) logical_type: LogicalType,
    pub(crate) codec: Codec,
    pub(crate) encoded_len: usize,
    pub(crate) decoded_len: usize,
    pub(crate) null_count: usize,
}

pub(crate) fn decode(
    bytes: &[u8],
    expected_rows: usize,
) -> Result<DecodedBooleanChunk, CassieError> {
    let parts = parse_chunk(bytes)?;
    if parts.value_count != expected_rows {
        return Err(invalid("Boolean column-batch domain mismatch"));
    }
    if parts.logical_type != LogicalType::Boolean || parts.codec != Codec::Plain {
        return Err(invalid("Plain Boolean column-batch required"));
    }
    let nonnull = parts.value_count - parts.null_count;
    if parts.payload.len() != nonnull.div_ceil(8) {
        return Err(invalid("invalid column-batch boolean bitmap"));
    }
    let mut values = Vec::with_capacity(expected_rows);
    let mut position = 0;
    for lane in 0..expected_rows {
        values.push(if bit_is_set(parts.validity, lane) {
            let value = bit_is_set(parts.payload, position);
            position += 1;
            Some(value)
        } else {
            None
        });
    }
    Ok(DecodedBooleanChunk {
        values,
        logical_type: parts.logical_type,
        codec: parts.codec,
        encoded_len: bytes.len(),
        decoded_len: parts.decoded_len,
        null_count: parts.null_count,
    })
}
