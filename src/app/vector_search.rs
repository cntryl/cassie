use super::vector_helpers::{
    compare_scored_vector_candidates, vector_distance_for_metric, vector_from_json,
    vector_search_columns, vector_search_row, ScoredVectorCandidate,
};
use super::{
    cosine_distance_from_normalized_query, dot_distance_from_normalized_target, normalize_vector,
    Arc, Cassie, CassieError, CollectionSchema, DistanceMetric, Embedding, Instant,
    NormalizedVectorCacheEntry, NormalizedVectorCacheKey, NormalizedVectorRecord,
    QueryEmbeddingCacheKey, QueryResult, RowDecode, VectorIndexRecord, VectorIndexType,
    VectorSearchResultCacheKey,
};
use crate::runtime::accounted::AccountedVec;
use crate::runtime::QueryExecutionControls;
use crate::types::Value;
use std::mem::size_of;

mod accounting;
mod ann;
mod cache;
#[cfg(test)]
mod tests;

use accounting::{
    check_controls, check_result_rows, checked_add, checked_mul, json_bytes, AccountedCandidates,
    AccountedVectorResult, VectorSearchDiagnostics,
};

#[derive(Clone, Copy)]
pub(crate) struct VectorSearchInput<'a> {
    pub(crate) collection: &'a str,
    pub(crate) vector_field: &'a str,
    pub(crate) query: &'a str,
    pub(crate) metric: Option<DistanceMetric>,
    pub(crate) limit: usize,
    pub(crate) offset: usize,
}

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
    controls: &'a QueryExecutionControls,
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
        self.run_controlled_vector_search(
            VectorSearchInput {
                collection,
                vector_field,
                query,
                metric,
                limit,
                offset,
            },
            Ok,
        )
    }

    pub(crate) fn run_controlled_vector_search<T>(
        &self,
        input: VectorSearchInput<'_>,
        convert: impl FnOnce(QueryResult) -> Result<T, CassieError>,
    ) -> Result<T, CassieError> {
        let controls = QueryExecutionControls::from_limits(&self.runtime.limits(), Instant::now());
        let result = self
            .execute_vector_search_controlled(input, &controls)
            .and_then(|accounted| self.finalize_vector_search(accounted, &controls, convert));
        self.runtime.record_query_memory(&controls);
        result
    }

    fn finalize_vector_search<T>(
        &self,
        accounted: AccountedVectorResult,
        controls: &QueryExecutionControls,
        convert: impl FnOnce(QueryResult) -> Result<T, CassieError>,
    ) -> Result<T, CassieError> {
        let AccountedVectorResult {
            result,
            memory,
            row_memory,
            diagnostics,
        } = accounted;
        let converted = convert(result).and_then(|result| {
            check_controls(controls)?;
            Ok(result)
        });
        if converted.is_ok() {
            diagnostics.record_success(self);
        }
        drop(row_memory);
        drop(memory);
        converted
    }

    fn execute_vector_search_controlled(
        &self,
        input: VectorSearchInput<'_>,
        controls: &QueryExecutionControls,
    ) -> Result<AccountedVectorResult, CassieError> {
        let index = self
            .catalog
            .get_vector_index(input.collection, input.vector_field)
            .ok_or_else(|| {
                CassieError::InvalidEmbedding(format!(
                    "vector index not found for collection '{}', field '{}'",
                    input.collection, input.vector_field,
                ))
            })?;
        self.validate_embedding_compatibility(&index, input.metric.as_ref())?;
        let schema = self
            .catalog
            .get_schema(input.collection)
            .ok_or_else(|| CassieError::CollectionNotFound(input.collection.to_owned()))?;
        if input.limit == 0 {
            return Ok(AccountedVectorResult {
                result: QueryResult {
                    columns: vector_search_columns(&schema),
                    rows: Vec::new(),
                    command: "SELECT".to_owned(),
                },
                memory: controls.reserve_query_memory(0)?,
                row_memory: None,
                diagnostics: VectorSearchDiagnostics::default(),
            });
        }
        check_controls(controls)?;
        let metric = input.metric.unwrap_or(index.metadata.metric);
        let top_needed = checked_add(input.limit, input.offset)?;
        let catalog_version = self.catalog.version();
        let result_cache_enabled =
            self.vector_search_result_cache_enabled(input.collection, input.vector_field, &index);
        let result_cache = if result_cache_enabled {
            let provider = self.embedding_provider.provider_name();
            let model = self.embedding_provider.model_name();
            let memory = cache::reserve_cache_key::<VectorSearchResultCacheKey>(
                controls,
                &[
                    provider,
                    model,
                    input.collection,
                    input.vector_field,
                    metric.as_str(),
                    input.query,
                ],
            )?;
            let key = VectorSearchResultCacheKey {
                catalog_version,
                provider: provider.to_owned(),
                model: model.to_owned(),
                dimensions: self.embedding_provider.dimensions(),
                collection: input.collection.to_owned(),
                field: input.vector_field.to_owned(),
                metric: metric.as_str().to_owned(),
                limit: input.limit,
                offset: input.offset,
                query: input.query.to_owned(),
            };
            if let Some(result) = self.cached_vector_search_result_controlled(&key, controls)? {
                return Ok(result);
            }
            Some((key, memory))
        } else {
            None
        };
        let (embedding, _embedding_memory) =
            self.cached_query_embedding_controlled(input.query, controls)?;
        Self::validate_embedding_payload(&index, &embedding)?;
        let request = ProjectedVectorSearch {
            schema: &schema,
            collection: input.collection,
            vector_field: input.vector_field,
            query: &embedding.values,
            metric,
            limit: input.limit,
            offset: input.offset,
            top_needed,
            controls,
        };
        let result = self.execute_projected_vector_search(&index, &request)?;
        if let Some((key, _key_memory)) = result_cache {
            self.cache_vector_search_result_controlled(key, &result.result, controls);
        }
        Ok(result)
    }

    fn execute_projected_vector_search(
        &self,
        index: &VectorIndexRecord,
        request: &ProjectedVectorSearch<'_>,
    ) -> Result<AccountedVectorResult, CassieError> {
        if index.metadata.index_type != VectorIndexType::Hnsw {
            if let Some(result) = self.try_complete_normalized_vector_search(request)? {
                return Ok(result);
            }
        }
        if index.metadata.index_type == VectorIndexType::Hnsw {
            if let Some(result) = self.try_hnsw_vector_search_controlled(index, request)? {
                return Ok(result);
            }
        }
        if index.metadata.index_type == VectorIndexType::IvfFlat {
            if let Some(result) = self.try_ivfflat_vector_search_controlled(request)? {
                return Ok(result);
            }
        }
        self.execute_row_vector_search_controlled(request)
    }

    fn execute_row_vector_search_controlled(
        &self,
        request: &ProjectedVectorSearch<'_>,
    ) -> Result<AccountedVectorResult, CassieError> {
        let Some(mut cursor) = self.midge.open_row_cursor(
            request.collection,
            RowDecode::Projected(vec![request.vector_field.to_owned()]),
        )?
        else {
            return Err(CassieError::Execution(
                "controlled vector row cursor unavailable".to_owned(),
            ));
        };
        let started_at = Instant::now();
        let mut top = AccountedCandidates::new(request.controls)?;
        let mut candidates = 0usize;
        while let Some(document) = cursor.next_accounted_document(&self.midge, request.controls)? {
            check_controls(request.controls)?;
            let Some(value) = document.document().payload.get(request.vector_field) else {
                continue;
            };
            let Some(values) = value.as_array() else {
                continue;
            };
            let _vector_memory = request
                .controls
                .reserve_query_memory(checked_mul(values.len(), size_of::<f32>())?)?;
            let Some(vector) = vector_from_json(value) else {
                continue;
            };
            let distance = vector_distance_for_metric(request.metric, request.query, &vector);
            candidates = candidates.saturating_add(1);
            top.push(distance, document.id(), request.top_needed)?;
        }
        let (ranked, _top_memory) = top.ranked();
        let mut result = self.vector_result_for_ids(
            request,
            ranked
                .into_iter()
                .skip(request.offset)
                .take(request.limit)
                .map(|candidate| candidate.id),
        )?;
        result.diagnostics.normalization = Some((0, candidates));
        result.diagnostics.execution =
            Some((started_at.elapsed(), candidates, result.result.rows.len()));
        Ok(result)
    }

    fn try_complete_normalized_vector_search(
        &self,
        request: &ProjectedVectorSearch<'_>,
    ) -> Result<Option<AccountedVectorResult>, CassieError> {
        if !matches!(request.metric, DistanceMetric::Cosine | DistanceMetric::Dot) {
            return Ok(None);
        }
        let Some((entry, _entry_memory)) = self.cached_normalized_vectors_controlled(request)?
        else {
            return Ok(None);
        };
        let _query_memory = request
            .controls
            .reserve_query_memory(checked_mul(request.query.len(), size_of::<f32>())?)?;
        let normalized_query = normalize_vector(request.query);
        if request.metric == DistanceMetric::Cosine && normalized_query.is_none() {
            return Ok(None);
        }
        let mut top = AccountedCandidates::new(request.controls)?;
        for (index, id) in entry.ids.iter().enumerate() {
            check_controls(request.controls)?;
            let start = checked_mul(index, entry.dimensions)?;
            let values = &entry.values[start..start + entry.dimensions];
            let distance = match request.metric {
                DistanceMetric::Cosine => cosine_distance_from_normalized_query(
                    &normalized_query
                        .as_ref()
                        .expect("validated normalized query")
                        .values,
                    values,
                ),
                DistanceMetric::Dot => dot_distance_from_normalized_target(
                    request.query,
                    values,
                    entry.magnitudes[index],
                ),
                DistanceMetric::L2 => unreachable!("L2 has no normalized fast path"),
            };
            top.push(distance, id, request.top_needed)?;
        }
        let (ranked, _top_memory) = top.ranked();
        let selected = ranked
            .into_iter()
            .skip(request.offset)
            .take(request.limit)
            .map(|candidate| candidate.id);
        let mut result = self.vector_result_for_ids(request, selected)?;
        let expected_rows = entry
            .ids
            .len()
            .saturating_sub(request.offset)
            .min(request.limit);
        if result.result.rows.len() != expected_rows {
            return Ok(None);
        }
        result.diagnostics.normalization = Some((entry.ids.len(), 0));
        Ok(Some(result))
    }

    fn vector_result_for_ids(
        &self,
        request: &ProjectedVectorSearch<'_>,
        selected: impl IntoIterator<Item = String>,
    ) -> Result<AccountedVectorResult, CassieError> {
        let mut rows = AccountedVec::try_new(request.controls)?;
        let identity_label_bytes = if request.schema.declares_id() { 3 } else { 2 };
        let initial_bytes = 2 * size_of::<QueryResult>()
            + "SELECT".len()
            + 2 * size_of::<crate::executor::ColumnMeta>()
            + identity_label_bytes
            + "text".len();
        let column_bytes = request
            .schema
            .fields
            .iter()
            .filter(|field| !crate::types::row_identity::is_row_identity_column(&field.name))
            .try_fold(initial_bytes, |bytes, field| {
                checked_add(
                    bytes,
                    checked_add(
                        2 * size_of::<crate::executor::ColumnMeta>(),
                        checked_add(
                            field.name.len(),
                            accounting::type_name_bytes(&field.data_type)?,
                        )?,
                    )?,
                )
            })?;
        let metadata_memory = request.controls.reserve_query_memory(column_bytes)?;
        for id in selected {
            check_controls(request.controls)?;
            let Some(document) = self.midge.get_retrieval_document_controlled(
                request.collection,
                &id,
                request.controls,
            )?
            else {
                continue;
            };
            check_result_rows(rows.len().saturating_add(1), request.controls)?;
            let (document, _document_memory) = document.into_parts();
            let bytes = result_row_bytes(request.schema, &document)?;
            rows.try_push_with(bytes, || vector_search_row(request.schema, document))?;
        }
        let columns = vector_search_columns(request.schema);
        let (rows, row_memory) = rows.into_parts();
        check_controls(request.controls)?;
        Ok(AccountedVectorResult {
            result: QueryResult {
                columns,
                rows,
                command: "SELECT".to_owned(),
            },
            memory: metadata_memory,
            row_memory: Some(row_memory),
            diagnostics: VectorSearchDiagnostics::default(),
        })
    }
}

fn result_row_bytes(
    schema: &CollectionSchema,
    document: &crate::midge::adapter::DocumentRef,
) -> Result<usize, CassieError> {
    // Keep both row arrays accounted while REST moves fields and expands f32 vectors to JSON.
    let slots = checked_mul(
        schema.fields.len().saturating_add(1),
        size_of::<Value>() + size_of::<serde_json::Value>(),
    )?;
    schema.fields.iter().try_fold(
        checked_add(
            size_of::<Vec<serde_json::Value>>(),
            checked_add(slots, document.id.len())?,
        )?,
        |bytes, field| {
            let Some(value) = document.payload.get(&field.name) else {
                return Ok(bytes);
            };
            let variable = if matches!(field.data_type, crate::types::DataType::Vector(_)) {
                value.as_array().map_or(Ok(0), |values| {
                    checked_mul(
                        values.len(),
                        size_of::<f32>() + size_of::<serde_json::Value>(),
                    )
                })?
            } else if let Some(text) = value.as_str() {
                text.len()
            } else if value.is_array() || value.is_object() {
                json_bytes(value)?
            } else {
                0
            };
            checked_add(bytes, variable)
        },
    )
}
