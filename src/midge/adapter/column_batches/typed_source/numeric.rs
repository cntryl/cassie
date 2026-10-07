//! Primitive numeric handoff uses existing metadata, keys and checksum authority.
use super::super::storage_v2;
use super::super::CURRENT_COLUMN_BATCH_CODEC_VERSION;
use crate::app::CassieError;
use crate::catalog::{ColumnBatchChunkMeta, ColumnBatchSegmentMeta};
use crate::midge::adapter::column_batch_format_v2::{decode_numeric_column_chunk, NumericValues};
use crate::midge::adapter::{ColumnBatchScanFallbackReason, Midge};

pub(super) struct Loaded {
    pub(super) raw: bytes::Bytes,
    pub(super) values: Vec<serde_json::Value>,
    pub(super) numeric: Option<NumericValues>,
}
pub(super) fn load(
    tx: &cntryl_midge::Transaction,
    relation_id: u64,
    index_id: u64,
    segment: &ColumnBatchSegmentMeta,
    field: &str,
    meta: &ColumnBatchChunkMeta,
) -> Result<Result<Loaded, ColumnBatchScanFallbackReason>, CassieError> {
    if !matches!(meta.logical_type.as_str(), "int64" | "float64") {
        return Ok(
            storage_v2::load_field_chunk(tx, relation_id, index_id, segment, field, meta)?.map(
                |loaded| Loaded {
                    raw: loaded.raw,
                    values: loaded.values,
                    numeric: None,
                },
            ),
        );
    }
    if meta.codec_version != CURRENT_COLUMN_BATCH_CODEC_VERSION {
        return Ok(Err(ColumnBatchScanFallbackReason::SegmentCodecMismatch));
    }
    let Some(raw) = tx
        .get(&Midge::column_batch_field_key(
            relation_id,
            index_id,
            segment.segment_id,
            segment.revision,
            field,
        ))
        .map_err(CassieError::from)?
    else {
        return Ok(Err(ColumnBatchScanFallbackReason::SegmentMissing));
    };
    if !storage_v2::valid_chunk_bytes(&raw, meta) {
        return Ok(Err(ColumnBatchScanFallbackReason::SegmentChecksumMismatch));
    }
    let Ok(decoded) = decode_numeric_column_chunk(&raw, segment.row_count) else {
        return Ok(Err(ColumnBatchScanFallbackReason::InvalidPayload));
    };
    if decoded.values.len() != segment.row_count
        || decoded.values.len() != meta.value_count
        || decoded.null_count != meta.null_count
        || decoded.logical_type.name() != meta.logical_type
        || decoded.codec as u8 != meta.codec_id
        || decoded.codec.name() != meta.codec_name
        || decoded.encoded_len != meta.encoded_len
        || decoded.decoded_len != meta.decoded_len
    {
        return Ok(Err(ColumnBatchScanFallbackReason::SegmentCodecMismatch));
    }
    Ok(Ok(Loaded {
        raw,
        values: Vec::new(),
        numeric: Some(decoded.values),
    }))
}
