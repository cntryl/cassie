use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

use super::super::key_encoding;
use super::{collect_scan, CassieError, Midge, Query, VectorIndexRecord, WriteOptions};
use crate::runtime::accounted::AccountedVec;
use crate::runtime::{QueryExecutionControls, QueryMemoryReservation};

#[derive(Debug, Serialize, Deserialize)]
pub(super) struct HnswSourceSummary {
    pub(super) built_generation: u64,
    pub(super) source_fingerprint: u64,
    pub(super) row_count: usize,
}

pub(crate) struct PersistedHnswCandidateBatch {
    pub(crate) built_generation: u64,
    pub(crate) candidates: Vec<crate::vector::hnsw::HnswCandidate>,
    pub(crate) candidate_count: usize,
    pub(crate) ann_reads: usize,
    pub(crate) candidate_memory: QueryMemoryReservation,
}

impl PersistedHnswCandidateBatch {
    pub(crate) fn into_parts(
        self,
    ) -> (
        u64,
        Vec<crate::vector::hnsw::HnswCandidate>,
        usize,
        usize,
        QueryMemoryReservation,
    ) {
        (
            self.built_generation,
            self.candidates,
            self.candidate_count,
            self.ann_reads,
            self.candidate_memory,
        )
    }
}

pub(crate) struct PersistedIvfFlatTrainingSnapshot {
    pub(crate) built_generation: u64,
    pub(crate) training: crate::embeddings::IvfFlatTrainingState,
    pub(crate) membership_count: usize,
    pub(crate) ann_reads: usize,
    pub(crate) manifest_memory: QueryMemoryReservation,
}

impl PersistedIvfFlatTrainingSnapshot {
    pub(crate) fn into_parts(
        self,
    ) -> (
        u64,
        crate::embeddings::IvfFlatTrainingState,
        usize,
        usize,
        QueryMemoryReservation,
    ) {
        (
            self.built_generation,
            self.training,
            self.membership_count,
            self.ann_reads,
            self.manifest_memory,
        )
    }
}

pub(crate) struct PersistedIvfFlatCandidateBatch {
    pub(crate) built_generation: u64,
    pub(crate) records: Vec<crate::embeddings::NormalizedVectorRecord>,
    pub(crate) membership_reads: usize,
    pub(crate) vector_reads: usize,
    pub(crate) candidate_memory: QueryMemoryReservation,
}

impl PersistedIvfFlatCandidateBatch {
    pub(crate) fn into_parts(
        self,
    ) -> (
        u64,
        Vec<crate::embeddings::NormalizedVectorRecord>,
        usize,
        usize,
        QueryMemoryReservation,
    ) {
        (
            self.built_generation,
            self.records,
            self.membership_reads,
            self.vector_reads,
            self.candidate_memory,
        )
    }
}

struct ControlledIvfReadContext<'a> {
    midge: &'a Midge,
    tx: &'a cntryl_midge::Transaction,
    collection: &'a str,
    field: &'a str,
    relation_id: u64,
    field_id: u32,
    controls: &'a QueryExecutionControls,
}

struct ControlledIvfMemberships {
    ids: Vec<String>,
    reads: usize,
    memory: QueryMemoryReservation,
}

struct ControlledHnswHeader {
    built_generation: u64,
    manifest: super::codec::PersistedHnswManifest,
    state_memory: QueryMemoryReservation,
    summary_memory: QueryMemoryReservation,
}

fn load_controlled_hnsw_header(
    midge: &Midge,
    tx: &cntryl_midge::Transaction,
    collection: &str,
    relation_id: u64,
    field_id: u32,
    controls: &QueryExecutionControls,
) -> Result<Option<ControlledHnswHeader>, CassieError> {
    let Some(raw) = tx
        .get(&Midge::vector_index_state_key(relation_id, field_id))
        .map_err(CassieError::from)?
    else {
        return Ok(None);
    };
    record_controlled_ann_read(midge, controls)?;
    let state_memory = controls.reserve_query_memory(raw.len())?;
    let persisted = super::codec::decode_vector_index_state(&raw)?;
    if persisted.built_generation != midge.collection_generation(collection)? {
        return Err(CassieError::Execution(
            "hnsw fallback:concurrent-source-change".to_string(),
        ));
    }
    let Some(manifest) = persisted.hnsw_graph else {
        return Err(CassieError::Execution(
            "hnsw fallback:missing-graph".to_string(),
        ));
    };
    let summary_raw = tx
        .get(&key_encoding::hnsw_source_summary_key(
            relation_id,
            field_id,
        ))
        .map_err(CassieError::from)?
        .ok_or_else(|| {
            CassieError::Execution("hnsw fallback:missing-source-summary".to_string())
        })?;
    record_controlled_ann_read(midge, controls)?;
    let summary_memory = controls.reserve_query_memory(summary_raw.len())?;
    let summary: HnswSourceSummary = serde_json::from_slice(&summary_raw)
        .map_err(|error| CassieError::Parse(format!("invalid hnsw source summary: {error}")))?;
    if summary.built_generation != persisted.built_generation {
        return Err(CassieError::Execution(
            "hnsw fallback:concurrent-source-change".to_string(),
        ));
    }
    if summary.source_fingerprint != manifest.source_fingerprint
        || summary.row_count != manifest.row_count
    {
        return Err(CassieError::Execution(
            "hnsw fallback:stale-source-fingerprint".to_string(),
        ));
    }
    Ok(Some(ControlledHnswHeader {
        built_generation: persisted.built_generation,
        manifest,
        state_memory,
        summary_memory,
    }))
}

fn load_controlled_ivfflat_summary(
    context: &ControlledIvfReadContext<'_>,
    training: &crate::embeddings::IvfFlatTrainingState,
) -> Result<(HnswSourceSummary, QueryMemoryReservation), CassieError> {
    let raw = context
        .tx
        .get(&key_encoding::ivfflat_source_summary_key(
            context.relation_id,
            context.field_id,
        ))
        .map_err(CassieError::from)?
        .ok_or_else(|| {
            CassieError::Execution("ivfflat fallback:missing-source-summary".to_string())
        })?;
    record_controlled_ann_read(context.midge, context.controls)?;
    let memory = context.controls.reserve_query_memory(raw.len())?;
    let summary: HnswSourceSummary = serde_json::from_slice(&raw)
        .map_err(|error| CassieError::Parse(format!("invalid vector source summary: {error}")))?;
    if summary.built_generation != context.midge.collection_generation(context.collection)? {
        return Err(CassieError::Execution(
            "ivfflat fallback:concurrent-source-change".to_string(),
        ));
    }
    if summary.source_fingerprint != training.source_fingerprint
        || summary.row_count != training.row_count
    {
        return Err(CassieError::Execution(
            "ivfflat fallback:stale-source-fingerprint".to_string(),
        ));
    }
    Ok((summary, memory))
}

fn load_controlled_ivfflat_memberships(
    context: &ControlledIvfReadContext<'_>,
    training: &crate::embeddings::IvfFlatTrainingState,
    probed_lists: &BTreeSet<usize>,
) -> Result<ControlledIvfMemberships, CassieError> {
    let membership_prefix =
        key_encoding::ivfflat_membership_prefix(context.relation_id, context.field_id);
    let mut ids = Vec::new();
    let mut seen_ids = BTreeSet::new();
    let mut reads = 0usize;
    let mut memory = context.controls.reserve_query_memory(0)?;
    for list in probed_lists {
        check_ann_controls(context.controls)?;
        let prefix = key_encoding::ivfflat_membership_list_prefix(
            context.relation_id,
            context.field_id,
            *list,
        );
        let scan = context
            .tx
            .scan(&Query::new().prefix(prefix.into()))
            .map_err(CassieError::from)?;
        let mut observed = 0usize;
        for entry in scan {
            check_ann_controls(context.controls)?;
            let (key, value) = entry.map_err(CassieError::from)?;
            record_controlled_ann_read(context.midge, context.controls)?;
            reads = reads.saturating_add(1);
            observed = observed.saturating_add(1);
            memory.try_grow(ivfflat_membership_bytes(&key, &value))?;
            let Some((stored_list, id)) =
                key_encoding::decode_ivfflat_membership_suffix(&key, &membership_prefix)
            else {
                return Err(invalid_ivfflat_membership_key());
            };
            if !value.is_empty() || stored_list != *list {
                return Err(invalid_ivfflat_membership_key());
            }
            if !seen_ids.insert(id.clone()) {
                return Err(stale_ivfflat_membership());
            }
            ids.try_reserve_exact(1).map_err(|error| {
                CassieError::ResourceLimit(format!(
                    "unable to retain controlled IVFFlat membership: {error}"
                ))
            })?;
            ids.push(id);
        }
        if training.list_sizes.get(*list).copied() != Some(observed) {
            return Err(stale_ivfflat_membership());
        }
    }
    Ok(ControlledIvfMemberships { ids, reads, memory })
}

fn load_controlled_ivfflat_vectors(
    context: &ControlledIvfReadContext<'_>,
    ids: Vec<String>,
    generation: u64,
) -> Result<
    (
        Vec<crate::embeddings::NormalizedVectorRecord>,
        QueryMemoryReservation,
        usize,
    ),
    CassieError,
> {
    let mut records = AccountedVec::try_new(context.controls)?;
    let mut reads = 0usize;
    for id in ids {
        check_ann_controls(context.controls)?;
        let Some(raw) = context
            .tx
            .get(&key_encoding::normalized_vector_key(
                context.relation_id,
                context.field_id,
                &id,
            ))
            .map_err(CassieError::from)?
        else {
            return Err(CassieError::Execution(
                "ivfflat fallback:missing-candidate".to_string(),
            ));
        };
        record_controlled_ann_read(context.midge, context.controls)?;
        reads = reads.saturating_add(1);
        let retained_bytes = raw.len().saturating_add(id.len());
        records.try_push_with_result(retained_bytes, || {
            super::codec::decode_normalized_vector(&raw, context.collection, context.field, &id)
        })?;
        if records
            .as_slice()
            .last()
            .is_some_and(|record| record.built_generation != generation)
        {
            return Err(CassieError::Execution(
                "ivfflat fallback:stale-candidate-generation".to_string(),
            ));
        }
    }
    let (records, memory) = records.into_parts();
    Ok((records, memory, reads))
}

fn ivfflat_membership_bytes(key: &[u8], value: &[u8]) -> usize {
    key.len()
        .saturating_add(value.len())
        .saturating_mul(2)
        .saturating_add(2 * std::mem::size_of::<String>())
}

fn invalid_ivfflat_membership_key() -> CassieError {
    CassieError::Execution("ivfflat fallback:invalid-membership-key".to_string())
}

fn stale_ivfflat_membership() -> CassieError {
    CassieError::Execution("ivfflat fallback:stale-list-membership".to_string())
}

impl Midge {
    /// Returns generation-bound vector candidate IDs without scanning source rows.
    ///
    /// # Errors
    ///
    /// Returns an error when a persisted vector artifact is stale or corrupt.
    pub fn persisted_vector_candidate_ids(
        &self,
        collection: &str,
        field: &str,
        query: &[f32],
        limit: usize,
    ) -> Result<Option<BTreeSet<String>>, CassieError> {
        let Some(index) = self.get_vector_index_definition(collection, field)? else {
            return Ok(None);
        };
        match index.metadata.index_type {
            crate::embeddings::VectorIndexType::Hnsw => {
                let Some(options) = index.metadata.hnsw.as_ref() else {
                    return Err(CassieError::Execution(
                        "hnsw fallback:missing-options".to_string(),
                    ));
                };
                let Some(result) =
                    self.search_hnsw_graph_point_read(collection, field, query, options, limit)?
                else {
                    return Ok(None);
                };
                Ok(Some(
                    result
                        .candidates
                        .into_iter()
                        .map(|candidate| candidate.id)
                        .collect(),
                ))
            }
            crate::embeddings::VectorIndexType::IvfFlat => {
                let Some((training, membership_count)) =
                    self.get_ivfflat_training_manifest(collection, field)?
                else {
                    return Ok(None);
                };
                if crate::vector::ivfflat::compact_manifest_fallback_reason(
                    &training,
                    query.len(),
                    membership_count,
                )
                .is_some()
                {
                    return Ok(None);
                }
                let normalized = crate::vector::normalize(query)
                    .map_or_else(|| query.to_vec(), |value| value.values);
                let lists = crate::vector::ivfflat::probe_lists(&normalized, &training);
                let records =
                    self.ivfflat_candidate_vectors(collection, field, &training, &lists)?;
                Ok(Some(
                    records
                        .into_iter()
                        .take(limit)
                        .map(|record| record.id)
                        .collect(),
                ))
            }
            crate::embeddings::VectorIndexType::BruteForce => Ok(None),
        }
    }

    /// Reads the compact IVF manifest without hydrating every list membership.
    ///
    /// # Errors
    ///
    /// Returns an error when the persisted state cannot be read or decoded.
    pub fn get_ivfflat_training_manifest(
        &self,
        collection: &str,
        field: &str,
    ) -> Result<Option<(crate::embeddings::IvfFlatTrainingState, usize)>, CassieError> {
        let collection = self.canonical_collection_name(collection);
        let (relation_id, field_id) = self.vector_storage_ids(&collection, field)?;
        let tx = self.begin_data_readonly_tx_for(&collection)?;
        let Some(raw) = tx
            .get(&Self::vector_index_state_key(relation_id, field_id))
            .map_err(CassieError::from)?
        else {
            return Ok(None);
        };
        let persisted = super::codec::decode_vector_index_state(&raw)?;
        let Some(manifest) = persisted.ivfflat_training else {
            return Ok(None);
        };
        if persisted.built_generation != self.collection_generation(&collection)? {
            return Ok(None);
        }
        Ok(Some((
            crate::embeddings::IvfFlatTrainingState {
                version: manifest.version,
                source_fingerprint: manifest.source_fingerprint,
                trained: manifest.trained,
                row_count: manifest.row_count,
                lists: manifest.lists,
                probes: manifest.probes,
                training_seed: manifest.training_seed,
                centroid_ids: manifest.centroid_ids,
                centroids: manifest.centroids,
                assignments: std::collections::BTreeMap::new(),
                list_sizes: manifest.list_sizes,
            },
            manifest.membership_count,
        )))
    }

    pub(crate) fn get_ivfflat_training_manifest_controlled(
        &self,
        collection: &str,
        field: &str,
        controls: &QueryExecutionControls,
    ) -> Result<Option<PersistedIvfFlatTrainingSnapshot>, CassieError> {
        check_ann_controls(controls)?;
        let collection = self.canonical_collection_name(collection);
        let (relation_id, field_id) = self.vector_storage_ids(&collection, field)?;
        let tx = self.begin_data_readonly_tx_for(&collection)?;
        let Some(raw) = tx
            .get(&Self::vector_index_state_key(relation_id, field_id))
            .map_err(CassieError::from)?
        else {
            return Ok(None);
        };
        record_controlled_ann_read(self, controls)?;
        let manifest_memory = controls.reserve_query_memory(raw.len())?;
        let persisted = super::codec::decode_vector_index_state(&raw)?;
        let Some(manifest) = persisted.ivfflat_training else {
            return Ok(None);
        };
        if persisted.built_generation != self.collection_generation(&collection)? {
            return Err(CassieError::Execution(
                "ivfflat fallback:concurrent-source-change".to_string(),
            ));
        }
        check_ann_controls(controls)?;
        Ok(Some(PersistedIvfFlatTrainingSnapshot {
            built_generation: persisted.built_generation,
            training: crate::embeddings::IvfFlatTrainingState {
                version: manifest.version,
                source_fingerprint: manifest.source_fingerprint,
                trained: manifest.trained,
                row_count: manifest.row_count,
                lists: manifest.lists,
                probes: manifest.probes,
                training_seed: manifest.training_seed,
                centroid_ids: manifest.centroid_ids,
                centroids: manifest.centroids,
                assignments: std::collections::BTreeMap::new(),
                list_sizes: manifest.list_sizes,
            },
            membership_count: manifest.membership_count,
            ann_reads: 1,
            manifest_memory,
        }))
    }

    /// Reads only normalized vectors assigned to the selected IVF lists.
    ///
    /// # Errors
    ///
    /// Returns an error when the source summary or a selected candidate is stale or corrupt.
    pub fn ivfflat_candidate_vectors(
        &self,
        collection: &str,
        field: &str,
        training: &crate::embeddings::IvfFlatTrainingState,
        probed_lists: &BTreeSet<usize>,
    ) -> Result<Vec<crate::embeddings::NormalizedVectorRecord>, CassieError> {
        let collection = self.canonical_collection_name(collection);
        let (relation_id, field_id) = self.vector_storage_ids(&collection, field)?;
        let tx = self.begin_data_readonly_tx_for(&collection)?;
        let summary_raw = tx
            .get(&key_encoding::ivfflat_source_summary_key(
                relation_id,
                field_id,
            ))
            .map_err(CassieError::from)?
            .ok_or_else(|| {
                CassieError::Execution("ivfflat fallback:missing-source-summary".to_string())
            })?;
        let summary: HnswSourceSummary = serde_json::from_slice(&summary_raw).map_err(|error| {
            CassieError::Parse(format!("invalid vector source summary: {error}"))
        })?;
        if summary.built_generation != self.collection_generation(&collection)?
            || summary.source_fingerprint != training.source_fingerprint
            || summary.row_count != training.row_count
        {
            return Err(CassieError::Execution(
                "ivfflat fallback:stale-source-fingerprint".to_string(),
            ));
        }
        let membership_prefix = key_encoding::ivfflat_membership_prefix(relation_id, field_id);
        let mut ids = Vec::new();
        let mut seen_ids = BTreeSet::new();
        for list in probed_lists {
            let prefix = key_encoding::ivfflat_membership_list_prefix(relation_id, field_id, *list);
            let entries = collect_scan(
                tx.scan(&Query::new().prefix(prefix.into()))
                    .map_err(CassieError::from)?,
            )?;
            let expected_count = training.list_sizes.get(*list).ok_or_else(|| {
                CassieError::Execution("ivfflat fallback:stale-list-membership".to_string())
            })?;
            if entries.len() != *expected_count {
                return Err(CassieError::Execution(
                    "ivfflat fallback:stale-list-membership".to_string(),
                ));
            }
            for (key, value) in entries {
                let Some((stored_list, id)) =
                    key_encoding::decode_ivfflat_membership_suffix(&key, &membership_prefix)
                else {
                    return Err(CassieError::Execution(
                        "ivfflat fallback:invalid-membership-key".to_string(),
                    ));
                };
                if !value.is_empty() || stored_list != *list {
                    return Err(CassieError::Execution(
                        "ivfflat fallback:invalid-membership-key".to_string(),
                    ));
                }
                if !seen_ids.insert(id.clone()) {
                    return Err(CassieError::Execution(
                        "ivfflat fallback:stale-list-membership".to_string(),
                    ));
                }
                ids.push(id);
            }
        }
        let mut records = Vec::with_capacity(ids.len());
        for id in ids {
            let Some(raw) = tx
                .get(&key_encoding::normalized_vector_key(
                    relation_id,
                    field_id,
                    &id,
                ))
                .map_err(CassieError::from)?
            else {
                return Err(CassieError::Execution(
                    "ivfflat fallback:missing-candidate".to_string(),
                ));
            };
            let record = super::codec::decode_normalized_vector(&raw, &collection, field, &id)?;
            if record.built_generation != summary.built_generation {
                return Err(CassieError::Execution(
                    "ivfflat fallback:stale-candidate-generation".to_string(),
                ));
            }
            records.push(record);
        }
        Ok(records)
    }

    pub(crate) fn ivfflat_candidate_vectors_controlled(
        &self,
        collection: &str,
        field: &str,
        training: &crate::embeddings::IvfFlatTrainingState,
        probed_lists: &BTreeSet<usize>,
        controls: &QueryExecutionControls,
    ) -> Result<PersistedIvfFlatCandidateBatch, CassieError> {
        check_ann_controls(controls)?;
        let collection = self.canonical_collection_name(collection);
        let (relation_id, field_id) = self.vector_storage_ids(&collection, field)?;
        let tx = self.begin_data_readonly_tx_for(&collection)?;
        let context = ControlledIvfReadContext {
            midge: self,
            tx: &tx,
            collection: &collection,
            field,
            relation_id,
            field_id,
            controls,
        };
        let (summary, summary_memory) = load_controlled_ivfflat_summary(&context, training)?;
        let memberships = load_controlled_ivfflat_memberships(&context, training, probed_lists)?;
        let (records, candidate_memory, vector_reads) =
            load_controlled_ivfflat_vectors(&context, memberships.ids, summary.built_generation)?;
        check_ann_controls(controls)?;
        drop(memberships.memory);
        drop(summary_memory);
        Ok(PersistedIvfFlatCandidateBatch {
            built_generation: summary.built_generation,
            records,
            membership_reads: memberships.reads,
            vector_reads,
            candidate_memory,
        })
    }

    /// Searches a generation-bound HNSW manifest by point-reading only requested nodes.
    ///
    /// # Errors
    ///
    /// Returns an error when persisted vector state or graph nodes are unreadable.
    pub fn search_hnsw_graph_point_read(
        &self,
        collection: &str,
        field: &str,
        query: &[f32],
        options: &crate::embeddings::HnswIndexOptions,
        limit: usize,
    ) -> Result<Option<crate::vector::hnsw::HnswSearchResult>, CassieError> {
        let collection = self.canonical_collection_name(collection);
        let (relation_id, field_id) = self.vector_storage_ids(&collection, field)?;
        let tx = self.begin_data_readonly_tx_for(&collection)?;
        let Some(raw) = tx
            .get(&Self::vector_index_state_key(relation_id, field_id))
            .map_err(CassieError::from)?
        else {
            return Ok(None);
        };
        let persisted = super::codec::decode_vector_index_state(&raw)?;
        if persisted.built_generation != self.collection_generation(&collection)? {
            return Err(CassieError::Execution(
                "hnsw fallback:stale-graph".to_string(),
            ));
        }
        let Some(manifest) = persisted.hnsw_graph else {
            return Err(CassieError::Execution(
                "hnsw fallback:missing-graph".to_string(),
            ));
        };
        let summary_raw = tx
            .get(&key_encoding::hnsw_source_summary_key(
                relation_id,
                field_id,
            ))
            .map_err(CassieError::from)?
            .ok_or_else(|| {
                CassieError::Execution("hnsw fallback:missing-source-summary".to_string())
            })?;
        let summary: HnswSourceSummary = serde_json::from_slice(&summary_raw)
            .map_err(|error| CassieError::Parse(format!("invalid hnsw source summary: {error}")))?;
        if summary.built_generation != persisted.built_generation
            || summary.source_fingerprint != manifest.source_fingerprint
            || summary.row_count != manifest.row_count
        {
            return Err(CassieError::Execution(
                "hnsw fallback:stale-source-fingerprint".to_string(),
            ));
        }
        let Some(entry_point) = manifest.entry_point.as_deref() else {
            return Err(CassieError::Execution(
                "hnsw fallback:missing-entry-point".to_string(),
            ));
        };
        let entry_key = key_encoding::hnsw_graph_node_key(relation_id, field_id, entry_point);
        let entry_raw = tx
            .get(&entry_key)
            .map_err(CassieError::from)?
            .ok_or_else(|| {
                CassieError::Execution("hnsw fallback:missing-entry-point".to_string())
            })?;
        let entry_node = super::codec::decode_hnsw_node(&entry_raw, entry_point)?;
        if entry_node.layers.len() <= manifest.max_layer {
            return Err(CassieError::Execution(
                "hnsw fallback:inconsistent-max-layer".to_string(),
            ));
        }
        let mut missing_node = false;
        let result = crate::vector::hnsw::search_graph_with_node_loader(
            manifest.metric,
            entry_point,
            manifest.max_layer,
            query,
            options,
            limit,
            |id| {
                let node = tx
                    .get(&key_encoding::hnsw_graph_node_key(
                        relation_id,
                        field_id,
                        id,
                    ))
                    .ok()
                    .flatten()
                    .and_then(|raw| super::codec::decode_hnsw_node(&raw, id).ok());
                if node.is_none() {
                    missing_node = true;
                }
                node
            },
        );
        if missing_node {
            return Err(CassieError::Execution(
                "hnsw fallback:unknown-neighbor-id".to_string(),
            ));
        }
        Ok(result)
    }

    pub(crate) fn search_hnsw_graph_point_read_controlled(
        &self,
        collection: &str,
        field: &str,
        query: &[f32],
        options: &crate::embeddings::HnswIndexOptions,
        limit: usize,
        controls: &QueryExecutionControls,
    ) -> Result<Option<PersistedHnswCandidateBatch>, CassieError> {
        check_ann_controls(controls)?;
        let collection = self.canonical_collection_name(collection);
        let (relation_id, field_id) = self.vector_storage_ids(&collection, field)?;
        let tx = self.begin_data_readonly_tx_for(&collection)?;
        let Some(header) =
            load_controlled_hnsw_header(self, &tx, &collection, relation_id, field_id, controls)?
        else {
            return Ok(None);
        };
        let Some(entry_point) = header.manifest.entry_point.as_deref() else {
            return Err(CassieError::Execution(
                "hnsw fallback:missing-entry-point".to_string(),
            ));
        };
        let mut ann_reads = 2usize;
        let mut missing_node = false;
        let result = crate::vector::hnsw::search_graph_with_controlled_node_loader(
            &crate::vector::hnsw::ControlledHnswSearchRequest {
                metric: header.manifest.metric,
                entry_point,
                max_layer: header.manifest.max_layer,
                query,
                options,
                limit,
            },
            controls,
            |id| {
                check_ann_controls(controls)?;
                let raw = tx
                    .get(&key_encoding::hnsw_graph_node_key(
                        relation_id,
                        field_id,
                        id,
                    ))
                    .map_err(CassieError::from)?;
                record_controlled_ann_read(self, controls)?;
                ann_reads = ann_reads.saturating_add(1);
                let Some(raw) = raw else {
                    missing_node = true;
                    return Ok(None);
                };
                let _decode_memory = controls.reserve_query_memory(raw.len())?;
                super::codec::decode_hnsw_node(&raw, id)
                    .map(Some)
                    .map_err(|_| CassieError::Execution("hnsw fallback:invalid-node".to_string()))
            },
        )?;
        if missing_node {
            return Err(CassieError::Execution(
                "hnsw fallback:unknown-neighbor-id".to_string(),
            ));
        }
        let Some(result) = result else {
            return Ok(None);
        };
        let candidate_bytes = result
            .candidates
            .iter()
            .map(|candidate| {
                std::mem::size_of::<crate::vector::hnsw::HnswCandidate>()
                    .saturating_add(candidate.id.len())
            })
            .sum();
        let candidate_memory = controls.reserve_query_memory(candidate_bytes)?;
        check_ann_controls(controls)?;
        drop(header.state_memory);
        drop(header.summary_memory);
        Ok(Some(PersistedHnswCandidateBatch {
            built_generation: header.built_generation,
            candidates: result.candidates,
            candidate_count: result.candidate_count,
            ann_reads,
            candidate_memory,
        }))
    }

    /// Returns vector-index metadata without hydrating graph or IVF derived state.
    ///
    /// # Errors
    ///
    /// Returns an error when vector-index metadata cannot be read or decoded.
    pub fn get_vector_index_definition(
        &self,
        collection: &str,
        field: &str,
    ) -> Result<Option<VectorIndexRecord>, CassieError> {
        let requested_collection = collection.to_string();
        let collection = self.canonical_collection_name(collection);
        let tx = self.begin_schema_readonly_tx()?;
        let Some(raw) = tx
            .get(&Self::vector_index_key(&collection, field))
            .map_err(CassieError::from)?
        else {
            return Ok(None);
        };
        let mut record: VectorIndexRecord = serde_json::from_slice(&raw).map_err(|error| {
            CassieError::Parse(format!("invalid vector index metadata: {error}"))
        })?;
        if !requested_collection.eq_ignore_ascii_case(&collection) {
            record.collection = self.display_collection_name(&requested_collection);
        }
        Ok(Some(record))
    }

    pub(super) fn write_hnsw_source_summary(
        &self,
        collection: &str,
        field: &str,
        graph: &crate::embeddings::HnswGraphState,
    ) -> Result<(), CassieError> {
        let generation = self.collection_generation(collection)?;
        let (relation_id, field_id) = self.vector_storage_ids(collection, field)?;
        let mut tx = self.begin_data_rw_tx_for(collection)?;
        Self::write_vector_source_summary_to_tx(
            &mut tx,
            relation_id,
            field_id,
            generation,
            graph.source_fingerprint,
            graph.row_count,
        )?;
        tx.commit(WriteOptions::sync()).map_err(CassieError::from)
    }

    pub(super) fn write_ivfflat_source_summary(
        &self,
        collection: &str,
        field: &str,
        source_fingerprint: u64,
        row_count: usize,
    ) -> Result<(), CassieError> {
        let generation = self.collection_generation(collection)?;
        let (relation_id, field_id) = self.vector_storage_ids(collection, field)?;
        let mut tx = self.begin_data_rw_tx_for(collection)?;
        Self::write_vector_source_summary_to_tx(
            &mut tx,
            relation_id,
            field_id,
            generation,
            source_fingerprint,
            row_count,
        )?;
        tx.commit(WriteOptions::sync()).map_err(CassieError::from)
    }

    pub(super) fn write_hnsw_source_summary_to_tx(
        tx: &mut cntryl_midge::Transaction,
        relation_id: u64,
        field_id: u32,
        generation: u64,
        graph: &crate::embeddings::HnswGraphState,
    ) -> Result<(), CassieError> {
        Self::write_vector_source_summary_to_tx(
            tx,
            relation_id,
            field_id,
            generation,
            graph.source_fingerprint,
            graph.row_count,
        )
    }

    pub(super) fn write_vector_source_summary_to_tx(
        tx: &mut cntryl_midge::Transaction,
        relation_id: u64,
        field_id: u32,
        generation: u64,
        source_fingerprint: u64,
        row_count: usize,
    ) -> Result<(), CassieError> {
        let summary = HnswSourceSummary {
            built_generation: generation,
            source_fingerprint,
            row_count,
        };
        tx.put(
            key_encoding::ivfflat_source_summary_key(relation_id, field_id),
            serde_json::to_vec(&summary).map_err(|error| CassieError::Parse(error.to_string()))?,
            None,
        )
        .map_err(CassieError::from)
    }
}

fn check_ann_controls(controls: &QueryExecutionControls) -> Result<(), CassieError> {
    if controls.is_cancelled() {
        return Err(CassieError::QueryCancelled);
    }
    if controls.is_timed_out() {
        return Err(CassieError::DeadlineExceeded);
    }
    Ok(())
}

fn record_controlled_ann_read(
    midge: &Midge,
    controls: &QueryExecutionControls,
) -> Result<(), CassieError> {
    check_ann_controls(controls)?;
    midge.record_query_scan_entry();
    if super::super::query_scan_control::should_cancel_controlled_query_scan() {
        return Err(CassieError::QueryCancelled);
    }
    check_ann_controls(controls)
}
