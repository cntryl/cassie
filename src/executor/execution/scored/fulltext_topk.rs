use super::{
    adaptive_candidate_decision, analyzer_for_search_field, batch, cached_search_context, filter,
    hybrid, json_search_term_stats_for_query, memory, persisted_search,
    posting_list_candidate_ids_controlled, record_adaptive_candidate_decision,
    score_fulltext_top_k_candidates, scored_candidates_to_rows, vector_topk, BatchRow, Cassie,
    CassieSession, FulltextCandidateScoringRequest, FulltextSearchTuning, FulltextTopKSpec,
    HashSet, Instant, QueryError, TokenizedFulltextDocument,
};
use crate::runtime::{QueryExecutionControls, QueryScoringTestBoundary};

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
            text_stats: json_search_term_stats_for_query(
                document.payload.get(&spec.text_field),
                &analyzer,
                &spec.query,
            ),
        })
        .collect::<Vec<_>>();
    let search_context = cached_search_context(
        cassie,
        session,
        &spec.collection,
        &spec.text_field,
        &search_documents,
        FulltextSearchTuning {
            boost: &search_index_options.field_boost,
            k1: &search_index_options.field_k1,
            b: &search_index_options.field_b,
            analyzer: &search_index_options.field_analyzer,
        },
        controls,
    )?;
    let search = filter::PreparedSearch::prepare(
        &spec.query,
        &analyzer,
        &search_context.vocabulary(&spec.text_field),
    )?;
    let _search_memory = controls.reserve_query_memory(search.estimated_bytes())?;
    let (candidate_ids, _candidate_memory) = if spec.require_match {
        let (candidate_ids, memory) = posting_list_candidate_ids_controlled(
            &search_documents,
            search.candidate_terms(),
            controls,
        )?;
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
            search: &search,
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
    if session.is_some_and(|session| session.has_collection_changes(&spec.collection)) {
        return Ok(PersistedFulltextTopK::Exact("transaction_overlay"));
    }
    let Some(index) = cassie
        .catalog
        .list_indexes(&spec.collection)
        .into_iter()
        .find(|index| {
            index.kind == crate::catalog::IndexKind::FullText
                && crate::sql::ColumnIdentifierPath::stored_field_key(&index.field)
                    == crate::sql::ColumnIdentifierPath::reference_field_key(&spec.text_field)
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
    let persisted_search::PersistedSearchCandidates {
        candidates,
        search,
        memory: _candidate_memory,
    } = match persisted_search::load_persisted_search_candidates(
        cassie,
        &spec.collection,
        &index.name,
        &spec.query,
        &analyzer,
        controls,
    )? {
        persisted_search::PersistedSearchLoad::Ready(loaded) => *loaded,
        persisted_search::PersistedSearchLoad::Exact(reason) => {
            return Ok(PersistedFulltextTopK::Exact(reason));
        }
    };
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
        let term_stats = search.persisted_stats(stats);
        if let Some(score) = search.evaluate(&search_context, Some(&spec.text_field), &term_stats) {
            top.try_push(spec.top_needed(), -score, score, id)?;
            controls.scored_candidate(QueryScoringTestBoundary::PersistedFulltext)?;
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
