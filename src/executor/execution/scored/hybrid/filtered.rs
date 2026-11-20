//! Hybrid retrieval restricted by an equality filter (#1363).
//!
//! `hybrid_score` is defined only for rows with a lexical match, and its vector component is the
//! exact distance read from the fetched row. The exact candidate set for `search(..) AND
//! tenant = $n` is therefore the persisted postings restricted to the scalar-index subset of the
//! equality conjuncts: no vector candidate window can add a row, it can only lose one. This path
//! intersects the postings with that subset, fetches only the surviving rows and lets the shared
//! exact scorer rank them under the corpus-wide BM25 statistics, so the result equals the exact
//! row scan. Anything it cannot serve declines (with a labelled reason in the filtered-vector
//! metrics) to the existing bounded or exact paths.

use std::collections::BTreeSet;

use super::super::fulltext_filtered_topk::{conjuncts, intersect_scalar_conjuncts};
use super::super::{search_predicate_args_with_params, HybridTopKSpec};
use super::{
    fetch_hybrid_candidate_rows, filter, is_query_control_error, persisted_search_context,
    BoundedHybridContext, BoundedHybridRows, Cassie, CassieSession, HybridCandidateFetch,
    HybridSelectionDiagnostics, QueryError, QueryExecutionControls, Value,
};
use crate::runtime::FilteredVectorWork;
use crate::search::query::SearchSyntax;
use crate::sql::ast::{BinaryOp, Expr};

fn decline(cassie: &Cassie, reason: &str) -> Option<BoundedHybridRows> {
    cassie.runtime.record_vector_filtered_fallback(reason);
    None
}

/// Splits `filter` into the `search(<text field>, <query>)` conjunct hybrid scoring already
/// implies and the remaining conjuncts. A search predicate for any other field or query stays in
/// the residual, which the exact-path support check then rejects.
fn residual_conjuncts<'a>(
    filter_expr: &'a Expr,
    spec: &HybridTopKSpec,
    params: &[Value],
) -> Vec<&'a Expr> {
    let mut parts = Vec::new();
    conjuncts(filter_expr, &mut parts);
    let mut matched = false;
    parts.retain(|part| {
        let Expr::Function(function) = part else {
            return true;
        };
        let is_scored_search = !matched
            && search_predicate_args_with_params(function, params).is_some_and(|(field, query)| {
                crate::sql::ColumnIdentifierPath::matches_stored_field(&spec.text_field, &field)
                    && query.syntax == SearchSyntax::Terms
                    && query.text == spec.query
            });
        matched |= is_scored_search;
        !is_scored_search
    });
    parts
}

fn and_all(parts: &[&Expr]) -> Option<Expr> {
    parts
        .iter()
        .rev()
        .map(|part| (*part).clone())
        .reduce(|right, left| Expr::Binary {
            left: Box::new(left),
            op: BinaryOp::And,
            right: Box::new(right),
        })
}

pub(super) fn filtered_hybrid_rows(
    cassie: &Cassie,
    session: Option<&CassieSession>,
    spec: &HybridTopKSpec,
    context: &BoundedHybridContext<'_>,
    controls: &QueryExecutionControls,
    diagnostics: &mut HybridSelectionDiagnostics,
) -> Result<Option<BoundedHybridRows>, QueryError> {
    let Some(filter_expr) = spec.filter.as_ref() else {
        return Ok(None);
    };
    if session.is_some_and(|session| session.has_collection_changes(&spec.collection)) {
        return Ok(decline(cassie, "transaction-overlay"));
    }
    let residual = residual_conjuncts(filter_expr, spec, context.params);
    let Some(residual_filter) = and_all(&residual) else {
        return Ok(None);
    };
    if !super::super::vector_prefilter_supported(&residual_filter, context.schema) {
        return Ok(decline(cassie, "unsupported-filter"));
    }
    let Some(fulltext_index) = cassie
        .catalog
        .list_indexes(&spec.collection)
        .into_iter()
        .find(|index| {
            index.kind == crate::catalog::IndexKind::FullText
                && crate::sql::ColumnIdentifierPath::stored_field_key(&index.field)
                    == crate::sql::ColumnIdentifierPath::reference_field_key(&spec.text_field)
        })
    else {
        return Ok(decline(cassie, "missing-text-index"));
    };
    let generation = cassie.midge.collection_generation(&spec.collection)?;
    let Some(subset) = intersect_scalar_conjuncts(
        cassie,
        session,
        &spec.collection,
        &residual,
        context.params,
        controls,
    )?
    else {
        return Ok(decline(cassie, "no-eligible-scalar-index"));
    };
    let adaptive_max = cassie.runtime.limits().adaptive_candidate_max;
    if subset.ids.len() > adaptive_max {
        return Ok(decline(cassie, "subset-over-candidate-cap"));
    }
    let (subset_ids, subset_memory) = collect_ids(controls, &subset.ids)?;
    let query_terms = filter::prepare_query_terms_with_analyzer(&spec.query, context.analyzer);
    let text_candidates = match cassie.midge.fulltext_candidate_set_for_ids_controlled(
        &spec.collection,
        &fulltext_index.name,
        &query_terms,
        &subset_ids,
        controls,
    ) {
        Ok(candidates) => candidates,
        Err(error) if is_query_control_error(&error) => return Err(QueryError::from(error)),
        Err(_) => return Ok(decline(cassie, "invalid-text-artifact")),
    };
    let (text_candidates, text_memory) = text_candidates.into_parts();
    let stats = &text_candidates.document_stats;
    if stats.len() > adaptive_max {
        return Ok(decline(cassie, "subset-over-candidate-cap"));
    }
    let (search_context, search_context_memory) =
        persisted_search_context(&text_candidates, context, spec, controls)?;
    let mut retrieval_memory = vec![subset_memory, text_memory, search_context_memory];
    retrieval_memory.extend(subset.memory);
    let mut residual_spec = spec.clone();
    residual_spec.filter = Some(residual_filter);
    let rows = fetch_hybrid_candidate_rows(
        &HybridCandidateFetch {
            cassie,
            session,
            spec: &residual_spec,
            context,
            vector_ids: &subset_ids,
            posting_reads: text_candidates.posting_block_reads,
            ann_reads: 0,
            ann_candidates: 0,
            controls,
        },
        stats.keys(),
        search_context,
        retrieval_memory,
        diagnostics,
    )?;
    let Some(rows) = rows else {
        return Ok(decline(cassie, "missing-candidate-row"));
    };
    if cassie.midge.collection_generation(&spec.collection)? != generation {
        return Ok(decline(cassie, "concurrent-source-change"));
    }
    cassie.runtime.record_vector_filtered_path(
        "hybrid.bounded-exact",
        &FilteredVectorWork {
            eligible_subset: subset.ids.len(),
            row_fetches: rows.candidate_row_fetches,
            eligible_candidates: stats.len(),
            ..FilteredVectorWork::default()
        },
    );
    Ok(Some(rows))
}

fn collect_ids(
    controls: &QueryExecutionControls,
    ids: &std::collections::HashSet<String>,
) -> Result<(BTreeSet<String>, crate::runtime::QueryMemoryReservation), QueryError> {
    let mut memory = controls.reserve_query_memory(0)?;
    let mut collected = BTreeSet::new();
    for id in ids {
        if controls.is_canceled() {
            return Err(QueryError::from(crate::app::CassieError::QueryCanceled));
        }
        memory.try_grow(
            id.len()
                .saturating_add(std::mem::size_of::<String>())
                .saturating_add(3usize.saturating_mul(std::mem::size_of::<usize>())),
        )?;
        collected.insert(id.clone());
    }
    Ok((collected, memory))
}
