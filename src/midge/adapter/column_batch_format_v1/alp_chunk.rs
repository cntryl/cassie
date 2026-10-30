use crate::app::CassieError;

use super::binary::invalid;
use super::chunk::{bit_is_set, parse_chunk, Codec, LogicalType};

pub(crate) struct DecodedAlpChunk {
    pub(crate) values: Vec<Option<i64>>,
    pub(crate) scale: u8,
    pub(crate) encoded_len: usize,
    pub(crate) decoded_len: usize,
}

pub(crate) fn decode_scaled(bytes: &[u8]) -> Result<DecodedAlpChunk, CassieError> {
    let parts = parse_chunk(bytes)?;
    if parts.logical_type != LogicalType::Float64 || parts.codec != Codec::Alp {
        return Err(invalid("ALP float chunk required"));
    }
    let non_null_count = parts.value_count - parts.null_count;
    let (scale, decoded) = super::alp::decode_scaled(parts.payload, non_null_count)?;
    if decoded.len() != non_null_count {
        return Err(invalid("column-batch decoded count mismatch"));
    }
    let mut values = Vec::with_capacity(parts.value_count);
    let mut next = decoded.into_iter();
    for position in 0..parts.value_count {
        if bit_is_set(parts.validity, position) {
            values
                .push(Some(next.next().ok_or_else(|| {
                    invalid("column-batch decoded value missing")
                })?));
        } else {
            values.push(None);
        }
    }
    if next.next().is_some() {
        return Err(invalid("trailing decoded column-batch values"));
    }
    Ok(DecodedAlpChunk {
        values,
        scale,
        encoded_len: bytes.len(),
        decoded_len: parts.decoded_len,
    })
}
