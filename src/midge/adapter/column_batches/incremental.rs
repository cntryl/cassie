use std::collections::{BTreeMap, BTreeSet};

use super::{
    column_values, encode_manifest, storage_v2, CassieError, ColumnBatchMetadata, ColumnBatchRow,
    ColumnBatchSegmentMeta, IndexKind, IndexMeta, Midge, WriteOptions,
    CURRENT_COLUMN_BATCH_METADATA_FORMAT_VERSION, CURRENT_COLUMN_BATCH_SUMMARY_FORMAT_VERSION,
};

struct WorkingSegment {
    metadata: ColumnBatchSegmentMeta,
    rows: Option<Vec<ColumnBatchRow>>,
    is_new: bool,
}

impl WorkingSegment {
    fn row_id_start(&self) -> Option<&str> {
        self.rows
            .as_ref()
            .and_then(|rows| rows.first())
            .map_or(self.metadata.row_id_start.as_deref(), |row| {
                Some(row.row_id.as_str())
            })
    }
}

impl Midge {
    pub(crate) fn refresh_column_batches_incrementally(
        &self,
        collection: &str,
        target_generation: u64,
        changed_ids: &[String],
    ) -> Result<(), CassieError> {
        let collection = self.canonical_collection_name(collection);
        let indexes = self
            .list_indexes()?
            .into_iter()
            .filter(|index| index.collection == collection && index.kind == IndexKind::Column)
            .collect::<Vec<_>>();
        for index in indexes {
            let refreshed = !changed_ids.is_empty()
                && self
                    .try_refresh_column_batch_index(&index, target_generation, changed_ids)
                    .unwrap_or(false);
            if !refreshed {
                self.rebuild_column_batches_for_index(&index)?;
            }
        }
        Ok(())
    }

    fn try_refresh_column_batch_index(
        &self,
        index: &IndexMeta,
        target_generation: u64,
        changed_ids: &[String],
    ) -> Result<bool, CassieError> {
        let Some(metadata) = self.get_column_batch_metadata(&index.collection, &index.name)? else {
            return Ok(false);
        };
        if metadata.metadata_format_version != CURRENT_COLUMN_BATCH_METADATA_FORMAT_VERSION
            || metadata.summary_format_version != CURRENT_COLUMN_BATCH_SUMMARY_FORMAT_VERSION
            || metadata.schema_version != self.row_schema(&index.collection)?.schema_version
            || metadata.built_generation.checked_add(1) != Some(target_generation)
            || metadata.segments.is_empty()
        {
            return Ok(false);
        }
        let changed_ids = changed_ids
            .iter()
            .cloned()
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect::<Vec<_>>();
        if changed_ids.is_empty() {
            return Ok(false);
        }
        let mut changes_by_segment = BTreeMap::<usize, Vec<String>>::new();
        for id in &changed_ids {
            let position = segment_position(metadata.segments.as_slice(), id);
            changes_by_segment
                .entry(position)
                .or_default()
                .push(id.clone());
        }

        let tx = self.begin_data_readonly_tx_for(&index.collection)?;
        let all_fields = metadata.fields.iter().cloned().collect::<BTreeSet<_>>();
        let mut segments = Vec::with_capacity(metadata.segments.len());
        for (position, segment) in metadata.segments.iter().enumerate() {
            let rows = if changes_by_segment.contains_key(&position) {
                let Ok(loaded) = storage_v2::load_segment(&tx, index, segment, &all_fields)? else {
                    return Ok(false);
                };
                Some(loaded.rows)
            } else {
                None
            };
            segments.push(WorkingSegment {
                metadata: segment.clone(),
                rows,
                is_new: false,
            });
        }
        drop(tx);

        for (position, ids) in changes_by_segment {
            let rows = segments[position]
                .rows
                .as_mut()
                .expect("changed segment rows were loaded");
            let mut rows_by_id = std::mem::take(rows)
                .into_iter()
                .map(|row| (row.row_id.clone(), row))
                .collect::<BTreeMap<_, _>>();
            for id in ids {
                if let Some(document) = self.get_document(&index.collection, &id)? {
                    rows_by_id.insert(
                        id.clone(),
                        ColumnBatchRow {
                            row_id: id,
                            values: column_values(&document.payload, &metadata.fields),
                        },
                    );
                } else {
                    rows_by_id.remove(&id);
                }
            }
            *rows = rows_by_id.into_values().collect();
        }
        segments.retain(|segment| segment.rows.as_ref().is_none_or(|rows| !rows.is_empty()));
        if segments.is_empty() {
            return self.publish_empty_manifest(index, metadata, target_generation);
        }
        split_oversized_segments(
            &mut segments,
            metadata.segment_size,
            metadata.next_segment_id,
        )?;
        segments.sort_by(|left, right| left.row_id_start().cmp(&right.row_id_start()));
        publish_incremental_segments(self, index, metadata, target_generation, segments)
    }

    fn publish_empty_manifest(
        &self,
        index: &IndexMeta,
        mut metadata: ColumnBatchMetadata,
        target_generation: u64,
    ) -> Result<bool, CassieError> {
        metadata.manifest_revision = metadata
            .manifest_revision
            .checked_add(1)
            .ok_or_else(|| CassieError::ResourceLimit("manifest revision overflow".to_string()))?;
        metadata.built_generation = target_generation;
        metadata.source_row_count = 0;
        metadata.segments.clear();
        let (relation_id, index_id) = Self::column_batch_storage_ids(index)?;
        let mut tx = self.begin_data_rw_tx_for(&index.collection)?;
        tx.set_conflict_policy(cntryl_midge::ConflictPolicy::AbortOnWriteConflict);
        fence_generation(&mut tx, &index.collection, target_generation)?;
        tx.put(
            Self::column_batch_metadata_key(relation_id, index_id),
            encode_manifest(&metadata)?,
            None,
        )
        .map_err(CassieError::from)?;
        tx.commit(WriteOptions::sync()).map_err(CassieError::from)?;
        self.record_empty_column_batch_maintenance();
        let _ = self.garbage_collect_column_batch_revisions(index, &metadata);
        Ok(true)
    }

    pub(super) fn garbage_collect_column_batch_revisions(
        &self,
        index: &IndexMeta,
        metadata: &ColumnBatchMetadata,
    ) -> Result<usize, CassieError> {
        let (relation_id, index_id) = Self::column_batch_storage_ids(index)?;
        let expected =
            super::validation::expected_column_batch_keys(metadata, relation_id, index_id);
        let mut tx = self.begin_data_rw_tx_for(&index.collection)?;
        let entries = super::collect_scan(
            tx.scan(
                &super::Query::new()
                    .prefix(Self::column_batch_index_prefix(relation_id, index_id).into()),
            )
            .map_err(CassieError::from)?,
        )?;
        let mut removed = 0usize;
        for (key, _) in entries {
            if !expected.contains(&key) {
                tx.delete(key).map_err(CassieError::from)?;
                removed = removed.saturating_add(1);
            }
        }
        tx.commit(WriteOptions::sync()).map_err(CassieError::from)?;
        self.record_column_batch_orphan_cleanup(removed);
        Ok(removed)
    }

    fn compact_column_batch_index(
        &self,
        index: &IndexMeta,
        metadata: &ColumnBatchMetadata,
    ) -> Result<bool, CassieError> {
        let ideal_segments = metadata.source_row_count.div_ceil(metadata.segment_size);
        if metadata.segments.len() <= ideal_segments.saturating_mul(2) {
            return Ok(false);
        }
        let mut documents = self.scan_documents(&index.collection)?;
        documents.sort_by(|left, right| left.id.cmp(&right.id));
        let row_schema = self.row_schema(&index.collection)?;
        let mut next_segment_id = metadata.next_segment_id;
        let mut encoded = Vec::new();
        for (position, chunk) in documents.chunks(metadata.segment_size).enumerate() {
            let next_position = position
                .checked_add(1)
                .and_then(|position| position.checked_mul(metadata.segment_size))
                .ok_or_else(|| {
                    CassieError::ResourceLimit("column compaction range overflow".to_string())
                })?;
            let rows = storage_v2::rows_for_segment(chunk, metadata.fields.as_slice());
            encoded.push(storage_v2::encode_segment(
                next_segment_id,
                1,
                rows.as_slice(),
                metadata.fields.as_slice(),
                &row_schema,
                documents
                    .get(next_position)
                    .map(|document| document.id.clone()),
            )?);
            next_segment_id = next_segment_id.checked_add(1).ok_or_else(|| {
                CassieError::ResourceLimit("column segment id overflow".to_string())
            })?;
        }
        let mut compacted = metadata.clone();
        compacted.manifest_revision = compacted
            .manifest_revision
            .checked_add(1)
            .ok_or_else(|| CassieError::ResourceLimit("manifest revision overflow".to_string()))?;
        compacted.next_segment_id = next_segment_id;
        compacted.segments = encoded
            .iter()
            .map(|segment| segment.metadata.clone())
            .collect();
        let (relation_id, index_id) = Self::column_batch_storage_ids(index)?;
        let mut tx = self.begin_data_rw_tx_for(&index.collection)?;
        tx.set_conflict_policy(cntryl_midge::ConflictPolicy::AbortOnWriteConflict);
        fence_generation(&mut tx, &index.collection, metadata.built_generation)?;
        for segment in &encoded {
            storage_v2::write_segment(&mut tx, relation_id, index_id, segment)?;
        }
        tx.put(
            Self::column_batch_metadata_key(relation_id, index_id),
            encode_manifest(&compacted)?,
            None,
        )
        .map_err(CassieError::from)?;
        tx.commit(WriteOptions::sync()).map_err(CassieError::from)?;
        self.record_column_batch_compaction();
        let _ = self.garbage_collect_column_batch_revisions(index, &compacted);
        Ok(true)
    }
}

fn segment_position(segments: &[ColumnBatchSegmentMeta], id: &str) -> usize {
    segments
        .iter()
        .position(|segment| segment.row_id_end.as_deref().is_none_or(|end| id < end))
        .unwrap_or_else(|| segments.len().saturating_sub(1))
}

fn split_oversized_segments(
    segments: &mut Vec<WorkingSegment>,
    segment_size: usize,
    mut next_segment_id: u64,
) -> Result<(), CassieError> {
    let maximum = segment_size
        .checked_mul(2)
        .ok_or_else(|| CassieError::ResourceLimit("column segment size overflow".to_string()))?;
    let mut position = 0usize;
    while position < segments.len() {
        let oversized = segments[position]
            .rows
            .as_ref()
            .is_some_and(|rows| rows.len() > maximum);
        if !oversized {
            position = position.saturating_add(1);
            continue;
        }
        let rows = segments[position]
            .rows
            .as_mut()
            .expect("oversized segment has loaded rows");
        let right_rows = rows.split_off(rows.len() / 2);
        let mut right_metadata = segments[position].metadata.clone();
        right_metadata.segment_id = next_segment_id;
        right_metadata.revision = 0;
        next_segment_id = next_segment_id
            .checked_add(1)
            .ok_or_else(|| CassieError::ResourceLimit("column segment id overflow".to_string()))?;
        segments.insert(
            position.saturating_add(1),
            WorkingSegment {
                metadata: right_metadata,
                rows: Some(right_rows),
                is_new: true,
            },
        );
    }
    Ok(())
}

fn publish_incremental_segments(
    midge: &Midge,
    index: &IndexMeta,
    mut metadata: ColumnBatchMetadata,
    target_generation: u64,
    segments: Vec<WorkingSegment>,
) -> Result<bool, CassieError> {
    let original_next_segment_id = metadata.next_segment_id;
    let row_schema = midge.row_schema(&index.collection)?;
    let next_starts = segments
        .iter()
        .skip(1)
        .map(|segment| segment.row_id_start().map(str::to_string))
        .chain(std::iter::once(None))
        .collect::<Vec<_>>();
    let mut encoded = Vec::new();
    let mut published = Vec::with_capacity(segments.len());
    for (mut segment, row_id_end) in segments.into_iter().zip(next_starts) {
        if let Some(rows) = segment.rows.take() {
            let revision = if segment.is_new {
                1
            } else {
                segment.metadata.revision.checked_add(1).ok_or_else(|| {
                    CassieError::ResourceLimit("column segment revision overflow".to_string())
                })?
            };
            let encoded_segment = storage_v2::encode_segment(
                segment.metadata.segment_id,
                revision,
                rows.as_slice(),
                metadata.fields.as_slice(),
                &row_schema,
                row_id_end,
            )?;
            published.push(encoded_segment.metadata.clone());
            encoded.push(encoded_segment);
        } else {
            segment.metadata.row_id_end = row_id_end;
            published.push(segment.metadata);
        }
    }
    metadata.manifest_revision = metadata
        .manifest_revision
        .checked_add(1)
        .ok_or_else(|| CassieError::ResourceLimit("manifest revision overflow".to_string()))?;
    metadata.next_segment_id = published
        .iter()
        .map(|segment| segment.segment_id)
        .max()
        .map_or(0, |segment_id| segment_id.saturating_add(1));
    metadata.built_generation = target_generation;
    metadata.source_row_count = published.iter().map(|segment| segment.row_count).sum();
    metadata.segments = published;
    let splits = metadata
        .segments
        .iter()
        .filter(|segment| segment.segment_id >= original_next_segment_id)
        .count();

    let (relation_id, index_id) = Midge::column_batch_storage_ids(index)?;
    let mut tx = midge.begin_data_rw_tx_for(&index.collection)?;
    tx.set_conflict_policy(cntryl_midge::ConflictPolicy::AbortOnWriteConflict);
    fence_generation(&mut tx, &index.collection, target_generation)?;
    for segment in &encoded {
        storage_v2::write_segment(&mut tx, relation_id, index_id, segment)?;
    }
    tx.put(
        Midge::column_batch_metadata_key(relation_id, index_id),
        encode_manifest(&metadata)?,
        None,
    )
    .map_err(CassieError::from)?;
    tx.commit(WriteOptions::sync()).map_err(CassieError::from)?;
    let maintenance_source_rows = encoded
        .iter()
        .map(|segment| segment.metadata.row_count)
        .sum();
    midge.record_column_batch_build(encoded.as_slice(), false, splits, maintenance_source_rows);
    let _ = midge.garbage_collect_column_batch_revisions(index, &metadata);
    let _ = midge.compact_column_batch_index(index, &metadata);
    Ok(true)
}

fn fence_generation(
    tx: &mut cntryl_midge::Transaction,
    collection: &str,
    target_generation: u64,
) -> Result<(), CassieError> {
    let key = Midge::collection_generation_key(collection);
    let raw = tx
        .get(&key)
        .map_err(CassieError::from)?
        .ok_or_else(|| CassieError::Execution("missing collection generation".to_string()))?;
    let bytes: [u8; 8] = raw
        .as_ref()
        .try_into()
        .map_err(|_| CassieError::Parse("invalid persisted collection generation".to_string()))?;
    if u64::from_be_bytes(bytes) != target_generation {
        return Err(CassieError::Execution(
            "column batch maintenance generation changed".to_string(),
        ));
    }
    tx.put(key, raw.to_vec(), None).map_err(CassieError::from)
}
