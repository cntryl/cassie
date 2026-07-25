use std::collections::{BTreeMap, BTreeSet};

use super::{
    column_batch_summaries, column_values, summary::json_to_typed_value, CassieError,
    ColumnBatchChunkMeta, ColumnBatchRow, ColumnBatchScanFallbackReason, ColumnBatchScanFilter,
    ColumnBatchScanOp, ColumnBatchSegmentMeta, IndexMeta, Midge,
};
use crate::midge::adapter::column_batch_format_v2::{
    checksum_hex, decode_column_chunk, decode_row_ids, decode_selected_column_chunk,
    encode_column_chunk, encode_plain_column_chunk, encode_row_ids, summary_checksum, LogicalType,
};
use crate::midge::row_blob::RowSchema;
use crate::types::semantic::compare_values;

const CODEC_VERSION: u32 = 1;

pub(super) struct EncodedSegment {
    pub(super) metadata: ColumnBatchSegmentMeta,
    pub(super) row_ids: Vec<u8>,
    pub(super) fields: BTreeMap<String, Vec<u8>>,
}

pub(super) struct LoadedSegment {
    pub(super) encoded_bytes: usize,
    pub(super) decoded_bytes: usize,
    pub(super) rows: Vec<ColumnBatchRow>,
    pub(super) chunks_read: usize,
    pub(super) predicate_values: usize,
    pub(super) candidate_rows: usize,
    pub(super) selected_rows: usize,
    pub(super) materialized_values: usize,
}

pub(super) struct AggregateSegment {
    pub(super) values: BTreeMap<String, Vec<crate::types::Value>>,
    pub(super) selected_rows: usize,
}

/// Loads only field chunks for a filtered aggregate.  In particular, this deliberately does not
/// validate or read the row-id chunk: aggregate results have no row identity observable to SQL.
pub(super) fn scan_aggregate_segment(
    tx: &cntryl_midge::Transaction,
    index: &IndexMeta,
    segment: &ColumnBatchSegmentMeta,
    fields: &BTreeSet<String>,
    filter: Option<&ColumnBatchScanFilter>,
) -> Result<Result<AggregateSegment, ColumnBatchScanFallbackReason>, CassieError> {
    let (relation_id, index_id) = Midge::column_batch_storage_ids(index)?;
    let mut decoded = BTreeMap::<String, Vec<serde_json::Value>>::new();
    let mut needed = fields.clone();
    if let Some(filter) = filter {
        needed.extend(
            filter
                .predicates
                .iter()
                .map(|predicate| predicate.field.clone()),
        );
    }
    for wanted in needed {
        let Some((field, meta)) = find_field_chunk(segment, &wanted) else {
            return Ok(Err(ColumnBatchScanFallbackReason::FieldCoverageMismatch));
        };
        let loaded = match load_field_chunk(tx, relation_id, index_id, segment, field, meta)? {
            Ok(loaded) => loaded,
            Err(reason) => return Ok(Err(reason)),
        };
        if loaded.values.len() != segment.row_count {
            return Ok(Err(ColumnBatchScanFallbackReason::SegmentCodecMismatch));
        }
        decoded.insert(field.clone(), loaded.values);
    }
    let (selection, _) = match encoded_selection(segment.row_count, filter, &decoded) {
        Ok(selection) => selection,
        Err(reason) => return Ok(Err(reason)),
    };
    let selected_rows = selection.iter().filter(|selected| **selected).count();
    let values = match fields
        .iter()
        .map(|field| {
            let (_, values) = decoded
                .iter()
                .find(|(stored, _)| stored.eq_ignore_ascii_case(field))
                .ok_or(ColumnBatchScanFallbackReason::FieldCoverageMismatch)?;
            Ok((
                field.clone(),
                values
                    .iter()
                    .zip(&selection)
                    .filter(|(_, selected)| **selected)
                    .map(|(value, _)| json_to_typed_value(value))
                    .collect::<Vec<_>>(),
            ))
        })
        .collect::<Result<BTreeMap<_, _>, ColumnBatchScanFallbackReason>>()
    {
        Ok(values) => values,
        Err(reason) => return Ok(Err(reason)),
    };
    Ok(Ok(AggregateSegment {
        values,
        selected_rows,
    }))
}

pub(super) fn encode_segment(
    segment_id: u64,
    revision: u64,
    rows: &[ColumnBatchRow],
    fields: &[String],
    row_schema: &RowSchema,
    row_id_end: Option<String>,
) -> Result<EncodedSegment, CassieError> {
    encode_segment_with_policy(
        segment_id, revision, rows, fields, row_schema, row_id_end, false,
    )
}

pub(super) fn encode_plain_segment_for_benchmark(
    segment_id: u64,
    revision: u64,
    rows: &[ColumnBatchRow],
    fields: &[String],
    row_schema: &RowSchema,
    row_id_end: Option<String>,
) -> Result<EncodedSegment, CassieError> {
    encode_segment_with_policy(
        segment_id, revision, rows, fields, row_schema, row_id_end, true,
    )
}

fn encode_segment_with_policy(
    segment_id: u64,
    revision: u64,
    rows: &[ColumnBatchRow],
    fields: &[String],
    row_schema: &RowSchema,
    row_id_end: Option<String>,
    force_plain: bool,
) -> Result<EncodedSegment, CassieError> {
    let row_ids = rows
        .iter()
        .map(|row| row.row_id.clone())
        .collect::<Vec<_>>();
    let encoded_row_ids = encode_row_ids(row_ids.as_slice())?;
    let row_ids_meta = ColumnBatchChunkMeta {
        logical_type: "row_id".to_string(),
        codec_id: 0,
        codec_name: "plain".to_string(),
        codec_version: CODEC_VERSION,
        decoded_len: row_ids.iter().map(String::len).sum(),
        encoded_len: encoded_row_ids.len(),
        value_count: row_ids.len(),
        null_count: 0,
        checksum_sha256: checksum_hex(encoded_row_ids.as_slice()),
    };
    let mut encoded_fields = BTreeMap::new();
    let mut field_chunks = BTreeMap::new();
    for field in fields {
        let values = rows
            .iter()
            .map(|row| {
                row.values
                    .iter()
                    .find(|(name, _)| name.eq_ignore_ascii_case(field))
                    .map_or(serde_json::Value::Null, |(_, value)| value.clone())
            })
            .collect::<Vec<_>>();
        let logical_type = field_logical_type(row_schema, field);
        let encoded = if force_plain {
            encode_plain_column_chunk(logical_type, values.as_slice())?
        } else {
            encode_column_chunk(logical_type, values.as_slice())?
        };
        field_chunks.insert(
            field.clone(),
            ColumnBatchChunkMeta {
                logical_type: encoded.logical_type.name().to_string(),
                codec_id: encoded.codec as u8,
                codec_name: encoded.codec.name().to_string(),
                codec_version: CODEC_VERSION,
                decoded_len: encoded.decoded_len,
                encoded_len: encoded.bytes.len(),
                value_count: encoded.value_count,
                null_count: encoded.null_count,
                checksum_sha256: checksum_hex(encoded.bytes.as_slice()),
            },
        );
        encoded_fields.insert(field.clone(), encoded.bytes);
    }
    let summaries = column_batch_summaries(rows, fields, row_schema);
    let summary_checksum = summary_checksum(rows.len(), &summaries)?;
    Ok(EncodedSegment {
        metadata: ColumnBatchSegmentMeta {
            segment_id,
            revision,
            row_id_start: row_ids.first().cloned(),
            row_id_end,
            row_count: rows.len(),
            null_bitmap_available: true,
            encoding_version: 2,
            row_ids: row_ids_meta,
            field_chunks,
            summary_checksum,
            summaries,
        },
        row_ids: encoded_row_ids,
        fields: encoded_fields,
    })
}

pub(super) fn write_segment(
    tx: &mut cntryl_midge::Transaction,
    relation_id: u64,
    index_id: u64,
    segment: &EncodedSegment,
) -> Result<(), CassieError> {
    tx.put(
        Midge::column_batch_row_ids_key(
            relation_id,
            index_id,
            segment.metadata.segment_id,
            segment.metadata.revision,
        ),
        segment.row_ids.clone(),
        None,
    )
    .map_err(CassieError::from)?;
    for (field, bytes) in &segment.fields {
        tx.put(
            Midge::column_batch_field_key(
                relation_id,
                index_id,
                segment.metadata.segment_id,
                segment.metadata.revision,
                field,
            ),
            bytes.clone(),
            None,
        )
        .map_err(CassieError::from)?;
    }
    Ok(())
}

pub(super) fn load_segment(
    tx: &cntryl_midge::Transaction,
    index: &IndexMeta,
    segment: &ColumnBatchSegmentMeta,
    wanted: &BTreeSet<String>,
) -> Result<Result<LoadedSegment, ColumnBatchScanFallbackReason>, CassieError> {
    let (relation_id, index_id) = Midge::column_batch_storage_ids(index)?;
    let Some(raw_row_ids) = tx
        .get(&Midge::column_batch_row_ids_key(
            relation_id,
            index_id,
            segment.segment_id,
            segment.revision,
        ))
        .map_err(CassieError::from)?
    else {
        return Ok(Err(ColumnBatchScanFallbackReason::SegmentMissing));
    };
    if !valid_chunk_bytes(&raw_row_ids, &segment.row_ids) {
        return Ok(Err(ColumnBatchScanFallbackReason::SegmentChecksumMismatch));
    }
    let Ok(row_ids) = decode_row_ids(&raw_row_ids) else {
        return Ok(Err(ColumnBatchScanFallbackReason::InvalidPayload));
    };
    if row_ids.len() != segment.row_count
        || row_ids.first() != segment.row_id_start.as_ref()
        || !row_ids.windows(2).all(|pair| pair[0] < pair[1])
        || segment
            .row_id_end
            .as_ref()
            .is_some_and(|end| row_ids.last().is_some_and(|last| last >= end))
    {
        return Ok(Err(ColumnBatchScanFallbackReason::SegmentDecodeFailed));
    }
    let mut rows = row_ids
        .into_iter()
        .map(|row_id| ColumnBatchRow {
            row_id,
            values: BTreeMap::new(),
        })
        .collect::<Vec<_>>();
    let mut encoded_bytes = raw_row_ids.len();
    let mut decoded_bytes = segment.row_ids.decoded_len;
    let mut chunks_read = 1usize;
    let mut values_decoded = 0usize;
    for wanted_field in wanted {
        let Some((field, chunk_meta)) = segment
            .field_chunks
            .iter()
            .find(|(field, _)| field.eq_ignore_ascii_case(wanted_field))
        else {
            return Ok(Err(ColumnBatchScanFallbackReason::FieldCoverageMismatch));
        };
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
        if !valid_chunk_bytes(&raw, chunk_meta) {
            return Ok(Err(ColumnBatchScanFallbackReason::SegmentChecksumMismatch));
        }
        let Ok(decoded) = decode_column_chunk(&raw) else {
            return Ok(Err(ColumnBatchScanFallbackReason::InvalidPayload));
        };
        if decoded.values.len() != rows.len()
            || decoded.logical_type.name() != chunk_meta.logical_type
            || decoded.codec as u8 != chunk_meta.codec_id
            || decoded.codec.name() != chunk_meta.codec_name
            || decoded.encoded_len != chunk_meta.encoded_len
            || decoded.decoded_len != chunk_meta.decoded_len
        {
            return Ok(Err(ColumnBatchScanFallbackReason::SegmentCodecMismatch));
        }
        for (row, value) in rows.iter_mut().zip(decoded.values) {
            row.values.insert(field.clone(), value);
        }
        encoded_bytes = encoded_bytes.saturating_add(raw.len());
        decoded_bytes = decoded_bytes.saturating_add(chunk_meta.decoded_len);
        chunks_read = chunks_read.saturating_add(1);
        values_decoded = values_decoded.saturating_add(rows.len());
    }
    Ok(Ok(LoadedSegment {
        encoded_bytes,
        decoded_bytes,
        rows,
        chunks_read,
        predicate_values: 0,
        candidate_rows: segment.row_count,
        selected_rows: segment.row_count,
        materialized_values: values_decoded,
    }))
}

pub(super) fn scan_segment(
    tx: &cntryl_midge::Transaction,
    index: &IndexMeta,
    segment: &ColumnBatchSegmentMeta,
    wanted: &BTreeSet<String>,
    filter: Option<&ColumnBatchScanFilter>,
) -> Result<Result<LoadedSegment, ColumnBatchScanFallbackReason>, CassieError> {
    let (relation_id, index_id) = Midge::column_batch_storage_ids(index)?;
    let mut decoded_fields = BTreeMap::<String, Vec<serde_json::Value>>::new();
    let mut encoded_bytes = 0usize;
    let mut decoded_bytes = 0usize;
    let mut chunks_read = 0usize;

    if let Some(filter) = filter {
        for predicate in &filter.predicates {
            if decoded_fields
                .keys()
                .any(|field| field.eq_ignore_ascii_case(&predicate.field))
            {
                continue;
            }
            let Some((field, meta)) = find_field_chunk(segment, &predicate.field) else {
                return Ok(Err(ColumnBatchScanFallbackReason::FieldCoverageMismatch));
            };
            let loaded = match load_field_chunk(tx, relation_id, index_id, segment, field, meta)? {
                Ok(loaded) => loaded,
                Err(reason) => return Ok(Err(reason)),
            };
            encoded_bytes = encoded_bytes.saturating_add(loaded.encoded_len);
            decoded_bytes = decoded_bytes.saturating_add(loaded.decoded_len);
            chunks_read = chunks_read.saturating_add(1);
            decoded_fields.insert(field.clone(), loaded.values);
        }
    }

    let (selection, predicate_values) =
        match encoded_selection(segment.row_count, filter, &decoded_fields) {
            Ok(selection) => selection,
            Err(reason) => return Ok(Err(reason)),
        };
    let selected_rows = selection.iter().filter(|selected| **selected).count();
    if selected_rows == 0 {
        return Ok(Ok(empty_loaded_segment(
            encoded_bytes,
            decoded_bytes,
            chunks_read,
            predicate_values,
            segment.row_count,
        )));
    }

    let (projection_encoded, projection_decoded, projection_chunks) = match load_projection_fields(
        tx,
        relation_id,
        index_id,
        segment,
        wanted,
        selection.as_slice(),
        &mut decoded_fields,
    )? {
        Ok(accounting) => accounting,
        Err(reason) => return Ok(Err(reason)),
    };
    encoded_bytes = encoded_bytes.saturating_add(projection_encoded);
    decoded_bytes = decoded_bytes.saturating_add(projection_decoded);
    chunks_read = chunks_read.saturating_add(projection_chunks);

    let row_ids = match load_row_ids(tx, relation_id, index_id, segment)? {
        Ok(row_ids) => row_ids,
        Err(reason) => return Ok(Err(reason)),
    };
    encoded_bytes = encoded_bytes.saturating_add(segment.row_ids.encoded_len);
    decoded_bytes = decoded_bytes.saturating_add(segment.row_ids.decoded_len);
    chunks_read = chunks_read.saturating_add(1);
    let rows = materialize_selected_rows(row_ids, selection, wanted, &decoded_fields);
    let materialized_values = rows.len().saturating_mul(wanted.len());
    Ok(Ok(LoadedSegment {
        encoded_bytes,
        decoded_bytes,
        rows,
        chunks_read,
        predicate_values,
        candidate_rows: segment.row_count,
        selected_rows,
        materialized_values,
    }))
}

fn materialize_selected_rows(
    row_ids: Vec<String>,
    selection: Vec<bool>,
    wanted: &BTreeSet<String>,
    decoded_fields: &BTreeMap<String, Vec<serde_json::Value>>,
) -> Vec<ColumnBatchRow> {
    row_ids
        .into_iter()
        .zip(selection)
        .enumerate()
        .filter_map(|(position, (row_id, selected))| {
            selected.then(|| {
                let values = wanted
                    .iter()
                    .filter_map(|wanted_field| {
                        decoded_fields
                            .iter()
                            .find(|(field, _)| field.eq_ignore_ascii_case(wanted_field))
                            .and_then(|(field, values)| {
                                values
                                    .get(position)
                                    .cloned()
                                    .map(|value| (field.clone(), value))
                            })
                    })
                    .collect();
                ColumnBatchRow { row_id, values }
            })
        })
        .collect()
}

fn empty_loaded_segment(
    encoded_bytes: usize,
    decoded_bytes: usize,
    chunks_read: usize,
    predicate_values: usize,
    candidate_rows: usize,
) -> LoadedSegment {
    LoadedSegment {
        encoded_bytes,
        decoded_bytes,
        rows: Vec::new(),
        chunks_read,
        predicate_values,
        candidate_rows,
        selected_rows: 0,
        materialized_values: 0,
    }
}

fn load_projection_fields(
    tx: &cntryl_midge::Transaction,
    relation_id: u64,
    index_id: u64,
    segment: &ColumnBatchSegmentMeta,
    wanted: &BTreeSet<String>,
    selection: &[bool],
    decoded_fields: &mut BTreeMap<String, Vec<serde_json::Value>>,
) -> Result<Result<(usize, usize, usize), ColumnBatchScanFallbackReason>, CassieError> {
    let mut encoded_bytes = 0usize;
    let mut decoded_bytes = 0usize;
    let mut chunks_read = 0usize;
    for wanted_field in wanted {
        if decoded_fields
            .keys()
            .any(|field| field.eq_ignore_ascii_case(wanted_field))
        {
            continue;
        }
        let Some((field, meta)) = find_field_chunk(segment, wanted_field) else {
            return Ok(Err(ColumnBatchScanFallbackReason::FieldCoverageMismatch));
        };
        let loaded = match load_field_chunk_selected(
            tx,
            relation_id,
            index_id,
            segment,
            field,
            meta,
            selection,
        )? {
            Ok(loaded) => loaded,
            Err(reason) => return Ok(Err(reason)),
        };
        encoded_bytes = encoded_bytes.saturating_add(loaded.encoded_len);
        decoded_bytes = decoded_bytes.saturating_add(loaded.decoded_len);
        chunks_read = chunks_read.saturating_add(1);
        decoded_fields.insert(field.clone(), loaded.values);
    }
    Ok(Ok((encoded_bytes, decoded_bytes, chunks_read)))
}

fn encoded_selection(
    row_count: usize,
    filter: Option<&ColumnBatchScanFilter>,
    decoded_fields: &BTreeMap<String, Vec<serde_json::Value>>,
) -> Result<(Vec<bool>, usize), ColumnBatchScanFallbackReason> {
    let mut selection = vec![true; row_count];
    let mut predicate_values = 0usize;
    if let Some(filter) = filter {
        for predicate in &filter.predicates {
            let (_, values) = decoded_fields
                .iter()
                .find(|(field, _)| field.eq_ignore_ascii_case(&predicate.field))
                .ok_or(ColumnBatchScanFallbackReason::FieldCoverageMismatch)?;
            for (selected, value) in selection.iter_mut().zip(values) {
                if *selected {
                    *selected = predicate_matches(value, predicate);
                }
                predicate_values = predicate_values.saturating_add(1);
            }
        }
    }
    Ok((selection, predicate_values))
}

pub(super) fn all_segment_fields(segment: &ColumnBatchSegmentMeta) -> BTreeSet<String> {
    segment.field_chunks.keys().cloned().collect()
}

pub(super) fn rows_for_segment(
    documents: &[super::DocumentRef],
    fields: &[String],
) -> Vec<ColumnBatchRow> {
    documents
        .iter()
        .map(|document| ColumnBatchRow {
            row_id: document.id.clone(),
            values: column_values(&document.payload, fields),
        })
        .collect()
}

fn field_logical_type(row_schema: &RowSchema, field: &str) -> LogicalType {
    row_schema
        .fields
        .iter()
        .find(|candidate| {
            !candidate.retired
                && (candidate.name.eq_ignore_ascii_case(field)
                    || candidate
                        .aliases
                        .iter()
                        .any(|alias| alias.eq_ignore_ascii_case(field)))
        })
        .and_then(|candidate| LogicalType::from_name(&candidate.data_type.type_name()).ok())
        .unwrap_or(LogicalType::Complex)
}

fn valid_chunk_bytes(bytes: &[u8], meta: &ColumnBatchChunkMeta) -> bool {
    bytes.len() == meta.encoded_len && checksum_hex(bytes) == meta.checksum_sha256
}

struct LoadedField {
    values: Vec<serde_json::Value>,
    encoded_len: usize,
    decoded_len: usize,
}

fn find_field_chunk<'a>(
    segment: &'a ColumnBatchSegmentMeta,
    field: &str,
) -> Option<(&'a String, &'a ColumnBatchChunkMeta)> {
    segment
        .field_chunks
        .iter()
        .find(|(stored, _)| stored.eq_ignore_ascii_case(field))
}

fn load_field_chunk(
    tx: &cntryl_midge::Transaction,
    relation_id: u64,
    index_id: u64,
    segment: &ColumnBatchSegmentMeta,
    field: &str,
    meta: &ColumnBatchChunkMeta,
) -> Result<Result<LoadedField, ColumnBatchScanFallbackReason>, CassieError> {
    load_field_chunk_with_selection(tx, relation_id, index_id, segment, field, meta, None)
}

fn load_field_chunk_selected(
    tx: &cntryl_midge::Transaction,
    relation_id: u64,
    index_id: u64,
    segment: &ColumnBatchSegmentMeta,
    field: &str,
    meta: &ColumnBatchChunkMeta,
    selection: &[bool],
) -> Result<Result<LoadedField, ColumnBatchScanFallbackReason>, CassieError> {
    load_field_chunk_with_selection(
        tx,
        relation_id,
        index_id,
        segment,
        field,
        meta,
        Some(selection),
    )
}

fn load_field_chunk_with_selection(
    tx: &cntryl_midge::Transaction,
    relation_id: u64,
    index_id: u64,
    segment: &ColumnBatchSegmentMeta,
    field: &str,
    meta: &ColumnBatchChunkMeta,
    selection: Option<&[bool]>,
) -> Result<Result<LoadedField, ColumnBatchScanFallbackReason>, CassieError> {
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
    if !valid_chunk_bytes(&raw, meta) {
        return Ok(Err(ColumnBatchScanFallbackReason::SegmentChecksumMismatch));
    }
    let decoded = selection.map_or_else(
        || decode_column_chunk(&raw),
        |selection| decode_selected_column_chunk(&raw, selection),
    );
    let Ok(decoded) = decoded else {
        return Ok(Err(ColumnBatchScanFallbackReason::InvalidPayload));
    };
    if decoded.values.len() != segment.row_count
        || decoded.logical_type.name() != meta.logical_type
        || decoded.codec as u8 != meta.codec_id
        || decoded.codec.name() != meta.codec_name
        || decoded.encoded_len != meta.encoded_len
        || decoded.decoded_len != meta.decoded_len
    {
        return Ok(Err(ColumnBatchScanFallbackReason::SegmentCodecMismatch));
    }
    Ok(Ok(LoadedField {
        values: decoded.values,
        encoded_len: decoded.encoded_len,
        decoded_len: decoded.decoded_len,
    }))
}

fn load_row_ids(
    tx: &cntryl_midge::Transaction,
    relation_id: u64,
    index_id: u64,
    segment: &ColumnBatchSegmentMeta,
) -> Result<Result<Vec<String>, ColumnBatchScanFallbackReason>, CassieError> {
    let Some(raw) = tx
        .get(&Midge::column_batch_row_ids_key(
            relation_id,
            index_id,
            segment.segment_id,
            segment.revision,
        ))
        .map_err(CassieError::from)?
    else {
        return Ok(Err(ColumnBatchScanFallbackReason::SegmentMissing));
    };
    if !valid_chunk_bytes(&raw, &segment.row_ids) {
        return Ok(Err(ColumnBatchScanFallbackReason::SegmentChecksumMismatch));
    }
    let Ok(row_ids) = decode_row_ids(&raw) else {
        return Ok(Err(ColumnBatchScanFallbackReason::InvalidPayload));
    };
    if row_ids.len() != segment.row_count
        || row_ids.first() != segment.row_id_start.as_ref()
        || !row_ids.windows(2).all(|pair| pair[0] < pair[1])
        || segment
            .row_id_end
            .as_ref()
            .is_some_and(|end| row_ids.last().is_some_and(|last| last >= end))
    {
        return Ok(Err(ColumnBatchScanFallbackReason::SegmentDecodeFailed));
    }
    Ok(Ok(row_ids))
}

fn predicate_matches(
    value: &serde_json::Value,
    predicate: &super::ColumnBatchScanPredicate,
) -> bool {
    match predicate.op {
        ColumnBatchScanOp::IsNull => value.is_null(),
        ColumnBatchScanOp::IsNotNull => !value.is_null(),
        ColumnBatchScanOp::Eq
        | ColumnBatchScanOp::Lt
        | ColumnBatchScanOp::Lte
        | ColumnBatchScanOp::Gt
        | ColumnBatchScanOp::Gte => {
            if value.is_null() {
                return false;
            }
            let Some(expected) = predicate.value.as_ref() else {
                return false;
            };
            let ordering =
                compare_values(&json_to_typed_value(value), &json_to_typed_value(expected));
            match predicate.op {
                ColumnBatchScanOp::Eq => ordering.is_eq(),
                ColumnBatchScanOp::Lt => ordering.is_lt(),
                ColumnBatchScanOp::Lte => !ordering.is_gt(),
                ColumnBatchScanOp::Gt => ordering.is_gt(),
                ColumnBatchScanOp::Gte => !ordering.is_lt(),
                ColumnBatchScanOp::IsNull | ColumnBatchScanOp::IsNotNull => false,
            }
        }
    }
}
