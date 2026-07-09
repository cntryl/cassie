use super::vector_helpers::{
    compare_scored_vector_candidates, vector_distance_for_metric, vector_from_json,
    vector_search_columns, vector_search_row, ScoredVectorCandidate,
};
use super::{
    cosine_distance_from_normalized_query, dot_distance_from_normalized_target, normalize_vector,
    Arc, BTreeMap, BinaryHeap, Cassie, CassieError, CmpOrdering, CollectionSchema, DistanceMetric,
    DocumentRef, Embedding, Instant, NormalizedVectorCacheEntry, NormalizedVectorCacheKey,
    NormalizedVectorRecord, QueryEmbeddingCacheKey, QueryResult, RowDecode, VectorIndexRecord,
    VectorIndexType, VectorSearchResultCacheKey,
};
use crate::vector::NormalizedVector;

#[derive(Clone, Copy)]
struct ProjectedVectorSearch<'a> {
    schema: &'a CollectionSchema,
    collection: &'a str,
    vector_field: &'a str,
    query: &'a [f32],
    metric: DistanceMetric,
    limit: usize,
    offset: usize,
    top_needed: usize,
}

struct RowVectorSearchResult {
    rows: Vec<Vec<crate::types::Value>>,
    normalized_candidate_count: usize,
    fallback_candidate_count: usize,
}

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

        let limit = limit.max(1);
        let request = ProjectedVectorSearch {
            schema: &self
                .catalog
                .get_schema(collection)
                .ok_or_else(|| CassieError::CollectionNotFound(collection.to_string()))?,
            collection,
            vector_field,
            query: &embedding.values,
            metric,
            limit,
            offset,
            top_needed: limit.saturating_add(offset).max(1),
        };
        let result = self.execute_projected_vector_search(&index, request)?;
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

    fn execute_projected_vector_search(
        &self,
        index: &VectorIndexRecord,
        request: ProjectedVectorSearch<'_>,
    ) -> Result<QueryResult, CassieError> {
        if index.metadata.index_type != VectorIndexType::Hnsw {
            if let Some(result) = self.try_complete_normalized_vector_search(&request)? {
                return Ok(result);
            }
        }

        if index.metadata.index_type == VectorIndexType::Hnsw {
            if let Some(result) = self.try_hnsw_vector_search(&request)? {
                return Ok(result);
            }
        }

        if index.metadata.index_type == VectorIndexType::IvfFlat {
            if let Some(result) = self.try_ivfflat_vector_search(&request)? {
                return Ok(result);
            }
        }

        let candidates = self.scan_vector_candidates(&request)?;
        let normalized_vectors = self.normalized_vector_records_for_metric(&request)?;
        let normalized_query = normalized_query_for_metric(request.metric, request.query);
        let result = self.execute_row_vector_search(
            &request,
            candidates,
            normalized_vectors.as_ref(),
            normalized_query.as_ref(),
        )?;
        self.runtime.record_vector_normalization_usage(
            result.normalized_candidate_count,
            result.fallback_candidate_count,
        );
        Ok(QueryResult {
            columns: vector_search_columns(request.schema),
            rows: result.rows,
            command: "SELECT".to_string(),
        })
    }

    fn scan_vector_candidates(
        &self,
        request: &ProjectedVectorSearch<'_>,
    ) -> Result<Vec<DocumentRef>, CassieError> {
        self.midge.scan_rows_for_rebuild(
            request.collection,
            RowDecode::Projected(vec![request.vector_field.to_string()]),
        )
    }

    fn try_hnsw_vector_search(
        &self,
        request: &ProjectedVectorSearch<'_>,
    ) -> Result<Option<QueryResult>, CassieError> {
        let Some(index) = self
            .midge
            .get_vector_index(request.collection, request.vector_field)?
        else {
            return Ok(None);
        };
        if index.metadata.index_type != VectorIndexType::Hnsw {
            return Ok(None);
        }
        if index.metadata.metric != request.metric {
            self.runtime.record_hnsw_fallback("incompatible-metric");
            return Ok(None);
        }
        let Some(options) = index.metadata.hnsw.as_ref() else {
            self.runtime.record_hnsw_fallback("missing-options");
            return Ok(None);
        };
        let normalized_vectors = self
            .midge
            .list_normalized_vectors(request.collection, request.vector_field)?;
        if let Some(reason) = crate::vector::hnsw::graph_fallback_reason(
            index.metadata.hnsw_graph.as_ref(),
            index.metadata.metric,
            index.metadata.dimensions,
            &normalized_vectors,
        ) {
            self.runtime.record_hnsw_fallback(reason);
            return Ok(None);
        }
        let graph = index
            .metadata
            .hnsw_graph
            .as_ref()
            .expect("validated hnsw graph");
        let started_at = Instant::now();
        let Some(search) =
            crate::vector::hnsw::search_graph(graph, request.query, options, request.top_needed)
        else {
            self.runtime.record_hnsw_fallback("search-unavailable");
            return Ok(None);
        };
        let selected = search
            .candidates
            .into_iter()
            .skip(request.offset)
            .take(request.limit)
            .map(|candidate| candidate.id);
        let rows = self.vector_rows_for_ids(request.schema, request.collection, selected)?;
        self.runtime.record_vector_execution(
            started_at.elapsed(),
            search.candidate_count,
            rows.len(),
        );
        self.runtime.record_hnsw_execution(search.candidate_count);
        Ok(Some(QueryResult {
            columns: vector_search_columns(request.schema),
            rows,
            command: "SELECT".to_string(),
        }))
    }

    fn try_ivfflat_vector_search(
        &self,
        request: &ProjectedVectorSearch<'_>,
    ) -> Result<Option<QueryResult>, CassieError> {
        let Some(index) = self
            .midge
            .get_vector_index(request.collection, request.vector_field)?
        else {
            return Ok(None);
        };
        if index.metadata.index_type != VectorIndexType::IvfFlat {
            return Ok(None);
        }
        if request.metric != DistanceMetric::L2 || index.metadata.metric != DistanceMetric::L2 {
            self.runtime.record_ivfflat_fallback("incompatible-metric");
            return Ok(None);
        }
        let Some(training) = index.metadata.ivfflat_training.as_ref() else {
            self.runtime.record_ivfflat_fallback("missing-training");
            return Ok(None);
        };
        let normalized_vectors = self
            .midge
            .list_normalized_vectors(request.collection, request.vector_field)?;
        if let Some(reason) = crate::vector::ivfflat::training_fallback_reason(
            training,
            request.query.len(),
            &normalized_vectors,
        ) {
            self.runtime.record_ivfflat_fallback(reason);
            return Ok(None);
        }
        let started_at = Instant::now();
        let normalized_query = normalize_vector(request.query)
            .map_or_else(|| request.query.to_vec(), |normalized| normalized.values);
        let probed_lists = crate::vector::ivfflat::probe_lists(&normalized_query, training);
        let mut top = BinaryHeap::with_capacity(request.top_needed.saturating_add(1));
        let mut candidate_count = 0usize;
        for record in normalized_vectors {
            let Some(list) = training.assignments.get(&record.id) else {
                continue;
            };
            if !probed_lists.contains(list) {
                continue;
            }
            let Some(vector) = crate::vector::ivfflat::denormalized_vector(&record) else {
                continue;
            };
            let distance = vector_distance_for_metric(request.metric, request.query, &vector);
            candidate_count = candidate_count.saturating_add(1);
            push_scored_vector_candidate(
                &mut top,
                request.top_needed,
                ScoredVectorCandidate {
                    distance,
                    id: record.id,
                },
            );
        }
        if candidate_count == 0 {
            self.runtime.record_ivfflat_fallback("empty-probed-lists");
            return Ok(None);
        }
        let rows = self.vector_rows_for_ids(
            request.schema,
            request.collection,
            ranked_vector_candidates(top, request.offset, request.limit)
                .map(|candidate| candidate.id),
        )?;
        self.runtime
            .record_vector_execution(started_at.elapsed(), candidate_count, rows.len());
        self.runtime
            .record_ivfflat_execution(training.lists, probed_lists.len(), candidate_count);
        Ok(Some(QueryResult {
            columns: vector_search_columns(request.schema),
            rows,
            command: "SELECT".to_string(),
        }))
    }

    fn normalized_vector_records_for_metric(
        &self,
        request: &ProjectedVectorSearch<'_>,
    ) -> Result<Option<BTreeMap<String, NormalizedVectorRecord>>, CassieError> {
        if !matches!(request.metric, DistanceMetric::Cosine | DistanceMetric::Dot) {
            return Ok(None);
        }
        Ok(Some(
            self.midge
                .list_normalized_vectors(request.collection, request.vector_field)?
                .into_iter()
                .map(|record| (record.id.clone(), record))
                .collect(),
        ))
    }

    fn execute_row_vector_search(
        &self,
        request: &ProjectedVectorSearch<'_>,
        candidates: Vec<DocumentRef>,
        normalized_vectors: Option<&BTreeMap<String, NormalizedVectorRecord>>,
        normalized_query: Option<&NormalizedVector>,
    ) -> Result<RowVectorSearchResult, CassieError> {
        let mut top = BinaryHeap::with_capacity(request.top_needed.saturating_add(1));
        let mut normalized_candidate_count = 0usize;
        let mut fallback_candidate_count = 0usize;
        for candidate in candidates {
            let vector = candidate
                .payload
                .get(request.vector_field)
                .and_then(vector_from_json)
                .unwrap_or_default();
            let normalized_record =
                normalized_vectors.and_then(|records| records.get(candidate.id.as_str()));
            let (distance, used_normalized) =
                row_vector_distance(request, normalized_record, normalized_query, &vector);
            if used_normalized {
                normalized_candidate_count += 1;
            } else {
                fallback_candidate_count += 1;
            }
            let scored = ScoredVectorCandidate {
                distance,
                id: candidate.id,
            };
            push_scored_vector_candidate(&mut top, request.top_needed, scored);
        }

        let rows = self.vector_rows_for_ids(
            request.schema,
            request.collection,
            ranked_vector_candidates(top, request.offset, request.limit)
                .map(|candidate| candidate.id),
        )?;
        Ok(RowVectorSearchResult {
            rows,
            normalized_candidate_count,
            fallback_candidate_count,
        })
    }

    fn vector_rows_for_ids(
        &self,
        schema: &CollectionSchema,
        collection: &str,
        selected: impl IntoIterator<Item = String>,
    ) -> Result<Vec<Vec<crate::types::Value>>, CassieError> {
        let mut rows = Vec::new();
        for id in selected {
            if let Some(document) = self.midge.get_document(collection, &id)? {
                rows.push(vector_search_row(schema, document));
            }
        }
        Ok(rows)
    }

    fn try_complete_normalized_vector_search(
        &self,
        request: &ProjectedVectorSearch<'_>,
    ) -> Result<Option<QueryResult>, CassieError> {
        if !matches!(request.metric, DistanceMetric::Cosine | DistanceMetric::Dot) {
            return Ok(None);
        }

        let catalog_version = self.catalog.version();
        let Some(expected_cardinality) = self.expected_normalized_vector_cardinality(request)
        else {
            return Ok(None);
        };

        let Some(entry) = self.cached_normalized_vectors(
            request.collection,
            request.vector_field,
            catalog_version,
            expected_cardinality,
            request.metric,
            request.query.len(),
        )?
        else {
            return Ok(None);
        };

        let normalized_query = normalized_query(request);
        let mut top = BinaryHeap::with_capacity(request.top_needed.saturating_add(1));
        let candidate_count = entry.ids.len();

        for (index, id) in entry.ids.iter().enumerate() {
            let value_start = index.saturating_mul(entry.dimensions);
            let value_end = value_start.saturating_add(entry.dimensions);
            let values = &entry.values[value_start..value_end];
            let distance = match request.metric {
                DistanceMetric::Cosine => cosine_distance_from_normalized_query(
                    normalized_query
                        .as_ref()
                        .expect("cosine search has normalized query")
                        .values
                        .as_slice(),
                    values,
                ),
                DistanceMetric::Dot => dot_distance_from_normalized_target(
                    request.query,
                    values,
                    entry.magnitudes[index],
                ),
                DistanceMetric::L2 => unreachable!("l2 does not use complete normalized fast path"),
            };
            if top.len() < request.top_needed {
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
            .skip(request.offset)
            .take(request.limit)
            .collect::<Vec<_>>();
        let mut rows = Vec::with_capacity(selected.len());
        for candidate in selected {
            let Some(document) = self.midge.get_document(request.collection, &candidate.id)? else {
                return Ok(None);
            };
            rows.push(vector_search_row(request.schema, document));
        }

        self.runtime
            .record_vector_normalization_usage(candidate_count, 0);

        Ok(Some(QueryResult {
            columns: vector_search_columns(request.schema),
            rows,
            command: "SELECT".to_string(),
        }))
    }

    fn expected_normalized_vector_cardinality(
        &self,
        request: &ProjectedVectorSearch<'_>,
    ) -> Option<usize> {
        let expected = self
            .catalog
            .get_cardinality_stats(request.collection)?
            .index_cardinality(
                &crate::catalog::CollectionCardinalityStats::vector_index_key(request.vector_field),
            )?;
        usize::try_from(expected).ok()
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

fn normalized_query(request: &ProjectedVectorSearch<'_>) -> Option<NormalizedVector> {
    if !matches!(request.metric, DistanceMetric::Cosine) {
        return None;
    }
    let normalized_query = normalize_vector(request.query)?;
    Some(normalized_query)
}

fn normalized_query_for_metric(metric: DistanceMetric, query: &[f32]) -> Option<NormalizedVector> {
    if matches!(metric, DistanceMetric::Cosine) {
        normalize_vector(query)
    } else {
        None
    }
}

fn row_vector_distance(
    request: &ProjectedVectorSearch<'_>,
    normalized_record: Option<&NormalizedVectorRecord>,
    normalized_query: Option<&NormalizedVector>,
    vector: &[f32],
) -> (f64, bool) {
    if !can_use_normalized_record(request, normalized_record) {
        return (
            vector_distance_for_metric(request.metric, request.query, vector),
            false,
        );
    }
    match request.metric {
        DistanceMetric::Cosine => normalized_query.map_or_else(
            || {
                (
                    vector_distance_for_metric(request.metric, request.query, vector),
                    false,
                )
            },
            |normalized_query| {
                let record = normalized_record.expect("normalized record");
                (
                    cosine_distance_from_normalized_query(
                        normalized_query.values.as_slice(),
                        record.values.as_slice(),
                    ),
                    true,
                )
            },
        ),
        DistanceMetric::Dot => {
            let record = normalized_record.expect("normalized record");
            (
                dot_distance_from_normalized_target(
                    request.query,
                    record.values.as_slice(),
                    record.magnitude,
                ),
                true,
            )
        }
        DistanceMetric::L2 => (
            vector_distance_for_metric(request.metric, request.query, vector),
            false,
        ),
    }
}

fn can_use_normalized_record(
    request: &ProjectedVectorSearch<'_>,
    normalized_record: Option<&NormalizedVectorRecord>,
) -> bool {
    normalized_record.is_some_and(|record| {
        record.payload_available
            && record.normalization_version == NormalizedVectorRecord::CURRENT_NORMALIZATION_VERSION
            && record.metric == request.metric
            && record.dimensions == request.query.len()
            && record.values.len() == request.query.len()
    })
}

fn push_scored_vector_candidate(
    top: &mut BinaryHeap<ScoredVectorCandidate>,
    top_needed: usize,
    scored: ScoredVectorCandidate,
) {
    if top.len() < top_needed {
        top.push(scored);
    } else if let Some(worst) = top.peek() {
        if scored.is_better_than(worst) {
            top.pop();
            top.push(scored);
        }
    }
}

fn ranked_vector_candidates(
    top: BinaryHeap<ScoredVectorCandidate>,
    offset: usize,
    limit: usize,
) -> impl Iterator<Item = ScoredVectorCandidate> {
    let mut ranked = top.into_vec();
    ranked.sort_by(compare_scored_vector_candidates);
    ranked.into_iter().skip(offset).take(limit)
}
