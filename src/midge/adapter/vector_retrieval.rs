use serde::{Deserialize, Serialize};

use super::super::key_encoding;
use super::{CassieError, Midge, VectorIndexRecord, WriteOptions};

#[derive(Debug, Serialize, Deserialize)]
pub(super) struct HnswSourceSummary {
    pub(super) built_generation: u64,
    pub(super) source_fingerprint: u64,
    pub(super) row_count: usize,
}

impl Midge {
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
        let tx = self.begin_data_readonly_tx_for(&collection)?;
        let Some(raw) = tx
            .get(&Self::vector_index_state_key(&collection, field))
            .map_err(CassieError::from)?
        else {
            return Ok(None);
        };
        let value: serde_json::Value = serde_json::from_slice(&raw)
            .map_err(|error| CassieError::Parse(format!("invalid vector index state: {error}")))?;
        if value["hnsw_graph"]["nodes"].is_array() {
            return Ok(None);
        }
        let persisted: super::PersistedVectorIndexState = serde_json::from_value(value)
            .map_err(|error| CassieError::Parse(format!("invalid vector index state: {error}")))?;
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
            .get(&key_encoding::hnsw_source_summary_key(&collection, field))
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
        let entry_key = key_encoding::hnsw_graph_node_key(&collection, field, entry_point);
        let entry_raw = tx
            .get(&entry_key)
            .map_err(CassieError::from)?
            .ok_or_else(|| {
                CassieError::Execution("hnsw fallback:missing-entry-point".to_string())
            })?;
        let entry_node: crate::embeddings::HnswGraphNode = serde_json::from_slice(&entry_raw)
            .map_err(|error| CassieError::Parse(format!("invalid hnsw graph node: {error}")))?;
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
                    .get(&key_encoding::hnsw_graph_node_key(&collection, field, id))
                    .ok()
                    .flatten()
                    .and_then(|raw| serde_json::from_slice(&raw).ok());
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
        let mut tx = self.begin_data_rw_tx_for(collection)?;
        Self::write_hnsw_source_summary_to_tx(&mut tx, collection, field, generation, graph)?;
        tx.commit(WriteOptions::sync()).map_err(CassieError::from)
    }

    pub(super) fn write_hnsw_source_summary_to_tx(
        tx: &mut cntryl_midge::Transaction,
        collection: &str,
        field: &str,
        generation: u64,
        graph: &crate::embeddings::HnswGraphState,
    ) -> Result<(), CassieError> {
        let summary = HnswSourceSummary {
            built_generation: generation,
            source_fingerprint: graph.source_fingerprint,
            row_count: graph.row_count,
        };
        tx.put(
            key_encoding::hnsw_source_summary_key(collection, field),
            serde_json::to_vec(&summary).map_err(|error| CassieError::Parse(error.to_string()))?,
            None,
        )
        .map_err(CassieError::from)
    }
}
