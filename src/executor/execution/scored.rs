use super::projected_read::{is_row_id_column, json_to_query_value};
use super::*;

pub(super) fn execute_vector_distance_top_k(
    cassie: &Cassie,
    session: Option<&CassieSession>,
    user_functions: &HashMap<String, FunctionMeta>,
    params: &[Value],
    plan: &LogicalPlan,
) -> Result<Option<Vec<BatchRow>>, QueryError> {
    let Some(spec) = vector_distance_top_k_spec(plan) else {
        return Ok(None);
    };

    let schema = cassie.catalog.get_schema(&spec.collection).ok_or_else(|| {
        QueryError::General(format!("collection '{}' not found", spec.collection))
    })?;
    let mut candidates = batch::flatten_batches(scan::scan(cassie, session, &spec.collection)?);
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
    let top_needed = spec.limit.saturating_add(spec.offset).max(1);
    let adaptive = adaptive_candidate_decision(cassie, &spec.collection, top_needed)?;
    let mut top = BinaryHeap::with_capacity(top_needed.saturating_add(1));

    let final_candidate_count = candidates.len();
    for candidate in candidates {
        let vector = candidate
            .get(&spec.vector_field)
            .and_then(value_to_vector)
            .unwrap_or_default();
        let score = if vector.len() == spec.query.len() && !vector.is_empty() {
            crate::vector::l2_distance(&vector, &spec.query)
        } else {
            f64::INFINITY
        };
        let candidate = SqlVectorCandidate {
            sort_value: match spec.direction {
                SortDirection::Asc => score,
                SortDirection::Desc => -score,
            },
            score,
            id: candidate
                .get("id")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string(),
        };
        if top.len() < top_needed {
            top.push(candidate);
        } else if let Some(worst) = top.peek() {
            if candidate.is_better_than(worst) {
                top.pop();
                top.push(candidate);
            }
        }
    }

    let mut ranked = top.into_vec();
    ranked.sort_by(compare_sql_vector_candidates);
    let rows: Vec<BatchRow> = ranked
        .into_iter()
        .skip(spec.offset)
        .take(spec.limit)
        .map(|candidate| {
            BatchRow::new(vec![
                (spec.id_column.clone(), Value::String(candidate.id)),
                (spec.score_column.clone(), Value::Float64(candidate.score)),
            ])
        })
        .collect();
    record_adaptive_candidate_decision(cassie, adaptive, final_candidate_count, rows.len());
    Ok(Some(rows))
}

struct AdaptiveCandidateDecision {
    initial_budget: usize,
    feedback_budget: Option<usize>,
}

fn adaptive_candidate_decision(
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

fn record_adaptive_candidate_decision(
    cassie: &Cassie,
    decision: AdaptiveCandidateDecision,
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

fn vector_distance_top_k_spec(plan: &LogicalPlan) -> Option<VectorDistanceTopKSpec> {
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
    if !order_matches_vector_distance_score(&plan.order[0], function, &score_column) {
        return None;
    }

    let (vector_field, query) = vector_distance_args(function)?;
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
) -> bool {
    match &order.expr {
        Expr::Column(column) => column.eq_ignore_ascii_case(score_column),
        Expr::Function(order_function) => {
            order_function.name.eq_ignore_ascii_case("vector_distance")
                && vector_distance_args(order_function) == vector_distance_args(function)
        }
        _ => false,
    }
}

fn vector_distance_args(function: &FunctionCall) -> Option<(String, Vec<f32>)> {
    if function.args.len() != 2 {
        return None;
    }
    let Expr::Column(vector_field) = &function.args[0] else {
        return None;
    };
    let Expr::StringLiteral(query) = &function.args[1] else {
        return None;
    };
    Some((vector_field.clone(), parse_vector_literal(query)?))
}

pub(super) fn parse_vector_literal(value: &str) -> Option<Vec<f32>> {
    let values = serde_json::from_str::<Vec<f32>>(value).ok()?;
    if values.is_empty() {
        return None;
    }
    Some(values)
}

fn vector_from_json(value: &serde_json::Value) -> Option<Vec<f32>> {
    let values = value.as_array()?;
    let mut out = Vec::with_capacity(values.len());
    for value in values {
        out.push(value.as_f64()? as f32);
    }
    Some(out)
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

pub(super) fn execute_scored_search_top_k(
    cassie: &Cassie,
    session: Option<&CassieSession>,
    user_functions: &HashMap<String, FunctionMeta>,
    params: &[Value],
    plan: &LogicalPlan,
) -> Result<Option<Vec<BatchRow>>, QueryError> {
    if let Some(spec) = fulltext_top_k_spec(plan) {
        return execute_fulltext_top_k(cassie, spec).map(Some);
    }
    if let Some(spec) = hybrid_top_k_spec(plan) {
        return execute_hybrid_top_k(cassie, session, user_functions, params, spec);
    }
    if let Some(spec) = fulltext_filtered_read_spec(plan) {
        if virtual_views::schema(&spec.collection).is_some()
            || cassie.catalog.get_view(&spec.collection).is_some()
        {
            return Ok(None);
        }
        return execute_fulltext_filtered_read(cassie, session, spec).map(Some);
    }
    Ok(None)
}

#[derive(Clone)]
struct TokenizedFulltextDocument {
    id: String,
    text_stats: filter::SearchTermStats,
}

struct TokenizedFulltextReadDocument {
    id: String,
    payload: serde_json::Value,
    text_stats: filter::SearchTermStats,
}

struct TokenizedHybridDocument {
    id: String,
    text_stats: filter::SearchTermStats,
    vector: Option<Vec<f32>>,
}

trait PostingListDocument {
    fn doc_id(&self) -> &str;
    fn term_stats(&self) -> &filter::SearchTermStats;
    fn term_counts(&self) -> &HashMap<String, usize>;
}

impl PostingListDocument for TokenizedFulltextDocument {
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

impl PostingListDocument for TokenizedHybridDocument {
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

fn posting_list_candidate_ids<D>(documents: &[D], query_terms: &[String]) -> HashSet<String>
where
    D: PostingListDocument,
{
    if query_terms.is_empty() {
        return HashSet::new();
    }

    let mut index = crate::search::inverted_index::InvertedIndex::default();
    for document in documents {
        index.index_term_counts(document.doc_id(), document.term_counts());
    }
    index.candidate_documents(query_terms)
}

fn cached_search_context<D>(
    cassie: &Cassie,
    collection: &str,
    field: &str,
    documents: &[D],
    field_boost: &HashMap<String, f64>,
    field_k1: &HashMap<String, f64>,
    field_b: &HashMap<String, f64>,
    field_analyzer: &HashMap<String, AnalyzerConfig>,
) -> Result<filter::SearchContext, QueryError>
where
    D: PostingListDocument,
{
    let schema_epoch = cassie.runtime.schema_epoch();
    let data_epoch = cassie.runtime.data_epoch();
    let analyzer_key = field_analyzer
        .get(&field.to_ascii_lowercase())
        .cloned()
        .unwrap_or_default()
        .cache_key();
    if let Some(context) = query_cache::lookup_fulltext_stats(
        &cassie.midge,
        &cassie.runtime,
        collection,
        field,
        &analyzer_key,
        schema_epoch,
        data_epoch,
    )
    .map_err(|error| QueryError::General(error.to_string()))?
    {
        return Ok(context);
    }

    let context = filter::SearchContext::from_term_stats(
        field,
        documents.iter().map(|document| document.term_stats()),
        field_boost,
        field_k1,
        field_b,
        field_analyzer,
    );
    query_cache::store_fulltext_stats(
        &cassie.midge,
        &cassie.runtime,
        collection,
        field,
        &analyzer_key,
        schema_epoch,
        data_epoch,
        &context,
    )
    .map_err(|error| QueryError::General(error.to_string()))?;
    Ok(context)
}

fn execute_fulltext_top_k(
    cassie: &Cassie,
    spec: FulltextTopKSpec,
) -> Result<Vec<BatchRow>, QueryError> {
    let started_at = Instant::now();
    let adaptive = adaptive_candidate_decision(cassie, &spec.collection, spec.top_needed())?;
    let documents = cassie
        .midge
        .scan_rows_for_rebuild(
            &spec.collection,
            RowDecode::Projected(vec![spec.text_field.clone()]),
        )
        .map_err(|error| QueryError::General(error.to_string()))?;
    let search_index_options = search_context_for_fields(
        cassie,
        &spec.collection,
        std::slice::from_ref(&spec.text_field),
    )?;
    let analyzer = analyzer_for_search_field(&search_index_options, &spec.text_field);
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
        &search_index_options.field_boost,
        &search_index_options.field_k1,
        &search_index_options.field_b,
        &search_index_options.field_analyzer,
    )?;
    let query_terms = filter::prepare_query_terms_with_analyzer(&spec.query, &analyzer);
    let candidate_ids = if spec.require_match {
        Some(posting_list_candidate_ids(&search_documents, &query_terms))
    } else {
        None
    };
    let top = score_fulltext_top_k_candidates(
        cassie,
        &search_documents,
        candidate_ids.as_ref(),
        &search_context,
        &spec.text_field,
        &query_terms,
        spec.require_match,
        spec.top_needed(),
    );

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
    record_adaptive_candidate_decision(cassie, adaptive, candidate_count, rows.len());
    Ok(rows)
}

fn score_fulltext_top_k_candidates(
    cassie: &Cassie,
    documents: &[TokenizedFulltextDocument],
    candidate_ids: Option<&HashSet<String>>,
    search_context: &filter::SearchContext,
    text_field: &str,
    query_terms: &[String],
    require_match: bool,
    top_needed: usize,
) -> BinaryHeap<ScoredSearchCandidate> {
    let worker_limit = cassie.runtime.limits().parallel_scoring_workers.max(1);
    if worker_limit == 1 || documents.len() < batch::DEFAULT_BATCH_SIZE {
        cassie.runtime.record_parallel_scoring_fallback();
        return score_fulltext_partition(
            documents,
            candidate_ids,
            search_context,
            text_field,
            query_terms,
            require_match,
            top_needed,
        );
    }

    let workers = worker_limit.min(documents.len().div_ceil(batch::DEFAULT_BATCH_SIZE).max(1));
    let chunk_size = documents.len().div_ceil(workers).max(1);
    let partials = std::thread::scope(|scope| {
        let mut handles = Vec::new();
        for chunk in documents.chunks(chunk_size) {
            handles.push(scope.spawn(move || {
                score_fulltext_partition(
                    chunk,
                    candidate_ids,
                    search_context,
                    text_field,
                    query_terms,
                    require_match,
                    top_needed,
                )
            }));
        }
        handles
            .into_iter()
            .map(|handle| handle.join().expect("parallel scoring worker"))
            .collect::<Vec<_>>()
    });

    let partitions = partials.len();
    let mut merged = BinaryHeap::with_capacity(top_needed.saturating_add(1));
    let mut rows = 0usize;
    for partial in partials {
        for candidate in partial.into_vec() {
            rows += 1;
            push_top_k(&mut merged, top_needed, candidate);
        }
    }
    cassie
        .runtime
        .record_parallel_scoring(workers, partitions, rows);
    merged
}

fn score_fulltext_partition(
    documents: &[TokenizedFulltextDocument],
    candidate_ids: Option<&HashSet<String>>,
    search_context: &filter::SearchContext,
    text_field: &str,
    query_terms: &[String],
    require_match: bool,
    top_needed: usize,
) -> BinaryHeap<ScoredSearchCandidate> {
    let mut top = BinaryHeap::with_capacity(top_needed.saturating_add(1));
    for document in documents {
        if let Some(candidate_ids) = candidate_ids {
            if !candidate_ids.contains(document.id.as_str()) {
                continue;
            }
        }
        let score =
            search_context.score_term_stats(Some(text_field), &document.text_stats, query_terms);
        if require_match && score == 0.0 {
            continue;
        }
        push_top_k(
            &mut top,
            top_needed,
            ScoredSearchCandidate {
                sort_value: -score,
                score,
                id: document.id.clone(),
            },
        );
    }
    top
}

fn execute_hybrid_top_k(
    cassie: &Cassie,
    session: Option<&CassieSession>,
    user_functions: &HashMap<String, FunctionMeta>,
    params: &[Value],
    spec: HybridTopKSpec,
) -> Result<Option<Vec<BatchRow>>, QueryError> {
    let started_at = Instant::now();
    let adaptive = adaptive_candidate_decision(cassie, &spec.collection, spec.top_needed())?;
    let schema = cassie.catalog.get_schema(&spec.collection).ok_or_else(|| {
        QueryError::General(format!("collection '{}' not found", spec.collection))
    })?;
    let mut rows = batch::flatten_batches(scan::scan(cassie, session, &spec.collection)?);
    if let Some(filter_expr) = &spec.filter {
        if vector_prefilter_supported(filter_expr, &schema) {
            let before = rows.len();
            rows = filter::filter_rows(rows, filter_expr, params, None, user_functions, session)?;
            cassie
                .runtime
                .record_hybrid_prefilter_usage(before, rows.len(), None);
        } else {
            return Ok(None);
        }
    }
    let search_index_options = search_context_for_fields(
        cassie,
        &spec.collection,
        std::slice::from_ref(&spec.text_field),
    )?;
    let analyzer = analyzer_for_search_field(&search_index_options, &spec.text_field);
    let search_documents = rows
        .into_iter()
        .map(|row| TokenizedHybridDocument {
            id: row
                .get("id")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string(),
            text_stats: json_search_term_stats_value(row.get(&spec.text_field), &analyzer),
            vector: row.get(&spec.vector_field).and_then(value_to_vector),
        })
        .collect::<Vec<_>>();
    let search_context = cached_search_context(
        cassie,
        &spec.collection,
        &spec.text_field,
        &search_documents,
        &search_index_options.field_boost,
        &search_index_options.field_k1,
        &search_index_options.field_b,
        &search_index_options.field_analyzer,
    )?;
    let query_terms = filter::prepare_query_terms_with_analyzer(&spec.query, &analyzer);
    let candidate_ids = posting_list_candidate_ids(&search_documents, &query_terms);
    let mut top = BinaryHeap::with_capacity(spec.top_needed().saturating_add(1));
    let mut text_candidate_count = 0usize;

    for document in &search_documents {
        if !candidate_ids.contains(document.id.as_str()) {
            continue;
        }
        let search_score = search_context.score_term_stats(
            Some(&spec.text_field),
            &document.text_stats,
            &query_terms,
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
        let candidate = ScoredSearchCandidate {
            sort_value: -score,
            score,
            id: document.id.clone(),
        };
        push_top_k(&mut top, spec.top_needed(), candidate);
    }

    let rows = scored_candidates_to_rows(
        top,
        spec.offset,
        spec.limit,
        &spec.id_column,
        &spec.score_column,
    );
    let candidate_count = candidate_ids.len();
    cassie
        .runtime
        .record_search_execution(started_at.elapsed(), candidate_count, rows.len());
    cassie
        .runtime
        .record_vector_execution(started_at.elapsed(), text_candidate_count, rows.len());
    cassie
        .runtime
        .record_hybrid_execution(started_at.elapsed(), text_candidate_count, rows.len());
    record_adaptive_candidate_decision(cassie, adaptive, text_candidate_count, rows.len());
    Ok(Some(rows))
}

fn execute_fulltext_filtered_read(
    cassie: &Cassie,
    session: Option<&CassieSession>,
    spec: FulltextFilteredReadSpec,
) -> Result<Vec<BatchRow>, QueryError> {
    let started_at = Instant::now();
    let scan_fields = fulltext_filtered_scan_fields(&spec);
    let document_batches = cassie
        .scan_projected_documents_batched_for_session(
            session,
            &spec.collection,
            batch::DEFAULT_BATCH_SIZE,
            &scan_fields,
            None,
        )
        .map_err(|error| QueryError::General(error.to_string()))?;
    let search_index_options = search_context_for_fields(
        cassie,
        &spec.collection,
        std::slice::from_ref(&spec.text_field),
    )?;
    let analyzer = analyzer_for_search_field(&search_index_options, &spec.text_field);
    let search_documents = document_batches
        .into_iter()
        .flat_map(|documents| documents.into_iter())
        .map(|document| TokenizedFulltextReadDocument {
            id: document.id,
            text_stats: json_search_term_stats(
                json_projected_value(&document.payload, &spec.text_field),
                &analyzer,
            ),
            payload: document.payload,
        })
        .collect::<Vec<_>>();
    let search_context = cached_search_context(
        cassie,
        &spec.collection,
        &spec.text_field,
        &search_documents,
        &search_index_options.field_boost,
        &search_index_options.field_k1,
        &search_index_options.field_b,
        &search_index_options.field_analyzer,
    )?;
    let query_terms = filter::prepare_query_terms_with_analyzer(&spec.query, &analyzer);
    let candidate_ids = posting_list_candidate_ids(&search_documents, &query_terms);

    let mut skipped = 0usize;
    let mut rows = Vec::new();
    for document in &search_documents {
        if !candidate_ids.contains(document.id.as_str()) {
            continue;
        }
        let score = search_context.score_term_stats(
            Some(&spec.text_field),
            &document.text_stats,
            &query_terms,
        );
        if score == 0.0 {
            continue;
        }
        if skipped < spec.offset {
            skipped += 1;
            continue;
        }
        if let Some(limit) = spec.limit {
            if rows.len() >= limit {
                break;
            }
        }

        let mut entries = Vec::with_capacity(spec.columns.len().saturating_add(1));
        for column in &spec.columns {
            let value = if is_row_id_column(&column.name) {
                Value::String(document.id.clone())
            } else {
                json_projected_value(&document.payload, &column.name)
                    .map(json_to_query_value)
                    .unwrap_or(Value::Null)
            };
            entries.push((column.output_name.clone(), value));
        }
        entries.push((spec.score_column.clone(), Value::Float64(score)));
        rows.push(BatchRow::new(entries));
    }

    let candidate_count = candidate_ids.len();
    cassie
        .runtime
        .record_search_execution(started_at.elapsed(), candidate_count, rows.len());
    Ok(rows)
}

fn search_context_for_fields(
    cassie: &Cassie,
    collection: &str,
    fields: &[String],
) -> Result<FulltextIndexOptions, QueryError> {
    let requested_fields = fields
        .iter()
        .map(|field| field.to_ascii_lowercase())
        .collect::<HashSet<_>>();
    load_fulltext_index_options(cassie, collection, &requested_fields)
}

struct FulltextTopKSpec {
    collection: String,
    text_field: String,
    query: String,
    id_column: String,
    score_column: String,
    require_match: bool,
    limit: usize,
    offset: usize,
}

impl FulltextTopKSpec {
    fn top_needed(&self) -> usize {
        self.limit.saturating_add(self.offset).max(1)
    }
}

pub(super) struct SearchProjectionColumn {
    pub(super) name: String,
    pub(super) output_name: String,
}

pub(super) struct FulltextFilteredReadSpec {
    pub(super) collection: String,
    pub(super) text_field: String,
    pub(super) query: String,
    pub(super) columns: Vec<SearchProjectionColumn>,
    pub(super) score_column: String,
    limit: Option<usize>,
    offset: usize,
}

struct HybridTopKSpec {
    collection: String,
    text_field: String,
    query: String,
    vector_field: String,
    vector_query: Vec<f32>,
    filter: Option<Expr>,
    id_column: String,
    score_column: String,
    limit: usize,
    offset: usize,
}

impl HybridTopKSpec {
    fn top_needed(&self) -> usize {
        self.limit.saturating_add(self.offset).max(1)
    }
}

fn fulltext_top_k_spec(plan: &LogicalPlan) -> Option<FulltextTopKSpec> {
    if !simple_scored_top_k_plan(plan) {
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
        scored_projection(plan.projection.as_slice(), "search_score")?;
    if !order_matches_function_score(&plan.order[0], function, &score_column) {
        return None;
    }
    let (text_field, query) = search_function_args(function)?;
    let require_match = match &plan.filter {
        None => false,
        Some(Expr::Function(filter)) => {
            let (filter_field, filter_query) = search_predicate_args(filter)?;
            filter_field.eq_ignore_ascii_case(&text_field) && filter_query == query
        }
        _ => return None,
    };
    if plan.filter.is_some() && !require_match {
        return None;
    }

    Some(FulltextTopKSpec {
        collection: collection.clone(),
        text_field,
        query,
        id_column,
        score_column,
        require_match,
        limit,
        offset,
    })
}

pub(super) fn fulltext_filtered_read_spec(plan: &LogicalPlan) -> Option<FulltextFilteredReadSpec> {
    if plan.command.is_some()
        || !plan.ctes.is_empty()
        || plan.distinct
        || !plan.distinct_on.is_empty()
        || !plan.group_by.is_empty()
        || plan.having.is_some()
        || plan.set.is_some()
        || !plan.order.is_empty()
    {
        return None;
    }
    let QuerySource::Collection(collection) = &plan.source else {
        return None;
    };
    let (columns, function, score_column) =
        fulltext_filtered_projection(plan.projection.as_slice())?;
    let (text_field, query) = search_function_args(function)?;
    let filter = plan.filter.as_ref()?;
    let Expr::Function(filter_function) = filter else {
        return None;
    };
    let (filter_field, filter_query) = search_predicate_args(filter_function)?;
    if !filter_field.eq_ignore_ascii_case(&text_field) || filter_query != query {
        return None;
    }

    let limit = if let Some(limit) = plan.limit {
        Some(usize::try_from(limit.max(0)).ok()?)
    } else {
        None
    };
    let offset = plan
        .offset
        .and_then(|offset| usize::try_from(offset.max(0)).ok())
        .unwrap_or(0);

    Some(FulltextFilteredReadSpec {
        collection: collection.clone(),
        text_field,
        query,
        columns,
        score_column,
        limit,
        offset,
    })
}

fn fulltext_filtered_projection(
    projection: &[SelectItem],
) -> Option<(Vec<SearchProjectionColumn>, &FunctionCall, String)> {
    let (last, columns) = projection.split_last()?;
    let SelectItem::Function {
        function,
        alias: score_alias,
    } = last
    else {
        return None;
    };
    if !function.name.eq_ignore_ascii_case("search_score") {
        return None;
    }
    let columns = columns
        .iter()
        .map(|item| match item {
            SelectItem::Column { name, alias } => Some(SearchProjectionColumn {
                name: name.clone(),
                output_name: alias.clone().unwrap_or_else(|| name.clone()),
            }),
            _ => None,
        })
        .collect::<Option<Vec<_>>>()?;
    if columns.is_empty() {
        return None;
    }

    Some((
        columns,
        function,
        score_alias.clone().unwrap_or_else(|| function.name.clone()),
    ))
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
    fields
}

fn hybrid_top_k_spec(plan: &LogicalPlan) -> Option<HybridTopKSpec> {
    if !simple_scored_top_k_plan(plan) {
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
        scored_projection(plan.projection.as_slice(), "hybrid_score")?;
    if !order_matches_function_score(&plan.order[0], function, &score_column) {
        return None;
    }
    let (text_field, query, vector_field, vector_query) = hybrid_function_args(function)?;

    Some(HybridTopKSpec {
        collection: collection.clone(),
        text_field,
        query,
        vector_field,
        vector_query,
        filter: plan.filter.clone(),
        id_column,
        score_column,
        limit,
        offset,
    })
}

fn simple_scored_top_k_plan(plan: &LogicalPlan) -> bool {
    plan.command.is_none()
        && plan.ctes.is_empty()
        && !plan.distinct
        && plan.distinct_on.is_empty()
        && plan.group_by.is_empty()
        && plan.having.is_none()
        && plan.set.is_none()
        && plan.order.len() == 1
        && matches!(plan.order[0].direction, SortDirection::Desc)
        && plan.order[0].nulls.is_none()
        && plan.projection.len() == 2
}

fn scored_projection<'a>(
    projection: &'a [SelectItem],
    function_name: &str,
) -> Option<(String, &'a FunctionCall, String)> {
    let SelectItem::Column { name, alias } = &projection[0] else {
        return None;
    };
    if !name.eq_ignore_ascii_case("id") && !name.eq_ignore_ascii_case("_id") {
        return None;
    }
    let SelectItem::Function {
        function,
        alias: score_alias,
    } = &projection[1]
    else {
        return None;
    };
    if !function.name.eq_ignore_ascii_case(function_name) {
        return None;
    }
    Some((
        alias.clone().unwrap_or_else(|| name.clone()),
        function,
        score_alias.clone().unwrap_or_else(|| function.name.clone()),
    ))
}

fn order_matches_function_score(
    order: &crate::sql::ast::OrderExpr,
    function: &FunctionCall,
    score_column: &str,
) -> bool {
    match &order.expr {
        Expr::Column(column) => column.eq_ignore_ascii_case(score_column),
        Expr::Function(order_function) => {
            function_call_key(order_function) == function_call_key(function)
        }
        _ => false,
    }
}

fn search_function_args(function: &FunctionCall) -> Option<(String, String)> {
    if !function.name.eq_ignore_ascii_case("search_score") || function.args.len() != 2 {
        return None;
    }
    let Expr::Column(field) = &function.args[0] else {
        return None;
    };
    let Expr::StringLiteral(query) = &function.args[1] else {
        return None;
    };
    Some((field.clone(), query.clone()))
}

fn search_predicate_args(function: &FunctionCall) -> Option<(String, String)> {
    if !matches!(
        function.name.to_ascii_lowercase().as_str(),
        "search" | "search_score"
    ) || function.args.len() != 2
    {
        return None;
    }
    let Expr::Column(field) = &function.args[0] else {
        return None;
    };
    let Expr::StringLiteral(query) = &function.args[1] else {
        return None;
    };
    Some((field.clone(), query.clone()))
}

fn hybrid_function_args(function: &FunctionCall) -> Option<(String, String, String, Vec<f32>)> {
    if !function.name.eq_ignore_ascii_case("hybrid_score") || function.args.len() != 2 {
        return None;
    }
    let Expr::Function(search_function) = &function.args[0] else {
        return None;
    };
    let Expr::Function(vector_function) = &function.args[1] else {
        return None;
    };
    let (text_field, query) = search_function_args(search_function)?;
    let (vector_field, vector_query) = vector_score_args(vector_function)?;
    Some((text_field, query, vector_field, vector_query))
}

fn vector_score_args(function: &FunctionCall) -> Option<(String, Vec<f32>)> {
    if !function.name.eq_ignore_ascii_case("vector_score") || function.args.len() != 2 {
        return None;
    }
    let Expr::Column(field) = &function.args[0] else {
        return None;
    };
    let Expr::StringLiteral(query) = &function.args[1] else {
        return None;
    };
    Some((field.clone(), parse_vector_literal(query)?))
}

fn function_call_key(function: &FunctionCall) -> String {
    let args = function
        .args
        .iter()
        .map(expr_key)
        .collect::<Vec<_>>()
        .join(",");
    format!("{}({})", function.name.to_ascii_lowercase(), args)
}

fn json_search_term_stats(
    value: Option<&serde_json::Value>,
    analyzer: &AnalyzerConfig,
) -> filter::SearchTermStats {
    filter::SearchTermStats::from_text_with_analyzer(
        value.and_then(serde_json::Value::as_str),
        analyzer,
    )
}

fn json_search_term_stats_value(
    value: Option<&Value>,
    analyzer: &AnalyzerConfig,
) -> filter::SearchTermStats {
    filter::SearchTermStats::from_text_with_analyzer(value.and_then(Value::as_str), analyzer)
}

fn analyzer_for_search_field(options: &FulltextIndexOptions, field: &str) -> AnalyzerConfig {
    options
        .field_analyzer
        .get(&field.to_ascii_lowercase())
        .cloned()
        .unwrap_or_default()
}

fn value_to_vector(value: &Value) -> Option<Vec<f32>> {
    match value {
        Value::Vector(vector) => Some(vector.values.clone()),
        Value::Json(json) => vector_from_json(json),
        _ => None,
    }
}

pub(crate) fn vector_prefilter_supported(expr: &Expr, schema: &CollectionSchema) -> bool {
    match expr {
        Expr::Column(name) => schema
            .fields
            .iter()
            .find(|field| field.name.eq_ignore_ascii_case(name))
            .map(|field| !matches!(field.data_type, DataType::Vector(_)))
            .unwrap_or(true),
        Expr::StringLiteral(_)
        | Expr::NumberLiteral(_)
        | Expr::BoolLiteral(_)
        | Expr::Null
        | Expr::Param(_) => true,
        Expr::Binary { left, op, right } => {
            matches!(
                op,
                BinaryOp::Eq
                    | BinaryOp::NotEq
                    | BinaryOp::Lt
                    | BinaryOp::Lte
                    | BinaryOp::Gt
                    | BinaryOp::Gte
                    | BinaryOp::And
                    | BinaryOp::Or
                    | BinaryOp::Like
            ) && vector_prefilter_supported(left, schema)
                && vector_prefilter_supported(right, schema)
        }
        Expr::IsNull { expr, .. } | Expr::Not { expr } | Expr::Cast { expr, .. } => {
            vector_prefilter_supported(expr, schema)
        }
        Expr::InList { expr, values, .. } => {
            vector_prefilter_supported(expr, schema)
                && values
                    .iter()
                    .all(|value| vector_prefilter_supported(value, schema))
        }
        Expr::Between {
            expr, low, high, ..
        } => {
            vector_prefilter_supported(expr, schema)
                && vector_prefilter_supported(low, schema)
                && vector_prefilter_supported(high, schema)
        }
        Expr::Function(_) | Expr::Exists(_) => false,
    }
}

pub(crate) fn vector_prefilter_fallback_reason(
    expr: &Expr,
    schema: &CollectionSchema,
) -> &'static str {
    if contains_vector_field(expr, schema) {
        "vector field in metadata predicate"
    } else {
        "unsupported metadata predicate"
    }
}

fn contains_vector_field(expr: &Expr, schema: &CollectionSchema) -> bool {
    match expr {
        Expr::Column(name) => schema.fields.iter().any(|field| {
            field.name.eq_ignore_ascii_case(name) && matches!(field.data_type, DataType::Vector(_))
        }),
        Expr::Binary { left, right, .. } => {
            contains_vector_field(left, schema) || contains_vector_field(right, schema)
        }
        Expr::IsNull { expr, .. } | Expr::Not { expr } | Expr::Cast { expr, .. } => {
            contains_vector_field(expr, schema)
        }
        Expr::InList { expr, values, .. } => {
            contains_vector_field(expr, schema)
                || values
                    .iter()
                    .any(|value| contains_vector_field(value, schema))
        }
        Expr::Between {
            expr, low, high, ..
        } => {
            contains_vector_field(expr, schema)
                || contains_vector_field(low, schema)
                || contains_vector_field(high, schema)
        }
        Expr::Function(function) => function
            .args
            .iter()
            .any(|arg| contains_vector_field(arg, schema)),
        Expr::Exists(_) => true,
        Expr::StringLiteral(_)
        | Expr::NumberLiteral(_)
        | Expr::BoolLiteral(_)
        | Expr::Null
        | Expr::Param(_) => false,
    }
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

#[derive(Debug, Clone, PartialEq)]
struct ScoredSearchCandidate {
    sort_value: f64,
    score: f64,
    id: String,
}

impl ScoredSearchCandidate {
    fn is_better_than(&self, other: &Self) -> bool {
        compare_scored_search_candidates(self, other) == CmpOrdering::Less
    }
}

impl Eq for ScoredSearchCandidate {}

impl PartialOrd for ScoredSearchCandidate {
    fn partial_cmp(&self, other: &Self) -> Option<CmpOrdering> {
        Some(self.cmp(other))
    }
}

impl Ord for ScoredSearchCandidate {
    fn cmp(&self, other: &Self) -> CmpOrdering {
        compare_scored_search_candidates(self, other)
    }
}

fn compare_scored_search_candidates(
    left: &ScoredSearchCandidate,
    right: &ScoredSearchCandidate,
) -> CmpOrdering {
    left.sort_value
        .total_cmp(&right.sort_value)
        .then_with(|| left.id.cmp(&right.id))
}

fn push_top_k(
    top: &mut BinaryHeap<ScoredSearchCandidate>,
    top_needed: usize,
    candidate: ScoredSearchCandidate,
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

fn scored_candidates_to_rows(
    top: BinaryHeap<ScoredSearchCandidate>,
    offset: usize,
    limit: usize,
    id_column: &str,
    score_column: &str,
) -> Vec<BatchRow> {
    let mut ranked = top.into_vec();
    ranked.sort_by(compare_scored_search_candidates);
    ranked
        .into_iter()
        .skip(offset)
        .take(limit)
        .map(|candidate| {
            BatchRow::new(vec![
                (id_column.to_string(), Value::String(candidate.id)),
                (score_column.to_string(), Value::Float64(candidate.score)),
            ])
        })
        .collect()
}
