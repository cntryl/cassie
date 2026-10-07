//! Private immutable CBC2 input owners. Existing validators decide eligibility and framing.
mod numeric;

use std::collections::BTreeSet;
use std::mem::size_of;
use std::sync::Arc;

use crate::executor::retained_memory::{add, mul};
use crate::runtime::accounted::Accounted;
use crate::runtime::column_batch_metrics::ColumnBatchScanMetrics;

use super::{
    check_column_batch_controls, CassieError, ColumnBatchMetadata, ColumnBatchScanFallbackReason,
    ColumnBatchScanFilters, Midge, PreparedColumnBatchScan, QueryExecutionControls,
    QueryMemoryReservation,
};

#[derive(Debug)]
pub(crate) struct ValidatedEncodedField {
    name: String,
    raw: bytes::Bytes,
    values: Vec<serde_json::Value>,
    numeric: Option<super::super::NumericValues>,
    metadata: Arc<Accounted<ColumnBatchMetadata>>,
    segment: usize,
}

impl ValidatedEncodedField {
    pub(crate) fn name(&self) -> &str {
        &self.name
    }
    pub(crate) fn raw(&self) -> &[u8] {
        &self.raw
    }
    pub(crate) fn values(&self) -> &[serde_json::Value] {
        &self.values
    }
    pub(crate) fn numeric_values(&self) -> Option<&super::super::NumericValues> {
        self.numeric.as_ref()
    }
    pub(crate) fn storage_type(&self) -> &str {
        &self.metadata.get().segments[self.segment].field_chunks[&self.name].logical_type
    }
}

pub(crate) struct EncodedSegment {
    pub(crate) row_ids: Vec<String>,
    pub(crate) fields: Vec<Arc<Accounted<ValidatedEncodedField>>>,
    _memory: QueryMemoryReservation,
}

pub(crate) struct EncodedSource {
    pub(crate) segments: Vec<EncodedSegment>,
    pub(crate) metrics: ColumnBatchScanMetrics,
    _memory: QueryMemoryReservation,
}

pub(crate) enum EncodedSourceDecision {
    Hit(EncodedSource),
    Fallback(ColumnBatchScanFallbackReason),
    TransportBudget,
}

impl Midge {
    pub(crate) fn controlled_encoded_source(
        &self,
        collection: &str,
        fields: &[String],
        controls: &QueryExecutionControls,
    ) -> Result<EncodedSourceDecision, CassieError> {
        check_column_batch_controls(self, controls)?;
        let collection = self.canonical_collection_name(collection);
        let plan = match self.prepare_column_batch_scan(
            &collection,
            1024,
            fields,
            None,
            ColumnBatchScanFilters {
                row: None,
                segment: None,
            },
            Some(controls),
        ) {
            Ok(PreparedColumnBatchScan::Ready(plan)) => *plan,
            Ok(PreparedColumnBatchScan::Fallback(reason)) => {
                return Ok(EncodedSourceDecision::Fallback(reason))
            }
            // Optional encoded metadata must fit before opening field data. The
            // existing controlled row reader does not retain this metadata.
            Err(CassieError::ResourceLimit(_)) => {
                return Ok(EncodedSourceDecision::TransportBudget)
            }
            Err(error) => return Err(error),
        };
        let mut metadata_memory = plan.query_memory.ok_or_else(|| {
            CassieError::ResourceLimit("encoded source metadata has no admission".to_owned())
        })?;
        let metadata_overhead = add(
            size_of::<Accounted<ColumnBatchMetadata>>(),
            2 * size_of::<usize>(),
        )?;
        let Some(source_bytes) =
            source_retained_bytes(self, &plan.metadata, &plan.wanted, controls)?
        else {
            return Ok(EncodedSourceDecision::Fallback(
                ColumnBatchScanFallbackReason::FieldCoverageMismatch,
            ));
        };
        let available = controls
            .query_memory_budget_bytes
            .saturating_sub(controls.current_query_memory_bytes());
        if source_bytes
            .checked_add(metadata_overhead)
            .is_none_or(|bytes| bytes > available)
        {
            return Ok(EncodedSourceDecision::TransportBudget);
        }
        match metadata_memory.try_grow(metadata_overhead) {
            Ok(()) => {}
            Err(CassieError::ResourceLimit(_)) => {
                return Ok(EncodedSourceDecision::TransportBudget)
            }
            Err(error) => return Err(error),
        }
        let metadata = Arc::new(Accounted::from_admitted(plan.metadata, metadata_memory));
        self.load_encoded_source(&collection, &plan.index, &plan.wanted, &metadata, controls)
    }

    fn load_encoded_source(
        &self,
        collection: &str,
        index: &crate::catalog::IndexMeta,
        wanted: &BTreeSet<String>,
        metadata: &Arc<Accounted<ColumnBatchMetadata>>,
        controls: &QueryExecutionControls,
    ) -> Result<EncodedSourceDecision, CassieError> {
        let memory = controls.reserve_query_memory(mul(
            metadata.get().segments.len(),
            size_of::<EncodedSegment>(),
        )?)?;
        let mut segments = Vec::with_capacity(metadata.get().segments.len());
        let (relation_id, index_id) = Self::column_batch_storage_ids(index)?;
        let tx = self.begin_data_readonly_tx_for(collection)?;
        let mut metrics = ColumnBatchScanMetrics::default();
        for (segment_index, segment) in metadata.get().segments.iter().enumerate() {
            check_column_batch_controls(self, controls)?;
            let row_bytes = row_retained_bytes(segment)?;
            let row_memory = controls.reserve_query_memory(add(
                row_bytes,
                mul(
                    wanted.len(),
                    size_of::<Arc<Accounted<ValidatedEncodedField>>>(),
                )?,
            )?)?;
            let row_ids =
                match super::storage_v2::load_row_ids(&tx, relation_id, index_id, segment)? {
                    Ok(row_ids) => row_ids,
                    Err(reason) => return Ok(EncodedSourceDecision::Fallback(reason)),
                };
            let mut output_fields = Vec::with_capacity(wanted.len());
            for wanted in wanted {
                check_column_batch_controls(self, controls)?;
                let Some((field, chunk)) = segment.field_chunks.iter().find(|(field, _)| {
                    crate::sql::ColumnIdentifierPath::matches_stored_field(wanted, field)
                }) else {
                    return Ok(EncodedSourceDecision::Fallback(
                        ColumnBatchScanFallbackReason::FieldCoverageMismatch,
                    ));
                };
                let bytes = field_retained_bytes(chunk, segment.row_count, field)?;
                let field_memory = controls.reserve_query_memory(bytes)?;
                let loaded = match numeric::load(&tx, relation_id, index_id, segment, field, chunk)?
                {
                    Ok(loaded) => loaded,
                    Err(reason) => return Ok(EncodedSourceDecision::Fallback(reason)),
                };
                metrics.chunks_read += 1;
                metrics.physical_bytes += loaded.raw.len();
                metrics.logical_bytes += chunk.decoded_len;
                metrics.materialized_values += loaded.values.len();
                output_fields.push(Arc::new(Accounted::from_admitted(
                    ValidatedEncodedField {
                        name: field.clone(),
                        raw: loaded.raw,
                        values: loaded.values,
                        numeric: loaded.numeric,
                        metadata: Arc::clone(metadata),
                        segment: segment_index,
                    },
                    field_memory,
                )));
            }
            record_segment(&mut metrics, segment, row_ids.len());
            segments.push(EncodedSegment {
                row_ids,
                fields: output_fields,
                _memory: row_memory,
            });
        }
        check_column_batch_controls(self, controls)?;
        Ok(EncodedSourceDecision::Hit(EncodedSource {
            segments,
            metrics,
            _memory: memory,
        }))
    }
}

fn source_retained_bytes(
    store: &Midge,
    metadata: &ColumnBatchMetadata,
    wanted: &BTreeSet<String>,
    controls: &QueryExecutionControls,
) -> Result<Option<usize>, CassieError> {
    let mut bytes = mul(metadata.segments.len(), size_of::<EncodedSegment>())?;
    for segment in &metadata.segments {
        check_column_batch_controls(store, controls)?;
        bytes = add(bytes, row_retained_bytes(segment)?)?;
        bytes = add(
            bytes,
            mul(
                wanted.len(),
                size_of::<Arc<Accounted<ValidatedEncodedField>>>(),
            )?,
        )?;
        for name in wanted {
            let Some((field, chunk)) = segment.field_chunks.iter().find(|(field, _)| {
                crate::sql::ColumnIdentifierPath::matches_stored_field(name, field)
            }) else {
                return Ok(None);
            };
            bytes = add(
                bytes,
                field_retained_bytes(chunk, segment.row_count, field)?,
            )?;
        }
    }
    Ok(Some(bytes))
}

fn field_retained_bytes(
    chunk: &crate::catalog::ColumnBatchChunkMeta,
    row_count: usize,
    field: &str,
) -> Result<usize, CassieError> {
    add(
        add(chunk.encoded_len, mul(chunk.decoded_len, 64)?)?,
        add(
            mul(row_count, size_of::<serde_json::Value>().max(64))?,
            add(
                field.len(),
                size_of::<Accounted<ValidatedEncodedField>>() + 2 * size_of::<usize>(),
            )?,
        )?,
    )
}

fn row_retained_bytes(
    segment: &crate::catalog::ColumnBatchSegmentMeta,
) -> Result<usize, CassieError> {
    add(
        mul(segment.row_count, size_of::<String>())?,
        mul(
            segment.row_ids.decoded_len.max(segment.row_ids.encoded_len),
            8,
        )?,
    )
}

fn record_segment(
    metrics: &mut ColumnBatchScanMetrics,
    segment: &crate::catalog::ColumnBatchSegmentMeta,
    rows: usize,
) {
    metrics.chunks_read += 1;
    metrics.segments_read += 1;
    metrics.rows += rows;
    metrics.candidate_rows += rows;
    metrics.selected_rows += rows;
    metrics.physical_bytes += segment.row_ids.encoded_len;
    metrics.logical_bytes += segment.row_ids.decoded_len;
}
