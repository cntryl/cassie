use crate::app::CassieError;

use super::binary::{invalid, Reader};
use super::chunk::{
    self, bit_is_set, checked_count, checked_length, checked_u64_length, decode_for_blocks,
    scalar_from_bytes, validate_validity, Codec, DecodedChunk, LogicalType, FORMAT_VERSION, MAGIC,
    MAX_CHUNK_BYTES, MAX_DECODED_BYTES, MAX_SCALAR_BYTES,
};

pub(super) fn decode(bytes: &[u8], selection: &[bool]) -> Result<DecodedChunk, CassieError> {
    if bytes.len() > MAX_CHUNK_BYTES {
        return Err(invalid("column-batch chunk exceeds limit"));
    }
    let mut reader = Reader::new(bytes);
    reader.expect_magic(*MAGIC)?;
    if reader.read_u16()? != FORMAT_VERSION {
        return Err(invalid("unsupported column-batch chunk version"));
    }
    let logical_type = LogicalType::from_tag(reader.read_u8()?)?;
    let codec = Codec::from_tag(reader.read_u8()?)?;
    if reader.read_u16()? != 0 {
        return Err(invalid("unsupported column-batch chunk flags"));
    }
    let value_count = checked_count(reader.read_u32()?)?;
    if selection.len() != value_count {
        return Err(invalid("column-batch selection length mismatch"));
    }
    let null_count = checked_count(reader.read_u32()?)?;
    if null_count > value_count {
        return Err(invalid("column-batch null count exceeds value count"));
    }
    let validity_len = checked_length(reader.read_u32()?, MAX_CHUNK_BYTES)?;
    let payload_len = checked_length(reader.read_u32()?, MAX_CHUNK_BYTES)?;
    let decoded_len = checked_u64_length(reader.read_u64()?, MAX_DECODED_BYTES)?;
    if validity_len != value_count.div_ceil(8) {
        return Err(invalid("invalid column-batch validity length"));
    }
    let expected_remaining = validity_len
        .checked_add(payload_len)
        .ok_or_else(|| invalid("column-batch chunk length overflow"))?;
    if reader.remaining() != expected_remaining {
        return Err(invalid("invalid column-batch chunk payload length"));
    }
    let validity = reader.read_exact(validity_len)?;
    validate_validity(validity, value_count, null_count)?;
    let payload = reader.read_exact(payload_len)?;
    reader.finish()?;
    let non_null_count = value_count - null_count;

    if codec == Codec::Fsst {
        let values =
            decode_selected_fsst_values(logical_type, value_count, validity, payload, selection)?;
        return Ok(DecodedChunk {
            values,
            logical_type,
            codec,
            encoded_len: bytes.len(),
            decoded_len,
        });
    }

    if codec != Codec::Dictionary {
        let mut decoded = chunk::decode(bytes)?;
        for (position, value) in decoded.values.iter_mut().enumerate() {
            if !selection[position] {
                *value = serde_json::Value::Null;
            }
        }
        return Ok(decoded);
    }

    let (dictionary, indices) = decode_dictionary_parts(logical_type, payload, non_null_count)?;
    let mut values = Vec::with_capacity(value_count);
    let mut non_null_position = 0usize;
    for (position, selected) in selection.iter().copied().enumerate() {
        if !bit_is_set(validity, position) {
            values.push(serde_json::Value::Null);
            continue;
        }
        let dictionary_position = indices
            .get(non_null_position)
            .and_then(|index| usize::try_from(*index).ok())
            .filter(|index| *index < dictionary.len())
            .ok_or_else(|| invalid("column-batch dictionary index out of range"))?;
        non_null_position = non_null_position
            .checked_add(1)
            .ok_or_else(|| invalid("column-batch selection position overflow"))?;
        if selected {
            values.push(dictionary[dictionary_position].clone());
        } else {
            values.push(serde_json::Value::Null);
        }
    }
    if non_null_position != non_null_count {
        return Err(invalid("column-batch decoded count mismatch"));
    }
    Ok(DecodedChunk {
        values,
        logical_type,
        codec,
        encoded_len: bytes.len(),
        decoded_len,
    })
}

fn decode_selected_fsst_values(
    logical_type: LogicalType,
    value_count: usize,
    validity: &[u8],
    payload: &[u8],
    selection: &[bool],
) -> Result<Vec<serde_json::Value>, CassieError> {
    if logical_type != LogicalType::Utf8 {
        return Err(invalid("FSST requires text"));
    }
    let non_null_selection = selection
        .iter()
        .copied()
        .enumerate()
        .filter_map(|(position, selected)| bit_is_set(validity, position).then_some(selected))
        .collect::<Vec<_>>();
    let non_null =
        super::fsst::decode_selected(payload, non_null_selection.len(), &non_null_selection)?;
    restore_validity(value_count, validity, non_null)
}

fn restore_validity(
    value_count: usize,
    validity: &[u8],
    non_null: Vec<serde_json::Value>,
) -> Result<Vec<serde_json::Value>, CassieError> {
    let mut values = Vec::with_capacity(value_count);
    let mut next = non_null.into_iter();
    for position in 0..value_count {
        if bit_is_set(validity, position) {
            values.push(
                next.next()
                    .ok_or_else(|| invalid("column-batch decoded value missing"))?,
            );
        } else {
            values.push(serde_json::Value::Null);
        }
    }
    if next.next().is_some() {
        return Err(invalid("trailing decoded column-batch values"));
    }
    Ok(values)
}

fn decode_dictionary_parts(
    logical_type: LogicalType,
    payload: &[u8],
    count: usize,
) -> Result<(Vec<serde_json::Value>, Vec<u64>), CassieError> {
    let mut reader = Reader::new(payload);
    let dictionary_count = checked_count(reader.read_u32()?)?;
    if dictionary_count > count && count != 0 {
        return Err(invalid("column-batch dictionary exceeds value count"));
    }
    let mut dictionary = Vec::with_capacity(dictionary_count);
    let mut previous: Option<Vec<u8>> = None;
    for _ in 0..dictionary_count {
        let scalar = reader.read_bounded_bytes(MAX_SCALAR_BYTES)?;
        if previous.as_deref().is_some_and(|value| value >= scalar) {
            return Err(invalid("column-batch dictionary is not strictly sorted"));
        }
        dictionary.push(scalar_from_bytes(logical_type, scalar)?);
        previous = Some(scalar.to_vec());
    }
    let indices = decode_for_blocks(&mut reader, count)?;
    reader.finish()?;
    Ok((dictionary, indices))
}
