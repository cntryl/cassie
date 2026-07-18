use std::time::Instant;

use crate::app::{Cassie, CassieSession};
use crate::executor::batch::BatchRow;
use crate::runtime::accounted::AccountedVec;
use crate::runtime::QueryExecutionControls;

use super::candidate::{
    candidate_sort_value, compare_sql_vector_candidates, vector_rows_from_ranked,
    SqlVectorCandidate,
};
use super::diagnostics::{
    record_hnsw_concurrent_source_change, source_generation_matches, wait_at_ann_rerank_boundary,
};
use super::{
    adaptive_candidate_decision, record_adaptive_candidate_decision, vector_from_json, QueryError,
    SortDirection, VectorDistanceTopKSpec,
};

pub(super) fn execute_hnsw_vector_top_k(
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
    let top_needed = spec.top_needed();
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
    let started_at = Instant::now();
    let batch = match cassie.midge.search_hnsw_graph_point_read_controlled(
        &spec.collection,
        &spec.vector_field,
        &spec.query,
        options,
        candidate_limit,
        controls,
    ) {
        Ok(Some(batch)) => batch,
        Ok(None) => {
            cassie.runtime.record_hnsw_fallback("missing-graph");
            return Ok(None);
        }
        Err(error) => return handle_hnsw_storage_error(cassie, error),
    };
    let (built_generation, candidates, candidate_count, ann_reads, candidate_memory) =
        batch.into_parts();

    wait_at_ann_rerank_boundary();
    if !source_generation_matches(cassie, spec, built_generation)? {
        record_hnsw_concurrent_source_change(cassie);
        return Ok(None);
    }
    let Some((mut reranked, rerank_memory)) = rerank_hnsw_candidates(
        cassie,
        session,
        spec,
        controls,
        built_generation,
        candidates,
    )?
    else {
        return Ok(None);
    };
    if !source_generation_matches(cassie, spec, built_generation)? {
        record_hnsw_concurrent_source_change(cassie);
        return Ok(None);
    }
    reranked.sort_by(compare_sql_vector_candidates);
    let exact_reranks = reranked.len();
    let rows = vector_rows_from_ranked(reranked, spec);
    drop(rerank_memory);
    drop(candidate_memory);

    cassie
        .runtime
        .record_vector_execution(started_at.elapsed(), candidate_count, rows.len());
    cassie
        .runtime
        .record_vector_retrieval_diagnostics(ann_reads, exact_reranks, exact_reranks);
    cassie.runtime.record_hnsw_execution(exact_reranks);
    record_adaptive_candidate_decision(cassie, &adaptive, candidate_count, rows.len());
    Ok(Some(rows))
}

fn hnsw_index(
    cassie: &Cassie,
    spec: &VectorDistanceTopKSpec,
) -> Result<Option<crate::embeddings::VectorIndexRecord>, QueryError> {
    let index = cassie
        .midge
        .get_vector_index_definition(&spec.collection, &spec.vector_field)?;
    let Some(index) = index else {
        return Ok(None);
    };
    if index.metadata.index_type == crate::embeddings::VectorIndexType::Hnsw {
        Ok(Some(index))
    } else {
        Ok(None)
    }
}

fn rerank_hnsw_candidates(
    cassie: &Cassie,
    session: Option<&CassieSession>,
    spec: &VectorDistanceTopKSpec,
    controls: &QueryExecutionControls,
    generation: u64,
    candidates: Vec<crate::vector::hnsw::HnswCandidate>,
) -> Result<
    Option<(
        Vec<SqlVectorCandidate>,
        crate::runtime::QueryMemoryReservation,
    )>,
    QueryError,
> {
    let mut reranked = AccountedVec::try_new(controls)?;
    for candidate in candidates {
        super::super::super::check_timeout(controls)?;
        if !source_generation_matches(cassie, spec, generation)? {
            record_hnsw_concurrent_source_change(cassie);
            return Ok(None);
        }
        let document = cassie.get_document_for_session(session, &spec.collection, &candidate.id)?;
        let Some(document) = document else {
            record_hnsw_concurrent_source_change(cassie);
            return Ok(None);
        };
        let Some(vector) = vector_from_json(&document.payload[&spec.vector_field]) else {
            record_hnsw_concurrent_source_change(cassie);
            return Ok(None);
        };
        if vector.len() != spec.query.len() || vector.is_empty() {
            record_hnsw_concurrent_source_change(cassie);
            return Ok(None);
        }
        if !source_generation_matches(cassie, spec, generation)? {
            record_hnsw_concurrent_source_change(cassie);
            return Ok(None);
        }
        let score = crate::vector::l2_distance(&vector, &spec.query);
        reranked.try_push_with(candidate.id.len(), || SqlVectorCandidate {
            sort_value: candidate_sort_value(&spec.direction, score),
            score,
            id: candidate.id,
        })?;
    }
    let (reranked, memory) = reranked.into_parts();
    Ok(Some((reranked, memory)))
}

fn handle_hnsw_storage_error(
    cassie: &Cassie,
    error: crate::app::CassieError,
) -> Result<Option<Vec<BatchRow>>, QueryError> {
    let message = error.to_string();
    if let Some(reason) = message
        .split_once("hnsw fallback:")
        .map(|(_, reason)| reason)
    {
        cassie.runtime.record_hnsw_fallback(reason);
        return Ok(None);
    }
    Err(QueryError::from(error))
}
