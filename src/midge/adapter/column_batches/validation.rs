use std::collections::{BTreeSet, HashSet};

use crate::catalog::{ColumnBatchMetadata, ColumnBatchRow, IndexKind, IndexMeta};
use crate::midge::adapter::ColumnBatchSummaryDecision;
use crate::runtime::{QueryExecutionControls, QueryMemoryReservation};

use super::{
    collect_scan, column_batch_summaries, column_index_segment_size, column_values,
    load_column_batch_segment, summary_checksum, CassieError, ColumnBatchScanFallbackReason, Midge,
    Query, CURRENT_COLUMN_BATCH_METADATA_FORMAT_VERSION,
    CURRENT_COLUMN_BATCH_SUMMARY_FORMAT_VERSION,
};

#[derive(Clone, Copy, PartialEq, Eq)]
enum ValidationDepth {
    Metadata,
    Summaries,
    Source,
}

struct SegmentValidationOptions<'a> {
    depth: ValidationDepth,
    current_generation: u64,
    controls: Option<&'a QueryExecutionControls>,
}

pub(crate) struct ControlledColumnBatchMetadata {
    pub(crate) metadata: Box<ColumnBatchMetadata>,
    pub(crate) memory: QueryMemoryReservation,
}

pub(crate) enum ControlledColumnBatchSummaryDecision {
    Ready(ControlledColumnBatchMetadata),
    Fallback(crate::midge::adapter::ColumnBatchScanFallbackReason),
}

type ValidationResult<T> = Result<T, ColumnBatchScanFallbackReason>;

impl Midge {
    /// Validates the latest column-batch format and every persisted segment before summaries are
    /// used to answer a query.
    ///
    /// # Errors
    ///
    /// Returns an error when the durable storage state cannot be read.
    pub fn prepare_column_batch_summaries(
        &self,
        collection: &str,
        index: &IndexMeta,
        requested_fields: &[String],
    ) -> Result<ColumnBatchSummaryDecision, CassieError> {
        self.validate_column_batch_index(
            collection,
            index,
            requested_fields,
            ValidationDepth::Summaries,
        )
    }

    pub(crate) fn prepare_column_batch_summaries_controlled(
        &self,
        collection: &str,
        index: &IndexMeta,
        requested_fields: &[String],
        controls: &QueryExecutionControls,
    ) -> Result<ControlledColumnBatchSummaryDecision, CassieError> {
        self.validate_column_batch_index_controlled(
            collection,
            index,
            requested_fields,
            ValidationDepth::Summaries,
            controls,
        )
    }

    pub(super) fn prepare_column_batch_scan_metadata(
        &self,
        collection: &str,
        index: &IndexMeta,
        requested_fields: &[String],
    ) -> Result<ColumnBatchSummaryDecision, CassieError> {
        self.validate_column_batch_index(
            collection,
            index,
            requested_fields,
            ValidationDepth::Metadata,
        )
    }

    pub(super) fn prepare_column_batch_scan_metadata_controlled(
        &self,
        collection: &str,
        index: &IndexMeta,
        requested_fields: &[String],
        controls: &QueryExecutionControls,
    ) -> Result<ControlledColumnBatchSummaryDecision, CassieError> {
        self.validate_column_batch_index_controlled(
            collection,
            index,
            requested_fields,
            ValidationDepth::Metadata,
            controls,
        )
    }

    pub(crate) fn reconcile_column_batch_indexes(&self) -> Result<(), CassieError> {
        let indexes = self
            .list_indexes()?
            .into_iter()
            .filter(|index| index.kind == IndexKind::Column)
            .collect::<Vec<_>>();
        for index in indexes {
            let collection = self.canonical_collection_name(&index.collection);
            let write_gate = self.collection_write_gate(&collection);
            let _write_guard = write_gate.lock();
            if self.has_column_batch_maintenance_debt(&collection)? {
                let generation = self.collection_generation(&collection)?;
                self.complete_column_batch_maintenance(&collection, generation)?;
            }
            let fields = index.normalized_fields();
            let valid = matches!(
                self.validate_column_batch_index(
                    &collection,
                    &index,
                    &fields,
                    ValidationDepth::Source,
                )?,
                ColumnBatchSummaryDecision::Ready(_)
            );
            if !valid {
                self.rebuild_column_batches_for_index(&index)?;
                if let ColumnBatchSummaryDecision::Fallback(reason) = self
                    .validate_column_batch_index(
                        &collection,
                        &index,
                        &fields,
                        ValidationDepth::Source,
                    )?
                {
                    return Err(CassieError::Parse(format!(
                        "column batch reconciliation failed: {}",
                        reason.as_str()
                    )));
                }
            }
        }
        Ok(())
    }

    fn validate_column_batch_index(
        &self,
        collection: &str,
        index: &IndexMeta,
        requested_fields: &[String],
        depth: ValidationDepth,
    ) -> Result<ColumnBatchSummaryDecision, CassieError> {
        let collection = self.canonical_collection_name(collection);
        if self.has_column_batch_maintenance_debt(&collection)? {
            return Ok(fallback(ColumnBatchScanFallbackReason::MaintenancePending));
        }
        let Some(index) = self.get_index(&collection, &index.name)? else {
            return Ok(fallback(ColumnBatchScanFallbackReason::NoCoveringIndex));
        };
        if index.kind != IndexKind::Column {
            return Ok(fallback(ColumnBatchScanFallbackReason::NoCoveringIndex));
        }
        let (relation_id, index_id) = Self::column_batch_storage_ids(&index)?;
        let tx = self.begin_data_readonly_tx_for(&collection)?;
        let metadata_key = Self::column_batch_metadata_key(relation_id, index_id);
        let Some(raw) = tx.get(&metadata_key).map_err(CassieError::from)? else {
            return Ok(fallback(ColumnBatchScanFallbackReason::MissingMetadata));
        };
        let metadata = match decode_latest_column_batch_metadata(&raw) {
            Ok(metadata) => metadata,
            Err(reason) => return Ok(fallback(reason)),
        };
        let current_generation = match self.validate_column_batch_contract(
            &collection,
            &index,
            requested_fields,
            &metadata,
        )? {
            Ok(generation) => generation,
            Err(reason) => return Ok(fallback(reason)),
        };
        if let Err(reason) =
            Self::validate_column_batch_keys(&tx, &metadata, relation_id, index_id)?
        {
            return Ok(fallback(reason));
        }
        let persisted_rows = match self.validate_column_batch_segments(
            &tx,
            &collection,
            &index,
            &metadata,
            &SegmentValidationOptions {
                depth,
                current_generation,
                controls: None,
            },
        )? {
            Ok(rows) => rows,
            Err(reason) => return Ok(fallback(reason)),
        };
        if depth == ValidationDepth::Source
            && !self.column_batch_source_matches(&collection, &metadata, &persisted_rows)?
        {
            return Ok(fallback(
                ColumnBatchScanFallbackReason::SourceRowCountMismatch,
            ));
        }
        if self.collection_generation(&collection)? != current_generation {
            return Ok(fallback(ColumnBatchScanFallbackReason::GenerationMismatch));
        }
        Ok(ColumnBatchSummaryDecision::Ready(Box::new(metadata)))
    }

    fn validate_column_batch_index_controlled(
        &self,
        collection: &str,
        index: &IndexMeta,
        requested_fields: &[String],
        depth: ValidationDepth,
        controls: &QueryExecutionControls,
    ) -> Result<ControlledColumnBatchSummaryDecision, CassieError> {
        check_controls(controls)?;
        let collection = self.canonical_collection_name(collection);
        if self.has_column_batch_maintenance_debt(&collection)? {
            return Ok(controlled_fallback(
                ColumnBatchScanFallbackReason::MaintenancePending,
            ));
        }
        let Some(index) = self.get_index(&collection, &index.name)? else {
            return Ok(controlled_fallback(
                ColumnBatchScanFallbackReason::NoCoveringIndex,
            ));
        };
        if index.kind != IndexKind::Column {
            return Ok(controlled_fallback(
                ColumnBatchScanFallbackReason::NoCoveringIndex,
            ));
        }
        let (relation_id, index_id) = Self::column_batch_storage_ids(&index)?;
        let tx = self.begin_data_readonly_tx_for(&collection)?;
        let metadata_key = Self::column_batch_metadata_key(relation_id, index_id);
        let Some(raw) = tx.get(&metadata_key).map_err(CassieError::from)? else {
            return Ok(controlled_fallback(
                ColumnBatchScanFallbackReason::MissingMetadata,
            ));
        };
        check_controls(controls)?;
        let retained_bytes = controlled_metadata_retained_bytes(&metadata_key, &raw)?;
        let memory = controls.reserve_query_memory(retained_bytes)?;
        let metadata = match decode_latest_column_batch_metadata(&raw) {
            Ok(metadata) => metadata,
            Err(reason) => return Ok(controlled_fallback(reason)),
        };
        let current_generation = match self.validate_column_batch_contract(
            &collection,
            &index,
            requested_fields,
            &metadata,
        )? {
            Ok(generation) => generation,
            Err(reason) => return Ok(controlled_fallback(reason)),
        };
        let persisted_rows = match self.validate_column_batch_segments(
            &tx,
            &collection,
            &index,
            &metadata,
            &SegmentValidationOptions {
                depth,
                current_generation,
                controls: Some(controls),
            },
        )? {
            Ok(rows) => rows,
            Err(reason) => return Ok(controlled_fallback(reason)),
        };
        debug_assert!(persisted_rows.is_empty());
        if self.collection_generation(&collection)? != current_generation {
            return Ok(controlled_fallback(
                ColumnBatchScanFallbackReason::GenerationMismatch,
            ));
        }
        Ok(ControlledColumnBatchSummaryDecision::Ready(
            ControlledColumnBatchMetadata {
                metadata: Box::new(metadata),
                memory,
            },
        ))
    }

    fn validate_column_batch_contract(
        &self,
        collection: &str,
        index: &IndexMeta,
        requested_fields: &[String],
        metadata: &ColumnBatchMetadata,
    ) -> Result<ValidationResult<u64>, CassieError> {
        if metadata.collection != collection || metadata.index_name != index.name {
            return Ok(Err(ColumnBatchScanFallbackReason::InvalidMetadata));
        }
        let current_generation = self.collection_generation(collection)?;
        if metadata.built_generation != current_generation {
            return Ok(Err(ColumnBatchScanFallbackReason::GenerationMismatch));
        }
        if metadata.schema_version != self.row_schema(collection)?.schema_version {
            return Ok(Err(ColumnBatchScanFallbackReason::SchemaVersionMismatch));
        }
        let index_fields = normalized_fields(index.normalized_fields().as_slice());
        let metadata_fields = normalized_fields(metadata.fields.as_slice());
        if metadata_fields != index_fields {
            return Ok(Err(ColumnBatchScanFallbackReason::FieldCoverageMismatch));
        }
        if metadata.segment_size != column_index_segment_size(index)? {
            return Ok(Err(ColumnBatchScanFallbackReason::SegmentSizeMismatch));
        }
        let requested = normalized_fields(requested_fields);
        if !requested.is_subset(&metadata_fields) {
            return Ok(Err(ColumnBatchScanFallbackReason::FieldCoverageMismatch));
        }
        if let Some(reason) = invalid_segment_manifest_reason(metadata) {
            return Ok(Err(reason));
        }
        Ok(Ok(current_generation))
    }

    fn validate_column_batch_keys(
        tx: &cntryl_midge::Transaction,
        metadata: &ColumnBatchMetadata,
        relation_id: u64,
        index_id: u64,
    ) -> Result<ValidationResult<()>, CassieError> {
        let prefix = Self::column_batch_index_prefix(relation_id, index_id);
        let entries = collect_scan(
            tx.scan(&Query::new().prefix(prefix.into()))
                .map_err(CassieError::from)?,
        )?;
        let expected_keys = expected_column_batch_keys(metadata, relation_id, index_id);
        let actual_keys = entries
            .iter()
            .map(|(key, _)| key.clone())
            .collect::<HashSet<_>>();
        if expected_keys.iter().any(|key| !actual_keys.contains(key)) {
            return Ok(Err(ColumnBatchScanFallbackReason::SegmentMissing));
        }
        if entries.len() != expected_keys.len()
            || entries.iter().any(|(key, _)| !expected_keys.contains(key))
        {
            return Ok(Err(ColumnBatchScanFallbackReason::SegmentManifestMismatch));
        }
        Ok(Ok(()))
    }

    fn validate_column_batch_segments(
        &self,
        tx: &cntryl_midge::Transaction,
        collection: &str,
        index: &IndexMeta,
        metadata: &ColumnBatchMetadata,
        options: &SegmentValidationOptions<'_>,
    ) -> Result<ValidationResult<Vec<ColumnBatchRow>>, CassieError> {
        let row_schema = self.row_schema(collection)?;
        let capacity = if options.depth == ValidationDepth::Source {
            metadata.source_row_count
        } else {
            0
        };
        let mut persisted_rows = Vec::with_capacity(capacity);
        let mut previous_end: Option<&str> = None;
        for segment in &metadata.segments {
            if previous_end.is_some_and(|previous| {
                segment
                    .row_id_start
                    .as_deref()
                    .is_none_or(|current| previous >= current)
            }) {
                return Ok(Err(ColumnBatchScanFallbackReason::SegmentManifestMismatch));
            }
            previous_end = segment.row_id_end.as_deref();
            if metadata.fields.iter().any(|field| {
                !segment
                    .summaries
                    .keys()
                    .any(|stored| stored.eq_ignore_ascii_case(field))
            }) {
                return Ok(Err(ColumnBatchScanFallbackReason::SummaryMissing));
            }
            if summary_checksum(segment.row_count, &segment.summaries)? != segment.summary_checksum
            {
                return Ok(Err(ColumnBatchScanFallbackReason::SummaryChecksumMismatch));
            }
            if options.depth == ValidationDepth::Metadata {
                continue;
            }
            let _segment_memory = if let Some(controls) = options.controls {
                super::check_column_batch_controls(self, controls)?;
                Some(controls.reserve_query_memory(controlled_segment_retained_bytes(segment)?)?)
            } else {
                None
            };
            let loaded = match load_column_batch_segment(tx, index, segment)? {
                Ok(loaded) => loaded,
                Err(reason) => return Ok(Err(reason)),
            };
            if !valid_segment_rows(segment, loaded.rows.as_slice()) {
                return Ok(Err(ColumnBatchScanFallbackReason::SegmentManifestMismatch));
            }
            let recomputed = column_batch_summaries(&loaded.rows, &metadata.fields, &row_schema);
            if recomputed != segment.summaries {
                return Ok(Err(ColumnBatchScanFallbackReason::SummaryChecksumMismatch));
            }
            if options.depth == ValidationDepth::Source {
                persisted_rows.extend(loaded.rows);
            }
            if self.collection_generation(collection)? != options.current_generation {
                return Ok(Err(ColumnBatchScanFallbackReason::GenerationMismatch));
            }
        }
        Ok(Ok(persisted_rows))
    }

    fn column_batch_source_matches(
        &self,
        collection: &str,
        metadata: &ColumnBatchMetadata,
        persisted_rows: &[ColumnBatchRow],
    ) -> Result<bool, CassieError> {
        let mut documents = self.scan_documents(collection)?;
        documents.sort_by(|left, right| left.id.cmp(&right.id));
        if documents.len() != metadata.source_row_count || documents.len() != persisted_rows.len() {
            return Ok(false);
        }
        Ok(documents.iter().zip(persisted_rows).all(|(document, row)| {
            document.id == row.row_id
                && column_values(&document.payload, &metadata.fields) == row.values
        }))
    }
}

fn decode_latest_column_batch_metadata(raw: &[u8]) -> ValidationResult<ColumnBatchMetadata> {
    let document = serde_json::from_slice::<serde_json::Value>(raw)
        .map_err(|_| ColumnBatchScanFallbackReason::InvalidMetadata)?;
    if version(&document, "metadata_format_version")
        != Some(CURRENT_COLUMN_BATCH_METADATA_FORMAT_VERSION)
    {
        return Err(ColumnBatchScanFallbackReason::MetadataFormatMismatch);
    }
    if version(&document, "summary_format_version")
        != Some(CURRENT_COLUMN_BATCH_SUMMARY_FORMAT_VERSION)
    {
        return Err(ColumnBatchScanFallbackReason::SummaryFormatMismatch);
    }
    serde_json::from_slice(raw).map_err(|_| ColumnBatchScanFallbackReason::InvalidMetadata)
}

fn fallback(reason: ColumnBatchScanFallbackReason) -> ColumnBatchSummaryDecision {
    ColumnBatchSummaryDecision::Fallback(reason)
}

fn controlled_fallback(
    reason: ColumnBatchScanFallbackReason,
) -> ControlledColumnBatchSummaryDecision {
    ControlledColumnBatchSummaryDecision::Fallback(reason)
}

fn check_controls(controls: &QueryExecutionControls) -> Result<(), CassieError> {
    if controls.is_cancelled() {
        return Err(CassieError::QueryCancelled);
    }
    if controls.is_timed_out() {
        return Err(CassieError::DeadlineExceeded);
    }
    Ok(())
}

fn controlled_metadata_retained_bytes(key: &[u8], raw: &[u8]) -> Result<usize, CassieError> {
    raw.len()
        .checked_mul(8)
        .and_then(|bytes| bytes.checked_add(key.len()))
        .and_then(|bytes| bytes.checked_add(std::mem::size_of::<ColumnBatchMetadata>()))
        .ok_or_else(|| CassieError::ResourceLimit("column metadata size overflow".to_string()))
}

fn controlled_segment_retained_bytes(
    segment: &crate::catalog::ColumnBatchSegmentMeta,
) -> Result<usize, CassieError> {
    segment
        .codec
        .uncompressed_len
        .checked_mul(4)
        .and_then(|bytes| bytes.checked_add(segment.codec.compressed_len))
        .ok_or_else(|| CassieError::ResourceLimit("column segment size overflow".to_string()))
}

fn version(document: &serde_json::Value, field: &str) -> Option<u32> {
    document
        .get(field)?
        .as_u64()
        .and_then(|value| u32::try_from(value).ok())
}

fn normalized_fields(fields: &[String]) -> BTreeSet<String> {
    fields
        .iter()
        .map(|field| field.to_ascii_lowercase())
        .collect()
}

fn invalid_segment_manifest_reason(
    metadata: &ColumnBatchMetadata,
) -> Option<ColumnBatchScanFallbackReason> {
    let Some(manifest_row_count) = metadata.segments.iter().try_fold(0usize, |total, segment| {
        total.checked_add(segment.row_count)
    }) else {
        return Some(ColumnBatchScanFallbackReason::SourceRowCountMismatch);
    };
    if manifest_row_count != metadata.source_row_count {
        return Some(ColumnBatchScanFallbackReason::SourceRowCountMismatch);
    }
    let expected_segments = metadata.source_row_count.div_ceil(metadata.segment_size);
    if metadata.segments.len() != expected_segments {
        return Some(ColumnBatchScanFallbackReason::SegmentManifestMismatch);
    }
    let structurally_valid = metadata
        .segments
        .iter()
        .enumerate()
        .all(|(position, segment)| {
            segment.segment_id == position as u64
                && segment.row_count > 0
                && segment.row_count <= metadata.segment_size
                && segment.row_id_start.is_some()
                && segment.row_id_end.is_some()
                && (position + 1 == metadata.segments.len()
                    || segment.row_count == metadata.segment_size)
        });
    if structurally_valid {
        None
    } else {
        Some(ColumnBatchScanFallbackReason::SegmentManifestMismatch)
    }
}

fn expected_column_batch_keys(
    metadata: &ColumnBatchMetadata,
    relation_id: u64,
    index_id: u64,
) -> HashSet<Vec<u8>> {
    let mut keys = HashSet::with_capacity(metadata.segments.len() + 1);
    keys.insert(Midge::column_batch_metadata_key(relation_id, index_id));
    keys.extend(
        metadata.segments.iter().map(|segment| {
            Midge::column_batch_segment_key(relation_id, index_id, segment.segment_id)
        }),
    );
    keys
}

fn valid_segment_rows(
    segment: &crate::catalog::ColumnBatchSegmentMeta,
    rows: &[ColumnBatchRow],
) -> bool {
    rows.len() == segment.row_count
        && rows.first().map(|row| &row.row_id) == segment.row_id_start.as_ref()
        && rows.last().map(|row| &row.row_id) == segment.row_id_end.as_ref()
        && rows.windows(2).all(|pair| pair[0].row_id < pair[1].row_id)
}
