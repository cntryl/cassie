use super::super::projected_read::{is_row_id_column, json_to_query_value};
use super::{
    analyzer_for_search_field, batch, cached_search_context, filter, json_search_term_stats,
    posting_list_candidate_ids, BatchRow, BinaryOp, Cassie, CassieSession, Expr,
    FulltextFilteredReadSpec, FunctionMeta, HashMap, HashSet, Instant, PostingListDocument,
    QueryError, Value,
};
use crate::runtime::FulltextIndexOptions;
use crate::runtime::QueryExecutionControls;
use crate::search::analyzer::AnalyzerConfig;

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
    if let Some(rows) = try_execute_persisted_fulltext_filtered_read(
        cassie,
        session,
        user_functions,
        params,
        spec,
        controls,
        started_at,
    )? {
        return Ok(rows);
    }
    execute_row_fulltext_filtered_read(
        cassie,
        session,
        user_functions,
        params,
        spec,
        controls,
        started_at,
    )
}

fn execute_row_fulltext_filtered_read(
    cassie: &Cassie,
    session: Option<&CassieSession>,
    user_functions: &HashMap<String, FunctionMeta>,
    params: &[Value],
    spec: &FulltextFilteredReadSpec,
    controls: &QueryExecutionControls,
    started_at: Instant,
) -> Result<Vec<BatchRow>, QueryError> {
    let (search_documents, search_index_options, analyzer) =
        load_tokenized_read_documents(cassie, session, spec)?;
    let search_document_bytes = tokenized_read_bytes(&search_documents);
    let _document_memory = controls.reserve_query_memory(search_document_bytes)?;
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
    let query_terms = filter::prepare_query_terms_with_analyzer(&spec.query, &analyzer);
    let candidate_ids = posting_list_candidate_ids(&search_documents, &query_terms);
    let _candidate_memory =
        controls.reserve_query_memory(candidate_ids.iter().map(String::len).sum())?;
    let rows = score_row_fulltext_documents(&RowFulltextScoreRequest {
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
    cassie
        .runtime
        .record_search_execution(started_at.elapsed(), candidate_ids.len(), rows.len());
    Ok(rows)
}

fn load_tokenized_read_documents(
    cassie: &Cassie,
    session: Option<&CassieSession>,
    spec: &FulltextFilteredReadSpec,
) -> Result<
    (
        Vec<TokenizedFulltextReadDocument>,
        FulltextIndexOptions,
        AnalyzerConfig,
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
    let document_batches = cassie
        .scan_projected_documents_batched_for_session(
            session,
            &spec.collection,
            batch::DEFAULT_BATCH_SIZE,
            &scan_fields,
            None,
        )
        .map_err(|error| QueryError::General(error.to_string()))?;
    let search_index_options = super::hybrid::search_context_for_fields(
        cassie,
        &spec.collection,
        std::slice::from_ref(&spec.text_field),
    )?;
    let analyzer = analyzer_for_search_field(&search_index_options, &spec.text_field);
    let search_documents = document_batches
        .into_iter()
        .flat_map(std::iter::IntoIterator::into_iter)
        .map(|document| TokenizedFulltextReadDocument {
            id: document.id,
            text_stats: json_search_term_stats(
                json_projected_value(&document.payload, &spec.text_field),
                &analyzer,
            ),
            payload: document.payload,
        })
        .collect::<Vec<_>>();
    Ok((search_documents, search_index_options, analyzer))
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
) -> Result<Vec<BatchRow>, QueryError> {
    let mut skipped = 0usize;
    let mut rows = Vec::new();
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

        rows.push(fulltext_result_row(
            &document.id,
            &document.payload,
            score,
            request.spec,
            request.query_terms,
        ));
    }

    Ok(rows)
}

fn try_execute_persisted_fulltext_filtered_read(
    cassie: &Cassie,
    session: Option<&CassieSession>,
    user_functions: &HashMap<String, FunctionMeta>,
    params: &[Value],
    spec: &FulltextFilteredReadSpec,
    controls: &QueryExecutionControls,
    started_at: Instant,
) -> Result<Option<Vec<BatchRow>>, QueryError> {
    let Some(persisted) = load_persisted_fulltext_read(cassie, session, spec, controls)? else {
        return Ok(None);
    };
    let scalar_candidates = scalar_prefilter_ids(cassie, spec, params)?;
    let matched = score_persisted_candidates(
        &persisted,
        scalar_candidates.as_ref(),
        &spec.text_field,
        controls,
    )?;
    let Some((rows, row_fetches)) = materialize_persisted_candidates(
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
    )?
    else {
        return Ok(None);
    };
    cassie.runtime.record_fulltext_retrieval_diagnostics(
        persisted.candidates.posting_block_reads,
        row_fetches,
    );
    cassie.runtime.record_search_execution(
        started_at.elapsed(),
        persisted.candidates.document_stats.len(),
        rows.len(),
    );
    Ok(Some(rows))
}

struct PersistedFulltextRead {
    candidates: crate::midge::adapter::fulltext_retrieval::PersistedFulltextCandidateSet,
    query_terms: Vec<String>,
    search_context: filter::SearchContext,
}

fn load_persisted_fulltext_read(
    cassie: &Cassie,
    session: Option<&CassieSession>,
    spec: &FulltextFilteredReadSpec,
    controls: &QueryExecutionControls,
) -> Result<Option<PersistedFulltextRead>, QueryError> {
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
    let search_index_options = super::hybrid::search_context_for_fields(
        cassie,
        &spec.collection,
        std::slice::from_ref(&spec.text_field),
    )?;
    let analyzer = analyzer_for_search_field(&search_index_options, &spec.text_field);
    let query_terms = filter::prepare_query_terms_with_analyzer(&spec.query, &analyzer);
    let Ok(candidates) =
        cassie
            .midge
            .fulltext_candidate_set(&spec.collection, &index.name, &query_terms)
    else {
        cassie
            .runtime
            .record_fulltext_row_scan_fallback("invalid_persisted_artifact");
        return Ok(None);
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
    Ok(Some(PersistedFulltextRead {
        candidates,
        query_terms,
        search_context,
    }))
}

fn score_persisted_candidates<'a>(
    persisted: &'a PersistedFulltextRead,
    scalar_candidates: Option<&HashSet<String>>,
    text_field: &str,
    controls: &QueryExecutionControls,
) -> Result<Vec<(&'a String, f64)>, QueryError> {
    let mut matched = Vec::new();
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
            matched.push((id, score));
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

fn materialize_persisted_candidates(
    request: &PersistedMaterializeRequest<'_>,
    matched: Vec<(&String, f64)>,
) -> Result<Option<(Vec<BatchRow>, usize)>, QueryError> {
    let mut skipped = 0usize;
    let mut row_fetches = 0usize;
    let mut rows = Vec::new();
    for (id, score) in matched {
        super::super::check_timeout(request.controls)?;
        if request.spec.limit.is_some_and(|limit| rows.len() >= limit) {
            break;
        }
        let Some(document) = request
            .cassie
            .get_document_for_session(request.session, &request.spec.collection, id)
            .map_err(|error| QueryError::General(error.to_string()))?
        else {
            request
                .cassie
                .runtime
                .record_fulltext_row_scan_fallback("missing_candidate_row");
            return Ok(None);
        };
        row_fetches = row_fetches.saturating_add(1);
        if let Some(residual_filter) = &request.spec.residual_filter {
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
        rows.push(fulltext_result_row(
            id,
            &document.payload,
            score,
            request.spec,
            request.query_terms,
        ));
    }
    Ok(Some((rows, row_fetches)))
}

fn scalar_prefilter_ids(
    cassie: &Cassie,
    spec: &FulltextFilteredReadSpec,
    params: &[Value],
) -> Result<Option<HashSet<String>>, QueryError> {
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
        .scan_scalar_index(
            &index,
            &crate::midge::adapter::ScalarIndexScanRequest {
                equality_prefix: vec![value],
                ..crate::midge::adapter::ScalarIndexScanRequest::default()
            },
        )
        .map_err(|error| QueryError::General(error.to_string()))?;
    Ok(Some(hits.into_iter().map(|hit| hit.id).collect()))
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

fn tokenized_read_bytes(documents: &[TokenizedFulltextReadDocument]) -> usize {
    documents
        .iter()
        .map(|document| {
            document
                .id
                .len()
                .saturating_add(
                    serde_json::to_vec(&document.payload)
                        .map(|bytes| bytes.len())
                        .unwrap_or_default(),
                )
                .saturating_add(
                    serde_json::to_vec(&document.text_stats)
                        .map(|bytes| bytes.len())
                        .unwrap_or_default(),
                )
        })
        .sum()
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
