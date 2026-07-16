use super::{
    json_search_term_stats_value, value_to_vector, vector_prefilter_supported, BatchRow, Cassie,
    CassieSession, CollectionSchema, FunctionMeta, HybridTopKSpec, QueryError,
    TokenizedHybridDocument, Value,
};
use crate::executor::{batch, filter, scan};
use crate::runtime::QueryExecutionControls;
use crate::search::analyzer::AnalyzerConfig;
use std::collections::HashMap;
use std::collections::{BinaryHeap, HashSet};

pub(super) fn search_context_for_fields(
    cassie: &Cassie,
    collection: &str,
    fields: &[String],
) -> Result<super::FulltextIndexOptions, QueryError> {
    let requested_fields = fields
        .iter()
        .map(|field| field.to_ascii_lowercase())
        .collect::<std::collections::HashSet<_>>();
    super::load_fulltext_index_options(cassie, collection, &requested_fields)
}

pub(super) fn record_hybrid_diagnostics(
    cassie: &Cassie,
    posting_reads: usize,
    ann_reads: usize,
    candidate_row_fetches: usize,
    exact_reranks: usize,
) {
    cassie.runtime.record_hybrid_retrieval_diagnostics(
        posting_reads,
        ann_reads,
        candidate_row_fetches,
        0,
        exact_reranks,
        0,
        0,
    );
}

pub(super) struct BoundedHybridRows {
    pub(super) rows: Vec<BatchRow>,
    pub(super) ann_reads: usize,
    pub(super) candidate_row_fetches: usize,
}

pub(super) fn score_hybrid_documents(
    documents: &[super::TokenizedHybridDocument],
    candidate_ids: &HashSet<String>,
    search_context: &filter::SearchContext,
    spec: &HybridTopKSpec,
    query_terms: &[String],
    controls: &QueryExecutionControls,
) -> Result<(BinaryHeap<super::ScoredSearchCandidate>, usize), QueryError> {
    let mut top = BinaryHeap::with_capacity(spec.top_needed().saturating_add(1));
    let mut text_candidate_count = 0usize;
    for document in documents {
        super::super::check_timeout(controls)?;
        if !candidate_ids.contains(document.id.as_str()) {
            continue;
        }
        let search_score = search_context.score_term_stats(
            Some(&spec.text_field),
            &document.text_stats,
            query_terms,
        );
        if search_score == 0.0 {
            continue;
        }
        text_candidate_count += 1;
        let vector = document.vector.as_ref().ok_or_else(|| {
            QueryError::General("vector_score expects vector in first argument".to_string())
        })?;
        if vector.len() != spec.vector_query.len() {
            return Err(QueryError::General(format!(
                "vector_score vector length mismatch: {} != {}",
                vector.len(),
                spec.vector_query.len()
            )));
        }
        let vector_score = 1.0 / (1.0 + crate::vector::l2_distance(vector, &spec.vector_query));
        let score = crate::hybrid::hybrid_score(search_score, vector_score, None);
        super::push_top_k(
            &mut top,
            spec.top_needed(),
            super::ScoredSearchCandidate {
                sort_value: -score,
                score,
                id: document.id.clone(),
            },
        );
    }
    Ok((top, text_candidate_count))
}

pub(super) fn bounded_hybrid_rows(
    cassie: &Cassie,
    session: Option<&CassieSession>,
    user_functions: &HashMap<String, FunctionMeta>,
    params: &[Value],
    spec: &HybridTopKSpec,
    schema: &CollectionSchema,
    analyzer: &AnalyzerConfig,
    candidate_limit: usize,
) -> Result<Option<BoundedHybridRows>, QueryError> {
    if session.is_some_and(|session| !session.collection_changes(&spec.collection).is_empty()) {
        cassie
            .runtime
            .record_hybrid_row_scan_fallback("transaction-overlay");
        return Ok(None);
    }
    let Some(fulltext_index) = cassie
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
            .record_hybrid_row_scan_fallback("missing-text-index");
        return Ok(None);
    };
    let vector_ids = match cassie.midge.persisted_vector_candidate_ids(
        &spec.collection,
        &spec.vector_field,
        &spec.vector_query,
        candidate_limit,
    ) {
        Ok(Some(ids)) => ids,
        Ok(None) => {
            cassie
                .runtime
                .record_hybrid_prefilter_usage(0, 0, Some("vector-artifact"));
            cassie
                .runtime
                .record_hybrid_row_scan_fallback("missing-ann-state");
            return Ok(None);
        }
        Err(error) => {
            let generation_rejection = usize::from(
                error.to_string().contains("generation") || error.to_string().contains("stale"),
            );
            cassie.runtime.record_hybrid_retrieval_diagnostics(
                0,
                0,
                0,
                generation_rejection,
                0,
                0,
                0,
            );
            cassie
                .runtime
                .record_hybrid_prefilter_usage(0, 0, Some("vector-artifact"));
            cassie
                .runtime
                .record_hybrid_row_scan_fallback("invalid-vector-artifact");
            return Ok(None);
        }
    };
    let query_terms = filter::prepare_query_terms_with_analyzer(&spec.query, analyzer);
    let text_result =
        cassie
            .midge
            .fulltext_candidate_stats(&spec.collection, &fulltext_index.name, &query_terms);
    let generation_rejection = text_result.as_ref().err().is_some_and(|error| {
        error.to_string().contains("generation") || error.to_string().contains("stale")
    });
    let Ok(stats) = text_result else {
        cassie.runtime.record_hybrid_retrieval_diagnostics(
            0,
            0,
            0,
            usize::from(generation_rejection),
            0,
            0,
            0,
        );
        cassie
            .runtime
            .record_hybrid_prefilter_usage(0, 0, Some("text-artifact"));
        cassie
            .runtime
            .record_hybrid_row_scan_fallback("invalid-text-artifact");
        return Ok(None);
    };
    if stats.len() > cassie.runtime.limits().adaptive_candidate_max {
        cassie.runtime.record_hybrid_budget_rejection();
        cassie.runtime.record_hybrid_prefilter_usage(
            stats.len(),
            stats.len(),
            Some("candidate-budget"),
        );
        cassie
            .runtime
            .record_hybrid_row_scan_fallback("candidate-budget");
        return Ok(None);
    }
    let fields = schema
        .fields
        .iter()
        .map(|field| field.name.clone())
        .collect::<Vec<_>>();
    let mut rows = Vec::with_capacity(stats.len().min(vector_ids.len()));
    let mut candidate_row_fetches = 0usize;
    for id in stats.keys() {
        if !vector_ids.contains(id) {
            continue;
        }
        candidate_row_fetches += 1;
        let Some(document) = cassie
            .get_document_for_session(session, &spec.collection, id)
            .map_err(|error| QueryError::General(error.to_string()))?
        else {
            cassie
                .runtime
                .record_hybrid_row_scan_fallback("missing-candidate-row");
            return Ok(None);
        };
        rows.push(scan::projected_document_to_row(
            document,
            &fields,
            Some(schema),
        ));
    }
    if let Some(filter_expr) = &spec.filter {
        if !vector_prefilter_supported(filter_expr, schema) {
            cassie
                .runtime
                .record_hybrid_row_scan_fallback("unsupported-filter");
            return Ok(None);
        }
        let before = rows.len();
        rows = filter::filter_rows(rows, filter_expr, params, None, user_functions, session)?;
        cassie
            .runtime
            .record_hybrid_prefilter_usage(before, rows.len(), None);
    }
    Ok(Some(BoundedHybridRows {
        rows,
        ann_reads: vector_ids.len(),
        candidate_row_fetches,
    }))
}

pub(super) fn prefilter_hybrid_rows(
    cassie: &Cassie,
    session: Option<&CassieSession>,
    user_functions: &HashMap<String, FunctionMeta>,
    params: &[Value],
    spec: &HybridTopKSpec,
    schema: &CollectionSchema,
) -> Result<Option<Vec<BatchRow>>, QueryError> {
    let mut rows = batch::flatten_batches(scan::scan(cassie, session, &spec.collection)?);
    if let Some(filter_expr) = &spec.filter {
        if !vector_prefilter_supported(filter_expr, schema) {
            return Ok(None);
        }
        let before = rows.len();
        rows = filter::filter_rows(rows, filter_expr, params, None, user_functions, session)?;
        cassie
            .runtime
            .record_hybrid_prefilter_usage(before, rows.len(), None);
    }
    Ok(Some(rows))
}

pub(super) fn hybrid_search_documents(
    rows: Vec<BatchRow>,
    spec: &HybridTopKSpec,
    analyzer: &AnalyzerConfig,
) -> Vec<TokenizedHybridDocument> {
    rows.into_iter()
        .map(|row| TokenizedHybridDocument {
            id: row
                .get("id")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string(),
            text_stats: json_search_term_stats_value(row.get(&spec.text_field), analyzer),
            vector: row.get(&spec.vector_field).and_then(value_to_vector),
        })
        .collect()
}
