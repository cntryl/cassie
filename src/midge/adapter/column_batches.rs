use std::collections::BTreeSet;
use std::time::Instant;

use super::{
    collect_scan, CassieError, ColumnBatchAggregateDecision, ColumnBatchAggregateOutcome,
    ColumnBatchAggregateSpec, ColumnBatchChunkMeta, ColumnBatchFieldSummary, ColumnBatchMetadata,
    ColumnBatchRow, ColumnBatchScanDecision, ColumnBatchScanFallbackReason, ColumnBatchScanFilter,
    ColumnBatchScanOp, ColumnBatchScanOutcome, ColumnBatchScanPredicate, ColumnBatchSegmentMeta,
    DocumentRef, IndexKind, IndexMeta, Midge, MidgeScanTimings, Query, RowFilter,
};

mod incremental;
mod output;
mod storage_v2;
mod summary;
mod validation;

use self::output::project_column_batch_document;
use self::storage_v2::{load_segment, scan_aggregate_segment, scan_segment, LoadedSegment};
use self::summary::{column_batch_summaries, column_values, compare_summary_to_json};
pub(crate) use self::validation::ControlledColumnBatchSummaryDecision;
use crate::midge::adapter::column_batch_format_v2::{
    decode_manifest, encode_manifest, summary_checksum, MANIFEST_FORMAT_VERSION,
    MANIFEST_SUMMARY_VERSION,
};
use crate::runtime::{QueryExecutionControls, QueryMemoryReservation};
use crate::types::semantic::compare_values;
use crate::types::Value;

pub(super) const CURRENT_COLUMN_BATCH_METADATA_FORMAT_VERSION: u32 = MANIFEST_FORMAT_VERSION as u32;
pub(super) const CURRENT_COLUMN_BATCH_SUMMARY_FORMAT_VERSION: u32 = MANIFEST_SUMMARY_VERSION as u32;

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

struct ColumnBatchScanState {
    batches: Vec<Vec<DocumentRef>>,
    current: Vec<DocumentRef>,
    emitted: usize,
    segments_read: usize,
    chunks_read: usize,
    physical_bytes: usize,
    logical_bytes: usize,
    predicate_values: usize,
    candidate_rows: usize,
    selected_rows: usize,
    materialized_values: usize,
    skipped_segments: usize,
    query_memory: Option<QueryMemoryReservation>,
}

enum DirectAggregateAccumulator {
    Count(i64),
    Sum { value: Option<Value>, seen: bool },
    Avg { sum: f64, count: usize },
    Min { value: Option<Value>, max: bool },
}

impl DirectAggregateAccumulator {
    fn new(spec: &ColumnBatchAggregateSpec) -> Self {
        match spec.function.as_str() {
            "count" => Self::Count(0),
            "sum" => Self::Sum {
                value: None,
                seen: false,
            },
            "avg" => Self::Avg { sum: 0.0, count: 0 },
            "max" => Self::Min {
                value: None,
                max: true,
            },
            _ => Self::Min {
                value: None,
                max: false,
            },
        }
    }

    fn update_count(&mut self, rows: usize) -> Result<(), CassieError> {
        let Self::Count(count) = self else {
            return Ok(());
        };
        *count = count
            .checked_add(
                i64::try_from(rows)
                    .map_err(|_| CassieError::Parse("aggregate row count overflow".to_string()))?,
            )
            .ok_or_else(|| CassieError::Parse("aggregate row count overflow".to_string()))?;
        Ok(())
    }

    fn update_values(
        &mut self,
        spec: &ColumnBatchAggregateSpec,
        values: &[Value],
    ) -> Result<(), CassieError> {
        match self {
            Self::Count(count) => {
                *count = count
                    .checked_add(
                        i64::try_from(values.iter().filter(|v| !v.is_null()).count()).map_err(
                            |_| CassieError::Parse("aggregate row count overflow".to_string()),
                        )?,
                    )
                    .ok_or_else(|| {
                        CassieError::Parse("aggregate row count overflow".to_string())
                    })?;
            }
            Self::Sum { value, seen } => {
                for current in values.iter().filter(|value| !value.is_null()) {
                    match current {
                        Value::Int64(next) => match value {
                            None => *value = Some(Value::Int64(*next)),
                            Some(Value::Int64(total)) => {
                                *total = total.checked_add(*next).ok_or_else(|| {
                                    CassieError::Parse("aggregate integer overflow".to_string())
                                })?;
                            }
                            Some(Value::Float64(total)) => *total += int_to_f64(*next),
                            _ => {
                                return Err(CassieError::Parse(
                                    "unsupported aggregate type".to_string(),
                                ))
                            }
                        },
                        Value::Float64(next) => {
                            if value.is_none() {
                                *value = Some(Value::Float64(0.0));
                            }
                            if let Some(Value::Int64(total)) = value {
                                *value = Some(Value::Float64(int_to_f64(*total)));
                            }
                            if let Some(Value::Float64(total)) = value {
                                *total += next;
                            }
                        }
                        _ => {
                            return Err(CassieError::Parse(format!(
                                "{} requires numeric input",
                                spec.function
                            )))
                        }
                    }
                    *seen = true;
                }
            }
            Self::Avg { sum, count } => {
                for current in values {
                    match current {
                        Value::Int64(value) => {
                            *sum += int_to_f64(*value);
                            *count = count.checked_add(1).ok_or_else(|| {
                                CassieError::Parse("aggregate row count overflow".to_string())
                            })?;
                        }
                        Value::Float64(value) => {
                            *sum += value;
                            *count = count.checked_add(1).ok_or_else(|| {
                                CassieError::Parse("aggregate row count overflow".to_string())
                            })?;
                        }
                        Value::Null => {}
                        _ => {
                            return Err(CassieError::Parse(
                                "avg requires numeric input".to_string(),
                            ))
                        }
                    }
                }
            }
            Self::Min { value, max } => {
                for current in values.iter().filter(|value| !value.is_null()) {
                    let replace = value.as_ref().is_none_or(|selected| {
                        let ordering = compare_values(current, selected);
                        if *max {
                            ordering.is_gt()
                        } else {
                            ordering.is_lt()
                        }
                    });
                    if replace {
                        *value = Some(current.clone());
                    }
                }
            }
        }
        Ok(())
    }

    fn finish(self) -> Value {
        match self {
            Self::Count(count) => Value::Int64(count),
            Self::Sum { value, seen } => {
                if seen {
                    value.unwrap_or(Value::Null)
                } else {
                    Value::Null
                }
            }
            Self::Avg { sum, count } => {
                if count == 0 {
                    Value::Null
                } else {
                    Value::Float64(sum / usize_to_f64(count))
                }
            }
            Self::Min { value, .. } => value.unwrap_or(Value::Null),
        }
    }
}

fn int_to_f64(value: i64) -> f64 {
    value
        .to_string()
        .parse::<f64>()
        .expect("i64 converts to f64")
}

fn usize_to_f64(value: usize) -> f64 {
    value
        .to_string()
        .parse::<f64>()
        .expect("usize converts to f64")
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
            segments_read: 0,
            chunks_read: 0,
            physical_bytes: 0,
            logical_bytes: 0,
            predicate_values: 0,
            candidate_rows: 0,
            selected_rows: 0,
            materialized_values: 0,
            skipped_segments: 0,
            query_memory,
        })
    }

    fn record_segment(&mut self, segment: &LoadedSegment) {
        self.segments_read = self.segments_read.saturating_add(1);
        self.chunks_read = self.chunks_read.saturating_add(segment.chunks_read);
        self.physical_bytes = self.physical_bytes.saturating_add(segment.encoded_bytes);
        self.logical_bytes = self.logical_bytes.saturating_add(segment.decoded_bytes);
        self.predicate_values = self
            .predicate_values
            .saturating_add(segment.predicate_values);
        self.candidate_rows = self.candidate_rows.saturating_add(segment.candidate_rows);
        self.selected_rows = self.selected_rows.saturating_add(segment.selected_rows);
        self.materialized_values = self
            .materialized_values
            .saturating_add(segment.materialized_values);
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
            segments_read: self.segments_read,
            chunks_read: self.chunks_read,
            physical_bytes: self.physical_bytes,
            logical_bytes: self.logical_bytes,
            predicate_values: self.predicate_values,
            candidate_rows: self.candidate_rows,
            selected_rows: self.selected_rows,
            materialized_values: self.materialized_values,
            skipped_segments: self.skipped_segments,
            query_memory: self.query_memory,
        })
    }
}

impl Midge {
    pub(crate) fn execute_column_batch_aggregates_controlled(
        &self,
        collection: &str,
        fields: &[String],
        filter: Option<&ColumnBatchScanFilter>,
        specs: &[ColumnBatchAggregateSpec],
        controls: &QueryExecutionControls,
    ) -> Result<ColumnBatchAggregateDecision, CassieError> {
        let collection = self.canonical_collection_name(collection);
        let plan =
            match self.prepare_column_batch_scan(&collection, 1, fields, None, Some(controls))? {
                PreparedColumnBatchScan::Ready(plan) => *plan,
                PreparedColumnBatchScan::Fallback(reason) => {
                    return Ok(ColumnBatchAggregateDecision::Fallback(reason));
                }
            };
        for spec in specs {
            let Some(field) = spec.field.as_ref() else {
                continue;
            };
            let numeric = plan.metadata.segments.iter().all(|segment| {
                segment
                    .field_chunks
                    .iter()
                    .find(|(name, _)| name.eq_ignore_ascii_case(field))
                    .is_some_and(|(_, chunk)| {
                        matches!(chunk.logical_type.as_str(), "int64" | "float64")
                    })
            });
            if !numeric {
                return Ok(ColumnBatchAggregateDecision::Fallback(
                    ColumnBatchScanFallbackReason::FieldCoverageMismatch,
                ));
            }
        }
        let data_tx = self.begin_data_readonly_tx_for(&collection)?;
        let wanted = fields
            .iter()
            .map(|field| field.to_ascii_lowercase())
            .collect::<BTreeSet<_>>();
        let mut accumulators = specs
            .iter()
            .map(DirectAggregateAccumulator::new)
            .collect::<Vec<_>>();
        let mut segments_read = 0usize;
        let mut selected_rows = 0usize;
        for segment in &plan.metadata.segments {
            check_column_batch_controls(self, controls)?;
            if !column_batch_segment_may_match(segment, filter) {
                continue;
            }
            let loaded =
                match scan_aggregate_segment(&data_tx, &plan.index, segment, &wanted, filter)? {
                    Ok(loaded) => loaded,
                    Err(reason) => return Ok(ColumnBatchAggregateDecision::Fallback(reason)),
                };
            segments_read = segments_read.saturating_add(1);
            selected_rows = selected_rows.saturating_add(loaded.selected_rows);
            for (accumulator, spec) in accumulators.iter_mut().zip(specs) {
                let Some(field) = spec.field.as_ref() else {
                    accumulator.update_count(loaded.selected_rows)?;
                    continue;
                };
                let Some((_, values)) = loaded
                    .values
                    .iter()
                    .find(|(name, _)| name.eq_ignore_ascii_case(field))
                else {
                    return Ok(ColumnBatchAggregateDecision::Fallback(
                        ColumnBatchScanFallbackReason::FieldCoverageMismatch,
                    ));
                };
                accumulator.update_values(spec, values)?;
            }
        }
        let values = specs
            .iter()
            .zip(accumulators)
            .map(|(spec, accumulator)| Ok((spec.output_name.clone(), accumulator.finish())))
            .collect::<Result<Vec<_>, CassieError>>()?;
        Ok(ColumnBatchAggregateDecision::Hit(
            ColumnBatchAggregateOutcome {
                values,
                segments_read,
                selected_rows,
            },
        ))
    }

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
        self.rebuild_column_batches_for_index_with_policy(index, false)
    }

    #[doc(hidden)]
    pub fn rebuild_column_batches_plain_for_benchmark(
        &self,
        collection: &str,
        index_name: &str,
    ) -> Result<ColumnBatchMetadata, CassieError> {
        let index = self
            .get_index(collection, index_name)?
            .ok_or_else(|| CassieError::Execution(format!("index not found: {index_name}")))?;
        self.rebuild_column_batches_for_index_with_policy(&index, true)
    }

    fn rebuild_column_batches_for_index_with_policy(
        &self,
        index: &IndexMeta,
        force_plain: bool,
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
        let mut encoded_segments = Vec::new();
        for (position, chunk) in documents.chunks(segment_size).enumerate() {
            let segment_id = u64::try_from(position)
                .map_err(|_| CassieError::ResourceLimit("too many column segments".to_string()))?;
            let rows = storage_v2::rows_for_segment(chunk, fields.as_slice());
            let next_position = position
                .checked_add(1)
                .and_then(|position| position.checked_mul(segment_size))
                .ok_or_else(|| {
                    CassieError::ResourceLimit("column segment range overflow".to_string())
                })?;
            let row_id_end = documents
                .get(next_position)
                .map(|document| document.id.clone());
            let encoded = if force_plain {
                storage_v2::encode_plain_segment_for_benchmark(
                    segment_id,
                    1,
                    rows.as_slice(),
                    fields.as_slice(),
                    &row_schema,
                    row_id_end,
                )?
            } else {
                storage_v2::encode_segment(
                    segment_id,
                    1,
                    rows.as_slice(),
                    fields.as_slice(),
                    &row_schema,
                    row_id_end,
                )?
            };
            encoded_segments.push(encoded);
        }

        let segments = encoded_segments
            .iter()
            .map(|segment| segment.metadata.clone())
            .collect::<Vec<_>>();
        let next_segment_id = u64::try_from(segments.len())
            .map_err(|_| CassieError::ResourceLimit("too many column segments".to_string()))?;
        let metadata = ColumnBatchMetadata {
            metadata_format_version: CURRENT_COLUMN_BATCH_METADATA_FORMAT_VERSION,
            summary_format_version: CURRENT_COLUMN_BATCH_SUMMARY_FORMAT_VERSION,
            manifest_revision: 1,
            next_segment_id,
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
        for segment in &encoded_segments {
            storage_v2::write_segment(&mut data_tx, relation_id, index_id, segment)?;
        }
        data_tx
            .put(
                Self::column_batch_metadata_key(relation_id, index_id),
                encode_manifest(&metadata)?,
                None,
            )
            .map_err(CassieError::from)?;
        data_tx
            .commit(self.write_options_sync())
            .map_err(CassieError::from)?;
        self.record_column_batch_build(encoded_segments.as_slice(), true, 0, source_row_count);

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
        decode_manifest(&raw).map(Some)
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
            .commit(self.write_options_sync())
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
                    controls.reserve_query_memory(segment_requested_bytes(segment, &plan.wanted))
                })
                .transpose()?;
            let loaded = match scan_segment(
                &data_tx,
                &plan.index,
                segment,
                &plan.wanted,
                request.segment_filter,
            )? {
                Ok(loaded) => loaded,
                Err(reason) => return Ok(ColumnBatchScanDecision::Fallback(reason)),
            };
            state.record_segment(&loaded);
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

    fn record_column_batch_build(
        &self,
        segments: &[storage_v2::EncodedSegment],
        full_rebuild: bool,
        splits: usize,
        source_rows: usize,
    ) {
        let mut metrics = self.column_batch_operational_metrics.lock();
        if full_rebuild {
            metrics.full_rebuilds = metrics.full_rebuilds.saturating_add(1);
        } else {
            metrics.segment_rewrites = metrics
                .segment_rewrites
                .saturating_add(u64::try_from(segments.len()).unwrap_or(u64::MAX));
            metrics.segment_splits = metrics
                .segment_splits
                .saturating_add(u64::try_from(splits).unwrap_or(u64::MAX));
            let bucket = match segments.len() {
                0 | 1 => 0,
                2 => 1,
                _ => 2,
            };
            metrics.write_amplification_buckets[bucket] =
                metrics.write_amplification_buckets[bucket].saturating_add(1);
        }
        let bytes = segments.iter().fold(0usize, |total, segment| {
            segment.fields.values().fold(
                total.saturating_add(segment.row_ids.len()),
                |total, field| total.saturating_add(field.len()),
            )
        });
        metrics.maintenance_bytes = metrics
            .maintenance_bytes
            .saturating_add(u64::try_from(bytes).unwrap_or(u64::MAX));
        metrics.maintenance_source_rows = metrics
            .maintenance_source_rows
            .saturating_add(u64::try_from(source_rows).unwrap_or(u64::MAX));
        for segment in segments {
            for chunk in segment.metadata.field_chunks.values() {
                let choices = metrics
                    .codec_choices
                    .entry(chunk.codec_name.clone())
                    .or_default();
                *choices = choices.saturating_add(1);
            }
        }
    }

    fn record_empty_column_batch_maintenance(&self) {
        let mut metrics = self.column_batch_operational_metrics.lock();
        metrics.segment_rewrites = metrics.segment_rewrites.saturating_add(1);
        metrics.write_amplification_buckets[0] =
            metrics.write_amplification_buckets[0].saturating_add(1);
    }

    fn record_column_batch_orphan_cleanup(&self, removed: usize) {
        let mut metrics = self.column_batch_operational_metrics.lock();
        metrics.orphan_revisions_cleaned = metrics
            .orphan_revisions_cleaned
            .saturating_add(u64::try_from(removed).unwrap_or(u64::MAX));
    }

    pub(crate) fn column_batch_operational_metrics(&self) -> serde_json::Value {
        let metrics = self.column_batch_operational_metrics.lock();
        serde_json::json!({
            "codec_choices": metrics.codec_choices,
            "full_rebuilds": metrics.full_rebuilds,
            "segment_rewrites": metrics.segment_rewrites,
            "segment_splits": metrics.segment_splits,
            "compactions": metrics.compactions,
            "maintenance_bytes": metrics.maintenance_bytes,
            "maintenance_source_rows": metrics.maintenance_source_rows,
            "orphan_revisions_cleaned": metrics.orphan_revisions_cleaned,
            "write_amplification_buckets": {
                "one_segment": metrics.write_amplification_buckets[0],
                "two_segments": metrics.write_amplification_buckets[1],
                "more_than_two_segments": metrics.write_amplification_buckets[2],
            },
        })
    }

    fn record_column_batch_compaction(&self) {
        let mut metrics = self.column_batch_operational_metrics.lock();
        metrics.compactions = metrics.compactions.saturating_add(1);
    }
}

fn wanted_column_batch_fields(fields: &[String]) -> BTreeSet<String> {
    fields
        .iter()
        .filter(|field| !field.eq_ignore_ascii_case("id") && !field.eq_ignore_ascii_case("_id"))
        .map(|field| field.to_ascii_lowercase())
        .collect()
}

fn segment_requested_bytes(segment: &ColumnBatchSegmentMeta, wanted: &BTreeSet<String>) -> usize {
    wanted.iter().fold(
        segment.row_ids.decoded_len.max(segment.row_ids.encoded_len),
        |bytes, wanted_field| {
            let retained = segment
                .field_chunks
                .iter()
                .find(|(field, _)| field.eq_ignore_ascii_case(wanted_field))
                .map_or(0, |(_, chunk)| chunk.decoded_len.max(chunk.encoded_len));
            bytes.saturating_add(retained)
        },
    )
}

fn load_column_batch_segment(
    data_tx: &cntryl_midge::Transaction,
    index: &IndexMeta,
    segment: &ColumnBatchSegmentMeta,
) -> Result<Result<LoadedSegment, ColumnBatchScanFallbackReason>, CassieError> {
    let wanted = storage_v2::all_segment_fields(segment);
    load_segment(data_tx, index, segment, &wanted)
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
