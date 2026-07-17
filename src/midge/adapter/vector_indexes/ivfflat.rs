use super::codec::{decode_normalized_vector, decode_vector_index_state, PersistedIvfManifest};
use super::math::{ivfflat_training_order, nearest_ivfflat_centroid};
use super::{
    collect_scan, CassieError, Midge, NormalizedVectorRecord, Query, VectorIndexRecord,
    VectorIndexState, WriteOptions,
};
use std::collections::BTreeMap;

impl Midge {
    /// Audits every persisted `IVFFlat` sidecar and rebuilds latest-only state from source rows.
    ///
    /// # Errors
    ///
    /// Returns an error when index definitions or source rows cannot be read or repaired.
    pub(crate) fn reconcile_ivfflat_indexes(&self) -> Result<(), CassieError> {
        for index in self
            .list_vector_index_definitions_canonical()?
            .into_iter()
            .filter(|index| {
                index.metadata.index_type == crate::embeddings::VectorIndexType::IvfFlat
            })
        {
            let write_gate = self.collection_write_gate(&index.collection);
            let _write_guard = write_gate.lock();
            self.reconcile_ivfflat_index(&index)?;
        }
        Ok(())
    }

    fn reconcile_ivfflat_index(&self, index: &VectorIndexRecord) -> Result<(), CassieError> {
        let generation = self.collection_generation(&index.collection)?;
        let records = self.normalized_vector_records_for_index(index)?;
        let training = Self::build_ivfflat_training_from_records(index, &records);
        if self.ivfflat_sidecars_match(index, generation, &records, &training)? {
            return Ok(());
        }
        self.rebuild_ivfflat_sidecars(index, generation, &records, &training)
    }

    fn ivfflat_sidecars_match(
        &self,
        index: &VectorIndexRecord,
        generation: u64,
        expected_records: &[NormalizedVectorRecord],
        expected_training: &crate::embeddings::IvfFlatTrainingState,
    ) -> Result<bool, CassieError> {
        let (relation_id, field_id) = self.vector_storage_ids(&index.collection, &index.field)?;
        let tx = self.begin_data_readonly_tx_for(&index.collection)?;
        let Some(raw_state) = tx
            .get(&Self::vector_index_state_key(relation_id, field_id))
            .map_err(CassieError::from)?
        else {
            return Ok(false);
        };
        let Ok(state) = decode_vector_index_state(&raw_state) else {
            return Ok(false);
        };
        let Some(manifest) = state.ivfflat_training.as_ref() else {
            return Ok(false);
        };
        if state.built_generation != generation
            || state.hnsw_graph.is_some()
            || !persisted_manifest_matches(manifest, expected_training)
        {
            return Ok(false);
        }

        let membership_prefix =
            super::super::key_encoding::ivfflat_membership_prefix(relation_id, field_id);
        let membership_entries = collect_scan(
            tx.scan(&Query::new().prefix(membership_prefix.clone().into()))
                .map_err(CassieError::from)?,
        )?;
        if membership_entries.len() != manifest.membership_count {
            return Ok(false);
        }
        let mut assignments = BTreeMap::new();
        for (key, value) in membership_entries {
            let Some((list, id)) = super::super::key_encoding::decode_ivfflat_membership_suffix(
                &key,
                &membership_prefix,
            ) else {
                return Ok(false);
            };
            if !value.is_empty()
                || list >= expected_training.lists
                || assignments.insert(id, list).is_some()
            {
                return Ok(false);
            }
        }
        if assignments != expected_training.assignments {
            return Ok(false);
        }

        let normalized_prefix = Self::normalized_vector_prefix(relation_id, field_id);
        let normalized_entries = collect_scan(
            tx.scan(&Query::new().prefix(normalized_prefix.clone().into()))
                .map_err(CassieError::from)?,
        )?;
        let mut actual_records = Vec::with_capacity(normalized_entries.len());
        for (key, raw) in normalized_entries {
            let Some(id) =
                super::super::key_encoding::utf8_suffix_after_prefix(&key, &normalized_prefix)
            else {
                return Ok(false);
            };
            let Ok(record) = decode_normalized_vector(&raw, &index.collection, &index.field, &id)
            else {
                return Ok(false);
            };
            actual_records.push(record);
        }
        actual_records.sort_by(|left, right| left.id.cmp(&right.id));
        let mut expected_records = expected_records.to_vec();
        for record in &mut expected_records {
            record.built_generation = generation;
        }
        expected_records.sort_by(|left, right| left.id.cmp(&right.id));
        if actual_records != expected_records {
            return Ok(false);
        }

        let Some(raw_summary) = tx
            .get(&super::super::key_encoding::ivfflat_source_summary_key(
                relation_id,
                field_id,
            ))
            .map_err(CassieError::from)?
        else {
            return Ok(false);
        };
        let Ok(summary) =
            serde_json::from_slice::<super::vector_retrieval::HnswSourceSummary>(&raw_summary)
        else {
            return Ok(false);
        };
        Ok(summary.built_generation == generation
            && summary.source_fingerprint == expected_training.source_fingerprint
            && summary.row_count == expected_training.row_count)
    }

    fn rebuild_ivfflat_sidecars(
        &self,
        index: &VectorIndexRecord,
        generation: u64,
        records: &[NormalizedVectorRecord],
        training: &crate::embeddings::IvfFlatTrainingState,
    ) -> Result<(), CassieError> {
        let row_schema = self.row_schema(&index.collection)?;
        let (relation_id, field_id) = self.vector_storage_ids(&index.collection, &index.field)?;
        let mut records = records.to_vec();
        for record in &mut records {
            record.built_generation = generation;
        }
        let mut tx = self.begin_data_rw_tx_for(&index.collection)?;
        Self::delete_normalized_vector_keys_with_prefix(
            &mut tx,
            Self::normalized_vector_prefix(relation_id, field_id),
        )?;
        Self::write_normalized_vector_records(&mut tx, &row_schema, &records)?;
        Self::write_vector_index_state_to_tx(
            &mut tx,
            relation_id,
            field_id,
            &VectorIndexState {
                built_generation: generation,
                hnsw_graph: None,
                ivfflat_training: Some(training.clone()),
            },
        )?;
        Self::write_vector_source_summary_to_tx(
            &mut tx,
            relation_id,
            field_id,
            generation,
            training.source_fingerprint,
            training.row_count,
        )?;
        tx.commit(WriteOptions::sync()).map_err(CassieError::from)
    }

    /// # Errors
    ///
    /// Returns an error when validation, storage, or execution fails.
    pub fn rebuild_ivfflat_training(
        &self,
        index: &VectorIndexRecord,
    ) -> Result<crate::embeddings::IvfFlatTrainingState, CassieError> {
        let records = self.list_normalized_vectors(&index.collection, &index.field)?;
        Ok(Self::build_ivfflat_training_from_records(index, &records))
    }

    pub(super) fn build_ivfflat_training_from_records(
        index: &VectorIndexRecord,
        records: &[NormalizedVectorRecord],
    ) -> crate::embeddings::IvfFlatTrainingState {
        let options = index.metadata.ivfflat.clone().unwrap_or_default();
        let row_count = records.len();
        let lists = options.lists.max(1).min(row_count.max(1));
        let probes = options.probes.max(1).min(lists);
        let source_fingerprint = crate::vector::normalized_vector_source_fingerprint(records);

        if records.is_empty() {
            return crate::embeddings::IvfFlatTrainingState {
                version: crate::vector::ivfflat::TRAINING_VERSION,
                source_fingerprint,
                trained: false,
                row_count,
                lists,
                probes,
                training_seed: options.training_seed,
                centroid_ids: Vec::new(),
                centroids: Vec::new(),
                assignments: std::collections::BTreeMap::default(),
                list_sizes: vec![0; lists],
            };
        }

        let mut sample = records.to_vec();
        sample.sort_by_key(|record| ivfflat_training_order(options.training_seed, &record.id));
        sample.truncate(options.training_sample_size.min(sample.len()).max(lists));

        let mut centroids = sample
            .iter()
            .take(lists)
            .map(|record| record.values.clone())
            .collect::<Vec<_>>();
        while centroids.len() < lists {
            centroids.push(records[centroids.len() % records.len()].values.clone());
        }
        let centroid_ids = sample
            .iter()
            .take(lists)
            .map(|record| record.id.clone())
            .collect::<Vec<_>>();

        let mut assignments = std::collections::BTreeMap::new();
        let mut list_sizes = vec![0usize; lists];
        for record in records {
            let list = nearest_ivfflat_centroid(&record.values, &centroids);
            assignments.insert(record.id.clone(), list);
            if let Some(size) = list_sizes.get_mut(list) {
                *size += 1;
            }
        }

        crate::embeddings::IvfFlatTrainingState {
            version: crate::vector::ivfflat::TRAINING_VERSION,
            source_fingerprint,
            trained: true,
            row_count,
            lists,
            probes,
            training_seed: options.training_seed,
            centroid_ids,
            centroids,
            assignments,
            list_sizes,
        }
    }

    /// # Errors
    ///
    /// Returns an error when validation, storage, or execution fails.
    pub fn refresh_ivfflat_indexes_for_collection(
        &self,
        collection: &str,
    ) -> Result<usize, CassieError> {
        let mut refreshed = 0usize;
        for index in self.list_vector_indexes_canonical()? {
            if index.collection != collection
                || index.metadata.index_type != crate::embeddings::VectorIndexType::IvfFlat
            {
                continue;
            }
            let state = VectorIndexState {
                built_generation: 0,
                hnsw_graph: None,
                ivfflat_training: Some(self.rebuild_ivfflat_training(&index)?),
            };
            self.write_vector_index_state(&index.collection, &index.field, state)?;
            refreshed += 1;
        }
        Ok(refreshed)
    }
}

fn persisted_manifest_matches(
    manifest: &PersistedIvfManifest,
    expected: &crate::embeddings::IvfFlatTrainingState,
) -> bool {
    manifest.version == crate::vector::ivfflat::TRAINING_VERSION
        && manifest.version == expected.version
        && manifest.source_fingerprint == expected.source_fingerprint
        && manifest.trained == expected.trained
        && manifest.row_count == expected.row_count
        && manifest.lists == expected.lists
        && manifest.probes == expected.probes
        && manifest.training_seed == expected.training_seed
        && manifest.centroid_ids == expected.centroid_ids
        && manifest.centroids == expected.centroids
        && manifest.list_sizes == expected.list_sizes
        && manifest.membership_count == expected.assignments.len()
}

pub(super) fn load_ivfflat_manifest(
    tx: &cntryl_midge::Transaction,
    relation_id: u64,
    field_id: u32,
    manifest: PersistedIvfManifest,
) -> crate::embeddings::IvfFlatTrainingState {
    let prefix = super::super::key_encoding::ivfflat_membership_prefix(relation_id, field_id);
    let mut assignments = std::collections::BTreeMap::new();
    if let Ok(scan) = tx.scan(&Query::new().prefix(prefix.clone().into())) {
        if let Ok(entries) = collect_scan(scan) {
            for (key, _) in entries {
                if let Some((list, id)) =
                    super::super::key_encoding::decode_ivfflat_membership_suffix(&key, &prefix)
                {
                    assignments.insert(id, list);
                }
            }
        }
    }
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
        assignments,
        list_sizes: manifest.list_sizes,
    }
}
