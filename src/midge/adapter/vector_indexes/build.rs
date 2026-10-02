use super::{CassieError, Midge, VectorIndexRecord, VectorIndexState};

impl Midge {
    /// Builds and publishes vector metadata after its sidecars are complete.
    ///
    /// # Errors
    /// Returns a storage or vector validation error.
    pub fn put_vector_index(&self, metadata: VectorIndexRecord) -> Result<(), CassieError> {
        let metadata = self.prepare_vector_index_sidecars(metadata)?;
        self.write_vector_index_metadata(&metadata)
    }

    /// # Errors
    ///
    /// Returns an error when validation, storage, or execution fails.
    pub(in crate::midge::adapter) fn prepare_vector_index_sidecars(
        &self,
        mut metadata: crate::embeddings::VectorIndexRecord,
    ) -> Result<VectorIndexRecord, CassieError> {
        let requested_collection = metadata.collection.clone();
        metadata.collection = self.canonical_collection_name(&metadata.collection);
        let records = self.normalized_vector_records_for_index(&metadata)?;
        let state = match metadata.metadata.index_type {
            crate::embeddings::VectorIndexType::Hnsw => VectorIndexState {
                built_generation: 0,
                hnsw_graph: Some(Self::build_hnsw_graph_from_records(
                    &metadata,
                    records.clone(),
                )),
                ivfflat_training: None,
            },
            crate::embeddings::VectorIndexType::IvfFlat => VectorIndexState {
                built_generation: 0,
                hnsw_graph: None,
                ivfflat_training: Some(Self::build_ivfflat_training_from_records(
                    &metadata, &records,
                )),
            },
            crate::embeddings::VectorIndexType::BruteForce => VectorIndexState::default(),
        };
        let hnsw_graph = state.hnsw_graph.clone();
        let ivfflat_training = state.ivfflat_training.clone();
        metadata.metadata.hnsw_graph = None;
        metadata.metadata.ivfflat_training = None;
        let mut stored_records = records;
        for record in &mut stored_records {
            record.collection.clone_from(&requested_collection);
        }
        let is_initial_build = self
            .get_vector_index(&metadata.collection, &metadata.field)?
            .is_none();
        if is_initial_build {
            self.write_initial_vector_index_sidecars(&metadata, &stored_records, &state)?;
        } else {
            self.write_normalized_vectors_for_index(&metadata, &stored_records)?;
            self.write_vector_index_state(&metadata.collection, &metadata.field, state)?;
        }
        if let Some(graph) = hnsw_graph {
            self.write_hnsw_source_summary(&metadata.collection, &metadata.field, &graph)?;
        } else if let Some(training) = ivfflat_training {
            self.write_ivfflat_source_summary(
                &metadata.collection,
                &metadata.field,
                training.source_fingerprint,
                training.row_count,
            )?;
        }
        Ok(metadata)
    }
}
