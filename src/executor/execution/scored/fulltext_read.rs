use super::super::projected_read::{is_row_id_column, json_to_query_value};
use super::{
    analyzer_for_search_field, batch, cached_search_context, filter, json_search_term_stats,
    posting_list_candidate_ids_controlled, BatchRow, BinaryOp, Cassie, CassieSession, Expr,
    FulltextFilteredReadSpec, FunctionMeta, HashMap, HashSet, Instant, PostingListDocument,
    QueryError, Value,
};
use crate::runtime::accounted::AccountedVec;
use crate::runtime::{FulltextIndexOptions, QueryExecutionControls, QueryMemoryReservation};
use crate::search::analyzer::AnalyzerConfig;

#[path = "fulltext_read/accounting.rs"]
mod accounting;

use accounting::{
    document_filter_row_bytes, fulltext_result_row_variable_bytes, persisted_search_context_bytes,
    reserve_analyzed_text, reserve_search_context,
};

pub(in crate::executor::execution) struct SearchProjectionColumn {
    pub(in crate::executor::execution) name: String,
    pub(in crate::executor::execution) output_name: String,
}

pub(in crate::executor::execution) struct SearchSnippetProjection {
    pub(in crate::executor::execution) field: String,
    pub(in crate::executor::execution) query: String,
    pub(in crate::executor::execution) output_name: String,
}

struct TokenizedFulltextReadDocument {
    id: String,
    payload: serde_json::Value,
    text_stats: filter::SearchTermStats,
}

struct FulltextFilteredExecution {
    rows: Vec<BatchRow>,
    candidate_count: usize,
    retrieval: Option<FulltextRetrievalMetrics>,
    fallback_reason: Option<&'static str>,
    _memory: Vec<QueryMemoryReservation>,
}

#[derive(Clone, Copy)]
struct FulltextRetrievalMetrics {
    posting_reads: usize,
    row_fetches: usize,
}

enum FilteredFulltextSelection {
    Selected(FulltextFilteredExecution),
    Exact(&'static str),
}

pub(super) enum FulltextFilterMatch {
    Exact,
    Residual(Expr),
}

pub(super) fn extract_fulltext_residual_filter(
    expr: &Expr,
    field: &str,
    query: &str,
    params: &[Value],
) -> Option<FulltextFilterMatch> {
    if let Expr::Function(function) = expr {
        let (filter_field, filter_query) =
            super::search_predicate_args_with_params(function, params)?;
        return (filter_field.eq_ignore_ascii_case(field) && filter_query == query)
            .then_some(FulltextFilterMatch::Exact);
    }
    let Expr::Binary {
        left,
        op: BinaryOp::And,
        right,
    } = expr
    else {
        return None;
    };
    if matches_fulltext_predicate(left, field, query, params) {
        return Some(FulltextFilterMatch::Residual((**right).clone()));
    }
    if matches_fulltext_predicate(right, field, query, params) {
        return Some(FulltextFilterMatch::Residual((**left).clone()));
    }
    None
}

fn matches_fulltext_predicate(expr: &Expr, field: &str, query: &str, params: &[Value]) -> bool {
    let Expr::Function(function) = expr else {
        return false;
    };
    super::search_predicate_args_with_params(function, params).is_some_and(
        |(filter_field, filter_query)| {
            filter_field.eq_ignore_ascii_case(field) && filter_query == query
        },
    )
}
impl PostingListDocument for TokenizedFulltextReadDocument {
    fn doc_id(&self) -> &str {
        &self.id
    }

    fn term_stats(&self) -> &filter::SearchTermStats {
        &self.text_stats
    }

    fn term_counts(&self) -> &HashMap<String, usize> {
        self.text_stats.term_counts()
    }
}
pub(super) fn execute_fulltext_filtered_read(
    cassie: &Cassie,
    session: Option<&CassieSession>,
    user_functions: &HashMap<String, FunctionMeta>,
    params: &[Value],
    spec: &FulltextFilteredReadSpec,
    controls: &QueryExecutionControls,
) -> Result<Vec<BatchRow>, QueryError> {
    super::super::check_timeout(controls)?;
    let started_at = Instant::now();
    let execution = match try_execute_persisted_fulltext_filtered_read(
        cassie,
        session,
        user_functions,
        params,
        spec,
        controls,
    )? {
        FilteredFulltextSelection::Selected(execution) => execution,
        FilteredFulltextSelection::Exact(reason) => execute_row_fulltext_filtered_read(
            cassie,
            session,
            user_functions,
            params,
            spec,
            controls,
            reason,
        )?,
    };
    publish_filtered_fulltext_metrics(cassie, started_at, &execution);
    Ok(execution.rows)
}

fn execute_row_fulltext_filtered_read(
    cassie: &Cassie,
    session: Option<&CassieSession>,
    user_functions: &HashMap<String, FunctionMeta>,
    params: &[Value],
    spec: &FulltextFilteredReadSpec,
    controls: &QueryExecutionControls,
    fallback_reason: &'static str,
) -> Result<FulltextFilteredExecution, QueryError> {
    let search_index_options = super::hybrid::search_context_for_fields(
        cassie,
        &spec.collection,
        std::slice::from_ref(&spec.text_field),
    )?;
    let analyzer = analyzer_for_search_field(&search_index_options, &spec.text_field);
    let (search_documents, mut memory) =
        load_tokenized_read_documents(cassie, session, spec, &analyzer, controls)?;
    let context_memory = reserve_search_context(
        controls,
        &search_documents,
        &search_index_options,
        &spec.text_field,
    )?;
    let search_context = cached_search_context(
        cassie,
        &spec.collection,
        &spec.text_field,
        &search_documents,
        super::FulltextSearchTuning {
            boost: &search_index_options.field_boost,
            k1: &search_index_options.field_k1,
            b: &search_index_options.field_b,
            analyzer: &search_index_options.field_analyzer,
        },
    )?;
    memory.push(context_memory);
    let query_memory = reserve_analyzed_text(controls, &spec.query, &analyzer)?;
    let query_terms = filter::prepare_query_terms_with_analyzer(&spec.query, &analyzer);
    memory.push(query_memory);
    let (candidate_ids, candidate_memory) =
        posting_list_candidate_ids_controlled(&search_documents, &query_terms, controls)?;
    let candidate_count = candidate_ids.len();
    memory.push(candidate_memory);
    let (rows, output_memory) = score_row_fulltext_documents(&RowFulltextScoreRequest {
        search_documents: &search_documents,
        candidate_ids: &candidate_ids,
        search_context: &search_context,
        query_terms: &query_terms,
        session,
        user_functions,
        params,
        spec,
        controls,
    })?;
    memory.push(output_memory);
    Ok(FulltextFilteredExecution {
        rows,
        candidate_count,
        retrieval: None,
        fallback_reason: Some(fallback_reason),
        _memory: memory,
    })
}

fn load_tokenized_read_documents(
    cassie: &Cassie,
    session: Option<&CassieSession>,
    spec: &FulltextFilteredReadSpec,
    analyzer: &AnalyzerConfig,
    controls: &QueryExecutionControls,
) -> Result<
    (
        Vec<TokenizedFulltextReadDocument>,
        Vec<QueryMemoryReservation>,
    ),
    QueryError,
> {
    let mut scan_fields = fulltext_filtered_scan_fields(spec);
    if spec.residual_filter.is_some() {
        if let Some(schema) = cassie.catalog.get_schema(&spec.collection) {
            for field in schema.fields {
                if !scan_fields
                    .iter()
                    .any(|existing| existing.eq_ignore_ascii_case(&field.name))
                {
                    scan_fields.push(field.name);
                }
            }
        }
    }
    let Some(mut cursor) = cassie
        .open_session_row_cursor(
            session,
            &spec.collection,
            crate::midge::adapter::RowDecode::ProjectedHistorical(scan_fields),
            controls,
        )
        .map_err(QueryError::from)?
    else {
        return Err(QueryError::General(
            "filtered fulltext exact fallback requires row storage".to_string(),
        ));
    };
    let mut search_documents = Vec::new();
    let mut memory = Vec::new();
    loop {
        let documents = cursor
            .next_accounted_documents(&cassie.midge, batch::DEFAULT_BATCH_SIZE, controls)
            .map_err(QueryError::from)?;
        if documents.is_empty() {
            break;
        }
        for document in documents {
            let text = json_projected_value(&document.document().payload, &spec.text_field)
                .and_then(serde_json::Value::as_str)
                .unwrap_or_default();
            let token_memory = controls.reserve_query_memory(
                std::mem::size_of::<TokenizedFulltextReadDocument>()
                    .saturating_add(super::memory::tokenized_text_upper_bound(text, analyzer)),
            )?;
            search_documents.try_reserve_exact(1).map_err(|error| {
                QueryError::from(crate::app::CassieError::ResourceLimit(format!(
                    "unable to retain filtered fulltext source: {error}"
                )))
            })?;
            let (document, source_memory) = document.into_parts();
            search_documents.push(TokenizedFulltextReadDocument {
                id: document.id,
                text_stats: json_search_term_stats(
                    json_projected_value(&document.payload, &spec.text_field),
                    analyzer,
                ),
                payload: document.payload,
            });
            memory.push(source_memory);
            memory.push(token_memory);
        }
    }
    Ok((search_documents, memory))
}

struct RowFulltextScoreRequest<'a> {
    search_documents: &'a [TokenizedFulltextReadDocument],
    candidate_ids: &'a HashSet<String>,
    search_context: &'a filter::SearchContext,
    query_terms: &'a [String],
    session: Option<&'a CassieSession>,
    user_functions: &'a HashMap<String, FunctionMeta>,
    params: &'a [Value],
    spec: &'a FulltextFilteredReadSpec,
    controls: &'a QueryExecutionControls,
}

fn score_row_fulltext_documents(
    request: &RowFulltextScoreRequest<'_>,
) -> Result<(Vec<BatchRow>, QueryMemoryReservation), QueryError> {
    let mut skipped = 0usize;
    let mut rows = AccountedVec::try_new(request.controls)?;
    for document in request.search_documents {
        super::super::check_timeout(request.controls)?;
        if !request.candidate_ids.contains(document.id.as_str()) {
            continue;
        }
        let score = request.search_context.score_term_stats(
            Some(&request.spec.text_field),
            &document.text_stats,
            request.query_terms,
        );
        if score == 0.0 {
            continue;
        }
        if let Some(residual_filter) = &request.spec.residual_filter {
            let _residual_memory = request
                .controls
                .reserve_query_memory(document_filter_row_bytes(&document.id, &document.payload))?;
            let candidate_row = document_filter_row(&document.id, &document.payload);
            if filter::filter_rows(
                vec![candidate_row],
                residual_filter,
                request.params,
                None,
                request.user_functions,
                request.session,
            )?
            .is_empty()
            {
                continue;
            }
        }
        if skipped < request.spec.offset {
            skipped += 1;
            continue;
        }
        if let Some(limit) = request.spec.limit {
            if rows.len() >= limit {
                break;
            }
        }

        rows.try_push_with(
            fulltext_result_row_variable_bytes(&document.id, &document.payload, request.spec),
            || {
                fulltext_result_row(
                    &document.id,
                    &document.payload,
                    score,
                    request.spec,
                    request.query_terms,
                )
            },
        )?;
    }

    Ok(rows.into_parts())
}

fn try_execute_persisted_fulltext_filtered_read(
    cassie: &Cassie,
    session: Option<&CassieSession>,
    user_functions: &HashMap<String, FunctionMeta>,
    params: &[Value],
    spec: &FulltextFilteredReadSpec,
    controls: &QueryExecutionControls,
) -> Result<FilteredFulltextSelection, QueryError> {
    let persisted = match load_persisted_fulltext_read(cassie, session, spec, controls)? {
        PersistedFulltextReadSelection::Ready(persisted) => *persisted,
        PersistedFulltextReadSelection::Exact(reason) => {
            return Ok(FilteredFulltextSelection::Exact(reason));
        }
    };
    let scalar_candidates = scalar_prefilter_ids(cassie, spec, params, controls)?;
    let matched = score_persisted_candidates(
        &persisted,
        scalar_candidates.as_ref().map(|candidates| &candidates.ids),
        &spec.text_field,
        controls,
    )?;
    let (matched, matched_memory) = matched.into_parts();
    let materialized = materialize_persisted_candidates(
        &PersistedMaterializeRequest {
            cassie,
            session,
            user_functions,
            params,
            spec,
            controls,
            query_terms: &persisted.query_terms,
        },
        matched,
    )?;
    let (rows, row_fetches, output_memory) = match materialized {
        PersistedMaterialization::Selected {
            rows,
            row_fetches,
            output_memory,
        } => (rows, row_fetches, output_memory),
        PersistedMaterialization::Exact(reason) => {
            return Ok(FilteredFulltextSelection::Exact(reason));
        }
    };
    let candidate_count = persisted.candidates.document_stats.len();
    let posting_reads = persisted.candidates.posting_block_reads;
    let mut memory = persisted.memory;
    memory.push(matched_memory);
    memory.push(output_memory);
    if let Some(scalar_candidates) = scalar_candidates {
        memory.extend(scalar_candidates.memory);
    }
    Ok(FilteredFulltextSelection::Selected(
        FulltextFilteredExecution {
            rows,
            candidate_count,
            retrieval: Some(FulltextRetrievalMetrics {
                posting_reads,
                row_fetches,
            }),
            fallback_reason: None,
            _memory: memory,
        },
    ))
}

struct PersistedFulltextRead {
    candidates: crate::midge::adapter::fulltext_retrieval::PersistedFulltextCandidateSet,
    query_terms: Vec<String>,
    search_context: filter::SearchContext,
    memory: Vec<QueryMemoryReservation>,
}

enum PersistedFulltextReadSelection {
    Ready(Box<PersistedFulltextRead>),
    Exact(&'static str),
}

fn load_persisted_fulltext_read(
    cassie: &Cassie,
    session: Option<&CassieSession>,
    spec: &FulltextFilteredReadSpec,
    controls: &QueryExecutionControls,
) -> Result<PersistedFulltextReadSelection, QueryError> {
    if session.is_some_and(|session| !session.collection_changes(&spec.collection).is_empty()) {
        return Ok(PersistedFulltextReadSelection::Exact("transaction_overlay"));
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
        return Ok(PersistedFulltextReadSelection::Exact("missing_index"));
    };
    let search_index_options = super::hybrid::search_context_for_fields(
        cassie,
        &spec.collection,
        std::slice::from_ref(&spec.text_field),
    )?;
    let analyzer = analyzer_for_search_field(&search_index_options, &spec.text_field);
    let query_memory = reserve_analyzed_text(controls, &spec.query, &analyzer)?;
    let query_terms = filter::prepare_query_terms_with_analyzer(&spec.query, &analyzer);
    let candidates = match cassie.midge.fulltext_candidate_set_controlled(
        &spec.collection,
        &index.name,
        &query_terms,
        controls,
    ) {
        Ok(candidates) => candidates,
        Err(error)
            if matches!(
                error,
                crate::app::CassieError::QueryCancelled
                    | crate::app::CassieError::DeadlineExceeded
                    | crate::app::CassieError::ResourceLimit(_)
            ) =>
        {
            return Err(QueryError::from(error));
        }
        Err(error) => {
            let reason = if error.to_string().contains("missing_candidate_row") {
                "missing_candidate_row"
            } else {
                "invalid_persisted_artifact"
            };
            return Ok(PersistedFulltextReadSelection::Exact(reason));
        }
    };
    let (candidates, candidate_memory) = candidates.into_parts();
    let context_memory = controls.reserve_query_memory(persisted_search_context_bytes(
        &candidates,
        &search_index_options,
        &spec.text_field,
    ))?;
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
    Ok(PersistedFulltextReadSelection::Ready(Box::new(
        PersistedFulltextRead {
            candidates,
            query_terms,
            search_context,
            memory: vec![query_memory, candidate_memory, context_memory],
        },
    )))
}

fn score_persisted_candidates<'a>(
    persisted: &'a PersistedFulltextRead,
    scalar_candidates: Option<&HashSet<String>>,
    text_field: &str,
    controls: &QueryExecutionControls,
) -> Result<AccountedVec<(&'a String, f64)>, QueryError> {
    let mut matched = AccountedVec::try_new(controls)?;
    for (id, stats) in &persisted.candidates.document_stats {
        super::super::check_timeout(controls)?;
        if scalar_candidates
            .as_ref()
            .is_some_and(|candidate_ids| !candidate_ids.contains(id))
        {
            continue;
        }
        let term_stats =
            filter::SearchTermStats::from_persisted(stats.doc_length, &stats.term_counts);
        let score = persisted.search_context.score_term_stats(
            Some(text_field),
            &term_stats,
            &persisted.query_terms,
        );
        if score > 0.0 {
            matched.try_push_with(0, || (id, score))?;
        }
    }
    Ok(matched)
}

struct PersistedMaterializeRequest<'a> {
    cassie: &'a Cassie,
    session: Option<&'a CassieSession>,
    user_functions: &'a HashMap<String, FunctionMeta>,
    params: &'a [Value],
    spec: &'a FulltextFilteredReadSpec,
    controls: &'a QueryExecutionControls,
    query_terms: &'a [String],
}

enum PersistedMaterialization {
    Selected {
        rows: Vec<BatchRow>,
        row_fetches: usize,
        output_memory: QueryMemoryReservation,
    },
    Exact(&'static str),
}

fn materialize_persisted_candidates(
    request: &PersistedMaterializeRequest<'_>,
    matched: Vec<(&String, f64)>,
) -> Result<PersistedMaterialization, QueryError> {
    let mut skipped = 0usize;
    let mut row_fetches = 0usize;
    let mut rows = AccountedVec::try_new(request.controls)?;
    for (id, score) in matched {
        super::super::check_timeout(request.controls)?;
        if request.spec.limit.is_some_and(|limit| rows.len() >= limit) {
            break;
        }
        let Some(document) = request
            .cassie
            .midge
            .get_retrieval_document_controlled(&request.spec.collection, id, request.controls)
            .map_err(QueryError::from)?
        else {
            return Ok(PersistedMaterialization::Exact("missing_candidate_row"));
        };
        let (document, _document_memory) = document.into_parts();
        row_fetches = row_fetches.saturating_add(1);
        if let Some(residual_filter) = &request.spec.residual_filter {
            let _residual_memory = request
                .controls
                .reserve_query_memory(document_filter_row_bytes(id, &document.payload))?;
            let candidate_row = document_filter_row(id, &document.payload);
            if filter::filter_rows(
                vec![candidate_row],
                residual_filter,
                request.params,
                None,
                request.user_functions,
                request.session,
            )?
            .is_empty()
            {
                continue;
            }
        }
        if skipped < request.spec.offset {
            skipped += 1;
            continue;
        }
        rows.try_push_with(
            fulltext_result_row_variable_bytes(id, &document.payload, request.spec),
            || {
                fulltext_result_row(
                    id,
                    &document.payload,
                    score,
                    request.spec,
                    request.query_terms,
                )
            },
        )?;
    }
    let (rows, output_memory) = rows.into_parts();
    Ok(PersistedMaterialization::Selected {
        rows,
        row_fetches,
        output_memory,
    })
}

struct ControlledScalarCandidates {
    ids: HashSet<String>,
    memory: Vec<QueryMemoryReservation>,
}

fn scalar_prefilter_ids(
    cassie: &Cassie,
    spec: &FulltextFilteredReadSpec,
    params: &[Value],
    controls: &QueryExecutionControls,
) -> Result<Option<ControlledScalarCandidates>, QueryError> {
    let Some(residual) = spec.residual_filter.as_ref() else {
        return Ok(None);
    };
    let Some((field, value)) = equality_literal(residual, params) else {
        return Ok(None);
    };
    let Some(index) = cassie
        .catalog
        .list_indexes(&spec.collection)
        .into_iter()
        .find(|index| {
            index.kind == crate::catalog::IndexKind::Scalar
                && index
                    .normalized_fields()
                    .first()
                    .is_some_and(|indexed| indexed.eq_ignore_ascii_case(&field))
        })
    else {
        return Ok(None);
    };
    let hits = cassie
        .midge
        .scan_scalar_index_controlled(
            &index,
            &crate::midge::adapter::ScalarIndexScanRequest {
                equality_prefix: vec![value],
                ..crate::midge::adapter::ScalarIndexScanRequest::default()
            },
            controls,
        )
        .map_err(QueryError::from)?;
    let (hits, hit_memory) = hits.into_parts();
    let retained_bytes = hits.iter().fold(0usize, |bytes, hit| {
        bytes
            .saturating_add(std::mem::size_of::<String>())
            .saturating_add(3 * std::mem::size_of::<usize>())
            .saturating_add(hit.id.len())
    });
    let id_memory = controls.reserve_query_memory(retained_bytes)?;
    let mut ids = HashSet::with_capacity(hits.len());
    ids.extend(hits.into_iter().map(|hit| hit.id));
    Ok(Some(ControlledScalarCandidates {
        ids,
        memory: vec![hit_memory, id_memory],
    }))
}

fn equality_literal(expr: &Expr, params: &[Value]) -> Option<(String, serde_json::Value)> {
    let Expr::Binary {
        left,
        op: BinaryOp::Eq,
        right,
    } = expr
    else {
        return None;
    };
    match (left.as_ref(), right.as_ref()) {
        (Expr::Column(field), literal) | (literal, Expr::Column(field)) => Some((
            crate::catalog::local_name(field),
            expression_literal(literal, params)?,
        )),
        _ => None,
    }
}

fn expression_literal(expr: &Expr, params: &[Value]) -> Option<serde_json::Value> {
    match expr {
        Expr::StringLiteral(value) => Some(serde_json::Value::String(value.clone())),
        Expr::NumberLiteral(value) => serde_json::Number::from_f64(*value).map(Into::into),
        Expr::BoolLiteral(value) => Some(serde_json::Value::Bool(*value)),
        Expr::Null => Some(serde_json::Value::Null),
        Expr::Param(index) => params.get(*index).and_then(value_literal),
        _ => None,
    }
}

fn value_literal(value: &Value) -> Option<serde_json::Value> {
    match value {
        Value::Null => Some(serde_json::Value::Null),
        Value::Bool(value) => Some(serde_json::Value::Bool(*value)),
        Value::Int64(value) => Some(serde_json::Value::Number((*value).into())),
        Value::Float64(value) => serde_json::Number::from_f64(*value).map(Into::into),
        Value::String(value) => Some(serde_json::Value::String(value.clone())),
        Value::Json(value) => Some(value.clone()),
        Value::Vector(_) => None,
    }
}

fn publish_filtered_fulltext_metrics(
    cassie: &Cassie,
    started_at: Instant,
    execution: &FulltextFilteredExecution,
) {
    if let Some(retrieval) = execution.retrieval {
        cassie
            .runtime
            .record_fulltext_retrieval_diagnostics(retrieval.posting_reads, retrieval.row_fetches);
    }
    if let Some(reason) = execution.fallback_reason {
        cassie.runtime.record_fulltext_row_scan_fallback(reason);
    }
    cassie.runtime.record_search_execution(
        started_at.elapsed(),
        execution.candidate_count,
        execution.rows.len(),
    );
}

fn document_filter_row(id: &str, payload: &serde_json::Value) -> BatchRow {
    let mut entries = vec![("id".to_string(), Value::String(id.to_string()))];
    if let Some(object) = payload.as_object() {
        entries.extend(
            object
                .iter()
                .map(|(name, value)| (name.clone(), json_to_query_value(value))),
        );
    }
    BatchRow::new(entries)
}

fn fulltext_result_row(
    id: &str,
    payload: &serde_json::Value,
    score: f64,
    spec: &FulltextFilteredReadSpec,
    query_terms: &[String],
) -> BatchRow {
    let mut entries = Vec::with_capacity(
        spec.columns
            .len()
            .saturating_add(spec.snippets.len())
            .saturating_add(1),
    );
    for column in &spec.columns {
        let value = if is_row_id_column(&column.name) {
            Value::String(id.to_string())
        } else {
            json_projected_value(payload, &column.name).map_or(Value::Null, json_to_query_value)
        };
        entries.push((column.output_name.clone(), value));
    }
    for snippet in &spec.snippets {
        let value = json_projected_value(payload, &snippet.field)
            .and_then(serde_json::Value::as_str)
            .map_or(Value::Null, |source| {
                Value::String(crate::search::snippet(source, query_terms))
            });
        entries.push((snippet.output_name.clone(), value));
    }
    entries.push((spec.score_column.clone(), Value::Float64(score)));
    BatchRow::new(entries)
}

fn fulltext_filtered_scan_fields(spec: &FulltextFilteredReadSpec) -> Vec<String> {
    let mut fields = vec![spec.text_field.clone()];
    for column in &spec.columns {
        if is_row_id_column(&column.name)
            || fields
                .iter()
                .any(|field| field.eq_ignore_ascii_case(&column.name))
        {
            continue;
        }
        fields.push(column.name.clone());
    }
    for snippet in &spec.snippets {
        if !fields
            .iter()
            .any(|field| field.eq_ignore_ascii_case(&snippet.field))
        {
            fields.push(snippet.field.clone());
        }
    }
    fields
}
fn json_projected_value<'a>(
    payload: &'a serde_json::Value,
    field: &str,
) -> Option<&'a serde_json::Value> {
    payload
        .as_object()?
        .iter()
        .find(|(name, _)| name.eq_ignore_ascii_case(field))
        .map(|(_, value)| value)
}
