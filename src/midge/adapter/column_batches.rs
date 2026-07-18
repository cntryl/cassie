use std::collections::{BTreeMap, BTreeSet};
use std::time::Instant;

use super::{
    collect_scan, CassieError, ColumnBatchCodecMeta, ColumnBatchColumn, ColumnBatchFieldSummary,
    ColumnBatchMetadata, ColumnBatchPayload, ColumnBatchRow, ColumnBatchScanDecision,
    ColumnBatchScanFallbackReason, ColumnBatchScanFilter, ColumnBatchScanOp,
    ColumnBatchScanOutcome, ColumnBatchScanPredicate, ColumnBatchSegmentMeta, ColumnBatchValueRun,
    DocumentRef, IndexKind, IndexMeta, Midge, MidgeScanTimings, Query, RowFilter, WriteOptions,
};

mod codec;
mod output;
mod summary;
mod validation;

use self::codec::{decode_column_batch, encode_column_batch};
use self::output::project_column_batch_document;
use self::summary::{
    column_batch_summaries, column_values, compare_summary_to_json, summary_checksum,
};
pub(crate) use self::validation::ControlledColumnBatchSummaryDecision;
use crate::runtime::{QueryExecutionControls, QueryMemoryReservation};

const COLUMN_BATCH_ENCODING_VERSION: u32 = 1;
const COLUMN_BATCH_CODEC_VERSION: u32 = 1;
pub(super) const CURRENT_COLUMN_BATCH_METADATA_FORMAT_VERSION: u32 = 1;
pub(super) const CURRENT_COLUMN_BATCH_SUMMARY_FORMAT_VERSION: u32 = 1;

struct EncodedColumnBatch {
    codec_name: String,
    codec_version: u32,
    uncompressed_len: usize,
    compressed_len: usize,
    checksum: String,
    bytes: Vec<u8>,
}

struct ColumnBatchScanPlan {
    index: IndexMeta,
    metadata: ColumnBatchMetadata,
    wanted: BTreeSet<String>,
    batch_size: usize,
    limit: usize,
    query_memory: Option<QueryMemoryReservation>,
}

pub(crate) struct ControlledColumnBatchScanRequest<'a> {
    pub collection: &'a str,
    pub batch_size: usize,
    pub fields: &'a [String],
    pub filter: Option<&'a RowFilter>,
    pub segment_filter: Option<&'a ColumnBatchScanFilter>,
    pub limit: Option<usize>,
    pub controls: &'a QueryExecutionControls,
}

struct ColumnBatchScanRequest<'a> {
    collection: &'a str,
    batch_size: usize,
    fields: &'a [String],
    filter: Option<&'a RowFilter>,
    segment_filter: Option<&'a ColumnBatchScanFilter>,
    limit: Option<usize>,
    controls: Option<&'a QueryExecutionControls>,
}

enum PreparedColumnBatchScan {
    Ready(Box<ColumnBatchScanPlan>),
    Fallback(ColumnBatchScanFallbackReason),
}

struct LoadedColumnBatchSegment {
    compressed_len: usize,
    uncompressed_len: usize,
    rows: Vec<ColumnBatchRow>,
}

struct ColumnBatchScanState {
    batches: Vec<Vec<DocumentRef>>,
    current: Vec<DocumentRef>,
    emitted: usize,
    compressed_bytes: usize,
    uncompressed_bytes: usize,
    skipped_segments: usize,
    decoded_columns: usize,
    query_memory: Option<QueryMemoryReservation>,
}

impl ColumnBatchScanState {
    fn new(
        controls: Option<&QueryExecutionControls>,
        query_memory: Option<QueryMemoryReservation>,
    ) -> Result<Self, CassieError> {
        let query_memory = match (controls, query_memory) {
            (Some(_), Some(memory)) => Some(memory),
            (Some(controls), None) => Some(controls.reserve_query_memory(0)?),
            (None, _) => None,
        };
        Ok(Self {
            batches: Vec::new(),
            current: Vec::new(),
            emitted: 0,
            compressed_bytes: 0,
            uncompressed_bytes: 0,
            skipped_segments: 0,
            decoded_columns: 0,
            query_memory,
        })
    }

    fn record_segment(&mut self, segment: &LoadedColumnBatchSegment, decoded_columns: usize) {
        self.compressed_bytes = self.compressed_bytes.saturating_add(segment.compressed_len);
        self.uncompressed_bytes = self
            .uncompressed_bytes
            .saturating_add(segment.uncompressed_len);
        self.decoded_columns = self.decoded_columns.saturating_add(decoded_columns);
    }

    fn push_projected_row(
        &mut self,
        row: ColumnBatchRow,
        fields: &[String],
        batch_size: usize,
    ) -> Result<(), CassieError> {
        let document = project_column_batch_document(self.query_memory.as_mut(), row, fields)?;
        self.current.push(document);
        self.emitted += 1;
        if self.current.len() >= batch_size {
            self.batches.push(std::mem::take(&mut self.current));
            self.current = Vec::new();
        }
        Ok(())
    }

    fn finish(mut self, started: Instant, index_name: String) -> ColumnBatchScanDecision {
        if !self.current.is_empty() {
            self.batches.push(self.current);
        }
        ColumnBatchScanDecision::Hit(ColumnBatchScanOutcome {
            batches: self.batches,
            timings: MidgeScanTimings {
                scan: started.elapsed(),
                row_decode: std::time::Duration::default(),
            },
            index_name,
            compressed_bytes: self.compressed_bytes,
            uncompressed_bytes: self.uncompressed_bytes,
            skipped_segments: self.skipped_segments,
            decoded_columns: self.decoded_columns,
            query_memory: self.query_memory,
        })
    }
}

impl Midge {
    /// # Errors
    ///
    /// Returns an error when validation, storage, or execution fails.
    pub fn rebuild_column_batches_for_collection(
        &self,
        collection: &str,
    ) -> Result<usize, CassieError> {
        let collection = self.canonical_collection_name(collection);
        let mut rebuilt = 0usize;
        for index in self.list_indexes()? {
            if index.collection == collection && index.kind == IndexKind::Column {
                self.rebuild_column_batches_for_index(&index)?;
                rebuilt += 1;
            }
        }
        Ok(rebuilt)
    }

    /// # Errors
    ///
    /// Returns an error when validation, storage, or execution fails.
    pub fn rebuild_column_batches_for_index(
        &self,
        index: &IndexMeta,
    ) -> Result<ColumnBatchMetadata, CassieError> {
        if index.kind != IndexKind::Column {
            return Err(CassieError::Unsupported(
                "column batch rebuild requires a column index".to_string(),
            ));
        }

        let mut index = index.clone();
        index.collection = self.canonical_collection_name(&index.collection);
        let fields = index.normalized_fields();
        let segment_size = column_index_segment_size(&index)?;
        let mut documents = self.scan_documents(&index.collection)?;
        documents.sort_by(|left, right| left.id.cmp(&right.id));

        let row_schema = self.row_schema(&index.collection)?;
        let schema_version = row_schema.schema_version;
        let built_generation = self.collection_generation(&index.collection)?;
        let source_row_count = documents.len();
        let mut segments = Vec::new();
        let mut payloads = Vec::new();
        for (segment_id, chunk) in documents.chunks(segment_size).enumerate() {
            let segment_id = segment_id as u64;
            let rows = chunk
                .iter()
                .map(|document| ColumnBatchRow {
                    row_id: document.id.clone(),
                    values: column_values(&document.payload, fields.as_slice()),
                })
                .collect::<Vec<_>>();
            let summaries = column_batch_summaries(rows.as_slice(), fields.as_slice(), &row_schema);
            let summary_checksum = summary_checksum(chunk.len(), &summaries)?;
            let value_count = rows.len().saturating_mul(fields.len());
            let (_, codec) = encode_column_batch_payload(rows.as_slice(), fields.as_slice())?;
            let payload = codec.bytes.clone();
            segments.push(ColumnBatchSegmentMeta {
                segment_id,
                row_id_start: chunk.first().map(|document| document.id.clone()),
                row_id_end: chunk.last().map(|document| document.id.clone()),
                row_count: chunk.len(),
                null_bitmap_available: true,
                encoding_version: COLUMN_BATCH_ENCODING_VERSION,
                codec: ColumnBatchCodecMeta {
                    codec_name: codec.codec_name,
                    codec_version: codec.codec_version,
                    uncompressed_len: codec.uncompressed_len,
                    compressed_len: codec.compressed_len,
                    value_count,
                    null_bitmap_encoding: "validity-bitmap".to_string(),
                    checksum: Some(codec.checksum),
                },
                summary_checksum,
                summaries,
            });
            payloads.push((segment_id, payload));
        }

        let metadata = ColumnBatchMetadata {
            metadata_format_version: CURRENT_COLUMN_BATCH_METADATA_FORMAT_VERSION,
            summary_format_version: CURRENT_COLUMN_BATCH_SUMMARY_FORMAT_VERSION,
            collection: index.collection.clone(),
            index_name: index.name.clone(),
            schema_version,
            built_generation,
            source_row_count,
            fields,
            segment_size,
            segments,
        };
        let (relation_id, index_id) = Self::column_batch_storage_ids(&index)?;

        let mut data_tx = self.begin_data_rw_tx_for(&index.collection)?;
        Self::delete_keys_with_prefix(
            &mut data_tx,
            Self::column_batch_index_prefix(relation_id, index_id),
        )?;
        data_tx
            .put(
                Self::column_batch_metadata_key(relation_id, index_id),
                serde_json::to_vec(&metadata)
                    .map_err(|error| CassieError::Parse(error.to_string()))?,
                None,
            )
            .map_err(CassieError::from)?;
        for (segment_id, payload) in payloads {
            data_tx
                .put(
                    Self::column_batch_segment_key(relation_id, index_id, segment_id),
                    payload,
                    None,
                )
                .map_err(CassieError::from)?;
        }
        data_tx
            .commit(WriteOptions::sync())
            .map_err(CassieError::from)?;

        Ok(metadata)
    }

    /// # Errors
    ///
    /// Returns an error when validation, storage, or execution fails.
    pub fn get_column_batch_metadata(
        &self,
        collection: &str,
        index_name: &str,
    ) -> Result<Option<ColumnBatchMetadata>, CassieError> {
        let Some(stored_index) = self.get_index(collection, index_name)? else {
            return Ok(None);
        };
        let stored_collection = stored_index.collection.clone();
        let (relation_id, index_id) = Self::column_batch_storage_ids(&stored_index)?;
        let tx = self.begin_data_readonly_tx_for(&stored_collection)?;
        let Some(raw) = tx
            .get(&Self::column_batch_metadata_key(relation_id, index_id))
            .map_err(CassieError::from)?
        else {
            return Ok(None);
        };
        serde_json::from_slice(&raw)
            .map(Some)
            .map_err(|error| CassieError::Parse(format!("invalid column batch metadata: {error}")))
    }

    /// # Errors
    ///
    /// Returns an error when validation, storage, or execution fails.
    pub fn delete_column_batches(
        &self,
        collection: &str,
        index_name: &str,
    ) -> Result<(), CassieError> {
        let Some(stored_index) = self.get_index(collection, index_name)? else {
            return Ok(());
        };
        let stored_collection = stored_index.collection.clone();
        let (relation_id, index_id) = Self::column_batch_storage_ids(&stored_index)?;
        let mut data_tx = self.begin_data_rw_tx_for(&stored_collection)?;
        Self::delete_keys_with_prefix(
            &mut data_tx,
            Self::column_batch_index_prefix(relation_id, index_id),
        )?;
        data_tx
            .commit(WriteOptions::sync())
            .map_err(CassieError::from)?;
        Ok(())
    }

    /// # Errors
    ///
    /// Returns an error when validation, storage, or execution fails.
    pub fn scan_column_batch_projected_rows(
        &self,
        collection: &str,
        batch_size: usize,
        fields: &[String],
        filter: Option<&RowFilter>,
        segment_filter: Option<&ColumnBatchScanFilter>,
        limit: Option<usize>,
    ) -> Result<ColumnBatchScanDecision, CassieError> {
        self.scan_column_batch_projected_rows_internal(&ColumnBatchScanRequest {
            collection,
            batch_size,
            fields,
            filter,
            segment_filter,
            limit,
            controls: None,
        })
    }

    pub(crate) fn scan_column_batch_projected_rows_controlled(
        &self,
        request: &ControlledColumnBatchScanRequest<'_>,
    ) -> Result<ColumnBatchScanDecision, CassieError> {
        self.scan_column_batch_projected_rows_internal(&ColumnBatchScanRequest {
            collection: request.collection,
            batch_size: request.batch_size,
            fields: request.fields,
            filter: request.filter,
            segment_filter: request.segment_filter,
            limit: request.limit,
            controls: Some(request.controls),
        })
    }

    fn scan_column_batch_projected_rows_internal(
        &self,
        request: &ColumnBatchScanRequest<'_>,
    ) -> Result<ColumnBatchScanDecision, CassieError> {
        let collection = self.canonical_collection_name(request.collection);
        let started = Instant::now();
        let plan = match self.prepare_column_batch_scan(
            &collection,
            request.batch_size,
            request.fields,
            request.limit,
            request.controls,
        )? {
            PreparedColumnBatchScan::Ready(plan) => *plan,
            PreparedColumnBatchScan::Fallback(reason) => {
                return Ok(ColumnBatchScanDecision::Fallback(reason));
            }
        };
        self.execute_column_batch_scan(&collection, started, plan, request)
    }

    fn prepare_column_batch_scan(
        &self,
        collection: &str,
        batch_size: usize,
        fields: &[String],
        limit: Option<usize>,
        controls: Option<&QueryExecutionControls>,
    ) -> Result<PreparedColumnBatchScan, CassieError> {
        let Some(index) = self.covering_column_index(collection, fields)? else {
            return Ok(PreparedColumnBatchScan::Fallback(
                ColumnBatchScanFallbackReason::NoCoveringIndex,
            ));
        };
        let wanted = wanted_column_batch_fields(fields);
        let requested = wanted.iter().cloned().collect::<Vec<_>>();
        let (metadata, query_memory) = if let Some(controls) = controls {
            match self.prepare_column_batch_scan_metadata_controlled(
                collection,
                &index,
                requested.as_slice(),
                controls,
            )? {
                ControlledColumnBatchSummaryDecision::Ready(controlled) => {
                    (*controlled.metadata, Some(controlled.memory))
                }
                ControlledColumnBatchSummaryDecision::Fallback(reason) => {
                    return Ok(PreparedColumnBatchScan::Fallback(reason));
                }
            }
        } else {
            match self.prepare_column_batch_scan_metadata(
                collection,
                &index,
                requested.as_slice(),
            )? {
                super::ColumnBatchSummaryDecision::Ready(metadata) => (*metadata, None),
                super::ColumnBatchSummaryDecision::Fallback(reason) => {
                    return Ok(PreparedColumnBatchScan::Fallback(reason));
                }
            }
        };
        Ok(PreparedColumnBatchScan::Ready(Box::new(
            ColumnBatchScanPlan {
                index,
                metadata,
                wanted,
                batch_size: batch_size.max(1),
                limit: limit.unwrap_or(usize::MAX),
                query_memory,
            },
        )))
    }

    fn execute_column_batch_scan(
        &self,
        collection: &str,
        started: Instant,
        plan: ColumnBatchScanPlan,
        request: &ColumnBatchScanRequest<'_>,
    ) -> Result<ColumnBatchScanDecision, CassieError> {
        let mut state = ColumnBatchScanState::new(request.controls, plan.query_memory)?;
        let data_tx = self.begin_data_readonly_tx_for(collection)?;
        for segment in &plan.metadata.segments {
            if let Some(controls) = request.controls {
                check_column_batch_controls(self, controls)?;
            }
            if !column_batch_segment_may_match(segment, request.segment_filter) {
                state.skipped_segments += 1;
                continue;
            }
            let _segment_memory = request
                .controls
                .map(|controls| {
                    controls.reserve_query_memory(
                        segment
                            .codec
                            .uncompressed_len
                            .max(segment.codec.compressed_len),
                    )
                })
                .transpose()?;
            let loaded = match load_column_batch_segment(&data_tx, &plan.index, segment)? {
                Ok(loaded) => loaded,
                Err(reason) => return Ok(ColumnBatchScanDecision::Fallback(reason)),
            };
            state.record_segment(&loaded, plan.wanted.len());
            for row in loaded.rows {
                if state.emitted >= plan.limit {
                    break;
                }
                if !column_batch_row_matches(&row, request.filter) {
                    continue;
                }
                state.push_projected_row(row, request.fields, plan.batch_size)?;
            }
            if state.emitted >= plan.limit {
                break;
            }
        }
        Ok(state.finish(started, plan.index.name))
    }

    pub(crate) fn delete_keys_with_prefix(
        tx: &mut cntryl_midge::Transaction,
        prefix: Vec<u8>,
    ) -> Result<(), CassieError> {
        let scan = collect_scan(
            tx.scan(&Query::new().prefix(prefix.into()))
                .map_err(CassieError::from)?,
        )?;
        let mut keys = Vec::new();
        for (key, _) in scan {
            keys.push(key);
        }
        for key in keys {
            tx.delete(key).map_err(CassieError::from)?;
        }
        Ok(())
    }

    fn covering_column_index(
        &self,
        collection: &str,
        fields: &[String],
    ) -> Result<Option<IndexMeta>, CassieError> {
        let wanted = fields
            .iter()
            .filter(|field| !field.eq_ignore_ascii_case("id") && !field.eq_ignore_ascii_case("_id"))
            .map(|field| field.to_ascii_lowercase())
            .collect::<BTreeSet<_>>();
        if wanted.is_empty() {
            return Ok(None);
        }

        Ok(self
            .list_indexes()?
            .into_iter()
            .filter(|index| index.collection == collection && index.kind == IndexKind::Column)
            .find(|index| {
                let available = index
                    .normalized_fields()
                    .into_iter()
                    .map(|field| field.to_ascii_lowercase())
                    .collect::<BTreeSet<_>>();
                wanted.is_subset(&available)
            }))
    }

    fn column_batch_storage_ids(index: &IndexMeta) -> Result<(u64, u64), CassieError> {
        let relation_id = index.relation_id().ok_or_else(|| {
            CassieError::Parse(format!("index '{}' is missing its relation id", index.name))
        })?;
        let index_id = index.storage_id().ok_or_else(|| {
            CassieError::Parse(format!("index '{}' is missing its storage id", index.name))
        })?;
        Ok((relation_id, index_id))
    }
}

fn wanted_column_batch_fields(fields: &[String]) -> BTreeSet<String> {
    fields
        .iter()
        .filter(|field| !field.eq_ignore_ascii_case("id") && !field.eq_ignore_ascii_case("_id"))
        .map(|field| field.to_ascii_lowercase())
        .collect()
}

fn load_column_batch_segment(
    data_tx: &cntryl_midge::Transaction,
    index: &IndexMeta,
    segment: &ColumnBatchSegmentMeta,
) -> Result<Result<LoadedColumnBatchSegment, ColumnBatchScanFallbackReason>, CassieError> {
    let (relation_id, index_id) = Midge::column_batch_storage_ids(index)?;
    let Some(raw) = data_tx
        .get(&Midge::column_batch_segment_key(
            relation_id,
            index_id,
            segment.segment_id,
        ))
        .map_err(CassieError::from)?
    else {
        return Ok(Err(ColumnBatchScanFallbackReason::SegmentMissing));
    };
    if segment
        .codec
        .checksum
        .as_ref()
        .is_some_and(|checksum| checksum != &checksum_hex(&raw))
    {
        return Ok(Err(ColumnBatchScanFallbackReason::SegmentChecksumMismatch));
    }
    let Ok(payload) = decode_column_batch(&raw) else {
        return Ok(Err(ColumnBatchScanFallbackReason::InvalidPayload));
    };
    if payload.encoding_version != COLUMN_BATCH_ENCODING_VERSION {
        return Ok(Err(ColumnBatchScanFallbackReason::InvalidEncodingVersion));
    }
    if payload.codec_name != segment.codec.codec_name
        || payload.codec_version != segment.codec.codec_version
    {
        return Ok(Err(ColumnBatchScanFallbackReason::SegmentCodecMismatch));
    }
    let Some(rows) = decode_column_batch_payload(&payload, segment.row_count) else {
        return Ok(Err(ColumnBatchScanFallbackReason::SegmentDecodeFailed));
    };
    Ok(Ok(LoadedColumnBatchSegment {
        compressed_len: segment.codec.compressed_len,
        uncompressed_len: segment.codec.uncompressed_len,
        rows,
    }))
}

fn column_index_segment_size(index: &IndexMeta) -> Result<usize, CassieError> {
    let raw = index
        .options
        .get("segment_size")
        .map_or("1024", String::as_str)
        .trim();
    let parsed = raw
        .parse::<usize>()
        .map_err(|_| CassieError::Parse("invalid column index segment_size".to_string()))?;
    Ok(parsed.max(1))
}

fn encode_column_batch_payload(
    rows: &[ColumnBatchRow],
    fields: &[String],
) -> Result<(ColumnBatchPayload, EncodedColumnBatch), CassieError> {
    let uncompressed = ColumnBatchPayload {
        encoding_version: COLUMN_BATCH_ENCODING_VERSION,
        codec_name: "uncompressed".to_string(),
        codec_version: COLUMN_BATCH_CODEC_VERSION,
        row_ids: Vec::new(),
        rows: rows.to_owned(),
        columns: Vec::new(),
    };
    let uncompressed_bytes = encode_column_batch(&uncompressed)?;
    let uncompressed_len = uncompressed_bytes.len();

    let rle = dictionary_rle_payload(rows, fields);
    let rle_bytes = encode_column_batch(&rle)?;

    let (payload, bytes) = if rle_bytes.len() < uncompressed_bytes.len() {
        (rle, rle_bytes)
    } else {
        (uncompressed, uncompressed_bytes)
    };
    let codec = EncodedColumnBatch {
        codec_name: payload.codec_name.clone(),
        codec_version: payload.codec_version,
        uncompressed_len,
        compressed_len: bytes.len(),
        checksum: checksum_hex(&bytes),
        bytes,
    };
    Ok((payload, codec))
}

fn dictionary_rle_payload(rows: &[ColumnBatchRow], fields: &[String]) -> ColumnBatchPayload {
    let row_ids = rows
        .iter()
        .map(|row| row.row_id.clone())
        .collect::<Vec<_>>();
    let columns = fields
        .iter()
        .map(|field| ColumnBatchColumn {
            field: field.clone(),
            runs: value_runs(rows, field),
        })
        .collect();
    ColumnBatchPayload {
        encoding_version: COLUMN_BATCH_ENCODING_VERSION,
        codec_name: "dictionary_rle".to_string(),
        codec_version: COLUMN_BATCH_CODEC_VERSION,
        row_ids,
        rows: Vec::new(),
        columns,
    }
}

fn value_runs(rows: &[ColumnBatchRow], field: &str) -> Vec<ColumnBatchValueRun> {
    let mut runs: Vec<ColumnBatchValueRun> = Vec::new();
    for row in rows {
        let value = row
            .values
            .iter()
            .find(|(name, _)| name.eq_ignore_ascii_case(field))
            .map_or(serde_json::Value::Null, |(_, value)| value.clone());
        if let Some(last) = runs.last_mut() {
            if last.value == value {
                last.len += 1;
                continue;
            }
        }
        runs.push(ColumnBatchValueRun { value, len: 1 });
    }
    runs
}

fn decode_column_batch_payload(
    payload: &ColumnBatchPayload,
    row_count: usize,
) -> Option<Vec<ColumnBatchRow>> {
    match (payload.codec_name.as_str(), payload.codec_version) {
        ("uncompressed", COLUMN_BATCH_CODEC_VERSION) => {
            (payload.rows.len() == row_count).then(|| payload.rows.clone())
        }
        ("dictionary_rle", COLUMN_BATCH_CODEC_VERSION) => {
            decode_dictionary_rle_payload(payload, row_count)
        }
        _ => None,
    }
}

fn decode_dictionary_rle_payload(
    payload: &ColumnBatchPayload,
    row_count: usize,
) -> Option<Vec<ColumnBatchRow>> {
    if payload.row_ids.len() != row_count {
        return None;
    }
    let mut rows = payload
        .row_ids
        .iter()
        .map(|row_id| ColumnBatchRow {
            row_id: row_id.clone(),
            values: BTreeMap::new(),
        })
        .collect::<Vec<_>>();

    for column in &payload.columns {
        let mut offset = 0usize;
        for run in &column.runs {
            if run.len == 0 || offset.saturating_add(run.len) > row_count {
                return None;
            }
            for row in rows.iter_mut().skip(offset).take(run.len) {
                row.values.insert(column.field.clone(), run.value.clone());
            }
            offset += run.len;
        }
        if offset != row_count {
            return None;
        }
    }

    Some(rows)
}

fn checksum_hex(bytes: &[u8]) -> String {
    let mut hash = 0xcbf2_9ce4_8422_2325_u64;
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0100_0000_01b3);
    }
    format!("{hash:016x}")
}

fn column_batch_segment_may_match(
    segment: &ColumnBatchSegmentMeta,
    filter: Option<&ColumnBatchScanFilter>,
) -> bool {
    let Some(filter) = filter else {
        return true;
    };
    filter
        .predicates
        .iter()
        .all(|predicate| segment_may_match_predicate(segment, predicate))
}

fn segment_may_match_predicate(
    segment: &ColumnBatchSegmentMeta,
    predicate: &ColumnBatchScanPredicate,
) -> bool {
    let Some(summary) = segment
        .summaries
        .iter()
        .find(|(field, _)| field.eq_ignore_ascii_case(&predicate.field))
        .map(|(_, summary)| summary)
    else {
        return true;
    };
    if !matches!(
        predicate.op,
        ColumnBatchScanOp::IsNull | ColumnBatchScanOp::IsNotNull
    ) && !column_batch_summary_supports_ordering(summary)
    {
        return true;
    }

    match predicate.op {
        ColumnBatchScanOp::IsNull => summary.non_null_count < segment.row_count,
        ColumnBatchScanOp::IsNotNull => summary.non_null_count > 0,
        ColumnBatchScanOp::Eq => {
            let Some(value) = predicate.value.as_ref() else {
                return true;
            };
            segment_range_may_contain(summary, value, value)
        }
        ColumnBatchScanOp::Lt => predicate
            .value
            .as_ref()
            .and_then(|value| {
                summary
                    .min
                    .as_ref()
                    .map(|min| compare_summary_to_json(min, value).is_lt())
            })
            .unwrap_or(true),
        ColumnBatchScanOp::Lte => predicate
            .value
            .as_ref()
            .and_then(|value| {
                summary
                    .min
                    .as_ref()
                    .map(|min| !compare_summary_to_json(min, value).is_gt())
            })
            .unwrap_or(true),
        ColumnBatchScanOp::Gt => predicate
            .value
            .as_ref()
            .and_then(|value| {
                summary
                    .max
                    .as_ref()
                    .map(|max| compare_summary_to_json(max, value).is_gt())
            })
            .unwrap_or(true),
        ColumnBatchScanOp::Gte => predicate
            .value
            .as_ref()
            .and_then(|value| {
                summary
                    .max
                    .as_ref()
                    .map(|max| !compare_summary_to_json(max, value).is_lt())
            })
            .unwrap_or(true),
    }
}

fn column_batch_summary_supports_ordering(summary: &ColumnBatchFieldSummary) -> bool {
    summary.min.iter().chain(summary.max.iter()).all(|value| {
        !matches!(
            value,
            crate::types::Value::Vector(_) | crate::types::Value::Json(_)
        )
    })
}

fn segment_range_may_contain(
    summary: &ColumnBatchFieldSummary,
    low: &serde_json::Value,
    high: &serde_json::Value,
) -> bool {
    if summary.non_null_count == 0 {
        return false;
    }
    if summary
        .max
        .as_ref()
        .is_some_and(|max| compare_summary_to_json(max, low).is_lt())
    {
        return false;
    }
    if summary
        .min
        .as_ref()
        .is_some_and(|min| compare_summary_to_json(min, high).is_gt())
    {
        return false;
    }
    true
}

fn column_batch_row_matches(row: &ColumnBatchRow, filter: Option<&RowFilter>) -> bool {
    let Some(filter) = filter else {
        return true;
    };
    row.values
        .iter()
        .find(|(field, _)| field.eq_ignore_ascii_case(&filter.field))
        .is_some_and(|(_, value)| value == &filter.value)
}

fn check_column_batch_controls(
    midge: &Midge,
    controls: &QueryExecutionControls,
) -> Result<(), CassieError> {
    if controls.is_cancelled() {
        return Err(CassieError::QueryCancelled);
    }
    if controls.is_timed_out() {
        return Err(CassieError::DeadlineExceeded);
    }
    midge.record_query_scan_entry();
    if super::query_scan_control::should_cancel_controlled_query_scan() {
        return Err(CassieError::QueryCancelled);
    }
    Ok(())
}
