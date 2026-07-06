use super::{
    normalize_vector, CassieError, Midge, NormalizedVectorRecord, Query, StorageFamily,
    VectorIndexRecord, WriteOptions,
};

impl Midge {
    /// # Errors
    ///
    /// Returns an error when validation, storage, or execution fails.
    pub fn put_vector_index(
        &self,
        mut metadata: crate::embeddings::VectorIndexRecord,
    ) -> Result<(), CassieError> {
        let records = self.normalized_vector_records_for_index(&metadata)?;
        match metadata.metadata.index_type {
            crate::embeddings::VectorIndexType::Hnsw => {
                metadata.metadata.hnsw_graph = Some(Self::build_hnsw_graph_from_records(
                    &metadata,
                    records.clone(),
                ));
            }
            crate::embeddings::VectorIndexType::IvfFlat => {
                metadata.metadata.ivfflat_training = Some(
                    Self::build_ivfflat_training_from_records(&metadata, &records),
                );
            }
            crate::embeddings::VectorIndexType::BruteForce => {}
        }
        self.write_normalized_vectors_for_index(&metadata, &records)?;
        self.write_vector_index_metadata(&metadata)?;
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
        let tx = self.begin_schema_readonly_tx()?;

        let raw = tx
            .get(&Self::vector_index_key(collection, field))
            .map_err(CassieError::from)?;
        let Some(raw) = raw else {
            return Ok(None);
        };

        serde_json::from_slice(&raw)
            .map(Some)
            .map_err(|error| CassieError::Parse(format!("invalid vector index metadata: {error}")))
    }

    /// # Errors
    ///
    /// Returns an error when validation, storage, or execution fails.
    pub fn list_vector_indexes(
        &self,
    ) -> Result<Vec<crate::embeddings::VectorIndexRecord>, CassieError> {
        let entries = self.raw_scan_prefix(StorageFamily::Schema, &Self::vector_index_prefix())?;
        let mut out = Vec::with_capacity(entries.len());

        for (_key, raw_value) in entries {
            let Ok(record) = serde_json::from_slice(&raw_value) else {
                continue;
            };
            out.push(record);
        }

        Ok(out)
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
        records: &[NormalizedVectorRecord],
    ) -> Result<(), CassieError> {
        for record in records {
            tx.put(
                Self::normalized_vector_key(&record.collection, &record.field, &record.id),
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
        let mut scan = tx
            .scan(&Query::new().prefix(prefix.into()))
            .map_err(CassieError::from)?;
        let mut keys = Vec::new();
        while let Some((key, _)) = scan.next() {
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

    fn normalized_vector_records_for_index(
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
        let mut tx = self.begin_data_rw_tx()?;
        Self::delete_normalized_vector_keys_with_prefix(
            &mut tx,
            Self::normalized_vector_prefix(&index.collection, &index.field),
        )?;
        Self::write_normalized_vector_records(&mut tx, records)?;
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
        for mut index in self.list_vector_indexes()? {
            if index.collection != collection
                || index.metadata.index_type != crate::embeddings::VectorIndexType::Hnsw
            {
                continue;
            }
            index.metadata.hnsw_graph = Some(self.rebuild_hnsw_graph(&index)?);
            self.write_vector_index_metadata(&index)?;
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
        for mut index in self.list_vector_indexes()? {
            if index.collection != collection
                || index.metadata.index_type != crate::embeddings::VectorIndexType::IvfFlat
            {
                continue;
            }
            index.metadata.ivfflat_training = Some(self.rebuild_ivfflat_training(&index)?);
            self.write_vector_index_metadata(&index)?;
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
        let entries = self.raw_scan_prefix(
            StorageFamily::Data,
            &Self::normalized_vector_prefix(collection, field),
        )?;
        let mut out: Vec<NormalizedVectorRecord> = Vec::with_capacity(entries.len());

        for (_key, raw_value) in entries {
            let Ok(record) = serde_json::from_slice(&raw_value) else {
                continue;
            };
            out.push(record);
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
        let tx = self.begin_data_readonly_tx()?;
        let raw = tx
            .get(&Self::normalized_vector_key(collection, field, id))
            .map_err(CassieError::from)?;
        let Some(raw) = raw else {
            return Ok(None);
        };

        serde_json::from_slice(&raw).map(Some).map_err(|error| {
            CassieError::Parse(format!("invalid normalized vector metadata: {error}"))
        })
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
