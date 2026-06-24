use super::vector_helpers::*;
use super::*;

impl Cassie {
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

        let embedding = self
            .embedding_provider
            .embed_query(query)
            .map_err(CassieError::from)?;
        self.validate_embedding_payload(&index, &embedding)?;

        let metric = metric.unwrap_or(index.metadata.metric.clone());
        self.execute_projected_vector_search(
            &index,
            collection,
            vector_field,
            &embedding.values,
            metric,
            limit,
            offset,
        )
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
                        .as_ref()
                        .map(|normalized_query| {
                            let record = normalized_record.expect("normalized record");
                            (
                                cosine_distance_from_normalized_query(
                                    normalized_query.values.as_slice(),
                                    record.values.as_slice(),
                                ),
                                true,
                            )
                        })
                        .unwrap_or_else(|| {
                            (vector_distance_for_metric(&metric, query, &vector), false)
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
                        (vector_distance_for_metric(&metric, query, &vector), false)
                    }
                }
            } else {
                (vector_distance_for_metric(&metric, query, &vector), false)
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
}
