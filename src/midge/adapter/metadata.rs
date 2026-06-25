use super::*;

impl Midge {
    pub fn put_vector_index(
        &self,
        mut metadata: crate::embeddings::VectorIndexRecord,
    ) -> Result<(), CassieError> {
        self.write_vector_index_metadata(&metadata)?;
        self.rebuild_normalized_vectors_for_index(&metadata)?;
        if metadata.metadata.index_type == crate::embeddings::VectorIndexType::IvfFlat {
            metadata.metadata.ivfflat_training = Some(self.rebuild_ivfflat_training(&metadata)?);
            self.write_vector_index_metadata(&metadata)?;
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

    pub fn put_index(&self, metadata: IndexMeta) -> Result<(), CassieError> {
        let mut tx = self.begin_schema_rw_tx()?;
        let key = Self::index_key(&metadata.collection, &metadata.name);
        let value =
            serde_json::to_vec(&metadata).map_err(|error| CassieError::Parse(error.to_string()))?;
        tx.put(key, value, None).map_err(CassieError::from)?;
        tx.commit(cntryl_midge::WriteOptions::sync())
            .map_err(CassieError::from)?;
        if metadata.kind == IndexKind::Scalar {
            self.rebuild_scalar_index_for_index(&metadata)?;
        }
        if metadata.kind == IndexKind::TimeSeries {
            self.rebuild_time_series_index_for_index(&metadata)?;
        }
        Ok(())
    }

    pub fn get_index(
        &self,
        collection: &str,
        name: &str,
    ) -> Result<Option<IndexMeta>, CassieError> {
        let tx = self.begin_schema_readonly_tx()?;
        let raw = tx
            .get(&Self::index_key(collection, name))
            .map_err(CassieError::from)?;
        let Some(raw) = raw else {
            return Ok(None);
        };

        serde_json::from_slice(&raw)
            .map(Some)
            .map_err(|error| CassieError::Parse(format!("invalid index metadata: {error}")))
    }

    pub fn list_indexes(&self) -> Result<Vec<IndexMeta>, CassieError> {
        let entries = self.raw_scan_prefix(StorageFamily::Schema, &Self::index_prefix())?;
        let mut out = Vec::with_capacity(entries.len());

        for (_key, raw_value) in entries {
            let Ok(record) = serde_json::from_slice(&raw_value) else {
                continue;
            };
            out.push(record);
        }

        Ok(out)
    }

    pub fn delete_index(&self, collection: &str, name: &str) -> Result<(), CassieError> {
        let metadata = self.get_index(collection, name)?;
        let mut tx = self.begin_schema_rw_tx()?;
        tx.delete(Self::index_key(collection, name))
            .map_err(CassieError::from)?;
        tx.commit(cntryl_midge::WriteOptions::sync())
            .map_err(CassieError::from)?;
        if let Some(index) = metadata {
            if index.kind == IndexKind::Scalar {
                self.delete_scalar_index_data(collection, name)?;
            }
            if index.kind == IndexKind::TimeSeries {
                self.delete_time_series_index_data(collection, name)?;
            }
        }
        Ok(())
    }

    pub fn put_rollup(&self, metadata: RollupMeta) -> Result<(), CassieError> {
        let mut tx = self.begin_schema_rw_tx()?;
        let value =
            serde_json::to_vec(&metadata).map_err(|error| CassieError::Parse(error.to_string()))?;
        tx.put(Self::rollup_key(&metadata.name), value, None)
            .map_err(CassieError::from)?;
        tx.commit(cntryl_midge::WriteOptions::sync())
            .map_err(CassieError::from)?;
        Ok(())
    }

    pub fn list_rollups(&self) -> Result<Vec<RollupMeta>, CassieError> {
        let entries = self.raw_scan_prefix(StorageFamily::Schema, &Self::rollup_prefix())?;
        let mut out = Vec::with_capacity(entries.len());
        for (_key, raw_value) in entries {
            let Ok(record) = serde_json::from_slice(&raw_value) else {
                continue;
            };
            out.push(record);
        }
        Ok(out)
    }

    pub fn delete_rollup(&self, name: &str) -> Result<(), CassieError> {
        let mut tx = self.begin_schema_rw_tx()?;
        tx.delete(Self::rollup_key(name))
            .map_err(CassieError::from)?;
        tx.commit(cntryl_midge::WriteOptions::sync())
            .map_err(CassieError::from)?;
        Ok(())
    }

    pub fn put_retention_policy(&self, metadata: RetentionPolicyMeta) -> Result<(), CassieError> {
        let mut tx = self.begin_schema_rw_tx()?;
        let value =
            serde_json::to_vec(&metadata).map_err(|error| CassieError::Parse(error.to_string()))?;
        tx.put(Self::retention_key(&metadata.name), value, None)
            .map_err(CassieError::from)?;
        tx.commit(cntryl_midge::WriteOptions::sync())
            .map_err(CassieError::from)?;
        Ok(())
    }

    pub fn list_retention_policies(&self) -> Result<Vec<RetentionPolicyMeta>, CassieError> {
        let entries = self.raw_scan_prefix(StorageFamily::Schema, &Self::retention_prefix())?;
        let mut out = Vec::with_capacity(entries.len());
        for (_key, raw_value) in entries {
            let Ok(record) = serde_json::from_slice(&raw_value) else {
                continue;
            };
            out.push(record);
        }
        Ok(out)
    }

    pub fn delete_retention_policy(&self, name: &str) -> Result<(), CassieError> {
        let mut tx = self.begin_schema_rw_tx()?;
        tx.delete(Self::retention_key(name))
            .map_err(CassieError::from)?;
        tx.commit(cntryl_midge::WriteOptions::sync())
            .map_err(CassieError::from)?;
        Ok(())
    }

    pub fn put_graph(&self, metadata: crate::catalog::GraphMeta) -> Result<(), CassieError> {
        let mut tx = self.begin_schema_rw_tx()?;
        let value =
            serde_json::to_vec(&metadata).map_err(|error| CassieError::Parse(error.to_string()))?;
        tx.put(Self::graph_key(&metadata.name), value, None)
            .map_err(CassieError::from)?;
        tx.commit(cntryl_midge::WriteOptions::sync())
            .map_err(CassieError::from)?;
        Ok(())
    }

    pub fn list_graphs(&self) -> Result<Vec<crate::catalog::GraphMeta>, CassieError> {
        let entries = self.raw_scan_prefix(StorageFamily::Schema, &Self::graph_prefix())?;
        let mut out = Vec::with_capacity(entries.len());
        for (_key, raw_value) in entries {
            let Ok(record) = serde_json::from_slice(&raw_value) else {
                continue;
            };
            out.push(record);
        }
        Ok(out)
    }

    pub fn delete_vector_index(&self, collection: &str, field: &str) -> Result<(), CassieError> {
        let mut tx = self.begin_schema_rw_tx()?;
        tx.delete(Self::vector_index_key(collection, field))
            .map_err(CassieError::from)?;
        tx.commit(cntryl_midge::WriteOptions::sync())
            .map_err(CassieError::from)?;

        let mut data_tx = self.begin_data_rw_tx()?;
        Self::delete_normalized_vector_keys_with_prefix(
            &mut data_tx,
            Self::normalized_vector_prefix(collection, field),
        )?;
        data_tx
            .commit(cntryl_midge::WriteOptions::sync())
            .map_err(CassieError::from)?;
        Ok(())
    }

    pub fn put_projection_comparison_report(
        &self,
        report: crate::catalog::ProjectionComparisonReportMeta,
    ) -> Result<(), CassieError> {
        let mut tx = self.begin_schema_rw_tx()?;
        let value =
            serde_json::to_vec(&report).map_err(|error| CassieError::Parse(error.to_string()))?;
        tx.put(
            Self::projection_comparison_report_key(&report.report_id),
            value,
            None,
        )
        .map_err(CassieError::from)?;
        tx.commit(cntryl_midge::WriteOptions::sync())
            .map_err(CassieError::from)?;
        Ok(())
    }

    pub fn list_projection_comparison_reports(
        &self,
    ) -> Result<Vec<crate::catalog::ProjectionComparisonReportMeta>, CassieError> {
        let entries = self.raw_scan_prefix(
            StorageFamily::Schema,
            &Self::projection_comparison_report_prefix(),
        )?;
        let mut out = Vec::with_capacity(entries.len());
        for (_key, raw_value) in entries {
            let Ok(report) = serde_json::from_slice(&raw_value) else {
                continue;
            };
            out.push(report);
        }
        out.sort_by_key(|report: &crate::catalog::ProjectionComparisonReportMeta| {
            report.report_id.clone()
        });
        Ok(out)
    }

    pub fn put_projection_consistency_report(
        &self,
        report: crate::catalog::ProjectionConsistencyReportMeta,
    ) -> Result<(), CassieError> {
        let mut tx = self.begin_schema_rw_tx()?;
        let value =
            serde_json::to_vec(&report).map_err(|error| CassieError::Parse(error.to_string()))?;
        tx.put(
            Self::projection_consistency_report_key(&report.report_id),
            value,
            None,
        )
        .map_err(CassieError::from)?;
        tx.commit(cntryl_midge::WriteOptions::sync())
            .map_err(CassieError::from)?;
        Ok(())
    }

    pub fn list_projection_consistency_reports(
        &self,
    ) -> Result<Vec<crate::catalog::ProjectionConsistencyReportMeta>, CassieError> {
        let entries = self.raw_scan_prefix(
            StorageFamily::Schema,
            &Self::projection_consistency_report_prefix(),
        )?;
        let mut out = Vec::with_capacity(entries.len());
        for (_key, raw_value) in entries {
            let Ok(report) = serde_json::from_slice(&raw_value) else {
                continue;
            };
            out.push(report);
        }
        out.sort_by_key(|report: &crate::catalog::ProjectionConsistencyReportMeta| {
            report.report_id.clone()
        });
        Ok(out)
    }

    pub fn save_constraints(
        &self,
        collection: &str,
        constraints: &[FieldConstraint],
    ) -> Result<(), CassieError> {
        let mut tx = self.begin_schema_rw_tx()?;
        let value = serde_json::to_vec(constraints)
            .map_err(|error| CassieError::Parse(error.to_string()))?;
        tx.put(Self::constraints_key(collection), value, None)
            .map_err(CassieError::from)?;
        tx.commit(cntryl_midge::WriteOptions::sync())
            .map_err(CassieError::from)?;
        Ok(())
    }

    pub fn load_constraints(&self, collection: &str) -> Result<Vec<FieldConstraint>, CassieError> {
        let tx = self.begin_schema_readonly_tx()?;
        let raw = tx
            .get(&Self::constraints_key(collection))
            .map_err(CassieError::from)?;
        let Some(raw) = raw else {
            return Ok(Vec::new());
        };

        serde_json::from_slice(&raw)
            .map_err(|error| CassieError::Parse(format!("invalid constraint metadata: {error}")))
    }

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

    pub fn get_cardinality_stats(
        &self,
        collection: &str,
    ) -> Result<Option<CollectionCardinalityStats>, CassieError> {
        let tx = self.begin_schema_readonly_tx()?;
        Self::load_cardinality_stats_from_tx(&tx, collection)
    }

    pub fn list_cardinality_stats(
        &self,
    ) -> Result<std::collections::HashMap<String, CollectionCardinalityStats>, CassieError> {
        let entries = self.raw_scan_prefix(StorageFamily::Schema, &Self::cardinality_prefix())?;
        let prefix = Self::cardinality_prefix();
        let mut out = std::collections::HashMap::new();
        for (key, raw_value) in entries {
            let Ok(stats) = serde_json::from_slice::<CollectionCardinalityStats>(&raw_value) else {
                continue;
            };
            if let Some(collection) = key_encoding::utf8_suffix_after_prefix(&key, &prefix) {
                out.insert(collection, stats);
            }
        }
        Ok(out)
    }

    pub fn save_cardinality_stats(
        &self,
        collection: &str,
        stats: &CollectionCardinalityStats,
    ) -> Result<(), CassieError> {
        let mut tx = self.begin_schema_rw_tx()?;
        Self::save_cardinality_stats_to_tx(&mut tx, collection, stats)?;
        tx.commit(WriteOptions::sync()).map_err(CassieError::from)?;
        Ok(())
    }

    pub fn delete_cardinality_stats(&self, collection: &str) -> Result<(), CassieError> {
        let mut tx = self.begin_schema_rw_tx()?;
        tx.delete(Self::cardinality_key(collection))
            .map_err(CassieError::from)?;
        tx.commit(WriteOptions::sync()).map_err(CassieError::from)?;
        Ok(())
    }

    pub fn rebuild_cardinality_stats_for_collection(
        &self,
        collection: &str,
    ) -> Result<CollectionCardinalityStats, CassieError> {
        let documents = self.scan_documents(collection)?;
        let row_count = documents.len() as u64;
        let mut stats = CollectionCardinalityStats {
            row_count,
            hydrated: true,
            ..CollectionCardinalityStats::default()
        };

        if let Some(schema) = self.collection_schema(collection) {
            for field in schema.fields {
                stats.set_field_stats(
                    field.name.clone(),
                    super::cardinality_stats::field_cardinality_stats(&documents, &field.name),
                );
            }
        }

        for index in self.list_indexes()? {
            if index.collection != collection {
                continue;
            }
            let cardinality = documents
                .iter()
                .filter(|document| payload_contains_index_membership(&document.payload, &index))
                .count() as u64;
            stats.set_index_cardinality(
                CollectionCardinalityStats::index_key(&index.kind, &index.name),
                cardinality,
            );
        }

        for record in self.list_vector_indexes()? {
            if record.collection != collection {
                continue;
            }
            let cardinality = documents
                .iter()
                .filter(|document| payload_contains_vector_membership(&document.payload, &record))
                .count() as u64;
            stats.set_index_cardinality(
                CollectionCardinalityStats::vector_index_key(&record.field),
                cardinality,
            );
        }

        self.save_cardinality_stats(collection, &stats)?;
        Ok(stats)
    }

    pub(super) fn normalized_vector_record_from_value(
        collection: &str,
        field: &str,
        id: &str,
        dimensions: usize,
        metric: &crate::embeddings::DistanceMetric,
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
            vector.push(number as f32);
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
            metric: metric.clone(),
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

    pub fn rebuild_normalized_vectors_for_index(
        &self,
        index: &VectorIndexRecord,
    ) -> Result<usize, CassieError> {
        let documents = self.scan_documents(&index.collection)?;
        let mut records = Vec::new();

        for document in documents {
            let Some(record) = Self::normalized_vector_record_from_value(
                &index.collection,
                &index.field,
                &document.id,
                index.metadata.dimensions,
                &index.metadata.metric,
                document.payload.get(&index.field),
            )?
            else {
                continue;
            };
            records.push(record);
        }

        let mut tx = self.begin_data_rw_tx()?;
        Self::delete_normalized_vector_keys_with_prefix(
            &mut tx,
            Self::normalized_vector_prefix(&index.collection, &index.field),
        )?;
        Self::write_normalized_vector_records(&mut tx, &records)?;
        tx.commit(WriteOptions::sync()).map_err(CassieError::from)?;
        Ok(records.len())
    }

    pub fn rebuild_ivfflat_training(
        &self,
        index: &VectorIndexRecord,
    ) -> Result<crate::embeddings::IvfFlatTrainingState, CassieError> {
        let options = index.metadata.ivfflat.clone().unwrap_or_default();
        let records = self.list_normalized_vectors(&index.collection, &index.field)?;
        let row_count = records.len();
        let lists = options.lists.max(1).min(row_count.max(1));
        let probes = options.probes.max(1).min(lists);

        if records.is_empty() {
            return Ok(crate::embeddings::IvfFlatTrainingState {
                version: 1,
                trained: false,
                row_count,
                lists,
                probes,
                training_seed: options.training_seed,
                centroid_ids: Vec::new(),
                centroids: Vec::new(),
                assignments: Default::default(),
                list_sizes: vec![0; lists],
            });
        }

        let mut sample = records.clone();
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
        for record in &records {
            let list = nearest_ivfflat_centroid(&record.values, &centroids);
            assignments.insert(record.id.clone(), list);
            if let Some(size) = list_sizes.get_mut(list) {
                *size += 1;
            }
        }

        Ok(crate::embeddings::IvfFlatTrainingState {
            version: 1,
            trained: true,
            row_count,
            lists,
            probes,
            training_seed: options.training_seed,
            centroid_ids,
            centroids,
            assignments,
            list_sizes,
        })
    }

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

    pub fn put_function(&self, metadata: crate::catalog::FunctionMeta) -> Result<(), CassieError> {
        let mut tx = self.begin_schema_rw_tx()?;
        let key = Self::function_key(&metadata.name);
        let value =
            serde_json::to_vec(&metadata).map_err(|error| CassieError::Parse(error.to_string()))?;
        tx.put(key, value, None).map_err(CassieError::from)?;
        tx.commit(cntryl_midge::WriteOptions::sync())
            .map_err(CassieError::from)?;
        Ok(())
    }

    pub fn get_function(
        &self,
        name: &str,
    ) -> Result<Option<crate::catalog::FunctionMeta>, CassieError> {
        let tx = self.begin_schema_readonly_tx()?;
        let raw = tx
            .get(&Self::function_key(name))
            .map_err(CassieError::from)?;
        let Some(raw) = raw else {
            return Ok(None);
        };

        serde_json::from_slice(&raw)
            .map(Some)
            .map_err(|error| CassieError::Parse(format!("invalid function metadata: {error}")))
    }

    pub fn list_functions(&self) -> Result<Vec<crate::catalog::FunctionMeta>, CassieError> {
        let entries = self.raw_scan_prefix(StorageFamily::Schema, &Self::function_prefix())?;
        let mut out: Vec<crate::catalog::FunctionMeta> = Vec::with_capacity(entries.len());
        for (_key, raw_value) in entries {
            let Ok(record) = serde_json::from_slice(&raw_value) else {
                continue;
            };
            out.push(record);
        }
        out.sort_by_key(|metadata| metadata.name.to_ascii_lowercase());
        Ok(out)
    }

    pub fn delete_function(&self, name: &str) -> Result<(), CassieError> {
        let mut tx = self.begin_schema_rw_tx()?;
        tx.delete(Self::function_key(name))
            .map_err(CassieError::from)?;
        tx.commit(cntryl_midge::WriteOptions::sync())
            .map_err(CassieError::from)?;
        Ok(())
    }

    pub fn put_procedure(
        &self,
        metadata: crate::catalog::ProcedureMeta,
    ) -> Result<(), CassieError> {
        let mut tx = self.begin_schema_rw_tx()?;
        let key = Self::procedure_key(&metadata.name);
        let value =
            serde_json::to_vec(&metadata).map_err(|error| CassieError::Parse(error.to_string()))?;
        tx.put(key, value, None).map_err(CassieError::from)?;
        tx.commit(cntryl_midge::WriteOptions::sync())
            .map_err(CassieError::from)?;
        Ok(())
    }

    pub fn get_procedure(
        &self,
        name: &str,
    ) -> Result<Option<crate::catalog::ProcedureMeta>, CassieError> {
        let tx = self.begin_schema_readonly_tx()?;
        let raw = tx
            .get(&Self::procedure_key(name))
            .map_err(CassieError::from)?;
        let Some(raw) = raw else {
            return Ok(None);
        };

        serde_json::from_slice(&raw)
            .map(Some)
            .map_err(|error| CassieError::Parse(format!("invalid procedure metadata: {error}")))
    }

    pub fn list_procedures(&self) -> Result<Vec<crate::catalog::ProcedureMeta>, CassieError> {
        let entries = self.raw_scan_prefix(StorageFamily::Schema, &Self::procedure_prefix())?;
        let mut out: Vec<crate::catalog::ProcedureMeta> = Vec::with_capacity(entries.len());
        for (_key, raw_value) in entries {
            let Ok(record) = serde_json::from_slice(&raw_value) else {
                continue;
            };
            out.push(record);
        }
        out.sort_by_key(|metadata| metadata.name.to_ascii_lowercase());
        Ok(out)
    }

    pub fn delete_procedure(&self, name: &str) -> Result<(), CassieError> {
        let mut tx = self.begin_schema_rw_tx()?;
        tx.delete(Self::procedure_key(name))
            .map_err(CassieError::from)?;
        tx.commit(cntryl_midge::WriteOptions::sync())
            .map_err(CassieError::from)?;
        Ok(())
    }

    pub fn put_view(&self, metadata: crate::catalog::ViewMeta) -> Result<(), CassieError> {
        let mut tx = self.begin_schema_rw_tx()?;
        let key = Self::view_key(&metadata.name);
        let value =
            serde_json::to_vec(&metadata).map_err(|error| CassieError::Parse(error.to_string()))?;
        tx.put(key, value, None).map_err(CassieError::from)?;
        tx.commit(cntryl_midge::WriteOptions::sync())
            .map_err(CassieError::from)?;
        Ok(())
    }

    pub fn get_view(&self, name: &str) -> Result<Option<crate::catalog::ViewMeta>, CassieError> {
        let tx = self.begin_schema_readonly_tx()?;
        let raw = tx.get(&Self::view_key(name)).map_err(CassieError::from)?;
        let Some(raw) = raw else {
            return Ok(None);
        };

        serde_json::from_slice(&raw)
            .map(Some)
            .map_err(|error| CassieError::Parse(format!("invalid view metadata: {error}")))
    }

    pub fn list_views(&self) -> Result<Vec<crate::catalog::ViewMeta>, CassieError> {
        let entries = self.raw_scan_prefix(StorageFamily::Schema, &Self::view_prefix())?;
        let mut out: Vec<crate::catalog::ViewMeta> = Vec::with_capacity(entries.len());
        for (_key, raw_value) in entries {
            let Ok(record) = serde_json::from_slice(&raw_value) else {
                continue;
            };
            out.push(record);
        }
        out.sort_by_key(|metadata| metadata.name.to_ascii_lowercase());
        Ok(out)
    }

    pub fn delete_view(&self, name: &str) -> Result<(), CassieError> {
        let mut tx = self.begin_schema_rw_tx()?;
        tx.delete(Self::view_key(name)).map_err(CassieError::from)?;
        tx.commit(cntryl_midge::WriteOptions::sync())
            .map_err(CassieError::from)?;
        Ok(())
    }

    pub fn put_role(&self, metadata: RoleMeta) -> Result<(), CassieError> {
        let mut tx = self.begin_schema_rw_tx()?;
        let key = Self::role_key(&metadata.name);
        let value =
            serde_json::to_vec(&metadata).map_err(|error| CassieError::Parse(error.to_string()))?;
        tx.put(key, value, None).map_err(CassieError::from)?;
        tx.commit(cntryl_midge::WriteOptions::sync())
            .map_err(CassieError::from)?;
        Ok(())
    }

    pub fn get_role(&self, name: &str) -> Result<Option<RoleMeta>, CassieError> {
        let tx = self.begin_schema_readonly_tx()?;
        let raw = tx.get(&Self::role_key(name)).map_err(CassieError::from)?;
        let Some(raw) = raw else {
            return Ok(None);
        };

        serde_json::from_slice(&raw)
            .map(Some)
            .map_err(|error| CassieError::Parse(format!("invalid role metadata: {error}")))
    }

    pub fn list_roles(&self) -> Result<Vec<RoleMeta>, CassieError> {
        let entries = self.raw_scan_prefix(StorageFamily::Schema, &Self::role_prefix())?;
        let mut out: Vec<RoleMeta> = Vec::with_capacity(entries.len());
        for (_key, raw_value) in entries {
            let Ok(record) = serde_json::from_slice(&raw_value) else {
                continue;
            };
            out.push(record);
        }
        out.sort_by_key(|metadata| metadata.name.to_ascii_lowercase());
        Ok(out)
    }

    pub fn delete_role(&self, name: &str) -> Result<(), CassieError> {
        let mut tx = self.begin_schema_rw_tx()?;
        tx.delete(Self::role_key(name)).map_err(CassieError::from)?;
        tx.commit(cntryl_midge::WriteOptions::sync())
            .map_err(CassieError::from)?;
        Ok(())
    }

    pub fn collection_schema(&self, name: &str) -> Option<Schema> {
        let tx = self.begin_schema_readonly_tx().ok()?;
        if let Ok(Some(row_schema)) = Self::load_row_schema_from_tx(&tx, name) {
            return Some(row_schema.active_schema());
        }
        let raw = tx.get(&Self::collection_schema_key(name)).ok()??;
        serde_json::from_slice(&raw).ok()
    }

    pub fn projection_metadata(
        &self,
        collection: &str,
    ) -> Result<Option<ProjectionMeta>, CassieError> {
        let tx = self.begin_schema_readonly_tx()?;
        Self::load_projection_metadata_from_tx(&tx, collection)
    }

    pub fn list_collections(&self) -> Vec<String> {
        let tx = match self.begin_schema_readonly_tx() {
            Ok(tx) => tx,
            Err(_) => return Vec::new(),
        };

        self.load_collections(&tx)
            .map(|mut values| {
                values.sort();
                values
            })
            .unwrap_or_else(|_| Vec::new())
    }

    pub fn list_collections_from_schema(&self) -> Vec<String> {
        let tx = match self.begin_schema_readonly_tx() {
            Ok(tx) => tx,
            Err(_) => return Vec::new(),
        };
        let Ok(mut scan) = tx.scan(&Query::new().prefix(Self::schema_collection_prefix().into()))
        else {
            return Vec::new();
        };

        let mut collections = Vec::new();
        let prefix = Self::schema_collection_prefix();
        while let Some((raw_key, _raw_value)) = scan.next() {
            if let Some(name) = key_encoding::utf8_suffix_after_prefix(&raw_key, &prefix) {
                collections.push(name);
            }
        }

        collections.sort();
        collections.dedup();
        collections
    }
}

fn ivfflat_training_order(seed: u64, id: &str) -> u64 {
    let mut state = 0xcbf29ce484222325_u64 ^ seed;
    for byte in id.as_bytes() {
        state ^= u64::from(*byte);
        state = state.wrapping_mul(0x100000001b3);
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
        .map(|(index, _)| index)
        .unwrap_or(0)
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
