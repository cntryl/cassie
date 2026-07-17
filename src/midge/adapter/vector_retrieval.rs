use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

use super::super::key_encoding;
use super::{collect_scan, CassieError, Midge, Query, VectorIndexRecord, WriteOptions};

#[derive(Debug, Serialize, Deserialize)]
pub(super) struct HnswSourceSummary {
    pub(super) built_generation: u64,
    pub(super) source_fingerprint: u64,
    pub(super) row_count: usize,
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
