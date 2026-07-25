use std::collections::BTreeMap;

use sha2::{Digest, Sha256};

use crate::app::CassieError;
use crate::catalog::{
    ColumnBatchChunkMeta, ColumnBatchFieldSummary, ColumnBatchMetadata, ColumnBatchNumericSum,
    ColumnBatchSegmentMeta,
};
use crate::types::{Value, Vector};

use super::binary::{
    invalid, write_bytes, write_f64, write_i128, write_i64, write_string, write_u16, write_u32,
    write_u64, Reader,
};
use super::chunk::{Codec, LogicalType};

const MAGIC: &[u8; 4] = b"CBM2";
pub(crate) const FORMAT_VERSION: u16 = 2;
pub(crate) const SUMMARY_VERSION: u16 = 2;
const CHECKSUM_LEN: usize = 32;
const MAX_MANIFEST_BYTES: usize = 64 * 1024 * 1024;
const MAX_STRING_BYTES: usize = 1024 * 1024;
const MAX_FIELDS: usize = 16_384;
const MAX_SEGMENTS: usize = 1_048_576;
const MAX_VECTOR_VALUES: usize = 1_048_576;
const MAX_JSON_BYTES: usize = 16 * 1024 * 1024;

pub(crate) fn encode(metadata: &ColumnBatchMetadata) -> Result<Vec<u8>, CassieError> {
    if metadata.metadata_format_version != u32::from(FORMAT_VERSION)
        || metadata.summary_format_version != u32::from(SUMMARY_VERSION)
    {
        return Err(invalid("unsupported column-batch manifest version"));
    }
    if metadata.fields.len() > MAX_FIELDS || metadata.segments.len() > MAX_SEGMENTS {
        return Err(invalid("column-batch manifest count exceeds limit"));
    }
    let mut out = Vec::new();
    out.extend_from_slice(MAGIC);
    write_u16(&mut out, FORMAT_VERSION);
    write_u16(&mut out, SUMMARY_VERSION);
    write_u64(&mut out, metadata.manifest_revision);
    write_u64(&mut out, metadata.next_segment_id);
    write_u32(&mut out, metadata.schema_version);
    write_u64(&mut out, metadata.built_generation);
    write_u64(
        &mut out,
        u64::try_from(metadata.source_row_count)
            .map_err(|_| invalid("column-batch source count exceeds u64"))?,
    );
    write_u32(
        &mut out,
        u32::try_from(metadata.segment_size)
            .map_err(|_| invalid("column-batch segment size exceeds u32"))?,
    );
    write_string(&mut out, &metadata.collection)?;
    write_string(&mut out, &metadata.index_name)?;
    write_u32(
        &mut out,
        u32::try_from(metadata.fields.len())
            .map_err(|_| invalid("column-batch field count exceeds u32"))?,
    );
    for field in &metadata.fields {
        write_string(&mut out, field)?;
    }
    write_u32(
        &mut out,
        u32::try_from(metadata.segments.len())
            .map_err(|_| invalid("column-batch segment count exceeds u32"))?,
    );
    for segment in &metadata.segments {
        encode_segment(&mut out, segment)?;
    }
    if out.len().saturating_add(CHECKSUM_LEN) > MAX_MANIFEST_BYTES {
        return Err(invalid("column-batch manifest exceeds limit"));
    }
    let checksum = Sha256::digest(out.as_slice());
    out.extend_from_slice(checksum.as_slice());
    Ok(out)
}

pub(crate) fn decode(bytes: &[u8]) -> Result<ColumnBatchMetadata, CassieError> {
    if bytes.len() < CHECKSUM_LEN || bytes.len() > MAX_MANIFEST_BYTES {
        return Err(invalid("invalid column-batch manifest length"));
    }
    let (payload, stored_checksum) = bytes.split_at(bytes.len() - CHECKSUM_LEN);
    if Sha256::digest(payload).as_slice() != stored_checksum {
        return Err(invalid("column-batch manifest checksum mismatch"));
    }
    let mut reader = Reader::new(payload);
    reader.expect_magic(*MAGIC)?;
    if reader.read_u16()? != FORMAT_VERSION {
        return Err(invalid("unsupported column-batch manifest version"));
    }
    if reader.read_u16()? != SUMMARY_VERSION {
        return Err(invalid("unsupported column-batch summary version"));
    }
    let manifest_revision = reader.read_u64()?;
    let next_segment_id = reader.read_u64()?;
    let schema_version = reader.read_u32()?;
    let built_generation = reader.read_u64()?;
    let source_row_count = read_usize_u64(&mut reader, "source row count")?;
    let segment_size = read_usize_u32(&mut reader, "segment size")?;
    if segment_size == 0 {
        return Err(invalid("column-batch segment size must be positive"));
    }
    let collection = reader.read_bounded_string(MAX_STRING_BYTES)?;
    let index_name = reader.read_bounded_string(MAX_STRING_BYTES)?;
    let field_count = read_count(&mut reader, MAX_FIELDS, 4)?;
    let mut fields = Vec::with_capacity(field_count);
    for _ in 0..field_count {
        fields.push(reader.read_bounded_string(MAX_STRING_BYTES)?);
    }
    let segment_count = read_count(&mut reader, MAX_SEGMENTS, 96)?;
    let mut segments = Vec::with_capacity(segment_count);
    for _ in 0..segment_count {
        segments.push(decode_segment(&mut reader, field_count)?);
    }
    reader.finish()?;
    Ok(ColumnBatchMetadata {
        metadata_format_version: u32::from(FORMAT_VERSION),
        summary_format_version: u32::from(SUMMARY_VERSION),
        manifest_revision,
        next_segment_id,
        collection,
        index_name,
        schema_version,
        built_generation,
        source_row_count,
        fields,
        segment_size,
        segments,
    })
}

pub(crate) fn has_current_header(bytes: &[u8]) -> bool {
    bytes.get(..4) == Some(MAGIC.as_slice())
        && bytes
            .get(4..6)
            .is_some_and(|raw| raw == FORMAT_VERSION.to_le_bytes())
}

pub(crate) fn checksum_hex(bytes: &[u8]) -> String {
    hex(Sha256::digest(bytes).as_slice())
}

pub(crate) fn summary_checksum(
    row_count: usize,
    summaries: &BTreeMap<String, ColumnBatchFieldSummary>,
) -> Result<String, CassieError> {
    let mut out = Vec::new();
    write_u16(&mut out, SUMMARY_VERSION);
    write_u64(
        &mut out,
        u64::try_from(row_count).map_err(|_| invalid("column-batch row count exceeds u64"))?,
    );
    encode_summaries(&mut out, summaries)?;
    Ok(checksum_hex(out.as_slice()))
}

fn encode_segment(out: &mut Vec<u8>, segment: &ColumnBatchSegmentMeta) -> Result<(), CassieError> {
    write_u64(out, segment.segment_id);
    write_u64(out, segment.revision);
    write_optional_string(out, segment.row_id_start.as_deref())?;
    write_optional_string(out, segment.row_id_end.as_deref())?;
    write_u64(
        out,
        u64::try_from(segment.row_count)
            .map_err(|_| invalid("column-batch segment row count exceeds u64"))?,
    );
    out.push(u8::from(segment.null_bitmap_available));
    write_u32(out, segment.encoding_version);
    encode_chunk_meta(out, &segment.row_ids)?;
    write_u32(
        out,
        u32::try_from(segment.field_chunks.len())
            .map_err(|_| invalid("column-batch field chunk count exceeds u32"))?,
    );
    for (field, chunk) in &segment.field_chunks {
        write_string(out, field)?;
        encode_chunk_meta(out, chunk)?;
    }
    write_checksum(out, &segment.summary_checksum)?;
    encode_summaries(out, &segment.summaries)
}

fn decode_segment(
    reader: &mut Reader<'_>,
    manifest_field_count: usize,
) -> Result<ColumnBatchSegmentMeta, CassieError> {
    let segment_id = reader.read_u64()?;
    let revision = reader.read_u64()?;
    let row_id_start = read_optional_string(reader)?;
    let row_id_end = read_optional_string(reader)?;
    let row_count = read_usize_u64(reader, "segment row count")?;
    let null_bitmap_available = match reader.read_u8()? {
        0 => false,
        1 => true,
        _ => return Err(invalid("invalid column-batch bitmap flag")),
    };
    let encoding_version = reader.read_u32()?;
    let row_ids = decode_chunk_meta(reader)?;
    let chunk_count = read_count(reader, MAX_FIELDS, 4)?;
    if chunk_count != manifest_field_count {
        return Err(invalid("column-batch field chunk count mismatch"));
    }
    let mut field_chunks = BTreeMap::new();
    for _ in 0..chunk_count {
        let field = reader.read_bounded_string(MAX_STRING_BYTES)?;
        let chunk = decode_chunk_meta(reader)?;
        if field_chunks.insert(field, chunk).is_some() {
            return Err(invalid("duplicate column-batch field chunk"));
        }
    }
    let summary_checksum = read_checksum(reader)?;
    let summaries = decode_summaries(reader, manifest_field_count)?;
    Ok(ColumnBatchSegmentMeta {
        segment_id,
        revision,
        row_id_start,
        row_id_end,
        row_count,
        null_bitmap_available,
        encoding_version,
        row_ids,
        field_chunks,
        summary_checksum,
        summaries,
    })
}

fn encode_chunk_meta(out: &mut Vec<u8>, chunk: &ColumnBatchChunkMeta) -> Result<(), CassieError> {
    let logical_type = logical_type_tag(&chunk.logical_type)?;
    let codec = codec_from_meta(chunk.codec_id, &chunk.codec_name)?;
    out.push(logical_type);
    out.push(codec as u8);
    write_u32(out, chunk.codec_version);
    write_u64(
        out,
        u64::try_from(chunk.decoded_len)
            .map_err(|_| invalid("column-batch decoded length exceeds u64"))?,
    );
    write_u64(
        out,
        u64::try_from(chunk.encoded_len)
            .map_err(|_| invalid("column-batch encoded length exceeds u64"))?,
    );
    write_u64(
        out,
        u64::try_from(chunk.value_count)
            .map_err(|_| invalid("column-batch value count exceeds u64"))?,
    );
    write_u64(
        out,
        u64::try_from(chunk.null_count)
            .map_err(|_| invalid("column-batch null count exceeds u64"))?,
    );
    write_checksum(out, &chunk.checksum_sha256)
}

fn decode_chunk_meta(reader: &mut Reader<'_>) -> Result<ColumnBatchChunkMeta, CassieError> {
    let logical_type = logical_type_name(reader.read_u8()?)?;
    let codec = Codec::from_tag_for_manifest(reader.read_u8()?)?;
    let codec_version = reader.read_u32()?;
    let decoded_len = read_usize_u64(reader, "decoded length")?;
    let encoded_len = read_usize_u64(reader, "encoded length")?;
    let value_count = read_usize_u64(reader, "value count")?;
    let null_count = read_usize_u64(reader, "null count")?;
    if null_count > value_count {
        return Err(invalid("column-batch null count exceeds value count"));
    }
    let checksum_sha256 = read_checksum(reader)?;
    Ok(ColumnBatchChunkMeta {
        logical_type: logical_type.to_string(),
        codec_id: codec as u8,
        codec_name: codec.name().to_string(),
        codec_version,
        decoded_len,
        encoded_len,
        value_count,
        null_count,
        checksum_sha256,
    })
}

fn encode_summaries(
    out: &mut Vec<u8>,
    summaries: &BTreeMap<String, ColumnBatchFieldSummary>,
) -> Result<(), CassieError> {
    if summaries.len() > MAX_FIELDS {
        return Err(invalid("column-batch summary count exceeds limit"));
    }
    write_u32(
        out,
        u32::try_from(summaries.len())
            .map_err(|_| invalid("column-batch summary count exceeds u32"))?,
    );
    for (field, summary) in summaries {
        write_string(out, field)?;
        write_u64(
            out,
            u64::try_from(summary.non_null_count)
                .map_err(|_| invalid("column-batch summary count exceeds u64"))?,
        );
        write_u64(
            out,
            u64::try_from(summary.numeric_count)
                .map_err(|_| invalid("column-batch numeric count exceeds u64"))?,
        );
        write_optional_value(out, summary.min.as_ref())?;
        write_optional_value(out, summary.max.as_ref())?;
        encode_numeric_sum(out, &summary.sum);
        write_optional_i128(out, summary.integer_total);
        write_optional_i128(out, summary.integer_prefix_min);
        write_optional_i128(out, summary.integer_prefix_max);
        write_optional_f64(out, summary.avg_sum);
        write_optional_usize(out, summary.distinct_hint)?;
    }
    Ok(())
}

fn decode_summaries(
    reader: &mut Reader<'_>,
    manifest_field_count: usize,
) -> Result<BTreeMap<String, ColumnBatchFieldSummary>, CassieError> {
    let count = read_count(reader, MAX_FIELDS, 32)?;
    if count != manifest_field_count {
        return Err(invalid("column-batch summary count mismatch"));
    }
    let mut summaries = BTreeMap::new();
    for _ in 0..count {
        let field = reader.read_bounded_string(MAX_STRING_BYTES)?;
        let summary = ColumnBatchFieldSummary {
            non_null_count: read_usize_u64(reader, "non-null count")?,
            numeric_count: read_usize_u64(reader, "numeric count")?,
            min: read_optional_value(reader)?,
            max: read_optional_value(reader)?,
            sum: decode_numeric_sum(reader)?,
            integer_total: read_optional_i128(reader)?,
            integer_prefix_min: read_optional_i128(reader)?,
            integer_prefix_max: read_optional_i128(reader)?,
            avg_sum: read_optional_f64(reader)?,
            distinct_hint: read_optional_usize(reader)?,
        };
        if summaries.insert(field, summary).is_some() {
            return Err(invalid("duplicate column-batch summary"));
        }
    }
    Ok(summaries)
}

fn write_optional_value(out: &mut Vec<u8>, value: Option<&Value>) -> Result<(), CassieError> {
    match value {
        None => out.push(0),
        Some(value) => {
            out.push(1);
            encode_value(out, value)?;
        }
    }
    Ok(())
}

fn read_optional_value(reader: &mut Reader<'_>) -> Result<Option<Value>, CassieError> {
    match reader.read_u8()? {
        0 => Ok(None),
        1 => decode_value(reader).map(Some),
        _ => Err(invalid("invalid optional column-batch value")),
    }
}

fn encode_value(out: &mut Vec<u8>, value: &Value) -> Result<(), CassieError> {
    match value {
        Value::Null => out.push(0),
        Value::Bool(value) => {
            out.push(1);
            out.push(u8::from(*value));
        }
        Value::Int64(value) => {
            out.push(2);
            write_i64(out, *value);
        }
        Value::Float64(value) => {
            out.push(3);
            write_f64(out, *value);
        }
        Value::String(value) => {
            out.push(4);
            write_string(out, value)?;
        }
        Value::Vector(vector) => {
            out.push(5);
            write_u32(
                out,
                u32::try_from(vector.values.len())
                    .map_err(|_| invalid("column-batch vector length exceeds u32"))?,
            );
            for value in &vector.values {
                write_u32(out, value.to_bits());
            }
        }
        Value::Json(value) => {
            out.push(6);
            let value = canonical_json(value);
            let raw = serde_json::to_vec(&value)
                .map_err(|_| invalid("failed to encode column-batch JSON summary"))?;
            if raw.len() > MAX_JSON_BYTES {
                return Err(invalid("column-batch JSON summary exceeds limit"));
            }
            write_bytes(out, raw.as_slice())?;
        }
    }
    Ok(())
}

fn decode_value(reader: &mut Reader<'_>) -> Result<Value, CassieError> {
    match reader.read_u8()? {
        0 => Ok(Value::Null),
        1 => match reader.read_u8()? {
            0 => Ok(Value::Bool(false)),
            1 => Ok(Value::Bool(true)),
            _ => Err(invalid("invalid column-batch summary boolean")),
        },
        2 => reader.read_i64().map(Value::Int64),
        3 => reader.read_f64().map(Value::Float64),
        4 => reader
            .read_bounded_string(MAX_STRING_BYTES)
            .map(Value::String),
        5 => {
            let count = read_count(reader, MAX_VECTOR_VALUES, 4)?;
            let values = (0..count)
                .map(|_| reader.read_u32().map(f32::from_bits))
                .collect::<Result<Vec<_>, _>>()?;
            Ok(Value::Vector(Vector::new(values)))
        }
        6 => {
            let raw = reader.read_bounded_bytes(MAX_JSON_BYTES)?;
            serde_json::from_slice(raw)
                .map(Value::Json)
                .map_err(|_| invalid("invalid column-batch JSON summary"))
        }
        _ => Err(invalid("unknown column-batch summary value tag")),
    }
}

fn encode_numeric_sum(out: &mut Vec<u8>, sum: &ColumnBatchNumericSum) {
    match sum {
        ColumnBatchNumericSum::Empty => out.push(0),
        ColumnBatchNumericSum::FloatEmpty => out.push(1),
        ColumnBatchNumericSum::Integer(value) => {
            out.push(2);
            write_i64(out, *value);
        }
        ColumnBatchNumericSum::Float(value) => {
            out.push(3);
            write_f64(out, *value);
        }
        ColumnBatchNumericSum::IntegerOverflow => out.push(4),
    }
}

fn decode_numeric_sum(reader: &mut Reader<'_>) -> Result<ColumnBatchNumericSum, CassieError> {
    match reader.read_u8()? {
        0 => Ok(ColumnBatchNumericSum::Empty),
        1 => Ok(ColumnBatchNumericSum::FloatEmpty),
        2 => reader.read_i64().map(ColumnBatchNumericSum::Integer),
        3 => reader.read_f64().map(ColumnBatchNumericSum::Float),
        4 => Ok(ColumnBatchNumericSum::IntegerOverflow),
        _ => Err(invalid("unknown column-batch numeric summary tag")),
    }
}

fn write_optional_i128(out: &mut Vec<u8>, value: Option<i128>) {
    match value {
        None => out.push(0),
        Some(value) => {
            out.push(1);
            write_i128(out, value);
        }
    }
}

fn read_optional_i128(reader: &mut Reader<'_>) -> Result<Option<i128>, CassieError> {
    match reader.read_u8()? {
        0 => Ok(None),
        1 => reader.read_i128().map(Some),
        _ => Err(invalid("invalid optional column-batch integer")),
    }
}

fn write_optional_f64(out: &mut Vec<u8>, value: Option<f64>) {
    match value {
        None => out.push(0),
        Some(value) => {
            out.push(1);
            write_f64(out, value);
        }
    }
}

fn read_optional_f64(reader: &mut Reader<'_>) -> Result<Option<f64>, CassieError> {
    match reader.read_u8()? {
        0 => Ok(None),
        1 => reader.read_f64().map(Some),
        _ => Err(invalid("invalid optional column-batch float")),
    }
}

fn write_optional_usize(out: &mut Vec<u8>, value: Option<usize>) -> Result<(), CassieError> {
    match value {
        None => out.push(0),
        Some(value) => {
            out.push(1);
            write_u64(
                out,
                u64::try_from(value)
                    .map_err(|_| invalid("column-batch optional count exceeds u64"))?,
            );
        }
    }
    Ok(())
}

fn read_optional_usize(reader: &mut Reader<'_>) -> Result<Option<usize>, CassieError> {
    match reader.read_u8()? {
        0 => Ok(None),
        1 => read_usize_u64(reader, "optional count").map(Some),
        _ => Err(invalid("invalid optional column-batch count")),
    }
}

fn write_optional_string(out: &mut Vec<u8>, value: Option<&str>) -> Result<(), CassieError> {
    match value {
        None => out.push(0),
        Some(value) => {
            out.push(1);
            write_string(out, value)?;
        }
    }
    Ok(())
}

fn read_optional_string(reader: &mut Reader<'_>) -> Result<Option<String>, CassieError> {
    match reader.read_u8()? {
        0 => Ok(None),
        1 => reader.read_bounded_string(MAX_STRING_BYTES).map(Some),
        _ => Err(invalid("invalid optional column-batch string")),
    }
}

fn write_checksum(out: &mut Vec<u8>, checksum: &str) -> Result<(), CassieError> {
    let raw = decode_hex(checksum)?;
    if raw.len() != CHECKSUM_LEN {
        return Err(invalid("invalid column-batch checksum length"));
    }
    out.extend_from_slice(raw.as_slice());
    Ok(())
}

fn read_checksum(reader: &mut Reader<'_>) -> Result<String, CassieError> {
    Ok(hex(reader.read_exact(CHECKSUM_LEN)?))
}

fn read_count(
    reader: &mut Reader<'_>,
    maximum: usize,
    minimum_record_bytes: usize,
) -> Result<usize, CassieError> {
    let count = read_usize_u32(reader, "count")?;
    if count > maximum
        || count
            .checked_mul(minimum_record_bytes)
            .is_none_or(|minimum| minimum > reader.remaining())
    {
        return Err(invalid("column-batch count exceeds limit"));
    }
    Ok(count)
}

fn read_usize_u32(reader: &mut Reader<'_>, name: &str) -> Result<usize, CassieError> {
    usize::try_from(reader.read_u32()?)
        .map_err(|_| invalid(&format!("column-batch {name} overflow")))
}

fn read_usize_u64(reader: &mut Reader<'_>, name: &str) -> Result<usize, CassieError> {
    usize::try_from(reader.read_u64()?)
        .map_err(|_| invalid(&format!("column-batch {name} overflow")))
}

fn logical_type_tag(name: &str) -> Result<u8, CassieError> {
    match name {
        "int64" => Ok(LogicalType::Int64 as u8),
        "float64" => Ok(LogicalType::Float64 as u8),
        "boolean" => Ok(LogicalType::Boolean as u8),
        "utf8" => Ok(LogicalType::Utf8 as u8),
        "complex" => Ok(LogicalType::Complex as u8),
        "row_id" => Ok(6),
        _ => Err(invalid("unknown column-batch chunk logical type")),
    }
}

fn logical_type_name(tag: u8) -> Result<&'static str, CassieError> {
    if tag == 6 {
        return Ok("row_id");
    }
    LogicalType::from_tag_for_manifest(tag).map(LogicalType::name)
}

fn codec_from_meta(id: u8, name: &str) -> Result<Codec, CassieError> {
    let codec = Codec::from_tag_for_manifest(id)?;
    if codec.name() != name {
        return Err(invalid("column-batch codec ID/name mismatch"));
    }
    Ok(codec)
}

fn canonical_json(value: &serde_json::Value) -> serde_json::Value {
    match value {
        serde_json::Value::Array(values) => {
            serde_json::Value::Array(values.iter().map(canonical_json).collect())
        }
        serde_json::Value::Object(values) => {
            let ordered = values
                .iter()
                .map(|(key, value)| (key.clone(), canonical_json(value)))
                .collect::<BTreeMap<_, _>>();
            serde_json::Value::Object(ordered.into_iter().collect())
        }
        value => value.clone(),
    }
}

fn hex(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len().saturating_mul(2));
    for byte in bytes {
        use std::fmt::Write as _;
        write!(&mut out, "{byte:02x}").expect("writing to a string cannot fail");
    }
    out
}

fn decode_hex(value: &str) -> Result<Vec<u8>, CassieError> {
    if !value.len().is_multiple_of(2) {
        return Err(invalid("invalid column-batch checksum hex"));
    }
    value
        .as_bytes()
        .chunks_exact(2)
        .map(|pair| {
            let text = std::str::from_utf8(pair)
                .map_err(|_| invalid("invalid column-batch checksum hex"))?;
            u8::from_str_radix(text, 16).map_err(|_| invalid("invalid column-batch checksum hex"))
        })
        .collect()
}
