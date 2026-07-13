use super::{
    check_document_write_failure_point, collect_scan, normalize_vector, CassieError,
    DocumentWriteFailurePoint, Midge, NormalizedVectorRecord, Query, StorageFamily,
    VectorIndexRecord, VectorIndexState, WriteOptions,
};
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
struct PersistedHnswManifest {
    version: u32,
    source_fingerprint: u64,
    row_count: usize,
    dimensions: usize,
    metric: crate::embeddings::DistanceMetric,
    entry_point: Option<String>,
    max_layer: usize,
}

#[derive(Debug, Serialize, Deserialize)]
struct PersistedVectorIndexState {
    built_generation: u64,
    hnsw_graph: Option<PersistedHnswManifest>,
    ivfflat_training: Option<crate::embeddings::IvfFlatTrainingState>,
}

impl Midge {
    /// # Errors
    ///
    /// Returns an error when validation, storage, or execution fails.
    pub fn put_vector_index(
        &self,
        mut metadata: crate::embeddings::VectorIndexRecord,
    ) -> Result<(), CassieError> {
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
        metadata.metadata.hnsw_graph = None;
        metadata.metadata.ivfflat_training = None;
        let mut stored_records = records;
        for record in &mut stored_records {
            record.collection.clone_from(&requested_collection);
        }
        self.write_normalized_vectors_for_index(&metadata, &stored_records)?;
        self.write_vector_index_state(&metadata.collection, &metadata.field, state)?;
        self.write_vector_index_metadata(&metadata)?;
        Ok(())
    }

    /// # Errors
    ///
    /// Returns an error when storage state cannot be read.
    pub fn get_vector_index_state(
        &self,
        collection: &str,
        field: &str,
    ) -> Result<Option<VectorIndexState>, CassieError> {
        let collection = self.canonical_collection_name(collection);
        let tx = self.begin_data_readonly_tx_for(&collection)?;
        let Some(raw) = tx
            .get(&Self::vector_index_state_key(&collection, field))
            .map_err(CassieError::from)?
        else {
            return Ok(None);
        };
        let raw_json: serde_json::Value = serde_json::from_slice(&raw)
            .map_err(|error| CassieError::Parse(format!("invalid vector index state: {error}")))?;
        let state = if raw_json["hnsw_graph"].get("nodes").is_some() {
            serde_json::from_value(raw_json).map_err(|error| {
                CassieError::Parse(format!("invalid vector index state: {error}"))
            })?
        } else {
            let persisted: PersistedVectorIndexState =
                serde_json::from_value(raw_json).map_err(|error| {
                    CassieError::Parse(format!("invalid vector index state: {error}"))
                })?;
            let hnsw_graph = persisted
                .hnsw_graph
                .map(
                    |manifest| -> Result<crate::embeddings::HnswGraphState, CassieError> {
                        let prefix =
                            super::key_encoding::hnsw_graph_node_prefix(&collection, field);
                        let nodes = collect_scan(
                            tx.scan(&Query::new().prefix(prefix.into()))
                                .map_err(CassieError::from)?,
                        )?
                        .into_iter()
                        .map(|(_, raw)| {
                            serde_json::from_slice(raw.as_slice()).map_err(|error| {
                                CassieError::Parse(format!("invalid hnsw graph node: {error}"))
                            })
                        })
                        .collect::<Result<Vec<crate::embeddings::HnswGraphNode>, CassieError>>()?;
                        Ok(crate::embeddings::HnswGraphState {
                            version: manifest.version,
                            source_fingerprint: manifest.source_fingerprint,
                            row_count: manifest.row_count,
                            dimensions: manifest.dimensions,
                            metric: manifest.metric,
                            entry_point: manifest.entry_point,
                            max_layer: manifest.max_layer,
                            nodes,
                        })
                    },
                )
                .transpose()?;
            VectorIndexState {
                built_generation: persisted.built_generation,
                hnsw_graph,
                ivfflat_training: persisted.ivfflat_training,
            }
        };
        if state.built_generation != self.collection_generation(&collection)? {
            return Ok(None);
        }
        Ok(Some(state))
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
        let normalized_vectors = self.list_normalized_vectors(&collection, field)?;
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
        let persisted: PersistedVectorIndexState = serde_json::from_value(value)
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
        let Some(entry_point) = manifest.entry_point.as_deref() else {
            return Ok(None);
        };
        let node_prefix = super::key_encoding::hnsw_graph_node_prefix(&collection, field);
        let nodes = collect_scan(
            tx.scan(&Query::new().prefix(node_prefix.into()))
                .map_err(CassieError::from)?,
        )?
        .into_iter()
        .map(|(_, raw)| {
            serde_json::from_slice::<crate::embeddings::HnswGraphNode>(&raw)
                .map_err(|error| CassieError::Parse(format!("invalid hnsw graph node: {error}")))
        })
        .collect::<Result<Vec<_>, CassieError>>()?;
        let graph = crate::embeddings::HnswGraphState {
            version: manifest.version,
            source_fingerprint: manifest.source_fingerprint,
            row_count: manifest.row_count,
            dimensions: manifest.dimensions,
            metric: manifest.metric,
            entry_point: Some(entry_point.to_string()),
            max_layer: manifest.max_layer,
            nodes,
        };
        if let Some(reason) = crate::vector::hnsw::graph_fallback_reason(
            Some(&graph),
            manifest.metric,
            manifest.dimensions,
            &normalized_vectors,
        ) {
            return Err(CassieError::Execution(format!("hnsw fallback:{reason}")));
        }
        let result = crate::vector::hnsw::search_graph_with_node_loader(
            manifest.metric,
            entry_point,
            manifest.max_layer,
            query,
            options,
            limit,
            |id| {
                tx.get(&super::key_encoding::hnsw_graph_node_key(
                    &collection,
                    field,
                    id,
                ))
                .ok()
                .flatten()
                .and_then(|raw| serde_json::from_slice(&raw).ok())
            },
        );
        Ok(result)
    }

    /// # Errors
    ///
    /// Returns an error when derived vector-index state cannot be persisted.
    pub fn put_vector_index_state(
        &self,
        collection: &str,
        field: &str,
        mut state: VectorIndexState,
    ) -> Result<(), CassieError> {
        let collection = self.canonical_collection_name(collection);
        state.built_generation = self.collection_generation(&collection)?;
        self.write_vector_index_state(&collection, field, state)
    }

    fn write_vector_index_state(
        &self,
        collection: &str,
        field: &str,
        mut state: VectorIndexState,
    ) -> Result<(), CassieError> {
        state.built_generation = self.collection_generation(collection)?;
        let mut tx = self.begin_data_rw_tx_for(collection)?;
        Self::write_vector_index_state_to_tx(&mut tx, collection, field, &state)?;
        drop(state);
        tx.commit(WriteOptions::sync()).map_err(CassieError::from)
    }

    pub(super) fn write_vector_index_state_to_tx(
        tx: &mut cntryl_midge::Transaction,
        collection: &str,
        field: &str,
        state: &VectorIndexState,
    ) -> Result<(), CassieError> {
        let node_prefix = super::key_encoding::hnsw_graph_node_prefix(collection, field);
        let old_node_keys = collect_scan(
            tx.scan(&Query::new().prefix(node_prefix.into()))
                .map_err(CassieError::from)?,
        )?
        .into_iter()
        .map(|(key, _)| key)
        .collect::<Vec<_>>();
        for key in old_node_keys {
            tx.delete(key).map_err(CassieError::from)?;
        }
        let hnsw_graph = state
            .hnsw_graph
            .as_ref()
            .map(|graph| PersistedHnswManifest {
                version: graph.version,
                source_fingerprint: graph.source_fingerprint,
                row_count: graph.row_count,
                dimensions: graph.dimensions,
                metric: graph.metric,
                entry_point: graph.entry_point.clone(),
                max_layer: graph.max_layer,
            });
        let persisted = PersistedVectorIndexState {
            built_generation: state.built_generation,
            hnsw_graph,
            ivfflat_training: state.ivfflat_training.clone(),
        };
        let value = serde_json::to_vec(&persisted)
            .map_err(|error| CassieError::Parse(error.to_string()))?;
        tx.put(Self::vector_index_state_key(collection, field), value, None)
            .map_err(CassieError::from)?;
        if let Some(graph) = &state.hnsw_graph {
            for node in &graph.nodes {
                tx.put(
                    super::key_encoding::hnsw_graph_node_key(collection, field, &node.id),
                    serde_json::to_vec(node)
                        .map_err(|error| CassieError::Parse(error.to_string()))?,
                    None,
                )
                .map_err(CassieError::from)?;
            }
        }
        check_document_write_failure_point(DocumentWriteFailurePoint::VectorState)?;
        Ok(())
    }

    pub(super) fn refresh_vector_index_states_in_tx(
        tx: &mut cntryl_midge::Transaction,
        indexes: &[VectorIndexRecord],
        records_by_field: &[(String, Vec<NormalizedVectorRecord>)],
    ) -> Result<(), CassieError> {
        for index in indexes {
            let records = records_by_field
                .iter()
                .find(|(field, _)| field == &index.field)
                .map_or_else(Vec::new, |(_, records)| records.clone());
            let state = match index.metadata.index_type {
                crate::embeddings::VectorIndexType::Hnsw => VectorIndexState {
                    built_generation: 0,
                    hnsw_graph: Some(Self::build_hnsw_graph_from_records(index, records)),
                    ivfflat_training: None,
                },
                crate::embeddings::VectorIndexType::IvfFlat => VectorIndexState {
                    built_generation: 0,
                    hnsw_graph: None,
                    ivfflat_training: Some(Self::build_ivfflat_training_from_records(
                        index, &records,
                    )),
                },
                crate::embeddings::VectorIndexType::BruteForce => continue,
            };
            Self::write_vector_index_state_to_tx(tx, &index.collection, &index.field, &state)?;
        }
        Ok(())
    }

    pub(super) fn stamp_vector_index_states_generation_in_tx(
        &self,
        tx: &mut cntryl_midge::Transaction,
        collection: &str,
        built_generation: u64,
    ) -> Result<(), CassieError> {
        for index in self
            .list_vector_indexes_canonical()?
            .into_iter()
            .filter(|index| index.collection == collection)
        {
            let key = Self::vector_index_state_key(collection, &index.field);
            let Some(raw) = tx.get(&key).map_err(CassieError::from)? else {
                continue;
            };
            let Ok(mut state) = serde_json::from_slice::<serde_json::Value>(&raw) else {
                continue;
            };
            state["built_generation"] = serde_json::json!(built_generation);
            tx.put(
                key,
                serde_json::to_vec(&state)
                    .map_err(|error| CassieError::Parse(error.to_string()))?,
                None,
            )
            .map_err(CassieError::from)?;
        }
        Ok(())
    }

    pub(super) fn stamp_normalized_vectors_generation_in_tx(
        tx: &mut cntryl_midge::Transaction,
        collection: &str,
        built_generation: u64,
        records_by_field: &[(String, Vec<NormalizedVectorRecord>)],
    ) -> Result<(), CassieError> {
        for (field, records) in records_by_field {
            for record in records {
                let mut record = record.clone();
                record.built_generation = built_generation;
                tx.put(
                    Self::normalized_vector_key(collection, field, &record.id),
                    serde_json::to_vec(&record)
                        .map_err(|error| CassieError::Parse(error.to_string()))?,
                    None,
                )
                .map_err(CassieError::from)?;
            }
        }
        Ok(())
    }

    fn write_vector_index_metadata(
        &self,
        metadata: &crate::embeddings::VectorIndexRecord,
    ) -> Result<(), CassieError> {
        let mut tx = self.begin_schema_rw_tx()?;
        let key = Self::vector_index_key(&metadata.collection, &metadata.field);

        let value =
            serde_json::to_vec(&metadata).map_err(|error| CassieError::Parse(error.to_string()))?;
        tx.put(key, value, None).map_err(CassieError::from)?;
        tx.commit(cntryl_midge::WriteOptions::sync())
            .map_err(CassieError::from)?;
        Ok(())
    }

    /// # Errors
    ///
    /// Returns an error when validation, storage, or execution fails.
    pub fn get_vector_index(
        &self,
        collection: &str,
        field: &str,
    ) -> Result<Option<crate::embeddings::VectorIndexRecord>, CassieError> {
        let requested_collection = collection.to_string();
        let collection = self.canonical_collection_name(collection);
        let tx = self.begin_schema_readonly_tx()?;

        let raw = tx
            .get(&Self::vector_index_key(&collection, field))
            .map_err(CassieError::from)?;
        let Some(raw) = raw else {
            return Ok(None);
        };

        let mut record: VectorIndexRecord = serde_json::from_slice(&raw).map_err(|error| {
            CassieError::Parse(format!("invalid vector index metadata: {error}"))
        })?;
        self.hydrate_vector_index_state(&mut record)?;
        if !requested_collection.eq_ignore_ascii_case(&collection) {
            record.collection = self.display_collection_name(&requested_collection);
        }
        Ok(Some(record))
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
    ) -> Result<Option<crate::embeddings::VectorIndexRecord>, CassieError> {
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

    /// # Errors
    ///
    /// Returns an error when validation, storage, or execution fails.
    pub fn list_vector_indexes(
        &self,
    ) -> Result<Vec<crate::embeddings::VectorIndexRecord>, CassieError> {
        self.list_vector_indexes_canonical().map(|mut records| {
            for record in &mut records {
                record.collection = self.display_collection_name(&record.collection);
            }
            records
        })
    }

    pub(crate) fn list_vector_indexes_canonical(
        &self,
    ) -> Result<Vec<crate::embeddings::VectorIndexRecord>, CassieError> {
        let entries = self.raw_scan_prefix(StorageFamily::Schema, &Self::vector_index_prefix())?;
        let mut out = Vec::with_capacity(entries.len());

        for (_key, raw_value) in entries {
            let Ok(record) = serde_json::from_slice(&raw_value) else {
                continue;
            };
            let mut record = record;
            self.hydrate_vector_index_state(&mut record)?;
            out.push(record);
        }

        Ok(out)
    }

    fn hydrate_vector_index_state(
        &self,
        record: &mut VectorIndexRecord,
    ) -> Result<(), CassieError> {
        let Some(state) = self.get_vector_index_state(&record.collection, &record.field)? else {
            return Ok(());
        };
        record.metadata.hnsw_graph = state.hnsw_graph;
        record.metadata.ivfflat_training = state.ivfflat_training;
        Ok(())
    }

    pub(super) fn normalized_vector_record_from_value(
        collection: &str,
        field: &str,
        id: &str,
        dimensions: usize,
        metric: crate::embeddings::DistanceMetric,
        value: Option<&serde_json::Value>,
    ) -> Result<Option<NormalizedVectorRecord>, CassieError> {
        let Some(value) = value else {
            return Ok(None);
        };
        if value.is_null() {
            return Ok(None);
        }

        let values = value.as_array().ok_or_else(|| {
            CassieError::InvalidVector(format!(
                "vector field '{field}' on collection '{collection}' expects array values"
            ))
        })?;
        if values.len() != dimensions {
            return Err(CassieError::InvalidVector(format!(
                "vector field '{field}' on collection '{collection}' expects {dimensions} dimensions"
            )));
        }

        let mut vector = Vec::with_capacity(dimensions);
        for value in values {
            let Some(number) = value.as_f64() else {
                return Err(CassieError::InvalidVector(format!(
                    "vector field '{field}' on collection '{collection}' expects numeric values"
                )));
            };
            if !number.is_finite() {
                return Err(CassieError::InvalidVector(format!(
                    "vector field '{field}' on collection '{collection}' expects finite numeric values"
                )));
            }
            vector.push(number.to_string().parse::<f32>().map_err(|_| {
                CassieError::InvalidVector(format!(
                    "vector field '{field}' on collection '{collection}' expects f32-range values"
                ))
            })?);
        }

        let Some(normalized) = normalize_vector(&vector) else {
            return Err(CassieError::InvalidVector(format!(
                "vector field '{field}' on collection '{collection}' could not be normalized"
            )));
        };

        Ok(Some(NormalizedVectorRecord {
            collection: collection.to_string(),
            field: field.to_string(),
            id: id.to_string(),
            built_generation: 0,
            dimensions,
            metric,
            normalization_version: NormalizedVectorRecord::CURRENT_NORMALIZATION_VERSION,
            payload_available: true,
            magnitude: normalized.magnitude,
            values: normalized.values,
        }))
    }

    pub(super) fn write_normalized_vector_records(
        tx: &mut cntryl_midge::Transaction,
        collection: &str,
        records: &[NormalizedVectorRecord],
    ) -> Result<(), CassieError> {
        for record in records {
            tx.put(
                Self::normalized_vector_key(collection, &record.field, &record.id),
                serde_json::to_vec(record)
                    .map_err(|error| CassieError::Parse(error.to_string()))?,
                None,
            )
            .map_err(CassieError::from)?;
        }

        Ok(())
    }

    pub(super) fn delete_normalized_vector_keys_with_prefix(
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

    pub(super) fn delete_normalized_vector_keys_for_document(
        tx: &mut cntryl_midge::Transaction,
        collection: &str,
        id: &str,
        fields: &[String],
    ) -> Result<usize, CassieError> {
        let mut deleted_keys = 0usize;
        for field in fields {
            let key = Self::normalized_vector_key(collection, field, id);
            if tx.get(&key).map_err(CassieError::from)?.is_some() {
                tx.delete(key).map_err(CassieError::from)?;
                deleted_keys = deleted_keys.saturating_add(1);
            }
        }

        Ok(deleted_keys)
    }

    /// # Errors
    ///
    /// Returns an error when validation, storage, or execution fails.
    pub fn rebuild_normalized_vectors_for_index(
        &self,
        index: &VectorIndexRecord,
    ) -> Result<usize, CassieError> {
        let records = self.normalized_vector_records_for_index(index)?;
        self.write_normalized_vectors_for_index(index, &records)?;
        Ok(records.len())
    }

    pub(super) fn normalized_vector_records_for_index(
        &self,
        index: &VectorIndexRecord,
    ) -> Result<Vec<NormalizedVectorRecord>, CassieError> {
        let documents = self.scan_documents(&index.collection)?;
        let mut records = Vec::new();

        for document in documents {
            let Some(record) = Self::normalized_vector_record_from_value(
                &index.collection,
                &index.field,
                &document.id,
                index.metadata.dimensions,
                index.metadata.metric,
                document.payload.get(&index.field),
            )?
            else {
                continue;
            };
            records.push(record);
        }

        records.sort_by(|left, right| left.id.cmp(&right.id));
        Ok(records)
    }

    fn write_normalized_vectors_for_index(
        &self,
        index: &VectorIndexRecord,
        records: &[NormalizedVectorRecord],
    ) -> Result<(), CassieError> {
        let generation = self.collection_generation(&index.collection)?;
        let mut records = records.to_vec();
        for record in &mut records {
            record.built_generation = generation;
        }
        let mut tx = self.begin_data_rw_tx_for(&index.collection)?;
        Self::delete_normalized_vector_keys_with_prefix(
            &mut tx,
            Self::normalized_vector_prefix(&index.collection, &index.field),
        )?;
        for record in &records {
            tx.put(
                Self::normalized_vector_key(&index.collection, &record.field, &record.id),
                serde_json::to_vec(record)
                    .map_err(|error| CassieError::Parse(error.to_string()))?,
                None,
            )
            .map_err(CassieError::from)?;
        }
        tx.commit(WriteOptions::sync()).map_err(CassieError::from)?;
        Ok(())
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

    fn build_ivfflat_training_from_records(
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
                version: 1,
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
            version: 1,
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
    pub fn rebuild_hnsw_graph(
        &self,
        index: &VectorIndexRecord,
    ) -> Result<crate::embeddings::HnswGraphState, CassieError> {
        let records = self.list_normalized_vectors(&index.collection, &index.field)?;
        Ok(Self::build_hnsw_graph_from_records(index, records))
    }

    fn build_hnsw_graph_from_records(
        index: &VectorIndexRecord,
        records: Vec<NormalizedVectorRecord>,
    ) -> crate::embeddings::HnswGraphState {
        let options = index.metadata.hnsw.clone().unwrap_or_default();
        crate::vector::hnsw::build_graph(
            records,
            &options,
            index.metadata.dimensions,
            index.metadata.metric,
        )
    }

    /// # Errors
    ///
    /// Returns an error when validation, storage, or execution fails.
    pub fn refresh_hnsw_indexes_for_collection(
        &self,
        collection: &str,
    ) -> Result<usize, CassieError> {
        let mut refreshed = 0usize;
        for index in self.list_vector_indexes_canonical()? {
            if index.collection != collection
                || index.metadata.index_type != crate::embeddings::VectorIndexType::Hnsw
            {
                continue;
            }
            let state = VectorIndexState {
                built_generation: 0,
                hnsw_graph: Some(self.rebuild_hnsw_graph(&index)?),
                ivfflat_training: None,
            };
            self.write_vector_index_state(&index.collection, &index.field, state)?;
            refreshed += 1;
        }
        Ok(refreshed)
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

    /// # Errors
    ///
    /// Returns an error when validation, storage, or execution fails.
    pub fn list_normalized_vectors(
        &self,
        collection: &str,
        field: &str,
    ) -> Result<Vec<NormalizedVectorRecord>, CassieError> {
        let requested_collection = collection.to_string();
        let collection = self.canonical_collection_name(collection);
        let entries = self.raw_scan_prefix_for_collection(
            &collection,
            &Self::normalized_vector_prefix(&collection, field),
        )?;
        let mut out: Vec<NormalizedVectorRecord> = Vec::with_capacity(entries.len());

        for (_key, raw_value) in entries {
            let Ok(record) = serde_json::from_slice::<NormalizedVectorRecord>(&raw_value) else {
                continue;
            };
            let mut record = record;
            record.collection.clone_from(&requested_collection);
            out.push(record);
        }

        let generation = self.collection_generation(&collection)?;
        if out
            .iter()
            .any(|record| record.built_generation != generation)
        {
            return Ok(Vec::new());
        }
        out.sort_by(|left, right| left.id.cmp(&right.id));
        Ok(out)
    }

    /// # Errors
    ///
    /// Returns an error when validation, storage, or execution fails.
    pub fn get_normalized_vector(
        &self,
        collection: &str,
        field: &str,
        id: &str,
    ) -> Result<Option<NormalizedVectorRecord>, CassieError> {
        let requested_collection = collection.to_string();
        let collection = self.canonical_collection_name(collection);
        let tx = self.begin_data_readonly_tx_for(&collection)?;
        let raw = tx
            .get(&Self::normalized_vector_key(&collection, field, id))
            .map_err(CassieError::from)?;
        let Some(raw) = raw else {
            return Ok(None);
        };

        let mut record: NormalizedVectorRecord = serde_json::from_slice(&raw).map_err(|error| {
            CassieError::Parse(format!("invalid normalized vector metadata: {error}"))
        })?;
        if record.built_generation != self.collection_generation(&collection)? {
            return Ok(None);
        }
        record.collection = requested_collection;
        Ok(Some(record))
    }
}

fn ivfflat_training_order(seed: u64, id: &str) -> u64 {
    let mut state = 0xcbf2_9ce4_8422_2325_u64 ^ seed;
    for byte in id.as_bytes() {
        state ^= u64::from(*byte);
        state = state.wrapping_mul(0x0100_0000_01b3);
    }
    state
}

fn nearest_ivfflat_centroid(vector: &[f32], centroids: &[Vec<f32>]) -> usize {
    centroids
        .iter()
        .enumerate()
        .min_by(|(left_index, left), (right_index, right)| {
            squared_l2(vector, left)
                .total_cmp(&squared_l2(vector, right))
                .then_with(|| left_index.cmp(right_index))
        })
        .map_or(0, |(index, _)| index)
}

fn squared_l2(left: &[f32], right: &[f32]) -> f64 {
    left.iter()
        .zip(right.iter())
        .map(|(left, right)| {
            let delta = f64::from(*left) - f64::from(*right);
            delta * delta
        })
        .sum()
}
