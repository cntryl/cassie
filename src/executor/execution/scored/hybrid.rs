use super::{
    json_search_term_stats_value, value_to_vector, vector_prefilter_supported, BatchRow, Cassie,
    CassieSession, CollectionSchema, FunctionMeta, HybridTopKSpec, QueryError,
    TokenizedHybridDocument, Value,
};
use crate::executor::{batch, filter, scan};
use crate::midge::adapter::RowDecode;
use crate::runtime::{HybridRetrievalDiagnostics, QueryExecutionControls, QueryMemoryReservation};
use crate::search::analyzer::AnalyzerConfig;
use std::collections::HashMap;
use std::collections::{BTreeSet, HashSet};

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
    generation_rejections: usize,
) {
    cassie
        .runtime
        .record_hybrid_retrieval_diagnostics(&HybridRetrievalDiagnostics {
            posting_reads,
            ann_reads,
            candidate_row_fetches,
            generation_rejections,
            exact_reranks,
            ..HybridRetrievalDiagnostics::default()
        });
}

pub(super) struct BoundedHybridRows {
    pub(super) rows: Vec<BatchRow>,
    pub(super) posting_reads: usize,
    pub(super) ann_reads: usize,
    pub(super) ann_candidates: usize,
    pub(super) candidate_row_fetches: usize,
    pub(super) retrieval_memory: Vec<QueryMemoryReservation>,
    pub(super) diagnostics: HybridSelectionDiagnostics,
}

pub(super) struct BoundedHybridContext<'a> {
    pub(super) user_functions: &'a HashMap<String, FunctionMeta>,
    pub(super) params: &'a [Value],
    pub(super) schema: &'a CollectionSchema,
    pub(super) analyzer: &'a AnalyzerConfig,
    pub(super) candidate_limit: usize,
}

#[derive(Clone, Copy, Default)]
pub(super) struct HybridSelectionDiagnostics {
    generation_rejections: usize,
    budget_rejection: bool,
    fallback_reason: Option<&'static str>,
    fallback_prefilter: Option<HybridPrefilterUsage>,
    selected_prefilter: Option<HybridPrefilterUsage>,
}

#[derive(Clone, Copy)]
struct HybridPrefilterUsage {
    input_candidates: usize,
    filtered_candidates: usize,
    fallback_reason: Option<&'static str>,
}

impl HybridSelectionDiagnostics {
    pub(super) const fn generation_rejections(self) -> usize {
        self.generation_rejections
    }

    pub(super) fn publish_path_decisions(self, cassie: &Cassie) {
        for usage in [self.fallback_prefilter, self.selected_prefilter]
            .into_iter()
            .flatten()
        {
            cassie.runtime.record_hybrid_prefilter_usage(
                usage.input_candidates,
                usage.filtered_candidates,
                usage.fallback_reason,
            );
        }
        if self.budget_rejection {
            cassie.runtime.record_hybrid_budget_rejection();
        }
        if let Some(reason) = self.fallback_reason {
            cassie.runtime.record_hybrid_row_scan_fallback(reason);
        }
    }

    fn select_fallback(&mut self, reason: &'static str, prefilter_reason: Option<&'static str>) {
        self.fallback_reason = Some(reason);
        self.fallback_prefilter = prefilter_reason.map(|fallback_reason| HybridPrefilterUsage {
            input_candidates: 0,
            filtered_candidates: 0,
            fallback_reason: Some(fallback_reason),
        });
    }

    fn select_candidate_budget_fallback(&mut self, candidates: usize) {
        self.budget_rejection = true;
        self.fallback_reason = Some("candidate-budget");
        self.fallback_prefilter = Some(HybridPrefilterUsage {
            input_candidates: candidates,
            filtered_candidates: candidates,
            fallback_reason: Some("candidate-budget"),
        });
    }

    fn select_prefilter(&mut self, input_candidates: usize, filtered_candidates: usize) {
        self.selected_prefilter = Some(HybridPrefilterUsage {
            input_candidates,
            filtered_candidates,
            fallback_reason: None,
        });
    }
}

pub(super) fn score_hybrid_documents(
    documents: &[super::TokenizedHybridDocument],
    candidate_ids: &HashSet<String>,
    search_context: &filter::SearchContext,
    spec: &HybridTopKSpec,
    query_terms: &[String],
    controls: &QueryExecutionControls,
) -> Result<(super::memory::AccountedScoredCandidates, usize), QueryError> {
    let mut top = super::memory::AccountedScoredCandidates::try_new(controls, spec.top_needed())?;
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
        top.try_push(spec.top_needed(), -score, score, &document.id)?;
    }
    Ok((top, text_candidate_count))
}

pub(super) fn bounded_hybrid_rows(
    cassie: &Cassie,
    session: Option<&CassieSession>,
    spec: &HybridTopKSpec,
    context: &BoundedHybridContext<'_>,
    controls: &QueryExecutionControls,
    diagnostics: &mut HybridSelectionDiagnostics,
) -> Result<Option<BoundedHybridRows>, QueryError> {
    if session.is_some_and(|session| !session.collection_changes(&spec.collection).is_empty()) {
        diagnostics.select_fallback("transaction-overlay", None);
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
        diagnostics.select_fallback("missing-text-index", None);
        return Ok(None);
    };
    let Some(vector_candidates) = load_bounded_vector_candidates(
        cassie,
        spec,
        context.candidate_limit,
        controls,
        diagnostics,
    )?
    else {
        return Ok(None);
    };
    let ControlledVectorCandidateIds {
        ids: vector_ids,
        ann_reads,
        memory: vector_memory,
    } = vector_candidates;
    let ann_candidates = vector_ids.len();
    let query_terms = filter::prepare_query_terms_with_analyzer(&spec.query, context.analyzer);
    let text_result = cassie.midge.fulltext_candidate_set_for_ids_controlled(
        &spec.collection,
        &fulltext_index.name,
        &query_terms,
        &vector_ids,
        controls,
    );
    let generation_rejection = text_result.as_ref().err().is_some_and(|error| {
        error.to_string().contains("generation") || error.to_string().contains("stale")
    });
    let text_candidates = match text_result {
        Ok(candidates) => candidates,
        Err(error) if is_query_control_error(&error) => return Err(QueryError::from(error)),
        Err(_) => {
            diagnostics.generation_rejections = usize::from(generation_rejection);
            diagnostics.select_fallback("invalid-text-artifact", Some("text-artifact"));
            return Ok(None);
        }
    };
    let (text_candidates, text_memory) = text_candidates.into_parts();
    let stats = &text_candidates.document_stats;
    if stats.len() > cassie.runtime.limits().adaptive_candidate_max {
        diagnostics.select_candidate_budget_fallback(stats.len());
        return Ok(None);
    }
    fetch_hybrid_candidate_rows(
        &HybridCandidateFetch {
            cassie,
            session,
            spec,
            context,
            vector_ids: &vector_ids,
            posting_reads: text_candidates.posting_block_reads,
            ann_reads,
            ann_candidates,
            controls,
        },
        stats.keys(),
        vec![vector_memory, text_memory],
        diagnostics,
    )
}

pub(super) fn select_hybrid_candidate_rows(
    cassie: &Cassie,
    session: Option<&CassieSession>,
    spec: &HybridTopKSpec,
    context: &BoundedHybridContext<'_>,
    controls: &QueryExecutionControls,
) -> Result<Option<BoundedHybridRows>, QueryError> {
    let mut diagnostics = HybridSelectionDiagnostics::default();
    if let Some(mut rows) =
        bounded_hybrid_rows(cassie, session, spec, context, controls, &mut diagnostics)?
    {
        rows.diagnostics = diagnostics;
        return Ok(Some(rows));
    }
    let Some(exact) =
        prefilter_hybrid_rows(cassie, session, spec, context, controls, &mut diagnostics)?
    else {
        return Ok(None);
    };
    Ok(Some(BoundedHybridRows {
        rows: exact.rows,
        posting_reads: 0,
        ann_reads: 0,
        ann_candidates: 0,
        candidate_row_fetches: 0,
        retrieval_memory: exact.memory,
        diagnostics,
    }))
}

struct ControlledVectorCandidateIds {
    ids: BTreeSet<String>,
    ann_reads: usize,
    memory: QueryMemoryReservation,
}

fn load_bounded_vector_candidates(
    cassie: &Cassie,
    spec: &HybridTopKSpec,
    limit: usize,
    controls: &QueryExecutionControls,
    diagnostics: &mut HybridSelectionDiagnostics,
) -> Result<Option<ControlledVectorCandidateIds>, QueryError> {
    match controlled_vector_candidate_ids(cassie, spec, limit, controls) {
        Ok(Some(candidates)) => Ok(Some(candidates)),
        Ok(None) => {
            diagnostics.select_fallback("missing-ann-state", Some("vector-artifact"));
            Ok(None)
        }
        Err(error) if is_query_control_error(&error) => Err(QueryError::from(error)),
        Err(error) => {
            diagnostics.generation_rejections = usize::from(
                error.to_string().contains("generation") || error.to_string().contains("stale"),
            );
            diagnostics.select_fallback("invalid-vector-artifact", Some("vector-artifact"));
            Ok(None)
        }
    }
}

fn controlled_vector_candidate_ids(
    cassie: &Cassie,
    spec: &HybridTopKSpec,
    limit: usize,
    controls: &QueryExecutionControls,
) -> Result<Option<ControlledVectorCandidateIds>, crate::app::CassieError> {
    let Some(index) = cassie
        .midge
        .get_vector_index_definition(&spec.collection, &spec.vector_field)?
    else {
        return Ok(None);
    };
    match index.metadata.index_type {
        crate::embeddings::VectorIndexType::Hnsw => {
            let Some(options) = index.metadata.hnsw.as_ref() else {
                return Err(crate::app::CassieError::Execution(
                    "hnsw fallback:missing-options".to_string(),
                ));
            };
            let Some(batch) = cassie.midge.search_hnsw_graph_point_read_controlled(
                &spec.collection,
                &spec.vector_field,
                &spec.vector_query,
                options,
                limit,
                controls,
            )?
            else {
                return Ok(None);
            };
            let (generation, candidates, _, ann_reads, candidate_memory) = batch.into_parts();
            if generation != cassie.midge.collection_generation(&spec.collection)? {
                return Err(crate::app::CassieError::Execution(
                    "hnsw fallback:concurrent-source-change".to_string(),
                ));
            }
            let (ids, memory) = collect_candidate_ids_controlled(
                candidates.into_iter().map(|candidate| candidate.id),
                controls,
            )?;
            drop(candidate_memory);
            Ok(Some(ControlledVectorCandidateIds {
                ids,
                ann_reads,
                memory,
            }))
        }
        crate::embeddings::VectorIndexType::IvfFlat => {
            controlled_ivfflat_candidate_ids(cassie, spec, limit, controls)
        }
        crate::embeddings::VectorIndexType::BruteForce => Ok(None),
    }
}

fn controlled_ivfflat_candidate_ids(
    cassie: &Cassie,
    spec: &HybridTopKSpec,
    limit: usize,
    controls: &QueryExecutionControls,
) -> Result<Option<ControlledVectorCandidateIds>, crate::app::CassieError> {
    let Some(training) = cassie.midge.get_ivfflat_training_manifest_controlled(
        &spec.collection,
        &spec.vector_field,
        controls,
    )?
    else {
        return Ok(None);
    };
    let (generation, training, membership_count, manifest_reads, manifest_memory) =
        training.into_parts();
    if crate::vector::ivfflat::compact_manifest_fallback_reason(
        &training,
        spec.vector_query.len(),
        membership_count,
    )
    .is_some()
    {
        return Ok(None);
    }
    let normalized = crate::vector::normalize(&spec.vector_query)
        .map_or_else(|| spec.vector_query.clone(), |value| value.values);
    let lists = crate::vector::ivfflat::probe_lists(&normalized, &training);
    let _list_memory =
        controls.reserve_query_memory(lists.len().saturating_mul(std::mem::size_of::<usize>()))?;
    let batch = cassie.midge.ivfflat_candidate_vectors_controlled(
        &spec.collection,
        &spec.vector_field,
        &training,
        &lists,
        controls,
    )?;
    let (batch_generation, records, membership_reads, vector_reads, candidate_memory) =
        batch.into_parts();
    if generation != batch_generation
        || generation != cassie.midge.collection_generation(&spec.collection)?
    {
        return Err(crate::app::CassieError::Execution(
            "ivfflat fallback:concurrent-source-change".to_string(),
        ));
    }
    let (ids, memory) = collect_candidate_ids_controlled(
        records.into_iter().take(limit).map(|record| record.id),
        controls,
    )?;
    drop(candidate_memory);
    drop(manifest_memory);
    Ok(Some(ControlledVectorCandidateIds {
        ids,
        ann_reads: manifest_reads
            .saturating_add(1)
            .saturating_add(membership_reads)
            .saturating_add(vector_reads),
        memory,
    }))
}

fn collect_candidate_ids_controlled(
    ids: impl Iterator<Item = String>,
    controls: &QueryExecutionControls,
) -> Result<(BTreeSet<String>, QueryMemoryReservation), crate::app::CassieError> {
    let mut candidates = BTreeSet::new();
    let mut memory = controls.reserve_query_memory(0)?;
    for id in ids {
        if controls.is_cancelled() {
            return Err(crate::app::CassieError::QueryCancelled);
        }
        if controls.is_timed_out() {
            return Err(crate::app::CassieError::DeadlineExceeded);
        }
        if candidates.contains(&id) {
            continue;
        }
        memory.try_grow(
            id.len()
                .saturating_add(std::mem::size_of::<String>())
                .saturating_add(3usize.saturating_mul(std::mem::size_of::<usize>())),
        )?;
        candidates.insert(id);
    }
    Ok((candidates, memory))
}

fn is_query_control_error(error: &crate::app::CassieError) -> bool {
    matches!(
        error,
        crate::app::CassieError::QueryCancelled
            | crate::app::CassieError::DeadlineExceeded
            | crate::app::CassieError::ResourceLimit(_)
    )
}

struct HybridCandidateFetch<'a> {
    cassie: &'a Cassie,
    session: Option<&'a CassieSession>,
    spec: &'a HybridTopKSpec,
    context: &'a BoundedHybridContext<'a>,
    vector_ids: &'a BTreeSet<String>,
    posting_reads: usize,
    ann_reads: usize,
    ann_candidates: usize,
    controls: &'a QueryExecutionControls,
}

fn fetch_hybrid_candidate_rows<'a>(
    request: &HybridCandidateFetch<'_>,
    text_ids: impl Iterator<Item = &'a String>,
    mut retrieval_memory: Vec<QueryMemoryReservation>,
    diagnostics: &mut HybridSelectionDiagnostics,
) -> Result<Option<BoundedHybridRows>, QueryError> {
    let fields = request
        .context
        .schema
        .fields
        .iter()
        .map(|field| field.name.clone())
        .collect::<Vec<_>>();
    let mut rows = Vec::new();
    let mut candidate_row_fetches = 0usize;
    for id in text_ids.filter(|id| request.vector_ids.contains(*id)) {
        candidate_row_fetches += 1;
        let Some(document) = request.cassie.midge.get_retrieval_document_controlled(
            &request.spec.collection,
            id,
            request.controls,
        )?
        else {
            diagnostics.select_fallback("missing-candidate-row", None);
            return Ok(None);
        };
        let (document, mut document_memory) = document.into_parts();
        let row_bytes = projected_hybrid_row_bytes(&document, &fields);
        document_memory.try_grow(row_bytes)?;
        let row = scan::projected_document_to_row(document, &fields, Some(request.context.schema));
        retrieval_memory.push(document_memory);
        rows.push(row);
    }
    if let Some(filter_expr) = &request.spec.filter {
        if !vector_prefilter_supported(filter_expr, request.context.schema) {
            diagnostics.select_fallback("unsupported-filter", None);
            return Ok(None);
        }
        let before = rows.len();
        retrieval_memory.push(
            request
                .controls
                .reserve_query_memory(before.saturating_mul(std::mem::size_of::<BatchRow>()))?,
        );
        rows = filter::filter_rows(
            rows,
            filter_expr,
            request.context.params,
            None,
            request.context.user_functions,
            request.session,
        )?;
        diagnostics.select_prefilter(before, rows.len());
    }
    Ok(Some(BoundedHybridRows {
        rows,
        posting_reads: request.posting_reads,
        ann_reads: request.ann_reads,
        ann_candidates: request.ann_candidates,
        candidate_row_fetches,
        retrieval_memory,
        diagnostics: HybridSelectionDiagnostics::default(),
    }))
}

struct AccountedHybridRows {
    rows: Vec<BatchRow>,
    memory: Vec<QueryMemoryReservation>,
}

fn prefilter_hybrid_rows(
    cassie: &Cassie,
    session: Option<&CassieSession>,
    spec: &HybridTopKSpec,
    context: &BoundedHybridContext<'_>,
    controls: &QueryExecutionControls,
    diagnostics: &mut HybridSelectionDiagnostics,
) -> Result<Option<AccountedHybridRows>, QueryError> {
    let fields = context
        .schema
        .fields
        .iter()
        .map(|field| field.name.clone())
        .collect::<Vec<_>>();
    let Some(mut cursor) = cassie
        .open_session_row_cursor(
            session,
            &spec.collection,
            RowDecode::ProjectedHistorical(fields.clone()),
            controls,
        )
        .map_err(QueryError::from)?
    else {
        return Ok(None);
    };
    let mut rows = Vec::new();
    let mut memory = Vec::new();
    loop {
        let accounted = cursor
            .next_accounted_documents(&cassie.midge, batch::DEFAULT_BATCH_SIZE, controls)
            .map_err(QueryError::from)?;
        if accounted.is_empty() {
            break;
        }
        for document in accounted {
            let (document, mut reservation) = document.into_parts();
            reservation.try_grow(projected_hybrid_row_bytes(&document, &fields))?;
            let row = scan::projected_document_to_row(document, &fields, Some(context.schema));
            memory.push(reservation);
            rows.push(row);
        }
    }
    if let Some(filter_expr) = &spec.filter {
        if !vector_prefilter_supported(filter_expr, context.schema) {
            return Ok(None);
        }
        let before = rows.len();
        memory.push(
            controls
                .reserve_query_memory(before.saturating_mul(std::mem::size_of::<BatchRow>()))?,
        );
        rows = filter::filter_rows(
            rows,
            filter_expr,
            context.params,
            None,
            context.user_functions,
            session,
        )?;
        diagnostics.select_prefilter(before, rows.len());
    }
    Ok(Some(AccountedHybridRows { rows, memory }))
}

fn projected_hybrid_row_bytes(
    document: &crate::midge::adapter::DocumentRef,
    fields: &[String],
) -> usize {
    let entries = fields.len().saturating_add(1);
    std::mem::size_of::<BatchRow>()
        .saturating_add(std::mem::size_of::<QueryMemoryReservation>())
        .saturating_add(entries.saturating_mul(std::mem::size_of::<(String, Value)>()))
        .saturating_add(document.id.len())
        .saturating_add(fields.iter().map(String::len).sum::<usize>())
        .saturating_add(json_retained_bytes(&document.payload))
}

fn json_retained_bytes(value: &serde_json::Value) -> usize {
    let inline = std::mem::size_of::<serde_json::Value>();
    match value {
        serde_json::Value::Null | serde_json::Value::Bool(_) | serde_json::Value::Number(_) => {
            inline
        }
        serde_json::Value::String(value) => inline.saturating_add(value.len()),
        serde_json::Value::Array(values) => values.iter().fold(inline, |bytes, value| {
            bytes.saturating_add(json_retained_bytes(value))
        }),
        serde_json::Value::Object(values) => values.iter().fold(inline, |bytes, (key, value)| {
            bytes
                .saturating_add(key.len())
                .saturating_add(json_retained_bytes(value))
        }),
    }
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
