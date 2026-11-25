//! Standalone vector top-k restricted by an equality filter (#1363).
//!
//! The filter's `column = literal` conjuncts over eligible scalar indexes yield an exact id
//! subset. A small subset is reranked exactly; a large one is searched with adaptive ANN windows
//! that grow until enough eligible candidates are found. A window that cannot be filled never
//! returns an underfilled result: it degrades to the exact subset, or to the full exact scan with
//! a labelled reason. Selection rules live in `docs/vector-filtered-search-contract.md`.

use crate::midge::fulltext::FulltextRequestReads;

use std::collections::BinaryHeap;
use std::time::Instant;

use super::super::fulltext_read::{document_filter_row, document_filter_row_bytes};
use super::super::vector_filtered::ann::{
    eligible_ann_candidates, AnnEligible, AnnOutcome, AnnRequest,
};
use super::super::vector_filtered::{
    eligible_subset, select_strategy, usable_ann_index, AnnQueryShape, EligibleSubset,
    FilteredSelection,
};
use super::candidate::{
    candidate_sort_value, vector_rows_from_top, AccountedVectorTopK, SqlVectorCandidate,
};
use super::diagnostics::source_generation_matches;
use super::exact::validate_exact_vector;
use super::{
    adaptive_candidate_decision, filter, record_adaptive_candidate_decision, vector_from_json,
    vector_prefilter_supported, BatchRow, Cassie, CassieSession, Expr, FunctionMeta, HashMap,
    LogicalPlan, QueryError, SortDirection, Value, VectorDistanceTopKSpec, VectorTopKShape,
};
use crate::embeddings::VectorIndexRecord;
use crate::runtime::{
    FilteredVectorWork, QueryExecutionControls, QueryMemoryReservation, QueryScoringTestBoundary,
};

pub(super) struct FilteredVectorRequest<'a> {
    pub(super) session: Option<&'a CassieSession>,
    pub(super) stored_vector_field: &'a str,
    pub(super) user_functions: &'a HashMap<String, FunctionMeta>,
    pub(super) params: &'a [Value],
    pub(super) filter_expr: &'a Expr,
    pub(super) controls: &'a QueryExecutionControls,
}

struct Context<'a> {
    cassie: &'a Cassie,
    spec: &'a VectorDistanceTopKSpec,
    request: &'a FilteredVectorRequest<'a>,
}

struct Reranked {
    top: BinaryHeap<SqlVectorCandidate>,
    memory: QueryMemoryReservation,
    candidates: usize,
}

fn decline(cassie: &Cassie, reason: &str) -> Option<Vec<BatchRow>> {
    cassie.runtime.record_vector_filtered_fallback(reason);
    None
}

/// Serves a filtered vector top-k from the scalar-index subset. `None` means the caller runs the
/// authoritative exact scan; the reason was recorded in the filtered-vector metrics.
pub(super) fn execute_filtered_vector_top_k(
    cassie: &Cassie,
    spec: &VectorDistanceTopKSpec,
    request: &FilteredVectorRequest<'_>,
    started_at: Instant,
) -> Result<Option<Vec<BatchRow>>, QueryError> {
    let schema = cassie.catalog.get_schema(&spec.collection).ok_or_else(|| {
        QueryError::General(format!("collection '{}' not found", spec.collection))
    })?;
    if !vector_prefilter_supported(request.filter_expr, &schema) {
        return Ok(decline(cassie, "unsupported-filter"));
    }
    let generation = cassie.midge.collection_generation(&spec.collection)?;
    let Some(subset) = eligible_subset(
        cassie,
        request.session,
        &spec.collection,
        request.filter_expr,
        request.params,
        request.controls,
    )?
    else {
        return Ok(decline(cassie, "no-eligible-scalar-index"));
    };
    let top_needed = spec.top_needed();
    let adaptive = adaptive_candidate_decision(cassie, &spec.collection, top_needed)?;
    let adaptive_max = cassie.runtime.limits().adaptive_candidate_max.max(1);
    let ann = usable_ann_index(
        cassie,
        &spec.collection,
        request.stored_vector_field,
        AnnQueryShape {
            metric: spec.metric,
            ascending: matches!(spec.direction, SortDirection::Asc),
            degenerate: spec.is_degenerate_ann_query(),
        },
    )?;
    let selection = select_strategy(subset.ids.len(), top_needed, adaptive_max, ann.is_some());
    let context = Context {
        cassie,
        spec,
        request,
    };
    let mut work = FilteredVectorWork {
        eligible_subset: subset.ids.len(),
        ..FilteredVectorWork::default()
    };
    let (path, reranked) = match (selection, ann) {
        (FilteredSelection::FullFallback(reason), _) => return Ok(decline(cassie, reason)),
        (FilteredSelection::FilteredAnn, Some(index)) => {
            match filtered_ann(&context, &index, &subset, generation, &mut work)? {
                Some(reranked) => ("standalone.filtered-ann", reranked),
                None if subset.ids.len() <= adaptive_max => {
                    let Some(reranked) = bounded_exact(&context, &subset, generation, &mut work)?
                    else {
                        return Ok(decline(cassie, "concurrent-source-change"));
                    };
                    ("standalone.bounded-exact", reranked)
                }
                None => return Ok(decline(cassie, "subset-over-candidate-cap")),
            }
        }
        _ => {
            let Some(reranked) = bounded_exact(&context, &subset, generation, &mut work)? else {
                return Ok(decline(cassie, "concurrent-source-change"));
            };
            ("standalone.bounded-exact", reranked)
        }
    };
    if !source_generation_matches(cassie, spec, generation)? {
        return Ok(decline(cassie, "concurrent-source-change"));
    }
    let Reranked {
        top,
        memory,
        candidates,
    } = reranked;
    let rows = vector_rows_from_top(top, spec);
    drop(memory);
    cassie
        .runtime
        .record_vector_execution(started_at.elapsed(), candidates, rows.len());
    record_adaptive_candidate_decision(cassie, &adaptive, candidates, rows.len());
    cassie.runtime.record_vector_filtered_path(path, &work);
    if path == "standalone.bounded-exact" {
        cassie
            .runtime
            .record_vector_prefilter_usage(subset.ids.len(), candidates, None);
    }
    Ok(Some(rows))
}

fn bounded_exact(
    context: &Context<'_>,
    subset: &EligibleSubset,
    generation: u64,
    work: &mut FilteredVectorWork,
) -> Result<Option<Reranked>, QueryError> {
    rerank(context, subset.ids.iter(), generation, work)
}

/// Adaptive ANN windows, then exact rerank of the eligible candidates. `None` when the window or
/// the rerank cannot fill the requested page, so the caller degrades to the exact subset.
fn filtered_ann(
    context: &Context<'_>,
    index: &VectorIndexRecord,
    subset: &EligibleSubset,
    generation: u64,
    work: &mut FilteredVectorWork,
) -> Result<Option<Reranked>, QueryError> {
    let Context {
        cassie,
        spec,
        request,
    } = *context;
    let outcome = eligible_ann_candidates(&AnnRequest {
        cassie,
        collection: &spec.collection,
        stored_field: request.stored_vector_field,
        query: &spec.query,
        metric: spec.metric,
        index,
        subset: &subset.ids,
        top_needed: spec.top_needed(),
        controls: request.controls,
    })?;
    let AnnEligible {
        ids,
        generation: ann_generation,
        windows,
        candidates_read,
        ann_reads,
        lists,
        memory: _ids_memory,
    } = match outcome {
        AnnOutcome::Eligible(eligible) => eligible,
        AnnOutcome::Declined(reason) => {
            cassie
                .runtime
                .record_vector_filtered_fallback(&format!("ann-{reason}"));
            return Ok(None);
        }
    };
    work.ann_windows = windows;
    work.ann_candidates = candidates_read;
    work.eligible_candidates = ids.len();
    if ann_generation != generation {
        cassie
            .runtime
            .record_vector_filtered_fallback("concurrent-source-change");
        return Ok(None);
    }
    let Some(reranked) = rerank(context, ids.iter(), generation, work)? else {
        return Ok(None);
    };
    if reranked.candidates < spec.top_needed().min(subset.ids.len()) {
        cassie
            .runtime
            .record_vector_filtered_fallback("ann-window-underfilled");
        return Ok(None);
    }
    cassie.runtime.record_vector_retrieval_diagnostics(
        ann_reads,
        work.row_fetches,
        work.exact_reranks,
    );
    if lists.0 == 0 {
        cassie.runtime.record_hnsw_execution(work.exact_reranks);
    } else {
        cassie
            .runtime
            .record_ivfflat_execution(lists.0, lists.1, work.exact_reranks);
    }
    Ok(Some(reranked))
}

/// Fetches each id, rechecks the whole filter on the fetched row and keeps the exact top-k.
/// `None` when a row vanished or the collection generation moved.
fn rerank<'a>(
    context: &Context<'_>,
    ids: impl Iterator<Item = &'a String>,
    generation: u64,
    work: &mut FilteredVectorWork,
) -> Result<Option<Reranked>, QueryError> {
    let Context {
        cassie,
        spec,
        request,
    } = *context;
    let controls = request.controls;
    let top_needed = spec.top_needed();
    let mut top = AccountedVectorTopK::try_new(controls)?;
    let mut candidates = 0usize;
    for id in ids {
        super::super::super::check_timeout(controls)?;
        if !source_generation_matches(cassie, spec, generation)? {
            return Ok(None);
        }
        let Some(document) =
            cassie
                .midge
                .get_retrieval_document_controlled(&spec.collection, id, controls)?
        else {
            return Ok(None);
        };
        work.row_fetches = work.row_fetches.saturating_add(1);
        let (document, _document_memory) = document.into_parts();
        let payload = &document.payload;
        let _row_memory = controls.reserve_query_memory(document_filter_row_bytes(id, payload))?;
        if filter::filter_rows(
            vec![document_filter_row(id, payload)],
            request.filter_expr,
            request.params,
            None,
            request.user_functions,
            request.session,
        )?
        .is_empty()
        {
            continue;
        }
        let Some(value) = payload
            .get(request.stored_vector_field)
            .filter(|value| !value.is_null())
        else {
            continue;
        };
        let vector = vector_from_json(value)
            .ok_or_else(|| QueryError::General("exact vector candidate is invalid".to_string()))?;
        validate_exact_vector(spec, &vector)?;
        work.exact_reranks = work.exact_reranks.saturating_add(1);
        candidates = candidates.saturating_add(1);
        let score = spec.exact_distance(&vector);
        top.try_push(
            SqlVectorCandidate {
                sort_value: candidate_sort_value(&spec.direction, score),
                score,
                id: id.clone(),
            },
            top_needed,
        )?;
        controls.scored_candidate(QueryScoringTestBoundary::ExactVector)?;
    }
    super::super::super::check_timeout(controls)?;
    let (top, memory) = top.into_parts();
    Ok(Some(Reranked {
        top,
        memory,
        candidates,
    }))
}

/// How EXPLAIN reports a filtered vector top-k.
pub(crate) struct FilteredExplain {
    /// `filtered-ann`, `bounded-exact-subset` or `structured-filter-exact` (full fallback).
    pub(crate) strategy: &'static str,
    pub(crate) reason: &'static str,
    pub(crate) subset: Option<usize>,
}

/// Resolves the strategy a filtered statement would select, using the same scalar probe and
/// selection rule as execution, without reading any source row.
pub(crate) fn explain_filtered_strategy(
    cassie: &Cassie,
    session: Option<&CassieSession>,
    shape: &VectorTopKShape,
    inputs: (&LogicalPlan, &[Value], &QueryExecutionControls),
    stored_vector_field: &str,
) -> Result<FilteredExplain, QueryError> {
    let (plan, params, controls) = inputs;
    let full = |reason| FilteredExplain {
        strategy: "structured-filter-exact",
        reason,
        subset: None,
    };
    let Some(filter_expr) = plan.filter.as_ref() else {
        return Ok(full("no-filter"));
    };
    if session.is_some_and(|session| session.has_collection_changes(&shape.collection)) {
        return Ok(full("transaction-overlay-exact"));
    }
    let Some(schema) = cassie.catalog.get_schema(&shape.collection) else {
        return Ok(full("no-collection"));
    };
    if !vector_prefilter_supported(filter_expr, &schema) {
        return Ok(full("unsupported-filter"));
    }
    let Some(subset) = eligible_subset(
        cassie,
        session,
        &shape.collection,
        filter_expr,
        params,
        controls,
    )?
    else {
        return Ok(full("no-eligible-scalar-index"));
    };
    let top_needed = plan
        .limit_value()
        .and_then(|limit| usize::try_from(limit).ok())
        .unwrap_or(0)
        .saturating_add(
            plan.offset_value()
                .and_then(|offset| usize::try_from(offset).ok())
                .unwrap_or(0),
        )
        .max(1);
    let ann = usable_ann_index(
        cassie,
        &shape.collection,
        stored_vector_field,
        AnnQueryShape {
            metric: shape.metric,
            ascending: shape.ascending,
            degenerate: false,
        },
    )?;
    let selection = select_strategy(
        subset.ids.len(),
        top_needed,
        cassie.runtime.limits().adaptive_candidate_max.max(1),
        ann.is_some(),
    );
    let subset_len = Some(subset.ids.len());
    Ok(match selection {
        FilteredSelection::BoundedExact => FilteredExplain {
            strategy: "bounded-exact-subset",
            reason: "subset-within-exact-limit",
            subset: subset_len,
        },
        FilteredSelection::FilteredAnn => FilteredExplain {
            strategy: "filtered-ann",
            reason: "adaptive-window",
            subset: subset_len,
        },
        FilteredSelection::FullFallback(reason) => FilteredExplain {
            subset: subset_len,
            ..full(reason)
        },
    })
}
