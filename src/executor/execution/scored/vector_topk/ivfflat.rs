use std::time::Instant;

use crate::app::{Cassie, CassieSession};
use crate::executor::batch::BatchRow;
use crate::runtime::QueryExecutionControls;

use super::candidate::{
    candidate_sort_value, vector_rows_from_top, AccountedVectorTopK, SqlVectorCandidate,
};
use super::diagnostics::{
    record_ivfflat_concurrent_source_change, source_generation_matches, wait_at_ann_rerank_boundary,
};
use super::{
    adaptive_candidate_decision, record_adaptive_candidate_decision, vector_from_json, QueryError,
    VectorDistanceTopKSpec,
};

pub(super) fn execute_ivfflat_vector_top_k(
    cassie: &Cassie,
    session: Option<&CassieSession>,
    spec: &VectorDistanceTopKSpec,
    controls: &QueryExecutionControls,
) -> Result<Option<Vec<BatchRow>>, QueryError> {
    let Some(snapshot) = ivfflat_training(cassie, spec, controls)? else {
        return Ok(None);
    };
    let (manifest_generation, training, membership_count, manifest_reads, manifest_memory) =
        snapshot.into_parts();
    let started_at = Instant::now();
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
    let _probe_memory = controls.reserve_query_memory(
        probed_lists
            .len()
            .saturating_mul(std::mem::size_of::<usize>()),
    )?;
    let batch = match cassie.midge.ivfflat_candidate_vectors_controlled(
        &spec.collection,
        &spec.vector_field,
        &training,
        &probed_lists,
        controls,
    ) {
        Ok(batch) => batch,
        Err(error) => return handle_ivfflat_storage_error(cassie, error),
    };
    let (built_generation, records, membership_reads, vector_reads, candidate_memory) =
        batch.into_parts();

    wait_at_ann_rerank_boundary();
    if built_generation != manifest_generation
        || !source_generation_matches(cassie, spec, built_generation)?
    {
        record_ivfflat_concurrent_source_change(cassie);
        return Ok(None);
    }
    let top_needed = spec.top_needed();
    let adaptive = adaptive_candidate_decision(cassie, &spec.collection, top_needed)?;
    let Some((top, top_memory, candidate_count)) = rerank_ivfflat_candidates(
        cassie,
        session,
        spec,
        controls,
        built_generation,
        records,
        top_needed,
    )?
    else {
        return Ok(None);
    };
    if candidate_count == 0 {
        cassie.runtime.record_ivfflat_fallback("empty-probed-lists");
        return Ok(None);
    }
    if !source_generation_matches(cassie, spec, built_generation)? {
        record_ivfflat_concurrent_source_change(cassie);
        return Ok(None);
    }

    let rows = vector_rows_from_top(top, spec);
    drop(top_memory);
    drop(candidate_memory);
    drop(manifest_memory);
    let ann_reads = manifest_reads
        .saturating_add(1)
        .saturating_add(membership_reads)
        .saturating_add(vector_reads);
    cassie
        .runtime
        .record_vector_execution(started_at.elapsed(), candidate_count, rows.len());
    cassie
        .runtime
        .record_vector_retrieval_diagnostics(ann_reads, candidate_count, candidate_count);
    cassie
        .runtime
        .record_ivfflat_execution(training.lists, probed_lists.len(), candidate_count);
    record_adaptive_candidate_decision(cassie, &adaptive, candidate_count, rows.len());
    Ok(Some(rows))
}

fn ivfflat_training(
    cassie: &Cassie,
    spec: &VectorDistanceTopKSpec,
    controls: &QueryExecutionControls,
) -> Result<Option<crate::midge::adapter::PersistedIvfFlatTrainingSnapshot>, QueryError> {
    let index = cassie
        .midge
        .get_vector_index_definition(&spec.collection, &spec.vector_field)?;
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
    let training = cassie.midge.get_ivfflat_training_manifest_controlled(
        &spec.collection,
        &spec.vector_field,
        controls,
    )?;
    if training.is_none() {
        cassie.runtime.record_ivfflat_fallback("missing-training");
    }
    Ok(training)
}

fn rerank_ivfflat_candidates(
    cassie: &Cassie,
    session: Option<&CassieSession>,
    spec: &VectorDistanceTopKSpec,
    controls: &QueryExecutionControls,
    generation: u64,
    records: Vec<crate::embeddings::NormalizedVectorRecord>,
    top_needed: usize,
) -> Result<
    Option<(
        std::collections::BinaryHeap<SqlVectorCandidate>,
        crate::runtime::QueryMemoryReservation,
        usize,
    )>,
    QueryError,
> {
    let mut top = AccountedVectorTopK::try_new(controls)?;
    let mut candidate_count = 0usize;
    for record in records {
        super::super::super::check_timeout(controls)?;
        if !source_generation_matches(cassie, spec, generation)? {
            record_ivfflat_concurrent_source_change(cassie);
            return Ok(None);
        }
        let document = cassie.get_document_for_session(session, &spec.collection, &record.id)?;
        let Some(document) = document else {
            record_ivfflat_concurrent_source_change(cassie);
            return Ok(None);
        };
        let Some(vector) = vector_from_json(&document.payload[&spec.vector_field]) else {
            record_ivfflat_concurrent_source_change(cassie);
            return Ok(None);
        };
        if vector.len() != spec.query.len() || vector.is_empty() {
            record_ivfflat_concurrent_source_change(cassie);
            return Ok(None);
        }
        if !source_generation_matches(cassie, spec, generation)? {
            record_ivfflat_concurrent_source_change(cassie);
            return Ok(None);
        }
        let score = crate::vector::l2_distance(&vector, &spec.query);
        candidate_count = candidate_count.saturating_add(1);
        top.try_push(
            SqlVectorCandidate {
                sort_value: candidate_sort_value(&spec.direction, score),
                score,
                id: record.id,
            },
            top_needed,
        )?;
    }
    let (top, memory) = top.into_parts();
    Ok(Some((top, memory, candidate_count)))
}

fn handle_ivfflat_storage_error(
    cassie: &Cassie,
    error: crate::app::CassieError,
) -> Result<Option<Vec<BatchRow>>, QueryError> {
    let message = error.to_string();
    if let Some(reason) = message
        .split_once("ivfflat fallback:")
        .map(|(_, reason)| reason)
    {
        cassie.runtime.record_ivfflat_fallback(reason);
        return Ok(None);
    }
    Err(QueryError::from(error))
}
