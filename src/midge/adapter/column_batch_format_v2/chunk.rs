use std::collections::BTreeMap;

use crate::app::CassieError;

use super::binary::{
    invalid, write_bytes, write_f64, write_i64, write_u16, write_u32, write_u64, Reader,
};
use super::bitpack;

pub(super) const MAGIC: &[u8; 4] = b"CBC2";
pub(super) const FORMAT_VERSION: u16 = 2;
const HEADER_LEN: usize = 34;
pub(super) const BLOCK_LEN: usize = 128;
pub(super) const MAX_VALUE_COUNT: usize = 1_048_576;
pub(super) const MAX_CHUNK_BYTES: usize = 64 * 1024 * 1024;
pub(super) const MAX_DECODED_BYTES: usize = 256 * 1024 * 1024;
pub(super) const MAX_SCALAR_BYTES: usize = 16 * 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub(crate) enum LogicalType {
    Int64 = 1,
    Float64 = 2,
    Boolean = 3,
    Utf8 = 4,
    Complex = 5,
}

impl LogicalType {
    pub(crate) fn from_name(name: &str) -> Result<Self, CassieError> {
        match name.to_ascii_lowercase().as_str() {
            "smallint" | "int" | "bigint" => Ok(Self::Int64),
            "float" => Ok(Self::Float64),
            "boolean" => Ok(Self::Boolean),
            "text" | "char" | "varchar" | "uuid" | "date" | "time" | "timestamp" => Ok(Self::Utf8),
            "bytea" | "json" | "array" | "vector" | "null" => Ok(Self::Complex),
            _ if name.starts_with("char(")
                || name.starts_with("varchar(")
                || name.starts_with("vector(")
                || name.ends_with("[]") =>
            {
                if name.starts_with("char(") || name.starts_with("varchar(") {
                    Ok(Self::Utf8)
                } else {
                    Ok(Self::Complex)
                }
            }
            _ => Err(invalid("unsupported column-batch logical type")),
        }
    }

    pub(super) fn from_tag(tag: u8) -> Result<Self, CassieError> {
        match tag {
            1 => Ok(Self::Int64),
            2 => Ok(Self::Float64),
            3 => Ok(Self::Boolean),
            4 => Ok(Self::Utf8),
            5 => Ok(Self::Complex),
            _ => Err(invalid("unknown column-batch logical type")),
        }
    }

    pub(super) fn from_tag_for_manifest(tag: u8) -> Result<Self, CassieError> {
        Self::from_tag(tag)
    }

    pub(crate) const fn name(self) -> &'static str {
        match self {
            Self::Int64 => "int64",
            Self::Float64 => "float64",
            Self::Boolean => "boolean",
            Self::Utf8 => "utf8",
            Self::Complex => "complex",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub(crate) enum Codec {
    Plain = 0,
    Constant = 1,
    Rle = 2,
    Dictionary = 3,
    FrameOfReference = 4,
    Alp = 5,
}

impl Codec {
    pub(super) fn from_tag(tag: u8) -> Result<Self, CassieError> {
        match tag {
            0 => Ok(Self::Plain),
            1 => Ok(Self::Constant),
            2 => Ok(Self::Rle),
            3 => Ok(Self::Dictionary),
            4 => Ok(Self::FrameOfReference),
            5 => Ok(Self::Alp),
            _ => Err(invalid("unknown column-batch codec")),
        }
    }

    pub(super) fn from_tag_for_manifest(tag: u8) -> Result<Self, CassieError> {
        Self::from_tag(tag)
    }

    pub(crate) const fn name(self) -> &'static str {
        match self {
            Self::Plain => "plain",
            Self::Constant => "constant",
            Self::Rle => "rle",
            Self::Dictionary => "dictionary",
            Self::FrameOfReference => "frame_of_reference",
            Self::Alp => "alp",
        }
    }
}

#[derive(Debug)]
pub(crate) struct EncodedChunk {
    pub(crate) bytes: Vec<u8>,
    pub(crate) logical_type: LogicalType,
    pub(crate) codec: Codec,
    pub(crate) value_count: usize,
    pub(crate) null_count: usize,
    pub(crate) decoded_len: usize,
}

#[derive(Debug)]
pub(crate) struct DecodedChunk {
    pub(crate) values: Vec<serde_json::Value>,
    pub(crate) logical_type: LogicalType,
    pub(crate) codec: Codec,
    pub(crate) encoded_len: usize,
    pub(crate) decoded_len: usize,
}

pub(crate) fn encode(
    logical_type: LogicalType,
    values: &[serde_json::Value],
) -> Result<EncodedChunk, CassieError> {
    encode_with_policy(logical_type, values, None)
}

pub(crate) fn encode_for(values: &[serde_json::Value]) -> Result<EncodedChunk, CassieError> {
    encode_with_policy(LogicalType::Int64, values, Some(Codec::FrameOfReference))
}

pub(crate) fn encode_plain_for_test(
    logical_type: LogicalType,
    values: &[serde_json::Value],
) -> Result<EncodedChunk, CassieError> {
    encode_with_policy(logical_type, values, Some(Codec::Plain))
}

fn encode_with_policy(
    logical_type: LogicalType,
    values: &[serde_json::Value],
    forced: Option<Codec>,
) -> Result<EncodedChunk, CassieError> {
    validate_count(values.len())?;
    let validity = validity_bitmap(values);
    let null_count = values.iter().filter(|value| value.is_null()).count();
    let non_null = values
        .iter()
        .filter(|value| !value.is_null())
        .collect::<Vec<_>>();
    for value in &non_null {
        validate_scalar(logical_type, value)?;
    }
    let plain_payload = encode_plain(logical_type, non_null.as_slice())?;
    let decoded_len = plain_payload.len();
    let plain = build_chunk(
        logical_type,
        Codec::Plain,
        values.len(),
        null_count,
        validity.as_slice(),
        plain_payload.as_slice(),
        decoded_len,
    )?;
    if forced == Some(Codec::Plain) {
        return Ok(encoded_chunk(
            plain,
            logical_type,
            Codec::Plain,
            values.len(),
            null_count,
            decoded_len,
        ));
    }

    let candidates = codec_candidates(logical_type, non_null.as_slice())?;

    if let Some(codec) = forced {
        let payload = candidates
            .into_iter()
            .find_map(|(candidate, payload)| (candidate == codec).then_some(payload))
            .ok_or_else(|| invalid("forced column-batch codec is not eligible"))?;
        let bytes = build_chunk(
            logical_type,
            codec,
            values.len(),
            null_count,
            validity.as_slice(),
            payload.as_slice(),
            decoded_len,
        )?;
        return Ok(encoded_chunk(
            bytes,
            logical_type,
            codec,
            values.len(),
            null_count,
            decoded_len,
        ));
    }

    let plain_len = plain.len();
    let mut selected = (Codec::Plain, plain);
    for (codec, payload) in candidates {
        let bytes = build_chunk(
            logical_type,
            codec,
            values.len(),
            null_count,
            validity.as_slice(),
            payload.as_slice(),
            decoded_len,
        )?;
        if bytes.len() < selected.1.len() {
            selected = (codec, bytes);
        }
    }
    let minimum_savings = 32usize.max(plain_len.div_ceil(20));
    if selected.0 != Codec::Plain && plain_len.saturating_sub(selected.1.len()) < minimum_savings {
        let payload = encode_plain(logical_type, non_null.as_slice())?;
        selected = (
            Codec::Plain,
            build_chunk(
                logical_type,
                Codec::Plain,
                values.len(),
                null_count,
                validity.as_slice(),
                payload.as_slice(),
                decoded_len,
            )?,
        );
    }
    Ok(encoded_chunk(
        selected.1,
        logical_type,
        selected.0,
        values.len(),
        null_count,
        decoded_len,
    ))
}

fn codec_candidates(
    logical_type: LogicalType,
    non_null: &[&serde_json::Value],
) -> Result<Vec<(Codec, Vec<u8>)>, CassieError> {
    let mut candidates = Vec::new();
    if constant_eligible(non_null) {
        candidates.push((Codec::Constant, encode_constant(logical_type, non_null)?));
    }
    candidates.push((Codec::Rle, encode_rle(logical_type, non_null)?));
    if matches!(
        logical_type,
        LogicalType::Int64 | LogicalType::Boolean | LogicalType::Utf8
    ) {
        candidates.push((
            Codec::Dictionary,
            encode_dictionary(logical_type, non_null)?,
        ));
    }
    if logical_type == LogicalType::Int64 {
        candidates.push((
            Codec::FrameOfReference,
            encode_frame_of_reference(non_null)?,
        ));
    }
    if logical_type == LogicalType::Float64 {
        if let Ok(payload) = super::alp::encode(non_null) {
            candidates.push((Codec::Alp, payload));
        }
    }
    Ok(candidates)
}

fn encoded_chunk(
    bytes: Vec<u8>,
    logical_type: LogicalType,
    codec: Codec,
    value_count: usize,
    null_count: usize,
    decoded_len: usize,
) -> EncodedChunk {
    EncodedChunk {
        bytes,
        logical_type,
        codec,
        value_count,
        null_count,
        decoded_len,
    }
}

pub(crate) fn decode(bytes: &[u8]) -> Result<DecodedChunk, CassieError> {
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
    let null_count = checked_count(reader.read_u32()?)?;
    if null_count > value_count {
        return Err(invalid("column-batch null count exceeds value count"));
    }
    let validity_len = checked_length(reader.read_u32()?, MAX_CHUNK_BYTES)?;
    let payload_len = checked_length(reader.read_u32()?, MAX_CHUNK_BYTES)?;
    let decoded_len = checked_u64_length(reader.read_u64()?, MAX_DECODED_BYTES)?;
    let expected_validity_len = value_count.div_ceil(8);
    if validity_len != expected_validity_len {
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
    let non_null = match codec {
        Codec::Plain => decode_plain(logical_type, payload, non_null_count)?,
        Codec::Constant => decode_constant(logical_type, payload, non_null_count)?,
        Codec::Rle => decode_rle(logical_type, payload, non_null_count)?,
        Codec::Dictionary => decode_dictionary(logical_type, payload, non_null_count)?,
        Codec::FrameOfReference => {
            if logical_type != LogicalType::Int64 {
                return Err(invalid("frame-of-reference requires integers"));
            }
            decode_frame_of_reference(payload, non_null_count)?
        }
        Codec::Alp => {
            if logical_type != LogicalType::Float64 {
                return Err(invalid("ALP requires floats"));
            }
            super::alp::decode(payload, non_null_count)?
        }
    };
    if non_null.len() != non_null_count {
        return Err(invalid("column-batch decoded count mismatch"));
    }
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
    Ok(DecodedChunk {
        values,
        logical_type,
        codec,
        encoded_len: bytes.len(),
        decoded_len,
    })
}

fn build_chunk(
    logical_type: LogicalType,
    codec: Codec,
    value_count: usize,
    null_count: usize,
    validity: &[u8],
    payload: &[u8],
    decoded_len: usize,
) -> Result<Vec<u8>, CassieError> {
    let capacity = HEADER_LEN
        .checked_add(validity.len())
        .and_then(|length| length.checked_add(payload.len()))
        .ok_or_else(|| invalid("column-batch chunk length overflow"))?;
    if capacity > MAX_CHUNK_BYTES || decoded_len > MAX_DECODED_BYTES {
        return Err(invalid("column-batch chunk exceeds limit"));
    }
    let mut out = Vec::with_capacity(capacity);
    out.extend_from_slice(MAGIC);
    write_u16(&mut out, FORMAT_VERSION);
    out.push(logical_type as u8);
    out.push(codec as u8);
    write_u16(&mut out, 0);
    write_u32(
        &mut out,
        u32::try_from(value_count).map_err(|_| invalid("column-batch value count exceeds u32"))?,
    );
    write_u32(
        &mut out,
        u32::try_from(null_count).map_err(|_| invalid("column-batch null count exceeds u32"))?,
    );
    write_u32(
        &mut out,
        u32::try_from(validity.len())
            .map_err(|_| invalid("column-batch validity length exceeds u32"))?,
    );
    write_u32(
        &mut out,
        u32::try_from(payload.len())
            .map_err(|_| invalid("column-batch payload length exceeds u32"))?,
    );
    write_u64(
        &mut out,
        u64::try_from(decoded_len)
            .map_err(|_| invalid("column-batch decoded length exceeds u64"))?,
    );
    out.extend_from_slice(validity);
    out.extend_from_slice(payload);
    Ok(out)
}

fn encode_plain(
    logical_type: LogicalType,
    values: &[&serde_json::Value],
) -> Result<Vec<u8>, CassieError> {
    let mut out = Vec::new();
    match logical_type {
        LogicalType::Int64 => {
            out.reserve(values.len().saturating_mul(8));
            for value in values {
                write_i64(
                    &mut out,
                    value
                        .as_i64()
                        .ok_or_else(|| invalid("column-batch integer expected"))?,
                );
            }
        }
        LogicalType::Float64 => {
            out.reserve(values.len().saturating_mul(8));
            for value in values {
                write_f64(
                    &mut out,
                    value
                        .as_f64()
                        .ok_or_else(|| invalid("column-batch float expected"))?,
                );
            }
        }
        LogicalType::Boolean => {
            out.resize(values.len().div_ceil(8), 0);
            for (position, value) in values.iter().enumerate() {
                if value
                    .as_bool()
                    .ok_or_else(|| invalid("column-batch boolean expected"))?
                {
                    out[position / 8] |= 1_u8 << (position % 8);
                }
            }
        }
        LogicalType::Utf8 | LogicalType::Complex => {
            for value in values {
                let scalar = scalar_bytes(logical_type, value)?;
                write_bytes(&mut out, scalar.as_slice())?;
            }
        }
    }
    Ok(out)
}

fn decode_plain(
    logical_type: LogicalType,
    payload: &[u8],
    count: usize,
) -> Result<Vec<serde_json::Value>, CassieError> {
    let mut reader = Reader::new(payload);
    let values = match logical_type {
        LogicalType::Int64 => (0..count)
            .map(|_| reader.read_i64().map(serde_json::Value::from))
            .collect::<Result<Vec<_>, _>>()?,
        LogicalType::Float64 => (0..count)
            .map(|_| {
                let value = reader.read_f64()?;
                serde_json::Number::from_f64(value)
                    .map(serde_json::Value::Number)
                    .ok_or_else(|| invalid("invalid column-batch float"))
            })
            .collect::<Result<Vec<_>, _>>()?,
        LogicalType::Boolean => {
            let expected = count.div_ceil(8);
            if payload.len() != expected {
                return Err(invalid("invalid column-batch boolean bitmap"));
            }
            let values = (0..count)
                .map(|position| serde_json::Value::Bool(bit_is_set(payload, position)))
                .collect();
            reader.read_exact(payload.len())?;
            values
        }
        LogicalType::Utf8 | LogicalType::Complex => (0..count)
            .map(|_| {
                let scalar = reader.read_bounded_bytes(MAX_SCALAR_BYTES)?;
                scalar_from_bytes(logical_type, scalar)
            })
            .collect::<Result<Vec<_>, _>>()?,
    };
    reader.finish()?;
    Ok(values)
}

fn encode_constant(
    logical_type: LogicalType,
    values: &[&serde_json::Value],
) -> Result<Vec<u8>, CassieError> {
    values
        .first()
        .map_or_else(|| Ok(Vec::new()), |value| scalar_bytes(logical_type, value))
}

fn decode_constant(
    logical_type: LogicalType,
    payload: &[u8],
    count: usize,
) -> Result<Vec<serde_json::Value>, CassieError> {
    if count == 0 {
        if payload.is_empty() {
            return Ok(Vec::new());
        }
        return Err(invalid("all-null constant chunk has a payload"));
    }
    let value = scalar_from_bytes(logical_type, payload)?;
    Ok(vec![value; count])
}

fn encode_rle(
    logical_type: LogicalType,
    values: &[&serde_json::Value],
) -> Result<Vec<u8>, CassieError> {
    let mut runs: Vec<(usize, Vec<u8>)> = Vec::new();
    for value in values {
        let scalar = scalar_bytes(logical_type, value)?;
        if let Some((length, previous)) = runs.last_mut() {
            if *previous == scalar {
                *length = length
                    .checked_add(1)
                    .ok_or_else(|| invalid("column-batch RLE length overflow"))?;
                continue;
            }
        }
        runs.push((1, scalar));
    }
    let mut out = Vec::new();
    write_u32(
        &mut out,
        u32::try_from(runs.len()).map_err(|_| invalid("too many column-batch RLE runs"))?,
    );
    for (length, scalar) in runs {
        write_u32(
            &mut out,
            u32::try_from(length).map_err(|_| invalid("column-batch RLE length exceeds u32"))?,
        );
        write_bytes(&mut out, scalar.as_slice())?;
    }
    Ok(out)
}

fn decode_rle(
    logical_type: LogicalType,
    payload: &[u8],
    count: usize,
) -> Result<Vec<serde_json::Value>, CassieError> {
    let mut reader = Reader::new(payload);
    let run_count = checked_count(reader.read_u32()?)?;
    if run_count > count && count != 0 {
        return Err(invalid("column-batch RLE run count exceeds values"));
    }
    let mut values = Vec::with_capacity(count);
    for _ in 0..run_count {
        let length = checked_count(reader.read_u32()?)?;
        if length == 0 || values.len().saturating_add(length) > count {
            return Err(invalid("invalid column-batch RLE run length"));
        }
        let scalar = reader.read_bounded_bytes(MAX_SCALAR_BYTES)?;
        let value = scalar_from_bytes(logical_type, scalar)?;
        values.extend(std::iter::repeat_n(value, length));
    }
    reader.finish()?;
    if values.len() != count {
        return Err(invalid("column-batch RLE count mismatch"));
    }
    Ok(values)
}

fn encode_dictionary(
    logical_type: LogicalType,
    values: &[&serde_json::Value],
) -> Result<Vec<u8>, CassieError> {
    let mut dictionary = BTreeMap::<Vec<u8>, u64>::new();
    for value in values {
        dictionary
            .entry(scalar_bytes(logical_type, value)?)
            .or_default();
    }
    for (position, index) in dictionary.values_mut().enumerate() {
        *index = u64::try_from(position).map_err(|_| invalid("dictionary index overflow"))?;
    }
    let mut out = Vec::new();
    write_u32(
        &mut out,
        u32::try_from(dictionary.len())
            .map_err(|_| invalid("column-batch dictionary count exceeds u32"))?,
    );
    for scalar in dictionary.keys() {
        write_bytes(&mut out, scalar.as_slice())?;
    }
    let indices = values
        .iter()
        .map(|value| {
            let scalar = scalar_bytes(logical_type, value)?;
            dictionary
                .get(&scalar)
                .copied()
                .ok_or_else(|| invalid("column-batch dictionary value missing"))
        })
        .collect::<Result<Vec<_>, _>>()?;
    encode_for_blocks(indices.as_slice(), &mut out)?;
    Ok(out)
}

fn decode_dictionary(
    logical_type: LogicalType,
    payload: &[u8],
    count: usize,
) -> Result<Vec<serde_json::Value>, CassieError> {
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
    indices
        .into_iter()
        .map(|index| {
            usize::try_from(index)
                .ok()
                .and_then(|index| dictionary.get(index))
                .cloned()
                .ok_or_else(|| invalid("column-batch dictionary index out of range"))
        })
        .collect()
}

fn encode_frame_of_reference(values: &[&serde_json::Value]) -> Result<Vec<u8>, CassieError> {
    let integers = values
        .iter()
        .map(|value| {
            value
                .as_i64()
                .ok_or_else(|| invalid("column-batch integer expected"))
        })
        .collect::<Result<Vec<_>, _>>()?;
    let mut out = Vec::new();
    write_u32(
        &mut out,
        u32::try_from(integers.len().div_ceil(BLOCK_LEN))
            .map_err(|_| invalid("too many frame-of-reference blocks"))?,
    );
    for block in integers.chunks(BLOCK_LEN) {
        let base = block.iter().copied().min().unwrap_or(0);
        let deltas = block
            .iter()
            .map(|value| {
                u64::try_from(i128::from(*value) - i128::from(base))
                    .map_err(|_| invalid("frame-of-reference delta overflow"))
            })
            .collect::<Result<Vec<_>, _>>()?;
        let width = bitpack::required_bits(deltas.iter().copied().max().unwrap_or(0));
        let packed = bitpack::pack(deltas.as_slice(), width)?;
        out.push(
            u8::try_from(block.len())
                .map_err(|_| invalid("frame-of-reference block count exceeds u8"))?,
        );
        write_i64(&mut out, base);
        out.push(width);
        write_bytes(&mut out, packed.as_slice())?;
    }
    Ok(out)
}

fn decode_frame_of_reference(
    payload: &[u8],
    count: usize,
) -> Result<Vec<serde_json::Value>, CassieError> {
    let mut reader = Reader::new(payload);
    let block_count = checked_count(reader.read_u32()?)?;
    if block_count != count.div_ceil(BLOCK_LEN) {
        return Err(invalid("invalid frame-of-reference block count"));
    }
    let mut values = Vec::with_capacity(count);
    for block_position in 0..block_count {
        let block_len = usize::from(reader.read_u8()?);
        let expected = (count - block_position * BLOCK_LEN).min(BLOCK_LEN);
        if block_len != expected {
            return Err(invalid("invalid frame-of-reference block length"));
        }
        let base = reader.read_i64()?;
        let width = reader.read_u8()?;
        let packed = reader.read_bounded_bytes(MAX_CHUNK_BYTES)?;
        let deltas = bitpack::unpack(packed, block_len, width)?;
        for delta in deltas {
            let value = i128::from(base)
                .checked_add(i128::from(delta))
                .and_then(|value| i64::try_from(value).ok())
                .ok_or_else(|| invalid("frame-of-reference value overflow"))?;
            values.push(serde_json::Value::from(value));
        }
    }
    reader.finish()?;
    Ok(values)
}

fn encode_for_blocks(values: &[u64], out: &mut Vec<u8>) -> Result<(), CassieError> {
    write_u32(
        out,
        u32::try_from(values.len().div_ceil(BLOCK_LEN))
            .map_err(|_| invalid("too many dictionary index blocks"))?,
    );
    for block in values.chunks(BLOCK_LEN) {
        let base = block.iter().copied().min().unwrap_or(0);
        let deltas = block.iter().map(|value| value - base).collect::<Vec<_>>();
        let width = bitpack::required_bits(deltas.iter().copied().max().unwrap_or(0));
        let packed = bitpack::pack(deltas.as_slice(), width)?;
        out.push(
            u8::try_from(block.len())
                .map_err(|_| invalid("dictionary index block count exceeds u8"))?,
        );
        write_u64(out, base);
        out.push(width);
        write_bytes(out, packed.as_slice())?;
    }
    Ok(())
}

pub(super) fn decode_for_blocks(
    reader: &mut Reader<'_>,
    count: usize,
) -> Result<Vec<u64>, CassieError> {
    let block_count = checked_count(reader.read_u32()?)?;
    if block_count != count.div_ceil(BLOCK_LEN) {
        return Err(invalid("invalid dictionary index block count"));
    }
    let mut values = Vec::with_capacity(count);
    for block_position in 0..block_count {
        let block_len = usize::from(reader.read_u8()?);
        let expected = (count - block_position * BLOCK_LEN).min(BLOCK_LEN);
        if block_len != expected {
            return Err(invalid("invalid dictionary index block length"));
        }
        let base = reader.read_u64()?;
        let width = reader.read_u8()?;
        let packed = reader.read_bounded_bytes(MAX_CHUNK_BYTES)?;
        for delta in bitpack::unpack(packed, block_len, width)? {
            values.push(
                base.checked_add(delta)
                    .ok_or_else(|| invalid("dictionary index overflow"))?,
            );
        }
    }
    Ok(values)
}

fn scalar_bytes(
    logical_type: LogicalType,
    value: &serde_json::Value,
) -> Result<Vec<u8>, CassieError> {
    let mut out = Vec::new();
    match logical_type {
        LogicalType::Int64 => write_i64(
            &mut out,
            value
                .as_i64()
                .ok_or_else(|| invalid("column-batch integer expected"))?,
        ),
        LogicalType::Float64 => write_f64(
            &mut out,
            value
                .as_f64()
                .ok_or_else(|| invalid("column-batch float expected"))?,
        ),
        LogicalType::Boolean => out.push(u8::from(
            value
                .as_bool()
                .ok_or_else(|| invalid("column-batch boolean expected"))?,
        )),
        LogicalType::Utf8 => out.extend_from_slice(
            value
                .as_str()
                .ok_or_else(|| invalid("column-batch string expected"))?
                .as_bytes(),
        ),
        LogicalType::Complex => {
            out = serde_json::to_vec(value)
                .map_err(|_| invalid("failed to encode complex column-batch value"))?;
        }
    }
    if out.len() > MAX_SCALAR_BYTES {
        return Err(invalid("column-batch scalar exceeds limit"));
    }
    Ok(out)
}

pub(super) fn scalar_from_bytes(
    logical_type: LogicalType,
    bytes: &[u8],
) -> Result<serde_json::Value, CassieError> {
    if bytes.len() > MAX_SCALAR_BYTES {
        return Err(invalid("column-batch scalar exceeds limit"));
    }
    let mut reader = Reader::new(bytes);
    let value = match logical_type {
        LogicalType::Int64 => serde_json::Value::from(reader.read_i64()?),
        LogicalType::Float64 => {
            let value = reader.read_f64()?;
            serde_json::Number::from_f64(value)
                .map(serde_json::Value::Number)
                .ok_or_else(|| invalid("invalid column-batch float"))?
        }
        LogicalType::Boolean => match reader.read_u8()? {
            0 => serde_json::Value::Bool(false),
            1 => serde_json::Value::Bool(true),
            _ => return Err(invalid("invalid column-batch boolean")),
        },
        LogicalType::Utf8 => serde_json::Value::String(
            std::str::from_utf8(reader.read_exact(reader.remaining())?)
                .map_err(|_| invalid("invalid column-batch UTF-8"))?
                .to_string(),
        ),
        LogicalType::Complex => {
            let raw = reader.read_exact(reader.remaining())?;
            serde_json::from_slice(raw)
                .map_err(|_| invalid("invalid complex column-batch value"))?
        }
    };
    reader.finish()?;
    Ok(value)
}

fn validate_scalar(
    logical_type: LogicalType,
    value: &serde_json::Value,
) -> Result<(), CassieError> {
    scalar_bytes(logical_type, value).map(|_| ())
}

fn constant_eligible(values: &[&serde_json::Value]) -> bool {
    values
        .first()
        .is_none_or(|first| values.iter().all(|value| *value == *first))
}

fn validity_bitmap(values: &[serde_json::Value]) -> Vec<u8> {
    let mut validity = vec![0_u8; values.len().div_ceil(8)];
    for (position, value) in values.iter().enumerate() {
        if !value.is_null() {
            validity[position / 8] |= 1_u8 << (position % 8);
        }
    }
    validity
}

pub(super) fn validate_validity(
    validity: &[u8],
    value_count: usize,
    null_count: usize,
) -> Result<(), CassieError> {
    if !value_count.is_multiple_of(8) {
        let trailing_mask = !((1_u8 << (value_count % 8)) - 1);
        if validity
            .last()
            .is_some_and(|byte| byte & trailing_mask != 0)
        {
            return Err(invalid("non-zero trailing validity bits"));
        }
    }
    let valid_count = validity
        .iter()
        .map(|byte| byte.count_ones() as usize)
        .try_fold(0usize, usize::checked_add)
        .ok_or_else(|| invalid("column-batch validity count overflow"))?;
    if value_count.saturating_sub(valid_count) != null_count {
        return Err(invalid("column-batch null count mismatch"));
    }
    Ok(())
}

pub(super) fn bit_is_set(bitmap: &[u8], position: usize) -> bool {
    bitmap
        .get(position / 8)
        .is_some_and(|byte| byte & (1_u8 << (position % 8)) != 0)
}

fn validate_count(count: usize) -> Result<(), CassieError> {
    if count > MAX_VALUE_COUNT {
        return Err(invalid("column-batch value count exceeds limit"));
    }
    Ok(())
}

pub(super) fn checked_count(count: u32) -> Result<usize, CassieError> {
    let count =
        usize::try_from(count).map_err(|_| invalid("column-batch count conversion overflow"))?;
    validate_count(count)?;
    Ok(count)
}

pub(super) fn checked_length(length: u32, maximum: usize) -> Result<usize, CassieError> {
    let length =
        usize::try_from(length).map_err(|_| invalid("column-batch length conversion overflow"))?;
    if length > maximum {
        return Err(invalid("column-batch length exceeds limit"));
    }
    Ok(length)
}

pub(super) fn checked_u64_length(length: u64, maximum: usize) -> Result<usize, CassieError> {
    let length =
        usize::try_from(length).map_err(|_| invalid("column-batch length conversion overflow"))?;
    if length > maximum {
        return Err(invalid("column-batch length exceeds limit"));
    }
    Ok(length)
}
