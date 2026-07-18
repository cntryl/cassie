use super::{
    adaptive_candidate_decision, analyzer_for_search_field, batch, cached_search_context, filter,
    hybrid, json_search_term_stats, memory, posting_list_candidate_ids_controlled,
    record_adaptive_candidate_decision, score_fulltext_top_k_candidates, scored_candidates_to_rows,
    vector_topk, BatchRow, Cassie, CassieSession, FulltextCandidateScoringRequest,
    FulltextSearchTuning, FulltextTopKSpec, HashSet, Instant, QueryError,
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
    let fallback_reason = match try_execute_persisted_fulltext_top_k(
        cassie, session, spec, controls, started_at, &adaptive,
    )? {
        PersistedFulltextTopK::Selected {
            rows,
            memory: _output_memory,
        } => return Ok(rows),
        PersistedFulltextTopK::Exact(reason) => reason,
    };
    let (documents, _source_memory) =
        scan_exact_fulltext_documents(cassie, session, spec, controls)?;
    let search_index_options = hybrid::search_context_for_fields(
        cassie,
        &spec.collection,
        std::slice::from_ref(&spec.text_field),
    )?;
    let analyzer = analyzer_for_search_field(&search_index_options, &spec.text_field);
    let _document_memory = memory::reserve_tokenized_fulltext_documents(
        controls,
        &documents,
        &spec.text_field,
        &analyzer,
    )?;
    let search_documents = documents
        .into_iter()
        .map(|document| TokenizedFulltextDocument {
            id: document.id,
            text_stats: json_search_term_stats(document.payload.get(&spec.text_field), &analyzer),
        })
        .collect::<Vec<_>>();
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
    let (candidate_ids, _candidate_memory) = if spec.require_match {
        let (candidate_ids, memory) =
            posting_list_candidate_ids_controlled(&search_documents, &query_terms, controls)?;
        (Some(candidate_ids), Some(memory))
    } else {
        (None, None)
    };
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
    let (top, _top_memory) = top.into_parts();
    let output = scored_candidates_to_rows(
        top,
        spec.offset,
        spec.limit,
        &spec.id_column,
        &spec.score_column,
        controls,
    )?;
    let (rows, _output_memory) = output.into_parts();
    let candidate_count = candidate_ids
        .as_ref()
        .map_or(search_documents.len(), HashSet::len);
    cassie
        .runtime
        .record_search_execution(started_at.elapsed(), candidate_count, rows.len());
    record_adaptive_candidate_decision(cassie, &adaptive, candidate_count, rows.len());
    cassie
        .runtime
        .record_fulltext_row_scan_fallback(fallback_reason);
    Ok(rows)
}

enum PersistedFulltextTopK {
    Selected {
        rows: Vec<BatchRow>,
        memory: crate::runtime::QueryMemoryReservation,
    },
    Exact(&'static str),
}

fn scan_exact_fulltext_documents(
    cassie: &Cassie,
    session: Option<&CassieSession>,
    spec: &FulltextTopKSpec,
    controls: &QueryExecutionControls,
) -> Result<
    (
        Vec<crate::midge::adapter::DocumentRef>,
        Vec<crate::runtime::QueryMemoryReservation>,
    ),
    QueryError,
> {
    let Some(mut cursor) = cassie
        .open_session_row_cursor(
            session,
            &spec.collection,
            crate::midge::adapter::RowDecode::ProjectedHistorical(vec![spec.text_field.clone()]),
            controls,
        )
        .map_err(QueryError::from)?
    else {
        return Err(QueryError::General(
            "fulltext exact fallback requires row storage".to_string(),
        ));
    };
    let mut documents = Vec::new();
    let mut memory = Vec::new();
    loop {
        let accounted = cursor
            .next_accounted_documents(&cassie.midge, batch::DEFAULT_BATCH_SIZE, controls)
            .map_err(QueryError::from)?;
        if accounted.is_empty() {
            break;
        }
        for document in accounted {
            let (document, reservation) = document.into_parts();
            documents.push(document);
            memory.push(reservation);
        }
    }
    Ok((documents, memory))
}

fn try_execute_persisted_fulltext_top_k(
    cassie: &Cassie,
    session: Option<&CassieSession>,
    spec: &FulltextTopKSpec,
    controls: &QueryExecutionControls,
    started_at: Instant,
    adaptive: &vector_topk::AdaptiveCandidateDecision,
) -> Result<PersistedFulltextTopK, QueryError> {
    if !spec.require_match {
        return Ok(PersistedFulltextTopK::Exact("zero_score_rows_required"));
    }
    if session.is_some_and(|session| !session.collection_changes(&spec.collection).is_empty()) {
        return Ok(PersistedFulltextTopK::Exact("transaction_overlay"));
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
        return Ok(PersistedFulltextTopK::Exact("missing_index"));
    };
    let search_index_options = hybrid::search_context_for_fields(
        cassie,
        &spec.collection,
        std::slice::from_ref(&spec.text_field),
    )?;
    let analyzer = analyzer_for_search_field(&search_index_options, &spec.text_field);
    let query_terms = filter::prepare_query_terms_with_analyzer(&spec.query, &analyzer);
    let candidates = match cassie.midge.fulltext_candidate_set_controlled(
        &spec.collection,
        &index.name,
        &query_terms,
        controls,
    ) {
        Ok(candidates) => candidates,
        Err(error) if is_query_control_error(&error) => return Err(QueryError::from(error)),
        Err(error) => {
            return Ok(PersistedFulltextTopK::Exact(persisted_fallback_reason(
                &error,
            )));
        }
    };
    let (candidates, _candidate_memory) = candidates.into_parts();
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
    let mut top = memory::AccountedScoredCandidates::try_new(controls, spec.top_needed())?;
    for (id, stats) in &candidates.document_stats {
        super::super::check_timeout(controls)?;
        let term_stats =
            filter::SearchTermStats::from_persisted(stats.doc_length, &stats.term_counts);
        let score =
            search_context.score_term_stats(Some(&spec.text_field), &term_stats, &query_terms);
        if score > 0.0 {
            top.try_push(spec.top_needed(), -score, score, id)?;
        }
    }
    let (top, _top_memory) = top.into_parts();
    let candidate_count = candidates.document_stats.len();
    let output = scored_candidates_to_rows(
        top,
        spec.offset,
        spec.limit,
        &spec.id_column,
        &spec.score_column,
        controls,
    )?;
    let (rows, output_memory) = output.into_parts();
    cassie
        .runtime
        .record_fulltext_retrieval_diagnostics(candidates.posting_block_reads, 0);
    cassie
        .runtime
        .record_search_execution(started_at.elapsed(), candidate_count, rows.len());
    record_adaptive_candidate_decision(cassie, adaptive, candidate_count, rows.len());
    Ok(PersistedFulltextTopK::Selected {
        rows,
        memory: output_memory,
    })
}

fn persisted_fallback_reason(error: &crate::app::CassieError) -> &'static str {
    if error.to_string().contains("missing_candidate_row") {
        "missing_candidate_row"
    } else {
        "invalid_persisted_artifact"
    }
}

fn is_query_control_error(error: &crate::app::CassieError) -> bool {
    matches!(
        error,
        crate::app::CassieError::QueryCancelled
            | crate::app::CassieError::DeadlineExceeded
            | crate::app::CassieError::ResourceLimit(_)
    )
}
