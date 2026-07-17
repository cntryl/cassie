use super::{
    adaptive_candidate_decision, analyzer_for_search_field, batch, cached_search_context, filter,
    hybrid, json_search_term_stats, memory, posting_list_candidate_ids, push_top_k,
    record_adaptive_candidate_decision, score_fulltext_top_k_candidates, scored_candidates_to_rows,
    vector_topk, BatchRow, BinaryHeap, Cassie, CassieSession, FulltextCandidateScoringRequest,
    FulltextSearchTuning, FulltextTopKSpec, HashSet, Instant, QueryError, ScoredSearchCandidate,
    TokenizedFulltextDocument,
};
use crate::runtime::QueryExecutionControls;

pub(super) fn execute_fulltext_top_k(
    cassie: &Cassie,
    session: Option<&CassieSession>,
    spec: &FulltextTopKSpec,
    controls: &QueryExecutionControls,
) -> Result<Vec<BatchRow>, QueryError> {
    let started_at = Instant::now();
    let adaptive = adaptive_candidate_decision(cassie, &spec.collection, spec.top_needed())?;
    if let Some(rows) = try_execute_persisted_fulltext_top_k(
        cassie, session, spec, controls, started_at, &adaptive,
    )? {
        return Ok(rows);
    }
    let documents = cassie
        .scan_projected_documents_batched_for_session(
            session,
            &spec.collection,
            batch::DEFAULT_BATCH_SIZE,
            std::slice::from_ref(&spec.text_field),
            None,
        )
        .map_err(|error| QueryError::General(error.to_string()))?;
    let search_index_options = hybrid::search_context_for_fields(
        cassie,
        &spec.collection,
        std::slice::from_ref(&spec.text_field),
    )?;
    let analyzer = analyzer_for_search_field(&search_index_options, &spec.text_field);
    let search_documents = documents
        .into_iter()
        .flatten()
        .map(|document| TokenizedFulltextDocument {
            id: document.id,
            text_stats: json_search_term_stats(document.payload.get(&spec.text_field), &analyzer),
        })
        .collect::<Vec<_>>();
    let _document_memory = memory::reserve_fulltext_documents(controls, &search_documents)?;
    let search_context = cached_search_context(
        cassie,
        &spec.collection,
        &spec.text_field,
        &search_documents,
        FulltextSearchTuning {
            boost: &search_index_options.field_boost,
            k1: &search_index_options.field_k1,
            b: &search_index_options.field_b,
            analyzer: &search_index_options.field_analyzer,
        },
    )?;
    let query_terms = filter::prepare_query_terms_with_analyzer(&spec.query, &analyzer);
    let candidate_ids = if spec.require_match {
        Some(posting_list_candidate_ids(&search_documents, &query_terms))
    } else {
        None
    };
    let _candidate_memory = memory::reserve_candidate_ids(controls, candidate_ids.as_ref())?;
    let top = score_fulltext_top_k_candidates(
        cassie,
        FulltextCandidateScoringRequest {
            documents: &search_documents,
            candidate_ids: candidate_ids.as_ref(),
            search_context: &search_context,
            text_field: &spec.text_field,
            query_terms: &query_terms,
            require_match: spec.require_match,
            top_needed: spec.top_needed(),
            controls,
        },
    )?;
    let _top_memory = memory::reserve_scored_candidates(controls, &top)?;
    let rows = scored_candidates_to_rows(
        top,
        spec.offset,
        spec.limit,
        &spec.id_column,
        &spec.score_column,
    );
    let candidate_count = candidate_ids
        .as_ref()
        .map_or(search_documents.len(), HashSet::len);
    cassie
        .runtime
        .record_search_execution(started_at.elapsed(), candidate_count, rows.len());
    record_adaptive_candidate_decision(cassie, &adaptive, candidate_count, rows.len());
    Ok(rows)
}

fn try_execute_persisted_fulltext_top_k(
    cassie: &Cassie,
    session: Option<&CassieSession>,
    spec: &FulltextTopKSpec,
    controls: &QueryExecutionControls,
    started_at: Instant,
    adaptive: &vector_topk::AdaptiveCandidateDecision,
) -> Result<Option<Vec<BatchRow>>, QueryError> {
    if !spec.require_match {
        cassie
            .runtime
            .record_fulltext_row_scan_fallback("zero_score_rows_required");
        return Ok(None);
    }
    if session.is_some_and(|session| !session.collection_changes(&spec.collection).is_empty()) {
        cassie
            .runtime
            .record_fulltext_row_scan_fallback("transaction_overlay");
        return Ok(None);
    }
    let Some(index) = cassie
        .catalog
        .list_indexes(&spec.collection)
        .into_iter()
        .find(|index| {
            index.kind == crate::catalog::IndexKind::FullText
                && index.field.eq_ignore_ascii_case(&spec.text_field)
        })
    else {
        cassie
            .runtime
            .record_fulltext_row_scan_fallback("missing_index");
        return Ok(None);
    };
    let search_index_options = hybrid::search_context_for_fields(
        cassie,
        &spec.collection,
        std::slice::from_ref(&spec.text_field),
    )?;
    let analyzer = analyzer_for_search_field(&search_index_options, &spec.text_field);
    let query_terms = filter::prepare_query_terms_with_analyzer(&spec.query, &analyzer);
    let candidates =
        match cassie
            .midge
            .fulltext_candidate_set(&spec.collection, &index.name, &query_terms)
        {
            Ok(candidates) => candidates,
            Err(error) => {
                record_persisted_fallback(cassie, &error);
                return Ok(None);
            }
        };
    let candidate_bytes = serde_json::to_vec(&candidates)
        .map(|bytes| bytes.len())
        .unwrap_or_default();
    let _candidate_memory = controls.reserve_query_memory(candidate_bytes)?;
    let search_context = filter::SearchContext::from_persisted_field_statistics(
        &spec.text_field,
        &filter::PersistedFieldStatistics {
            total_documents: candidates.total_documents,
            average_document_length: candidates.average_document_length,
            document_frequency: &candidates.document_frequency,
            field_boost: &search_index_options.field_boost,
            field_k1: &search_index_options.field_k1,
            field_b: &search_index_options.field_b,
            field_analyzer: &search_index_options.field_analyzer,
        },
    );
    let mut top = BinaryHeap::with_capacity(spec.top_needed().saturating_add(1));
    for (id, stats) in &candidates.document_stats {
        super::super::check_timeout(controls)?;
        let term_stats =
            filter::SearchTermStats::from_persisted(stats.doc_length, &stats.term_counts);
        let score =
            search_context.score_term_stats(Some(&spec.text_field), &term_stats, &query_terms);
        if score > 0.0 {
            push_top_k(
                &mut top,
                spec.top_needed(),
                ScoredSearchCandidate {
                    sort_value: -score,
                    score,
                    id: id.clone(),
                },
            );
        }
    }
    let _top_memory = memory::reserve_scored_candidates(controls, &top)?;
    let candidate_count = candidates.document_stats.len();
    let rows = scored_candidates_to_rows(
        top,
        spec.offset,
        spec.limit,
        &spec.id_column,
        &spec.score_column,
    );
    cassie
        .runtime
        .record_fulltext_retrieval_diagnostics(candidates.posting_block_reads, 0);
    cassie
        .runtime
        .record_search_execution(started_at.elapsed(), candidate_count, rows.len());
    record_adaptive_candidate_decision(cassie, adaptive, candidate_count, rows.len());
    Ok(Some(rows))
}

fn record_persisted_fallback(cassie: &Cassie, error: &crate::app::CassieError) {
    let reason = if error.to_string().contains("missing_candidate_row") {
        "missing_candidate_row"
    } else {
        "invalid_persisted_artifact"
    };
    cassie.runtime.record_fulltext_row_scan_fallback(reason);
}
