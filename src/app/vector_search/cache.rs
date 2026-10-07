use std::mem::size_of;

use super::accounting::{
    check_controls, check_result_rows, checked_add, checked_mul, result_bytes,
    AccountedVectorResult,
};
use super::{
    Arc, Cassie, CassieError, Embedding, NormalizedVectorCacheEntry, NormalizedVectorCacheKey,
    NormalizedVectorRecord, ProjectedVectorSearch, QueryEmbeddingCacheKey, VectorIndexRecord,
    VectorIndexType, VectorSearchResultCacheKey,
};
use crate::runtime::{QueryExecutionControls, QueryMemoryReservation};

impl Cassie {
    pub(super) fn cached_query_embedding_controlled(
        &self,
        query: &str,
        controls: &QueryExecutionControls,
    ) -> Result<(Embedding, QueryMemoryReservation), CassieError> {
        check_controls(controls)?;
        let provider = self.embedding_provider.provider_name();
        let model = self.embedding_provider.model_name();
        let _key_memory =
            reserve_cache_key::<QueryEmbeddingCacheKey>(controls, &[provider, model, query])?;
        let key = QueryEmbeddingCacheKey {
            provider: provider.to_owned(),
            model: model.to_owned(),
            dimensions: self.embedding_provider.dimensions(),
            query: query.to_owned(),
        };
        let cached = self.query_embedding_cache.lock().get(&key).cloned();
        let dimensions = cached
            .as_ref()
            .map_or(key.dimensions, |values| values.len());
        let mut memory =
            controls.reserve_query_memory(checked_mul(dimensions, size_of::<f32>())?)?;
        if let Some(values) = cached {
            return Ok((
                Embedding {
                    values: values.as_ref().clone(),
                },
                memory,
            ));
        }
        let embedding = self
            .embedding_provider
            .embed_query_with_controls(query, controls)
            .map_err(CassieError::from)?;
        let bytes = checked_mul(embedding.values.len(), size_of::<f32>())?;
        if bytes > memory.bytes() {
            memory.try_grow(bytes - memory.bytes())?;
        }
        check_controls(controls)?;
        let mut cache = self.query_embedding_cache.lock();
        let cache_memory = cache_value_bytes::<Vec<f32>>(bytes)
            .and_then(|bytes| {
                checked_add(
                    bytes,
                    cache_node_bytes::<QueryEmbeddingCacheKey, Arc<Vec<f32>>>(cache.len())?,
                )
            })
            .and_then(|bytes| controls.reserve_query_memory(bytes));
        if let Ok(_cache_memory) = cache_memory {
            if cache.len() >= 1024 {
                cache.pop_first();
            }
            cache.insert(key, Arc::new(embedding.values.clone()));
        }
        Ok((embedding, memory))
    }

    pub(super) fn vector_search_result_cache_enabled(
        &self,
        collection: &str,
        vector_field: &str,
        index: &VectorIndexRecord,
    ) -> bool {
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
            .is_some_and(|cardinality| cardinality >= 1024)
    }

    pub(super) fn cached_vector_search_result_controlled(
        &self,
        key: &VectorSearchResultCacheKey,
        controls: &QueryExecutionControls,
    ) -> Result<Option<AccountedVectorResult>, CassieError> {
        check_controls(controls)?;
        let cached = self.vector_search_result_cache.lock().get(key).cloned();
        let Some(cached) = cached else {
            return Ok(None);
        };
        check_result_rows(cached.rows.len(), controls)?;
        let memory = controls.reserve_query_memory(result_bytes(&cached)?)?;
        Ok(Some(AccountedVectorResult {
            result: cached.as_ref().clone(),
            memory,
            row_memory: None,
            diagnostics: super::VectorSearchDiagnostics::default(),
        }))
    }

    pub(super) fn cache_vector_search_result_controlled(
        &self,
        key: VectorSearchResultCacheKey,
        result: &crate::executor::QueryResult,
        controls: &QueryExecutionControls,
    ) {
        if self.catalog.version() != key.catalog_version {
            return;
        }
        let mut cache = self.vector_search_result_cache.lock();
        let Ok(bytes) = result_bytes(result)
            .and_then(cache_value_bytes::<crate::executor::QueryResult>)
            .and_then(|bytes| {
                checked_add(
                    bytes,
                    cache_node_bytes::<
                        VectorSearchResultCacheKey,
                        Arc<crate::executor::QueryResult>,
                    >(cache.len())?,
                )
            })
        else {
            return;
        };
        let Ok(_cache_memory) = controls.reserve_query_memory(bytes) else {
            return;
        };
        cache.retain(|cached, _| cached.catalog_version == key.catalog_version);
        if cache.len() >= 256 {
            cache.pop_first();
        }
        cache.insert(key, Arc::new(result.clone()));
    }

    pub(super) fn cached_normalized_vectors_controlled(
        &self,
        request: &ProjectedVectorSearch<'_>,
    ) -> Result<Option<(Arc<NormalizedVectorCacheEntry>, QueryMemoryReservation)>, CassieError>
    {
        let catalog_version = self.catalog.version();
        let Some((expected, collection_generation)) = self
            .midge
            .get_cardinality_stats(request.collection)?
            .filter(|stats| stats.hydrated)
            .and_then(|stats| {
                // Source membership does not count explicit vectors whose source is NULL.
                // Completeness must cover the stored vector field itself.
                stats
                    .field_stats(request.vector_field)
                    .and_then(|field| usize::try_from(field.non_null_count).ok())
                    .map(|expected| (expected, stats.built_generation))
            })
        else {
            return Ok(None);
        };
        let _key_memory = reserve_cache_key::<NormalizedVectorCacheKey>(
            request.controls,
            &[request.collection, request.vector_field],
        )?;
        let key = NormalizedVectorCacheKey {
            catalog_version,
            collection_generation,
            collection: request.collection.to_owned(),
            field: request.vector_field.to_owned(),
            cardinality: expected,
        };
        let cached = self.normalized_vector_cache.lock().get(&key).cloned();
        if let Some(entry) = cached {
            let memory = request
                .controls
                .reserve_query_memory(normalized_entry_bytes(&entry)?)?;
            let current = entry.ids.len() > 1024
                || self.normalized_vector_cache_entry_current_controlled(request, &entry)?;
            if entry.metric == request.metric && entry.dimensions == request.query.len() && current
            {
                return Ok(Some((entry, memory)));
            }
            self.normalized_vector_cache.lock().remove(&key);
        }
        let (records, records_memory) = self.midge.list_normalized_vectors_controlled(
            request.collection,
            request.vector_field,
            request.controls,
        )?;
        if records.len() != expected
            || !records.iter().all(|record| {
                record.payload_available
                    && record.normalization_version
                        == NormalizedVectorRecord::CURRENT_NORMALIZATION_VERSION
                    && record.metric == request.metric
                    && record.dimensions == request.query.len()
                    && record.values.len() == request.query.len()
            })
        {
            return Ok(None);
        }
        let entry_bytes = flattened_entry_bytes(&records, request.query.len())?;
        let memory = request.controls.reserve_query_memory(entry_bytes)?;
        let first_record = records.first().cloned();
        let last_record = records.last().cloned();
        let mut ids = Vec::with_capacity(records.len());
        let mut values = Vec::with_capacity(checked_mul(records.len(), request.query.len())?);
        let mut magnitudes = Vec::with_capacity(records.len());
        for record in records {
            ids.push(record.id);
            values.extend(record.values);
            magnitudes.push(record.magnitude);
        }
        drop(records_memory);
        let entry = Arc::new(NormalizedVectorCacheEntry {
            ids,
            values,
            magnitudes,
            dimensions: request.query.len(),
            metric: request.metric,
            first_record,
            last_record,
        });
        if self.catalog.version() == catalog_version {
            let mut cache = self.normalized_vector_cache.lock();
            let cache_memory = cache_node_bytes::<
                NormalizedVectorCacheKey,
                Arc<NormalizedVectorCacheEntry>,
            >(cache.len())
            .and_then(|bytes| checked_add(size_of::<Arc<NormalizedVectorCacheEntry>>(), bytes))
            .and_then(|bytes| request.controls.reserve_query_memory(bytes));
            let Ok(_cache_memory) = cache_memory else {
                return Ok(Some((entry, memory)));
            };
            cache.retain(|cached, _| {
                cached.catalog_version == catalog_version
                    && (cached.collection != key.collection
                        || cached.field != key.field
                        || cached.collection_generation == collection_generation)
            });
            cache.insert(key, entry.clone());
        }
        Ok(Some((entry, memory)))
    }

    fn normalized_vector_cache_entry_current_controlled(
        &self,
        request: &ProjectedVectorSearch<'_>,
        entry: &NormalizedVectorCacheEntry,
    ) -> Result<bool, CassieError> {
        for expected in [&entry.first_record, &entry.last_record]
            .into_iter()
            .flatten()
        {
            let Some((stored, _memory)) = self.midge.get_normalized_vector_controlled(
                request.collection,
                request.vector_field,
                &expected.id,
                request.controls,
            )?
            else {
                return Ok(false);
            };
            if stored != *expected {
                return Ok(false);
            }
        }
        Ok(true)
    }
}

pub(super) fn reserve_cache_key<K>(
    controls: &QueryExecutionControls,
    strings: &[&str],
) -> Result<QueryMemoryReservation, CassieError> {
    let bytes = strings
        .iter()
        .try_fold(size_of::<K>(), |bytes, text| checked_add(bytes, text.len()))?;
    controls.reserve_query_memory(bytes)
}

fn cache_value_bytes<T>(payload: usize) -> Result<usize, CassieError> {
    checked_add(payload, size_of::<T>() + 3 * size_of::<usize>())
}

fn cache_node_bytes<K, V>(entries: usize) -> Result<usize, CassieError> {
    // BTree nodes retain eleven key/value slots plus an internal node's header and edges.
    let node = checked_add(
        14 * size_of::<usize>(),
        checked_mul(11, checked_add(size_of::<K>(), size_of::<V>())?)?,
    )?;
    // A binary-tree height bounds BTree split propagation, including a new root.
    let nodes = usize::try_from(entries.saturating_add(1).bit_width())
        .map_err(|_| CassieError::ResourceLimit("cache node height overflow".to_owned()))?;
    checked_mul(node, checked_add(nodes, 1)?)
}

fn normalized_record_bytes(record: &NormalizedVectorRecord) -> Result<usize, CassieError> {
    checked_add(
        size_of::<NormalizedVectorRecord>(),
        checked_add(
            record.collection.len(),
            checked_add(
                record.field.len(),
                checked_add(
                    record.id.len(),
                    checked_mul(record.values.len(), size_of::<f32>())?,
                )?,
            )?,
        )?,
    )
}

fn flattened_entry_bytes(
    records: &[NormalizedVectorRecord],
    dimensions: usize,
) -> Result<usize, CassieError> {
    let inline = checked_mul(records.len(), size_of::<String>() + size_of::<f64>())?;
    let values = checked_mul(checked_mul(records.len(), dimensions)?, size_of::<f32>())?;
    let ids = records
        .iter()
        .try_fold(0usize, |bytes, record| checked_add(bytes, record.id.len()))?;
    let sentinels = records
        .first()
        .into_iter()
        .chain(records.last())
        .try_fold(0usize, |bytes, record| {
            checked_add(bytes, normalized_record_bytes(record)?)
        })?;
    checked_add(
        size_of::<NormalizedVectorCacheEntry>() + 2 * size_of::<usize>(),
        checked_add(inline, checked_add(values, checked_add(ids, sentinels)?)?)?,
    )
}

fn normalized_entry_bytes(entry: &NormalizedVectorCacheEntry) -> Result<usize, CassieError> {
    let inline = checked_mul(entry.ids.len(), size_of::<String>() + size_of::<f64>())?;
    let ids = entry
        .ids
        .iter()
        .try_fold(0usize, |bytes, id| checked_add(bytes, id.len()))?;
    let sentinels = [&entry.first_record, &entry.last_record]
        .into_iter()
        .flatten()
        .try_fold(0usize, |bytes, record| {
            checked_add(bytes, normalized_record_bytes(record)?)
        })?;
    checked_add(
        size_of::<NormalizedVectorCacheEntry>() + 2 * size_of::<usize>(),
        checked_add(
            inline,
            checked_add(
                checked_mul(entry.values.len(), size_of::<f32>())?,
                checked_add(ids, sentinels)?,
            )?,
        )?,
    )
}
