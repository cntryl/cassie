use super::vector_helpers::{vector_from_json, vector_search_row, vector_search_columns, vector_distance_for_metric, ScoredVectorCandidate, compare_scored_vector_candidates};
use super::{Cassie, DistanceMetric, QueryResult, CassieError, VectorSearchResultCacheKey, Embedding, QueryEmbeddingCacheKey, Arc, VectorIndexRecord, VectorIndexType, RowDecode, BTreeMap, normalize_vector, BinaryHeap, NormalizedVectorRecord, cosine_distance_from_normalized_query, dot_distance_from_normalized_target, CollectionSchema, CmpOrdering, NormalizedVectorCacheEntry, NormalizedVectorCacheKey};

impl Cassie {
    /// # Errors
    ///
    /// Returns an error when validation, storage, or execution fails.
    pub fn execute_vector_search(
        &self,
        collection: &str,
        vector_field: &str,
        query: &str,
        metric: Option<DistanceMetric>,
        limit: usize,
        offset: usize,
    ) -> Result<QueryResult, CassieError> {
        let index = self
            .catalog
            .get_vector_index(collection, vector_field)
            .ok_or_else(|| {
                CassieError::InvalidEmbedding(format!(
                    "vector index not found for collection '{collection}', field '{vector_field}'"
                ))
            })?;

        self.validate_embedding_compatibility(&index, metric.as_ref())?;

        let metric = metric.unwrap_or(index.metadata.metric);
        let limit = limit.max(1);
        let catalog_version = self.catalog.version();
        let result_cache_key = VectorSearchResultCacheKey {
            catalog_version,
            provider: self.embedding_provider.provider_name().to_string(),
            model: self.embedding_provider.model_name().to_string(),
            dimensions: self.embedding_provider.dimensions(),
            collection: collection.to_string(),
            field: vector_field.to_string(),
            metric: metric.as_str().to_string(),
            limit,
            offset,
            query: query.to_string(),
        };
        let result_cache_enabled =
            self.vector_search_result_cache_enabled(collection, vector_field, &index);
        if result_cache_enabled {
            if let Some(result) =
                self.cached_vector_search_result(&result_cache_key, catalog_version)
            {
                return Ok(result);
            }
        }

        let embedding = self.cached_query_embedding(query)?;
        Self::validate_embedding_payload(&index, &embedding)?;

        let result = self.execute_projected_vector_search(
            &index,
            collection,
            vector_field,
            &embedding.values,
            metric,
            limit,
            offset,
        )?;
        if result_cache_enabled {
            self.cache_vector_search_result(result_cache_key, catalog_version, &result);
        }
        Ok(result)
    }

    fn cached_query_embedding(&self, query: &str) -> Result<Embedding, CassieError> {
        const MAX_QUERY_EMBEDDING_CACHE_ENTRIES: usize = 1024;

        let key = QueryEmbeddingCacheKey {
            provider: self.embedding_provider.provider_name().to_string(),
            model: self.embedding_provider.model_name().to_string(),
            dimensions: self.embedding_provider.dimensions(),
            query: query.to_string(),
        };
        if let Some(values) = self.query_embedding_cache.lock().get(&key).cloned() {
            return Ok(Embedding {
                values: values.as_ref().clone(),
            });
        }

        let embedding = self
            .embedding_provider
            .embed_query(query)
            .map_err(CassieError::from)?;

        let mut cache = self.query_embedding_cache.lock();
        if cache.len() >= MAX_QUERY_EMBEDDING_CACHE_ENTRIES {
            if let Some(first_key) = cache.keys().next().cloned() {
                cache.remove(&first_key);
            }
        }
        cache.insert(key, Arc::new(embedding.values.clone()));
        Ok(embedding)
    }

    fn vector_search_result_cache_enabled(
        &self,
        collection: &str,
        vector_field: &str,
        index: &VectorIndexRecord,
    ) -> bool {
        const MIN_RESULT_CACHE_CARDINALITY: u64 = 1024;

        if index.metadata.index_type == VectorIndexType::Hnsw {
            return false;
        }
        self.catalog
            .get_cardinality_stats(collection)
            .and_then(|stats| {
                stats.index_cardinality(
                    &crate::catalog::CollectionCardinalityStats::vector_index_key(vector_field),
                )
            })
            .is_some_and(|cardinality| cardinality >= MIN_RESULT_CACHE_CARDINALITY)
    }

    fn cached_vector_search_result(
        &self,
        key: &VectorSearchResultCacheKey,
        catalog_version: u64,
    ) -> Option<QueryResult> {
        if key.catalog_version != catalog_version {
            return None;
        }
        self.vector_search_result_cache
            .lock()
            .get(key)
            .map(|result| result.as_ref().clone())
    }

    fn cache_vector_search_result(
        &self,
        key: VectorSearchResultCacheKey,
        catalog_version: u64,
        result: &QueryResult,
    ) {
        const MAX_VECTOR_SEARCH_RESULT_CACHE_ENTRIES: usize = 256;

        if self.catalog.version() != catalog_version {
            return;
        }
        let mut cache = self.vector_search_result_cache.lock();
        cache.retain(|cached_key, _| cached_key.catalog_version == catalog_version);
        if cache.len() >= MAX_VECTOR_SEARCH_RESULT_CACHE_ENTRIES {
            if let Some(first_key) = cache.keys().next().cloned() {
                cache.remove(&first_key);
            }
        }
        cache.insert(key, Arc::new(result.clone()));
    }

    #[allow(clippy::too_many_arguments)]
    fn execute_projected_vector_search(
        &self,
        index: &VectorIndexRecord,
        collection: &str,
        vector_field: &str,
        query: &[f32],
        metric: DistanceMetric,
        limit: usize,
        offset: usize,
    ) -> Result<QueryResult, CassieError> {
        let limit = limit.max(1);
        let top_needed = limit.saturating_add(offset).max(1);
        let schema = self
            .catalog
            .get_schema(collection)
            .ok_or_else(|| CassieError::CollectionNotFound(collection.to_string()))?;
        if index.metadata.index_type != VectorIndexType::Hnsw {
            if let Some(result) = self.try_complete_normalized_vector_search(
                &schema,
                collection,
                vector_field,
                query,
                metric,
                limit,
                offset,
            )? {
                return Ok(result);
            }
        }
        let candidates = self.midge.scan_rows_for_rebuild(
            collection,
            RowDecode::Projected(vec![vector_field.to_string()]),
        )?;
        let normalized_vectors = if matches!(&metric, DistanceMetric::Cosine | DistanceMetric::Dot)
        {
            Some(
                self.midge
                    .list_normalized_vectors(collection, vector_field)?
                    .into_iter()
                    .map(|record| (record.id.clone(), record))
                    .collect::<BTreeMap<_, _>>(),
            )
        } else {
            None
        };
        let normalized_query = if matches!(&metric, DistanceMetric::Cosine) {
            normalize_vector(query)
        } else {
            None
        };

        if index.metadata.index_type == VectorIndexType::Hnsw {
            let metric_fn: fn(&[f32], &[f32]) -> f64 = match metric {
                DistanceMetric::Cosine => crate::vector::cosine_distance,
                DistanceMetric::Dot => crate::vector::dot_distance,
                DistanceMetric::L2 => crate::vector::l2_distance,
            };
            let hnsw_candidates = candidates
                .into_iter()
                .filter_map(|candidate| {
                    candidate
                        .payload
                        .get(vector_field)
                        .and_then(vector_from_json)
                        .map(|vector| (candidate.id, vector))
                })
                .collect::<Vec<_>>();
            let selected =
                crate::vector::hnsw::search(query, hnsw_candidates, top_needed, metric_fn)
                    .into_iter()
                    .skip(offset)
                    .take(limit);
            let mut rows = Vec::new();
            for candidate in selected {
                if let Some(document) = self.midge.get_document(collection, &candidate.id)? {
                    rows.push(vector_search_row(&schema, document));
                }
            }
            self.runtime
                .record_vector_normalization_usage(0, rows.len());
            return Ok(QueryResult {
                columns: vector_search_columns(&schema),
                rows,
                command: "SELECT".to_string(),
            });
        }

        let mut top = BinaryHeap::with_capacity(top_needed.saturating_add(1));
        let mut normalized_candidate_count = 0usize;
        let mut fallback_candidate_count = 0usize;
        for candidate in candidates {
            let vector = candidate
                .payload
                .get(vector_field)
                .and_then(vector_from_json)
                .unwrap_or_default();
            let normalized_record = normalized_vectors
                .as_ref()
                .and_then(|records| records.get(candidate.id.as_str()));
            let can_use_normalized = normalized_record.is_some_and(|record| {
                record.payload_available
                    && record.normalization_version
                        == NormalizedVectorRecord::CURRENT_NORMALIZATION_VERSION
                    && record.metric == metric
                    && record.dimensions == query.len()
                    && record.values.len() == query.len()
            });
            let (distance, used_normalized) = if can_use_normalized {
                match &metric {
                    DistanceMetric::Cosine => normalized_query
                        .as_ref().map_or_else(|| {
                            (vector_distance_for_metric(metric, query, &vector), false)
                        }, |normalized_query| {
                            let record = normalized_record.expect("normalized record");
                            (
                                cosine_distance_from_normalized_query(
                                    normalized_query.values.as_slice(),
                                    record.values.as_slice(),
                                ),
                                true,
                            )
                        }),
                    DistanceMetric::Dot => {
                        let record = normalized_record.expect("normalized record");
                        (
                            dot_distance_from_normalized_target(
                                query,
                                record.values.as_slice(),
                                record.magnitude,
                            ),
                            true,
                        )
                    }
                    DistanceMetric::L2 => {
                        (vector_distance_for_metric(metric, query, &vector), false)
                    }
                }
            } else {
                (vector_distance_for_metric(metric, query, &vector), false)
            };
            if used_normalized {
                normalized_candidate_count += 1;
            } else {
                fallback_candidate_count += 1;
            }
            let scored = ScoredVectorCandidate {
                distance,
                id: candidate.id,
            };
            if top.len() < top_needed {
                top.push(scored);
            } else if let Some(worst) = top.peek() {
                if scored.is_better_than(worst) {
                    top.pop();
                    top.push(scored);
                }
            }
        }

        let mut ranked = top.into_vec();
        ranked.sort_by(compare_scored_vector_candidates);
        let selected = ranked.into_iter().skip(offset).take(limit);
        let mut rows = Vec::new();
        for candidate in selected {
            if let Some(document) = self.midge.get_document(collection, &candidate.id)? {
                rows.push(vector_search_row(&schema, document));
            }
        }

        self.runtime.record_vector_normalization_usage(
            normalized_candidate_count,
            fallback_candidate_count,
        );

        Ok(QueryResult {
            columns: vector_search_columns(&schema),
            rows,
            command: "SELECT".to_string(),
        })
    }

    #[allow(clippy::too_many_arguments)]
    fn try_complete_normalized_vector_search(
        &self,
        schema: &CollectionSchema,
        collection: &str,
        vector_field: &str,
        query: &[f32],
        metric: DistanceMetric,
        limit: usize,
        offset: usize,
    ) -> Result<Option<QueryResult>, CassieError> {
        if !matches!(metric, DistanceMetric::Cosine | DistanceMetric::Dot) {
            return Ok(None);
        }

        let catalog_version = self.catalog.version();
        let expected_cardinality =
            self.catalog
                .get_cardinality_stats(collection)
                .and_then(|stats| {
                    stats.index_cardinality(
                        &crate::catalog::CollectionCardinalityStats::vector_index_key(vector_field),
                    )
                });
        let Some(expected_cardinality) = expected_cardinality else {
            return Ok(None);
        };
        let Ok(expected_cardinality) = usize::try_from(expected_cardinality) else {
            return Ok(None);
        };

        let Some(entry) = self.cached_normalized_vectors(
            collection,
            vector_field,
            catalog_version,
            expected_cardinality,
            metric,
            query.len(),
        )?
        else {
            return Ok(None);
        };

        let normalized_query = if matches!(metric, DistanceMetric::Cosine) {
            let Some(normalized_query) = normalize_vector(query) else {
                return Ok(None);
            };
            Some(normalized_query)
        } else {
            None
        };
        let limit = limit.max(1);
        let top_needed = limit.saturating_add(offset).max(1);
        let mut top = BinaryHeap::with_capacity(top_needed.saturating_add(1));
        let candidate_count = entry.ids.len();

        for (index, id) in entry.ids.iter().enumerate() {
            let value_start = index.saturating_mul(entry.dimensions);
            let value_end = value_start.saturating_add(entry.dimensions);
            let values = &entry.values[value_start..value_end];
            let distance = match metric {
                DistanceMetric::Cosine => cosine_distance_from_normalized_query(
                    normalized_query
                        .as_ref()
                        .expect("cosine search has normalized query")
                        .values
                        .as_slice(),
                    values,
                ),
                DistanceMetric::Dot => {
                    dot_distance_from_normalized_target(query, values, entry.magnitudes[index])
                }
                DistanceMetric::L2 => unreachable!("l2 does not use complete normalized fast path"),
            };
            if top.len() < top_needed {
                top.push(ScoredVectorCandidate {
                    distance,
                    id: id.clone(),
                });
            } else if let Some(worst) = top.peek() {
                let candidate_is_better = distance
                    .total_cmp(&worst.distance)
                    .then_with(|| id.cmp(&worst.id))
                    == CmpOrdering::Less;
                if candidate_is_better {
                    top.pop();
                    top.push(ScoredVectorCandidate {
                        distance,
                        id: id.clone(),
                    });
                }
            }
        }

        let mut ranked = top.into_vec();
        ranked.sort_by(compare_scored_vector_candidates);
        let selected = ranked
            .into_iter()
            .skip(offset)
            .take(limit)
            .collect::<Vec<_>>();
        let mut rows = Vec::with_capacity(selected.len());
        for candidate in selected {
            let Some(document) = self.midge.get_document(collection, &candidate.id)? else {
                return Ok(None);
            };
            rows.push(vector_search_row(schema, document));
        }

        self.runtime
            .record_vector_normalization_usage(candidate_count, 0);

        Ok(Some(QueryResult {
            columns: vector_search_columns(schema),
            rows,
            command: "SELECT".to_string(),
        }))
    }

    fn cached_normalized_vectors(
        &self,
        collection: &str,
        vector_field: &str,
        catalog_version: u64,
        expected_cardinality: usize,
        metric: DistanceMetric,
        dimensions: usize,
    ) -> Result<Option<Arc<NormalizedVectorCacheEntry>>, CassieError> {
        let key = NormalizedVectorCacheKey {
            catalog_version,
            collection: collection.to_string(),
            field: vector_field.to_string(),
            cardinality: expected_cardinality,
        };
        let cached_entry = { self.normalized_vector_cache.lock().get(&key).cloned() };
        if let Some(entry) = cached_entry {
            const SENTINEL_VALIDATION_MAX_CARDINALITY: usize = 1024;
            let entry_current = entry.ids.len() > SENTINEL_VALIDATION_MAX_CARDINALITY
                || self.normalized_vector_cache_entry_current(collection, vector_field, &entry)?;
            if entry.metric == metric && entry.dimensions == dimensions && entry_current {
                return Ok(Some(entry));
            }
            self.normalized_vector_cache.lock().remove(&key);
        }

        let records = self
            .midge
            .list_normalized_vectors(collection, vector_field)?;
        let Some(entry) = Self::build_normalized_vector_cache_entry(
            records,
            expected_cardinality,
            metric,
            dimensions,
        ) else {
            return Ok(None);
        };
        let entry = Arc::new(entry);
        if self.catalog.version() != catalog_version {
            return Ok(Some(entry));
        }

        let mut cache = self.normalized_vector_cache.lock();
        cache.retain(|cached_key, _| cached_key.catalog_version == catalog_version);
        Ok(Some(
            cache.entry(key).or_insert_with(|| entry.clone()).clone(),
        ))
    }

    fn normalized_vector_cache_entry_current(
        &self,
        collection: &str,
        vector_field: &str,
        entry: &NormalizedVectorCacheEntry,
    ) -> Result<bool, CassieError> {
        let Some(first) = entry.first_record.as_ref() else {
            return Ok(true);
        };
        let Some(stored_first) =
            self.midge
                .get_normalized_vector(collection, vector_field, &first.id)?
        else {
            return Ok(false);
        };
        if stored_first != *first {
            return Ok(false);
        }

        let Some(last) = entry.last_record.as_ref() else {
            return Ok(true);
        };
        if last.id == first.id {
            return Ok(true);
        }
        let Some(stored_last) =
            self.midge
                .get_normalized_vector(collection, vector_field, &last.id)?
        else {
            return Ok(false);
        };
        Ok(stored_last == *last)
    }

    fn build_normalized_vector_cache_entry(
        records: Vec<NormalizedVectorRecord>,
        expected_cardinality: usize,
        metric: DistanceMetric,
        dimensions: usize,
    ) -> Option<NormalizedVectorCacheEntry> {
        if records.len() != expected_cardinality {
            return None;
        }
        if !records.iter().all(|record| {
            record.payload_available
                && record.normalization_version
                    == NormalizedVectorRecord::CURRENT_NORMALIZATION_VERSION
                && record.metric == metric
                && record.dimensions == dimensions
                && record.values.len() == dimensions
        }) {
            return None;
        }

        let first_record = records.first().cloned();
        let last_record = records.last().cloned();
        let mut ids = Vec::with_capacity(records.len());
        let mut values = Vec::with_capacity(records.len().saturating_mul(dimensions));
        let mut magnitudes = Vec::with_capacity(records.len());
        for record in records {
            ids.push(record.id);
            values.extend(record.values);
            magnitudes.push(record.magnitude);
        }

        Some(NormalizedVectorCacheEntry {
            ids,
            values,
            magnitudes,
            dimensions,
            metric,
            first_record,
            last_record,
        })
    }
}
