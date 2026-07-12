use std::collections::{BTreeMap, BTreeSet};
use std::time::Instant;

use super::{
    collect_scan, CassieError, ColumnBatchCodecMeta, ColumnBatchColumn, ColumnBatchFieldSummary,
    ColumnBatchMetadata, ColumnBatchPayload, ColumnBatchRow, ColumnBatchScanDecision,
    ColumnBatchScanFallbackReason, ColumnBatchScanFilter, ColumnBatchScanOp,
    ColumnBatchScanOutcome, ColumnBatchScanPredicate, ColumnBatchSegmentMeta, ColumnBatchValueRun,
    DocumentRef, IndexKind, IndexMeta, Midge, MidgeScanTimings, Query, RowFilter, WriteOptions,
};

const COLUMN_BATCH_ENCODING_VERSION: u32 = 1;
const COLUMN_BATCH_CODEC_VERSION: u32 = 1;

struct EncodedColumnBatch {
    codec_name: String,
    codec_version: u32,
    uncompressed_len: usize,
    compressed_len: usize,
    checksum: String,
}

struct ColumnBatchScanPlan {
    index: IndexMeta,
    metadata: ColumnBatchMetadata,
    wanted: BTreeSet<String>,
    batch_size: usize,
    limit: usize,
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
}

impl ColumnBatchScanState {
    fn new(batch_size: usize) -> Self {
        Self {
            batches: Vec::new(),
            current: Vec::with_capacity(batch_size),
            emitted: 0,
            compressed_bytes: 0,
            uncompressed_bytes: 0,
            skipped_segments: 0,
            decoded_columns: 0,
        }
    }

    fn record_segment(&mut self, segment: &LoadedColumnBatchSegment, decoded_columns: usize) {
        self.compressed_bytes = self.compressed_bytes.saturating_add(segment.compressed_len);
        self.uncompressed_bytes = self
            .uncompressed_bytes
            .saturating_add(segment.uncompressed_len);
        self.decoded_columns = self.decoded_columns.saturating_add(decoded_columns);
    }

    fn push_row(&mut self, row: DocumentRef, batch_size: usize) {
        self.current.push(row);
        self.emitted += 1;
        if self.current.len() >= batch_size {
            self.batches.push(std::mem::take(&mut self.current));
            self.current = Vec::with_capacity(batch_size);
        }
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

        let schema_epoch = self.schema_epoch()?;
        let built_generation = self.collection_generation(&index.collection)?;
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
            let summaries = column_batch_summaries(rows.as_slice(), fields.as_slice());
            let value_count = rows.len().saturating_mul(fields.len());
            let (payload, codec) = encode_column_batch_payload(rows.as_slice(), fields.as_slice())?;
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
                    null_bitmap_encoding: "inline-json-null".to_string(),
                    checksum: Some(codec.checksum),
                },
                summaries,
            });
            payloads.push((segment_id, payload));
        }

        let metadata = ColumnBatchMetadata {
            collection: index.collection.clone(),
            index_name: index.name.clone(),
            schema_epoch,
            built_generation,
            fields,
            segment_size,
            segments,
        };

        let mut data_tx = self.begin_data_rw_tx_for(&index.collection)?;
        Self::delete_keys_with_prefix(
            &mut data_tx,
            Self::column_batch_index_prefix(&index.collection, &index.name),
        )?;
        data_tx
            .put(
                Self::column_batch_metadata_key(&index.collection, &index.name),
                serde_json::to_vec(&metadata)
                    .map_err(|error| CassieError::Parse(error.to_string()))?,
                None,
            )
            .map_err(CassieError::from)?;
        for (segment_id, payload) in payloads {
            data_tx
                .put(
                    Self::column_batch_segment_key(&index.collection, &index.name, segment_id),
                    serde_json::to_vec(&payload)
                        .map_err(|error| CassieError::Parse(error.to_string()))?,
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
        let stored_index = self.get_index(collection, index_name)?;
        let stored_collection = stored_index
            .as_ref()
            .map_or_else(|| collection.to_string(), |index| index.collection.clone());
        let stored_index_name = stored_index
            .as_ref()
            .map_or_else(|| index_name.to_string(), |index| index.name.clone());
        let tx = self.begin_data_readonly_tx_for(&stored_collection)?;
        let Some(raw) = tx
            .get(&Self::column_batch_metadata_key(
                &stored_collection,
                &stored_index_name,
            ))
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
        let stored_index = self.get_index(collection, index_name)?;
        let stored_collection = stored_index
            .as_ref()
            .map_or_else(|| collection.to_string(), |index| index.collection.clone());
        let stored_index_name = stored_index
            .as_ref()
            .map_or_else(|| index_name.to_string(), |index| index.name.clone());
        let mut data_tx = self.begin_data_rw_tx_for(&stored_collection)?;
        Self::delete_keys_with_prefix(
            &mut data_tx,
            Self::column_batch_index_prefix(&stored_collection, &stored_index_name),
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
        let collection = self.canonical_collection_name(collection);
        let started = Instant::now();
        let plan = match self.prepare_column_batch_scan(&collection, batch_size, fields, limit)? {
            PreparedColumnBatchScan::Ready(plan) => *plan,
            PreparedColumnBatchScan::Fallback(reason) => {
                return Ok(ColumnBatchScanDecision::Fallback(reason));
            }
        };
        self.execute_column_batch_scan(&collection, fields, filter, segment_filter, started, plan)
    }

    fn prepare_column_batch_scan(
        &self,
        collection: &str,
        batch_size: usize,
        fields: &[String],
        limit: Option<usize>,
    ) -> Result<PreparedColumnBatchScan, CassieError> {
        let Some(index) = self.covering_column_index(collection, fields)? else {
            return Ok(PreparedColumnBatchScan::Fallback(
                ColumnBatchScanFallbackReason::NoCoveringIndex,
            ));
        };
        if self.has_column_batch_maintenance_debt(collection)? {
            return Ok(PreparedColumnBatchScan::Fallback(
                ColumnBatchScanFallbackReason::MaintenancePending,
            ));
        }
        let Some(metadata) = self.get_column_batch_metadata(collection, &index.name)? else {
            return Ok(PreparedColumnBatchScan::Fallback(
                ColumnBatchScanFallbackReason::MissingMetadata,
            ));
        };
        if metadata.fields.len() != index.normalized_fields().len()
            || metadata.segment_size != column_index_segment_size(&index)?
        {
            return Ok(PreparedColumnBatchScan::Fallback(
                ColumnBatchScanFallbackReason::SegmentSizeMismatch,
            ));
        }
        if metadata.built_generation != self.collection_generation(collection)? {
            return Ok(PreparedColumnBatchScan::Fallback(
                ColumnBatchScanFallbackReason::GenerationMismatch,
            ));
        }
        let wanted = wanted_column_batch_fields(fields);
        if !wanted.is_subset(&available_column_batch_fields(&metadata)) {
            return Ok(PreparedColumnBatchScan::Fallback(
                ColumnBatchScanFallbackReason::FieldCoverageMismatch,
            ));
        }
        Ok(PreparedColumnBatchScan::Ready(Box::new(
            ColumnBatchScanPlan {
                index,
                metadata,
                wanted,
                batch_size: batch_size.max(1),
                limit: limit.unwrap_or(usize::MAX),
            },
        )))
    }

    fn execute_column_batch_scan(
        &self,
        collection: &str,
        fields: &[String],
        filter: Option<&RowFilter>,
        segment_filter: Option<&ColumnBatchScanFilter>,
        started: Instant,
        plan: ColumnBatchScanPlan,
    ) -> Result<ColumnBatchScanDecision, CassieError> {
        let mut state = ColumnBatchScanState::new(plan.batch_size);
        let data_tx = self.begin_data_readonly_tx_for(collection)?;
        for segment in &plan.metadata.segments {
            if !column_batch_segment_may_match(segment, segment_filter) {
                state.skipped_segments += 1;
                continue;
            }
            let loaded =
                match load_column_batch_segment(&data_tx, collection, &plan.index.name, segment)? {
                    Ok(loaded) => loaded,
                    Err(reason) => return Ok(ColumnBatchScanDecision::Fallback(reason)),
                };
            state.record_segment(&loaded, plan.wanted.len());
            for row in loaded.rows {
                if state.emitted >= plan.limit {
                    break;
                }
                if !column_batch_row_matches(&row, filter) {
                    continue;
                }
                let payload = project_column_batch_row(&row, fields);
                state.push_row(
                    DocumentRef {
                        id: row.row_id,
                        payload,
                    },
                    plan.batch_size,
                );
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
}

fn wanted_column_batch_fields(fields: &[String]) -> BTreeSet<String> {
    fields
        .iter()
        .filter(|field| !field.eq_ignore_ascii_case("id") && !field.eq_ignore_ascii_case("_id"))
        .map(|field| field.to_ascii_lowercase())
        .collect()
}

fn available_column_batch_fields(metadata: &ColumnBatchMetadata) -> BTreeSet<String> {
    metadata
        .fields
        .iter()
        .map(|field| field.to_ascii_lowercase())
        .collect()
}

fn load_column_batch_segment(
    data_tx: &cntryl_midge::Transaction,
    collection: &str,
    index_name: &str,
    segment: &ColumnBatchSegmentMeta,
) -> Result<Result<LoadedColumnBatchSegment, ColumnBatchScanFallbackReason>, CassieError> {
    let Some(raw) = data_tx
        .get(&Midge::column_batch_segment_key(
            collection,
            index_name,
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
    let Ok(payload) = serde_json::from_slice::<ColumnBatchPayload>(&raw) else {
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
    let uncompressed_bytes =
        serde_json::to_vec(&uncompressed).map_err(|error| CassieError::Parse(error.to_string()))?;

    let rle = dictionary_rle_payload(rows, fields);
    let rle_bytes =
        serde_json::to_vec(&rle).map_err(|error| CassieError::Parse(error.to_string()))?;

    let (payload, bytes) = if rle_bytes.len() < uncompressed_bytes.len() {
        (rle, rle_bytes)
    } else {
        (uncompressed, uncompressed_bytes)
    };
    let codec = EncodedColumnBatch {
        codec_name: payload.codec_name.clone(),
        codec_version: payload.codec_version,
        uncompressed_len: serde_json::to_vec(rows)
            .map_err(|error| CassieError::Parse(error.to_string()))?
            .len(),
        compressed_len: bytes.len(),
        checksum: checksum_hex(&bytes),
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
        ("uncompressed", COLUMN_BATCH_CODEC_VERSION) => Some(payload.rows.clone()),
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

fn column_values(
    payload: &serde_json::Value,
    fields: &[String],
) -> BTreeMap<String, serde_json::Value> {
    let object = payload.as_object();
    fields
        .iter()
        .map(|field| {
            let value = object
                .and_then(|object| {
                    object.get(field).or_else(|| {
                        object
                            .iter()
                            .find(|(name, _)| name.eq_ignore_ascii_case(field))
                            .map(|(_, value)| value)
                    })
                })
                .cloned()
                .unwrap_or(serde_json::Value::Null);
            (field.clone(), value)
        })
        .collect()
}

fn column_batch_summaries(
    rows: &[ColumnBatchRow],
    fields: &[String],
) -> BTreeMap<String, ColumnBatchFieldSummary> {
    fields
        .iter()
        .map(|field| (field.clone(), column_batch_field_summary(rows, field)))
        .collect()
}

fn column_batch_field_summary(rows: &[ColumnBatchRow], field: &str) -> ColumnBatchFieldSummary {
    let mut non_null_count = 0usize;
    let mut min: Option<serde_json::Value> = None;
    let mut max: Option<serde_json::Value> = None;
    let mut sum = 0.0f64;
    let mut has_sum = false;
    let mut all_int = true;
    let mut distinct = BTreeSet::new();

    for row in rows {
        let value = row
            .values
            .iter()
            .find(|(name, _)| name.eq_ignore_ascii_case(field))
            .map_or(&serde_json::Value::Null, |(_, value)| value);
        if value.is_null() {
            continue;
        }
        non_null_count += 1;
        distinct.insert(value.to_string());
        if min
            .as_ref()
            .is_none_or(|current| compare_json_values(value, current).is_lt())
        {
            min = Some(value.clone());
        }
        if max
            .as_ref()
            .is_none_or(|current| compare_json_values(value, current).is_gt())
        {
            max = Some(value.clone());
        }
        if let Some(number) = value.as_i64() {
            if let Ok(parsed) = number.to_string().parse::<f64>() {
                sum += parsed;
                has_sum = true;
            } else {
                all_int = false;
            }
        } else if let Some(number) = value.as_f64() {
            sum += number;
            has_sum = true;
            all_int = false;
        } else {
            all_int = false;
        }
    }

    ColumnBatchFieldSummary {
        non_null_count,
        min,
        max,
        sum: has_sum.then_some(sum),
        all_int,
        distinct_hint: Some(distinct.len()),
    }
}

fn compare_json_values(left: &serde_json::Value, right: &serde_json::Value) -> std::cmp::Ordering {
    match (left, right) {
        (serde_json::Value::Number(left), serde_json::Value::Number(right)) => left
            .as_f64()
            .partial_cmp(&right.as_f64())
            .unwrap_or(std::cmp::Ordering::Equal),
        (serde_json::Value::String(left), serde_json::Value::String(right)) => left.cmp(right),
        (serde_json::Value::Bool(left), serde_json::Value::Bool(right)) => left.cmp(right),
        _ => left.to_string().cmp(&right.to_string()),
    }
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
                    .map(|min| compare_json_values(min, value).is_lt())
            })
            .unwrap_or(true),
        ColumnBatchScanOp::Lte => predicate
            .value
            .as_ref()
            .and_then(|value| {
                summary
                    .min
                    .as_ref()
                    .map(|min| !compare_json_values(min, value).is_gt())
            })
            .unwrap_or(true),
        ColumnBatchScanOp::Gt => predicate
            .value
            .as_ref()
            .and_then(|value| {
                summary
                    .max
                    .as_ref()
                    .map(|max| compare_json_values(max, value).is_gt())
            })
            .unwrap_or(true),
        ColumnBatchScanOp::Gte => predicate
            .value
            .as_ref()
            .and_then(|value| {
                summary
                    .max
                    .as_ref()
                    .map(|max| !compare_json_values(max, value).is_lt())
            })
            .unwrap_or(true),
    }
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
        .is_some_and(|max| compare_json_values(max, low).is_lt())
    {
        return false;
    }
    if summary
        .min
        .as_ref()
        .is_some_and(|min| compare_json_values(min, high).is_gt())
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

fn project_column_batch_row(row: &ColumnBatchRow, fields: &[String]) -> serde_json::Value {
    let mut object = serde_json::Map::new();
    for field in fields {
        if field.eq_ignore_ascii_case("id") || field.eq_ignore_ascii_case("_id") {
            continue;
        }
        let value = row
            .values
            .iter()
            .find(|(name, _)| name.eq_ignore_ascii_case(field))
            .map_or(serde_json::Value::Null, |(_, value)| value.clone());
        object.insert(field.clone(), value);
    }
    serde_json::Value::Object(object)
}
