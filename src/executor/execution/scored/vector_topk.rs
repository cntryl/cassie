use super::{
    batch, filter, scan, value_to_vector, vector_prefilter_supported, BatchRow, BinaryHeap, Cassie,
    CassieSession, CmpOrdering, Expr, FunctionCall, FunctionMeta, HashMap, LogicalPlan, QueryError,
    QuerySource, SelectItem, SortDirection, Value,
};
use crate::runtime::QueryExecutionControls;

type AnnRerankBarriers = (
    std::sync::Arc<std::sync::Barrier>,
    std::sync::Arc<std::sync::Barrier>,
);

static ANN_RERANK_BARRIERS: std::sync::OnceLock<std::sync::Mutex<Option<AnnRerankBarriers>>> =
    std::sync::OnceLock::new();

pub(crate) fn install_ann_rerank_barriers(barriers: Option<AnnRerankBarriers>) {
    *ANN_RERANK_BARRIERS
        .get_or_init(|| std::sync::Mutex::new(None))
        .lock()
        .expect("ANN rerank barrier lock") = barriers;
}

fn wait_at_ann_rerank_boundary() {
    let barriers = ANN_RERANK_BARRIERS
        .get_or_init(|| std::sync::Mutex::new(None))
        .lock()
        .expect("ANN rerank barrier lock")
        .clone();
    if let Some((selected, resume)) = barriers {
        selected.wait();
        resume.wait();
        install_ann_rerank_barriers(None);
    }
}

pub(crate) fn execute_vector_distance_top_k(
    cassie: &Cassie,
    session: Option<&CassieSession>,
    user_functions: &HashMap<String, FunctionMeta>,
    params: &[Value],
    plan: &LogicalPlan,
    controls: &QueryExecutionControls,
) -> Result<Option<Vec<BatchRow>>, QueryError> {
    super::super::check_timeout(controls)?;
    let Some(spec) = vector_distance_top_k_spec(plan, params) else {
        return Ok(None);
    };
    let started_at = std::time::Instant::now();

    let schema = cassie.catalog.get_schema(&spec.collection).ok_or_else(|| {
        QueryError::General(format!("collection '{}' not found", spec.collection))
    })?;
    validate_vector_top_k_dimensions(&schema, &spec)?;
    if session.is_some_and(|session| !session.collection_changes(&spec.collection).is_empty()) {
        record_transaction_overlay_exact_fallback(cassie, &spec)?;
    } else if plan.filter.is_none() {
        if let Some(rows) = execute_hnsw_vector_top_k(cassie, session, &spec, controls)? {
            return Ok(Some(rows));
        }
        if let Some(rows) = execute_ivfflat_vector_top_k(cassie, session, &spec, controls)? {
            return Ok(Some(rows));
        }
    } else {
        record_filtered_ann_exact_fallback(cassie, &spec)?;
    }
    let top_needed = spec.limit.saturating_add(spec.offset).max(1);
    let adaptive = adaptive_candidate_decision(cassie, &spec.collection, top_needed)?;
    let request = ExactVectorRequest {
        session,
        user_functions,
        params,
        filter_expr: plan.filter.as_ref(),
        controls,
        top_needed,
    };
    if let Some((top, final_candidate_count)) =
        stream_exact_vector_candidates(cassie, &spec, &request)?
    {
        let rows = vector_rows_from_top(top, &spec);
        cassie.runtime.record_vector_execution(
            started_at.elapsed(),
            final_candidate_count,
            rows.len(),
        );
        record_adaptive_candidate_decision(cassie, &adaptive, final_candidate_count, rows.len());
        return Ok(Some(rows));
    }
    let mut candidates =
        batch::flatten_batches(scan::scan(cassie, session, &spec.collection, controls)?);
    if let Some(filter_expr) = &plan.filter {
        if vector_prefilter_supported(filter_expr, &schema) {
            let before = candidates.len();
            candidates = filter::filter_rows(
                candidates,
                filter_expr,
                params,
                None,
                user_functions,
                session,
            )?;
            cassie
                .runtime
                .record_vector_prefilter_usage(before, candidates.len(), None);
        } else {
            return Ok(None);
        }
    }
    let mut top = BinaryHeap::with_capacity(top_needed.saturating_add(1));

    let final_candidate_count = candidates.len();
    for candidate in candidates {
        super::super::check_timeout(controls)?;
        let vector = candidate
            .get(&spec.vector_field)
            .and_then(value_to_vector)
            .unwrap_or_default();
        if vector.len() != spec.query.len() || vector.is_empty() {
            return Err(QueryError::General(format!(
                "vector field '{}' on collection '{}' does not match query dimensions {}",
                spec.vector_field,
                spec.collection,
                spec.query.len()
            )));
        }
        let score = crate::vector::l2_distance(&vector, &spec.query);
        push_top_candidate(
            &mut top,
            SqlVectorCandidate {
                sort_value: candidate_sort_value(&spec.direction, score),
                score,
                id: candidate
                    .get("id")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string(),
            },
            top_needed,
        );
    }

    let rows = vector_rows_from_top(top, &spec);
    cassie
        .runtime
        .record_vector_execution(started_at.elapsed(), final_candidate_count, rows.len());
    record_adaptive_candidate_decision(cassie, &adaptive, final_candidate_count, rows.len());
    Ok(Some(rows))
}

struct ExactVectorRequest<'a> {
    session: Option<&'a CassieSession>,
    user_functions: &'a HashMap<String, FunctionMeta>,
    params: &'a [Value],
    filter_expr: Option<&'a Expr>,
    controls: &'a QueryExecutionControls,
    top_needed: usize,
}

fn stream_exact_vector_candidates(
    cassie: &Cassie,
    spec: &VectorDistanceTopKSpec,
    request: &ExactVectorRequest<'_>,
) -> Result<Option<(BinaryHeap<SqlVectorCandidate>, usize)>, QueryError> {
    if request
        .session
        .is_some_and(|session| !session.collection_changes(&spec.collection).is_empty())
    {
        return Ok(None);
    }
    let decode = if request.filter_expr.is_some() {
        crate::midge::adapter::RowDecode::Full
    } else {
        crate::midge::adapter::RowDecode::ProjectedHistorical(vec![spec.vector_field.clone()])
    };
    let Some(mut cursor) = cassie
        .midge
        .open_row_cursor(&spec.collection, decode)
        .map_err(|error| QueryError::General(error.to_string()))?
    else {
        return Ok(None);
    };
    let _top_memory = request.controls.reserve_query_memory(
        request
            .top_needed
            .saturating_mul(std::mem::size_of::<SqlVectorCandidate>() + 64),
    )?;
    let mut top = BinaryHeap::with_capacity(request.top_needed.saturating_add(1));
    let mut candidate_count = 0usize;
    let mut scanned_count = 0usize;
    loop {
        let documents = cursor
            .next_documents(&cassie.midge, batch::DEFAULT_BATCH_SIZE, request.controls)
            .map_err(QueryError::from)?;
        if documents.is_empty() {
            break;
        }
        let batch_bytes = documents
            .iter()
            .map(|document| document.id.len() + document.payload.to_string().len())
            .sum();
        let _batch_memory = request.controls.reserve_query_memory(batch_bytes)?;
        for document in documents {
            scanned_count = scanned_count.saturating_add(1);
            if !document_matches_filter(
                &document,
                request.filter_expr,
                request.params,
                request.user_functions,
                request.session,
            )? {
                continue;
            }
            let vector =
                vector_from_json(&document.payload[&spec.vector_field]).ok_or_else(|| {
                    QueryError::General("exact vector candidate is invalid".to_string())
                })?;
            let score = crate::vector::l2_distance(&vector, &spec.query);
            candidate_count = candidate_count.saturating_add(1);
            push_top_candidate(
                &mut top,
                SqlVectorCandidate {
                    sort_value: candidate_sort_value(&spec.direction, score),
                    score,
                    id: document.id,
                },
                request.top_needed,
            );
        }
    }
    if request.filter_expr.is_some() {
        cassie
            .runtime
            .record_vector_prefilter_usage(scanned_count, candidate_count, None);
    }
    Ok(Some((top, candidate_count)))
}

fn document_matches_filter(
    document: &crate::midge::adapter::DocumentRef,
    filter_expr: Option<&Expr>,
    params: &[Value],
    user_functions: &HashMap<String, FunctionMeta>,
    session: Option<&CassieSession>,
) -> Result<bool, QueryError> {
    let Some(filter_expr) = filter_expr else {
        return Ok(true);
    };
    let mut entries = vec![("id".to_string(), Value::String(document.id.clone()))];
    if let Some(payload) = document.payload.as_object() {
        entries.extend(payload.iter().map(|(name, value)| {
            (
                name.clone(),
                super::super::projected_read::json_to_query_value(value),
            )
        }));
    }
    filter::filter_rows(
        vec![BatchRow::new(entries)],
        filter_expr,
        params,
        None,
        user_functions,
        session,
    )
    .map(|rows| !rows.is_empty())
}

fn vector_rows_from_top(
    top: BinaryHeap<SqlVectorCandidate>,
    spec: &VectorDistanceTopKSpec,
) -> Vec<BatchRow> {
    let mut ranked = top.into_vec();
    ranked.sort_by(compare_sql_vector_candidates);
    ranked
        .into_iter()
        .skip(spec.offset)
        .take(spec.limit)
        .map(|candidate| {
            BatchRow::new(vec![
                (spec.id_column.clone(), Value::String(candidate.id)),
                (spec.score_column.clone(), Value::Float64(candidate.score)),
            ])
        })
        .collect()
}

fn record_transaction_overlay_exact_fallback(
    cassie: &Cassie,
    spec: &VectorDistanceTopKSpec,
) -> Result<(), QueryError> {
    record_ann_exact_fallback(cassie, spec, "transaction-overlay-exact")
}

fn record_filtered_ann_exact_fallback(
    cassie: &Cassie,
    spec: &VectorDistanceTopKSpec,
) -> Result<(), QueryError> {
    record_ann_exact_fallback(cassie, spec, "structured-filter-exact")
}

fn record_ann_exact_fallback(
    cassie: &Cassie,
    spec: &VectorDistanceTopKSpec,
    reason: &str,
) -> Result<(), QueryError> {
    let index = cassie
        .midge
        .get_vector_index_definition(&spec.collection, &spec.vector_field)
        .map_err(|error| QueryError::General(error.to_string()))?;
    match index.map(|record| record.metadata.index_type) {
        Some(crate::embeddings::VectorIndexType::Hnsw) => {
            cassie.runtime.record_hnsw_fallback(reason);
        }
        Some(crate::embeddings::VectorIndexType::IvfFlat) => {
            cassie.runtime.record_ivfflat_fallback(reason);
        }
        Some(crate::embeddings::VectorIndexType::BruteForce) | None => {}
    }
    Ok(())
}

fn execute_hnsw_vector_top_k(
    cassie: &Cassie,
    session: Option<&CassieSession>,
    spec: &VectorDistanceTopKSpec,
    controls: &QueryExecutionControls,
) -> Result<Option<Vec<BatchRow>>, QueryError> {
    let Some(index) = hnsw_index(cassie, spec)? else {
        return Ok(None);
    };
    if !matches!(spec.direction, SortDirection::Asc) {
        cassie.runtime.record_hnsw_fallback("unsupported-sort");
        return Ok(None);
    }
    if index.metadata.metric != crate::embeddings::DistanceMetric::L2 {
        cassie.runtime.record_hnsw_fallback("incompatible-metric");
        return Ok(None);
    }
    let Some(options) = index.metadata.hnsw.as_ref() else {
        cassie.runtime.record_hnsw_fallback("missing-options");
        return Ok(None);
    };
    let top_needed = spec.limit.saturating_add(spec.offset).max(1);
    let adaptive = adaptive_candidate_decision(cassie, &spec.collection, top_needed)?;
    let candidate_limit = if top_needed < 10 {
        top_needed
    } else {
        options.ef_search.max(top_needed.saturating_mul(64)).min(
            cassie
                .runtime
                .limits()
                .adaptive_candidate_max
                .max(top_needed),
        )
    };
    let started_at = std::time::Instant::now();
    let source_generation = vector_source_generation(cassie, spec)?;
    let search = match cassie.midge.search_hnsw_graph_point_read(
        &spec.collection,
        &spec.vector_field,
        &spec.query,
        options,
        candidate_limit,
    ) {
        Ok(Some(search)) => search,
        Ok(None) => {
            cassie.runtime.record_hnsw_fallback("missing-graph");
            return Ok(None);
        }
        Err(error) => {
            let message = error.to_string();
            if let Some(reason) = message
                .split_once("hnsw fallback:")
                .map(|(_, reason)| reason)
            {
                cassie.runtime.record_hnsw_fallback(reason);
                return Ok(None);
            }
            return Err(QueryError::General(message));
        }
    };
    wait_at_ann_rerank_boundary();
    if vector_source_changed(cassie, spec, source_generation)? {
        cassie
            .runtime
            .record_hnsw_fallback("concurrent-source-change");
        return Ok(None);
    }
    let Some(mut reranked) = rerank_hnsw_candidates(
        cassie,
        session,
        spec,
        controls,
        source_generation,
        search.candidates,
    )?
    else {
        return Ok(None);
    };
    if vector_source_changed(cassie, spec, source_generation)? {
        cassie
            .runtime
            .record_hnsw_fallback("concurrent-source-change");
        return Ok(None);
    }
    reranked.sort_by(compare_sql_vector_candidates);
    let rows = reranked
        .into_iter()
        .skip(spec.offset)
        .take(spec.limit)
        .map(|candidate| {
            BatchRow::new(vec![
                (spec.id_column.clone(), Value::String(candidate.id)),
                (spec.score_column.clone(), Value::Float64(candidate.score)),
            ])
        })
        .collect::<Vec<_>>();
    cassie.runtime.record_vector_execution(
        started_at.elapsed(),
        search.candidate_count,
        rows.len(),
    );
    cassie.runtime.record_hnsw_execution(search.candidate_count);
    record_adaptive_candidate_decision(cassie, &adaptive, search.candidate_count, rows.len());
    Ok(Some(rows))
}

fn hnsw_index(
    cassie: &Cassie,
    spec: &VectorDistanceTopKSpec,
) -> Result<Option<crate::embeddings::VectorIndexRecord>, QueryError> {
    let index = cassie
        .midge
        .get_vector_index_definition(&spec.collection, &spec.vector_field)
        .map_err(|error| QueryError::General(error.to_string()))?;
    let Some(index) = index else {
        return Ok(None);
    };
    if index.metadata.index_type == crate::embeddings::VectorIndexType::Hnsw {
        Ok(Some(index))
    } else {
        Ok(None)
    }
}

fn execute_ivfflat_vector_top_k(
    cassie: &Cassie,
    session: Option<&CassieSession>,
    spec: &VectorDistanceTopKSpec,
    controls: &QueryExecutionControls,
) -> Result<Option<Vec<BatchRow>>, QueryError> {
    let Some((training, membership_count)) = ivfflat_training(cassie, spec)? else {
        return Ok(None);
    };

    let started_at = std::time::Instant::now();
    let normalized_query = crate::vector::normalize(&spec.query)
        .map_or_else(|| spec.query.clone(), |normalized| normalized.values);
    if let Some(reason) = crate::vector::ivfflat::compact_manifest_fallback_reason(
        &training,
        spec.query.len(),
        membership_count,
    ) {
        cassie.runtime.record_ivfflat_fallback(reason);
        return Ok(None);
    }
    let probed_lists = crate::vector::ivfflat::probe_lists(&normalized_query, &training);
    let source_generation = vector_source_generation(cassie, spec)?;
    let normalized_vectors = match cassie.midge.ivfflat_candidate_vectors(
        &spec.collection,
        &spec.vector_field,
        &training,
        &probed_lists,
    ) {
        Ok(records) => records,
        Err(error) => {
            let message = error.to_string();
            if let Some(reason) = message
                .split_once("ivfflat fallback:")
                .map(|(_, reason)| reason)
            {
                cassie.runtime.record_ivfflat_fallback(reason);
                return Ok(None);
            }
            return Err(QueryError::General(message));
        }
    };
    wait_at_ann_rerank_boundary();
    if vector_source_changed(cassie, spec, source_generation)? {
        cassie
            .runtime
            .record_ivfflat_fallback("concurrent-source-change");
        return Ok(None);
    }
    let top_needed = spec.limit.saturating_add(spec.offset).max(1);
    let adaptive = adaptive_candidate_decision(cassie, &spec.collection, top_needed)?;
    let Some((top, candidate_count)) = rerank_ivfflat_candidates(
        cassie,
        session,
        spec,
        controls,
        source_generation,
        normalized_vectors,
        top_needed,
    )?
    else {
        return Ok(None);
    };

    if candidate_count == 0 {
        cassie.runtime.record_ivfflat_fallback("empty-probed-lists");
        return Ok(None);
    }
    if vector_source_changed(cassie, spec, source_generation)? {
        cassie
            .runtime
            .record_ivfflat_fallback("concurrent-source-change");
        return Ok(None);
    }

    let mut ranked = top.into_vec();
    ranked.sort_by(compare_sql_vector_candidates);
    let rows = ranked
        .into_iter()
        .skip(spec.offset)
        .take(spec.limit)
        .map(|candidate| {
            BatchRow::new(vec![
                (spec.id_column.clone(), Value::String(candidate.id)),
                (spec.score_column.clone(), Value::Float64(candidate.score)),
            ])
        })
        .collect::<Vec<_>>();
    cassie
        .runtime
        .record_vector_execution(started_at.elapsed(), candidate_count, rows.len());
    cassie
        .runtime
        .record_ivfflat_execution(training.lists, probed_lists.len(), candidate_count);
    record_adaptive_candidate_decision(cassie, &adaptive, candidate_count, rows.len());
    Ok(Some(rows))
}

fn vector_source_generation(
    cassie: &Cassie,
    spec: &VectorDistanceTopKSpec,
) -> Result<u64, QueryError> {
    cassie
        .midge
        .collection_generation(&spec.collection)
        .map_err(|error| QueryError::General(error.to_string()))
}

fn rerank_hnsw_candidates(
    cassie: &Cassie,
    session: Option<&CassieSession>,
    spec: &VectorDistanceTopKSpec,
    controls: &QueryExecutionControls,
    generation: u64,
    candidates: Vec<crate::vector::hnsw::HnswCandidate>,
) -> Result<Option<Vec<SqlVectorCandidate>>, QueryError> {
    let mut reranked = Vec::with_capacity(candidates.len());
    for candidate in candidates {
        super::super::check_timeout(controls)?;
        if vector_source_changed(cassie, spec, generation)? {
            cassie
                .runtime
                .record_hnsw_fallback("concurrent-source-change");
            return Ok(None);
        }
        let document = cassie
            .get_document_for_session(session, &spec.collection, &candidate.id)
            .map_err(|error| QueryError::General(error.to_string()))?;
        let Some(vector) =
            document.and_then(|document| vector_from_json(&document.payload[&spec.vector_field]))
        else {
            cassie
                .runtime
                .record_hnsw_fallback("concurrent-source-change");
            return Ok(None);
        };
        let score = crate::vector::l2_distance(&vector, &spec.query);
        reranked.push(SqlVectorCandidate {
            sort_value: score,
            score,
            id: candidate.id,
        });
    }
    Ok(Some(reranked))
}

fn rerank_ivfflat_candidates(
    cassie: &Cassie,
    session: Option<&CassieSession>,
    spec: &VectorDistanceTopKSpec,
    controls: &QueryExecutionControls,
    generation: u64,
    records: Vec<crate::embeddings::NormalizedVectorRecord>,
    top_needed: usize,
) -> Result<Option<(BinaryHeap<SqlVectorCandidate>, usize)>, QueryError> {
    let mut top = BinaryHeap::with_capacity(top_needed.saturating_add(1));
    let mut candidate_count = 0usize;
    for record in records {
        super::super::check_timeout(controls)?;
        if vector_source_changed(cassie, spec, generation)? {
            cassie
                .runtime
                .record_ivfflat_fallback("concurrent-source-change");
            return Ok(None);
        }
        let document = cassie
            .get_document_for_session(session, &spec.collection, &record.id)
            .map_err(|error| QueryError::General(error.to_string()))?;
        let Some(vector) =
            document.and_then(|document| vector_from_json(&document.payload[&spec.vector_field]))
        else {
            cassie
                .runtime
                .record_ivfflat_fallback("concurrent-source-change");
            return Ok(None);
        };
        let score = crate::vector::l2_distance(&vector, &spec.query);
        candidate_count = candidate_count.saturating_add(1);
        push_top_candidate(
            &mut top,
            SqlVectorCandidate {
                sort_value: candidate_sort_value(&spec.direction, score),
                score,
                id: record.id,
            },
            top_needed,
        );
    }
    Ok(Some((top, candidate_count)))
}

fn vector_source_changed(
    cassie: &Cassie,
    spec: &VectorDistanceTopKSpec,
    expected: u64,
) -> Result<bool, QueryError> {
    vector_source_generation(cassie, spec).map(|current| current != expected)
}

fn ivfflat_training(
    cassie: &Cassie,
    spec: &VectorDistanceTopKSpec,
) -> Result<Option<(crate::embeddings::IvfFlatTrainingState, usize)>, QueryError> {
    let index = cassie
        .midge
        .get_vector_index_definition(&spec.collection, &spec.vector_field)
        .map_err(|error| QueryError::General(error.to_string()))?;
    let Some(index) = index else {
        return Ok(None);
    };
    if index.metadata.index_type != crate::embeddings::VectorIndexType::IvfFlat {
        return Ok(None);
    }
    if index.metadata.metric != crate::embeddings::DistanceMetric::L2 {
        cassie
            .runtime
            .record_ivfflat_fallback("incompatible-metric");
        return Ok(None);
    }
    let Some(training) = cassie
        .midge
        .get_ivfflat_training_manifest(&spec.collection, &spec.vector_field)
        .map_err(|error| QueryError::General(error.to_string()))?
    else {
        cassie.runtime.record_ivfflat_fallback("missing-training");
        return Ok(None);
    };
    Ok(Some(training))
}

const ANN_CANDIDATE_OVERSAMPLE: usize = 64;

pub(super) struct AdaptiveCandidateDecision {
    initial_budget: usize,
    feedback_budget: Option<usize>,
}

impl AdaptiveCandidateDecision {
    pub(super) fn ann_candidate_budget(&self, max_budget: usize) -> usize {
        self.initial_budget
            .saturating_mul(ANN_CANDIDATE_OVERSAMPLE)
            .min(max_budget.max(1))
    }
}

pub(super) fn adaptive_candidate_decision(
    cassie: &Cassie,
    collection: &str,
    top_needed: usize,
) -> Result<AdaptiveCandidateDecision, QueryError> {
    let limits = cassie.runtime.limits();
    let max_budget = limits.adaptive_candidate_max.max(1);
    if top_needed > max_budget {
        cassie.runtime.record_adaptive_candidate_limit_error();
        return Err(QueryError::General(format!(
            "top-k candidate requirement {top_needed} exceeds adaptive candidate max {max_budget}"
        )));
    }

    let min_budget = limits.adaptive_candidate_min.max(1).min(max_budget);
    let feedback_budget = cassie
        .runtime
        .feedback_candidate_budget(collection)
        .map(|budget| budget.min(max_budget));
    let initial_budget = top_needed
        .max(min_budget)
        .max(feedback_budget.unwrap_or_default())
        .min(max_budget);

    Ok(AdaptiveCandidateDecision {
        initial_budget,
        feedback_budget,
    })
}

pub(super) fn record_adaptive_candidate_decision(
    cassie: &Cassie,
    decision: &AdaptiveCandidateDecision,
    final_candidate_count: usize,
    result_count: usize,
) {
    let expansions = if final_candidate_count > decision.initial_budget {
        final_candidate_count
            .saturating_sub(decision.initial_budget)
            .saturating_add(decision.initial_budget - 1)
            / decision.initial_budget
    } else {
        0
    };
    let exhausted = result_count < decision.initial_budget.min(final_candidate_count);
    cassie.runtime.record_adaptive_candidate_decision(
        decision.initial_budget,
        decision.feedback_budget,
        expansions,
        final_candidate_count,
        exhausted,
    );
}

fn validate_vector_top_k_dimensions(
    schema: &crate::catalog::CollectionSchema,
    spec: &VectorDistanceTopKSpec,
) -> Result<(), QueryError> {
    let Some(field) = schema
        .fields
        .iter()
        .find(|field| field.name.eq_ignore_ascii_case(&spec.vector_field))
    else {
        return Err(QueryError::General(format!(
            "vector field '{}' does not exist on collection '{}'",
            spec.vector_field, spec.collection
        )));
    };
    let crate::types::DataType::Vector(dimensions) = &field.data_type else {
        return Err(QueryError::General(format!(
            "field '{}' on collection '{}' is not a vector field",
            spec.vector_field, spec.collection
        )));
    };
    if spec.query.len() != *dimensions {
        return Err(QueryError::General(format!(
            "vector_distance query for field '{}' on collection '{}' expects {} dimensions but received {}",
            spec.vector_field,
            spec.collection,
            dimensions,
            spec.query.len()
        )));
    }
    Ok(())
}

struct VectorDistanceTopKSpec {
    collection: String,
    vector_field: String,
    query: Vec<f32>,
    id_column: String,
    score_column: String,
    direction: SortDirection,
    limit: usize,
    offset: usize,
}

fn vector_distance_top_k_spec(
    plan: &LogicalPlan,
    params: &[Value],
) -> Option<VectorDistanceTopKSpec> {
    if plan.command.is_some()
        || !plan.ctes.is_empty()
        || plan.distinct
        || !plan.distinct_on.is_empty()
        || !plan.group_by.is_empty()
        || plan.having.is_some()
        || plan.set.is_some()
        || plan.order.len() != 1
        || plan.projection.len() != 2
    {
        return None;
    }

    let QuerySource::Collection(collection) = &plan.source else {
        return None;
    };
    let limit = usize::try_from(plan.limit?).ok()?.max(1);
    let offset = plan
        .offset
        .and_then(|offset| usize::try_from(offset).ok())
        .unwrap_or(0);

    let (id_column, function, score_column) =
        vector_distance_projection(plan.projection.as_slice())?;
    if !order_matches_vector_distance_score(&plan.order[0], function, &score_column, params) {
        return None;
    }

    let (vector_field, query) = vector_distance_args(function, params)?;
    Some(VectorDistanceTopKSpec {
        collection: collection.clone(),
        vector_field,
        query,
        id_column,
        score_column,
        direction: plan.order[0].direction.clone(),
        limit,
        offset,
    })
}

fn vector_distance_projection(
    projection: &[SelectItem],
) -> Option<(String, &FunctionCall, String)> {
    let SelectItem::Column { name, alias: _ } = &projection[0] else {
        return None;
    };
    if !name.eq_ignore_ascii_case("id") && !name.eq_ignore_ascii_case("_id") {
        return None;
    }
    let SelectItem::Function { function, alias } = &projection[1] else {
        return None;
    };
    if !function.name.eq_ignore_ascii_case("vector_distance") {
        return None;
    }
    Some((
        alias.clone().unwrap_or_else(|| name.clone()),
        function,
        alias.clone().unwrap_or_else(|| function.name.clone()),
    ))
}

fn order_matches_vector_distance_score(
    order: &crate::sql::ast::OrderExpr,
    function: &FunctionCall,
    score_column: &str,
    params: &[Value],
) -> bool {
    match &order.expr {
        Expr::Column(column) => column.eq_ignore_ascii_case(score_column),
        Expr::Function(order_function) => {
            order_function.name.eq_ignore_ascii_case("vector_distance")
                && vector_distance_args(order_function, params)
                    == vector_distance_args(function, params)
        }
        _ => false,
    }
}

fn vector_distance_args(function: &FunctionCall, params: &[Value]) -> Option<(String, Vec<f32>)> {
    if function.args.len() != 2 {
        return None;
    }
    let Expr::Column(vector_field) = &function.args[0] else {
        return None;
    };
    let query = match &function.args[1] {
        Expr::StringLiteral(query) => parse_vector_literal(query)?,
        Expr::Param(index) => match params.get(*index)? {
            Value::String(query) => parse_vector_literal(query)?,
            Value::Vector(query) => query.values.clone(),
            _ => return None,
        },
        _ => return None,
    };
    Some((vector_field.clone(), query))
}

pub(crate) fn parse_vector_literal(value: &str) -> Option<Vec<f32>> {
    let values = serde_json::from_str::<Vec<f32>>(value).ok()?;
    if values.is_empty() {
        return None;
    }
    Some(values)
}

pub(super) fn vector_from_json(value: &serde_json::Value) -> Option<Vec<f32>> {
    let values = value.as_array()?;
    let mut out = Vec::with_capacity(values.len());
    for value in values {
        out.push(finite_f32(value.as_f64()?, "vector element").ok()?);
    }
    Some(out)
}

fn push_top_candidate(
    top: &mut BinaryHeap<SqlVectorCandidate>,
    candidate: SqlVectorCandidate,
    top_needed: usize,
) {
    if top.len() < top_needed {
        top.push(candidate);
    } else if let Some(worst) = top.peek() {
        if candidate.is_better_than(worst) {
            top.pop();
            top.push(candidate);
        }
    }
}

fn candidate_sort_value(direction: &SortDirection, score: f64) -> f64 {
    match direction {
        SortDirection::Asc => score,
        SortDirection::Desc => -score,
    }
}

fn finite_f32(value: f64, context: &str) -> Result<f32, QueryError> {
    if !value.is_finite() || value < f64::from(f32::MIN) || value > f64::from(f32::MAX) {
        return Err(QueryError::General(format!(
            "{context} is outside f32 range"
        )));
    }
    value
        .to_string()
        .parse::<f32>()
        .map_err(|_| QueryError::General(format!("failed to parse {context} as f32")))
}

#[derive(Debug, Clone, PartialEq)]
struct SqlVectorCandidate {
    sort_value: f64,
    score: f64,
    id: String,
}

impl SqlVectorCandidate {
    fn is_better_than(&self, other: &Self) -> bool {
        compare_sql_vector_candidates(self, other) == CmpOrdering::Less
    }
}

impl Eq for SqlVectorCandidate {}

impl PartialOrd for SqlVectorCandidate {
    fn partial_cmp(&self, other: &Self) -> Option<CmpOrdering> {
        Some(self.cmp(other))
    }
}

impl Ord for SqlVectorCandidate {
    fn cmp(&self, other: &Self) -> CmpOrdering {
        compare_sql_vector_candidates(self, other)
    }
}

fn compare_sql_vector_candidates(
    left: &SqlVectorCandidate,
    right: &SqlVectorCandidate,
) -> CmpOrdering {
    left.sort_value
        .total_cmp(&right.sort_value)
        .then_with(|| left.id.cmp(&right.id))
}
