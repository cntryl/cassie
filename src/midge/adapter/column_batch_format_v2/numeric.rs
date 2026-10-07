//! Primitive CBC2 numeric decode. Framing and validity share the existing parser.
use super::binary::{invalid, Reader};
use super::chunk::{
    bit_is_set, checked_count, decode_for_blocks, parse_chunk, Codec, LogicalType, BLOCK_LEN,
    MAX_CHUNK_BYTES, MAX_SCALAR_BYTES,
};
use crate::app::CassieError;

#[derive(Debug)]
pub(crate) enum NumericValues {
    Integer(Vec<Option<i64>>),
    Float(Vec<Option<f64>>),
}
impl NumericValues {
    pub(crate) fn len(&self) -> usize {
        match self {
            Self::Integer(values) => values.len(),
            Self::Float(values) => values.len(),
        }
    }
}
pub(crate) struct DecodedNumericChunk {
    pub(crate) values: NumericValues,
    pub(crate) logical_type: LogicalType,
    pub(crate) codec: Codec,
    pub(crate) encoded_len: usize,
    pub(crate) decoded_len: usize,
    pub(crate) null_count: usize,
}
#[derive(Clone, Copy)]
enum Scalar {
    Integer(i64),
    Float(f64),
}

pub(crate) fn decode(
    bytes: &[u8],
    expected_rows: usize,
) -> Result<DecodedNumericChunk, CassieError> {
    let parts = parse_chunk(bytes)?;
    if parts.value_count != expected_rows {
        return Err(invalid("numeric column-batch domain mismatch"));
    }
    if !matches!(
        parts.logical_type,
        LogicalType::Int64 | LogicalType::Float64
    ) {
        return Err(invalid("numeric column-batch type required"));
    }
    let count = parts.value_count - parts.null_count;
    let nonnull = match parts.codec {
        Codec::Plain => plain(parts.logical_type, parts.payload, count)?,
        Codec::Constant => {
            if count == 0 {
                if !parts.payload.is_empty() {
                    return Err(invalid("all-null constant chunk has a payload"));
                }
                Vec::new()
            } else {
                vec![scalar(parts.logical_type, parts.payload)?; count]
            }
        }
        Codec::Rle => rle(parts.logical_type, parts.payload, count)?,
        Codec::Dictionary => dictionary(parts.logical_type, parts.payload, count)?,
        Codec::FrameOfReference if parts.logical_type == LogicalType::Int64 => {
            frame_of_reference(parts.payload, count)?
        }
        Codec::Alp if parts.logical_type == LogicalType::Float64 => {
            super::alp::decode_f64(parts.payload, count)?
                .into_iter()
                .map(Scalar::Float)
                .collect()
        }
        _ => return Err(invalid("unsupported numeric column-batch codec")),
    };
    if nonnull.len() != count {
        return Err(invalid("numeric column-batch decoded count mismatch"));
    }
    let mut input = nonnull.into_iter();
    let values = match parts.logical_type {
        LogicalType::Int64 => {
            let mut values = Vec::with_capacity(expected_rows);
            for lane in 0..expected_rows {
                values.push(if bit_is_set(parts.validity, lane) {
                    match input.next() {
                        Some(Scalar::Integer(value)) => Some(value),
                        _ => return Err(invalid("numeric integer lane missing")),
                    }
                } else {
                    None
                });
            }
            NumericValues::Integer(values)
        }
        LogicalType::Float64 => {
            let mut values = Vec::with_capacity(expected_rows);
            for lane in 0..expected_rows {
                values.push(if bit_is_set(parts.validity, lane) {
                    match input.next() {
                        Some(Scalar::Float(value)) if value.is_finite() => Some(value),
                        _ => return Err(invalid("invalid numeric FLOAT lane")),
                    }
                } else {
                    None
                });
            }
            NumericValues::Float(values)
        }
        _ => return Err(invalid("numeric type required")),
    };
    if input.next().is_some() {
        return Err(invalid("trailing numeric decoded lanes"));
    }
    Ok(DecodedNumericChunk {
        values,
        logical_type: parts.logical_type,
        codec: parts.codec,
        encoded_len: bytes.len(),
        decoded_len: parts.decoded_len,
        null_count: parts.null_count,
    })
}
fn read_scalar(logical_type: LogicalType, reader: &mut Reader<'_>) -> Result<Scalar, CassieError> {
    match logical_type {
        LogicalType::Int64 => reader.read_i64().map(Scalar::Integer),
        LogicalType::Float64 => {
            let value = reader.read_f64()?;
            if !value.is_finite() {
                return Err(invalid("invalid column-batch float"));
            }
            Ok(Scalar::Float(value))
        }
        _ => Err(invalid("numeric scalar required")),
    }
}
fn scalar(logical_type: LogicalType, payload: &[u8]) -> Result<Scalar, CassieError> {
    let mut reader = Reader::new(payload);
    let value = read_scalar(logical_type, &mut reader)?;
    reader.finish()?;
    Ok(value)
}
fn plain(
    logical_type: LogicalType,
    payload: &[u8],
    count: usize,
) -> Result<Vec<Scalar>, CassieError> {
    let mut reader = Reader::new(payload);
    let mut values = Vec::with_capacity(count);
    for _ in 0..count {
        values.push(read_scalar(logical_type, &mut reader)?);
    }
    reader.finish()?;
    Ok(values)
}
fn rle(
    logical_type: LogicalType,
    payload: &[u8],
    count: usize,
) -> Result<Vec<Scalar>, CassieError> {
    let mut reader = Reader::new(payload);
    let runs = checked_count(reader.read_u32()?)?;
    if runs > count {
        return Err(invalid("column-batch RLE run count exceeds values"));
    }
    let mut values = Vec::with_capacity(count);
    for _ in 0..runs {
        let length = checked_count(reader.read_u32()?)?;
        if length == 0 || values.len().saturating_add(length) > count {
            return Err(invalid("invalid column-batch RLE run length"));
        }
        let value = scalar(logical_type, reader.read_bounded_bytes(MAX_SCALAR_BYTES)?)?;
        values.extend(std::iter::repeat_n(value, length));
    }
    reader.finish()?;
    if values.len() != count {
        return Err(invalid("column-batch RLE count mismatch"));
    }
    Ok(values)
}
fn dictionary(
    logical_type: LogicalType,
    payload: &[u8],
    count: usize,
) -> Result<Vec<Scalar>, CassieError> {
    let mut reader = Reader::new(payload);
    let entries = checked_count(reader.read_u32()?)?;
    if entries > count {
        return Err(invalid("column-batch dictionary exceeds value count"));
    }
    let mut dictionary = Vec::with_capacity(entries);
    let mut previous: Option<Vec<u8>> = None;
    for _ in 0..entries {
        let bytes = reader.read_bounded_bytes(MAX_SCALAR_BYTES)?;
        if previous
            .as_deref()
            .is_some_and(|previous| previous >= bytes)
        {
            return Err(invalid("column-batch dictionary is not strictly sorted"));
        }
        dictionary.push(scalar(logical_type, bytes)?);
        previous = Some(bytes.to_vec());
    }
    let indices = decode_for_blocks(&mut reader, count)?;
    reader.finish()?;
    let mut values = Vec::with_capacity(count);
    for index in indices {
        let value = usize::try_from(index)
            .ok()
            .and_then(|index| dictionary.get(index))
            .copied()
            .ok_or_else(|| invalid("column-batch dictionary index out of range"))?;
        values.push(value);
    }
    Ok(values)
}
fn frame_of_reference(payload: &[u8], count: usize) -> Result<Vec<Scalar>, CassieError> {
    let mut reader = Reader::new(payload);
    let blocks = checked_count(reader.read_u32()?)?;
    if blocks != count.div_ceil(BLOCK_LEN) {
        return Err(invalid("invalid frame-of-reference block count"));
    }
    let mut values = Vec::with_capacity(count);
    for block in 0..blocks {
        let length = usize::from(reader.read_u8()?);
        if length != (count - block * BLOCK_LEN).min(BLOCK_LEN) {
            return Err(invalid("invalid frame-of-reference block length"));
        }
        let base = reader.read_i64()?;
        let width = reader.read_u8()?;
        let deltas =
            super::bitpack::unpack(reader.read_bounded_bytes(MAX_CHUNK_BYTES)?, length, width)?;
        for delta in deltas {
            let value = i128::from(base)
                .checked_add(i128::from(delta))
                .and_then(|value| i64::try_from(value).ok())
                .ok_or_else(|| invalid("frame-of-reference value overflow"))?;
            values.push(Scalar::Integer(value));
        }
    }
    reader.finish()?;
    Ok(values)
}
