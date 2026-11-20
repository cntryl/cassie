//! Source rows for `ORDER BY <vector distance> LIMIT n` over a plain collection.
//!
//! A query that orders a table by its distance to a vector and keeps a few rows only needs those
//! rows. Streaming the whole table into converted rows charges every row's decoded vector, which
//! grows with the table. This stage ranks the table with a bounded exact heap, fetches the
//! selected rows by key, and returns them as the source; the unchanged generic phases then apply
//! the filter, the ordering, the offset, the limit and the projection to those rows.

use std::sync::Arc;

use super::super::scored;
use super::source_rows::plan_uses_aggregate;
use super::{
    check_timeout, ensure_query_memory_budget, load_source_batches, scan, Batch, BatchRow,
    CteContext, LogicalPlan, QueryError, QuerySource, SourceExecutionEnv, Value,
};
use crate::sql::ast::SelectItem;

/// A plan's source rows, with the row count to report as candidates when the exact vector
/// selection (rather than a full scan) produced them.
pub(super) struct LoadedSource {
    pub(super) batches: Vec<Batch>,
    pub(super) text_fields: Vec<String>,
    pub(super) scanned_rows: Option<usize>,
}

/// Loads the plan's source rows: the exact vector candidates when the plan orders a plain table
/// by vector distance, otherwise the ordinary source scan.
pub(super) fn load(
    env: &SourceExecutionEnv<'_>,
    plan: &LogicalPlan,
    cte_context: &mut CteContext,
    outer_row: Option<&BatchRow>,
) -> Result<LoadedSource, QueryError> {
    if outer_row.is_none() {
        if let Some(loaded) = try_candidate_batches(env, plan, cte_context)? {
            return Ok(loaded);
        }
    }
    let (batches, text_fields) = load_source_batches(env, plan, cte_context, outer_row)?;
    Ok(LoadedSource {
        batches,
        text_fields,
        scanned_rows: None,
    })
}

/// The exact candidate rows of a vector-distance ordering and the rows scanned to find them, or
/// `None` to run the generic scan.
fn try_candidate_batches(
    env: &SourceExecutionEnv<'_>,
    plan: &LogicalPlan,
    cte_context: &CteContext,
) -> Result<Option<LoadedSource>, QueryError> {
    let Some(bounds) = candidate_bounds(env, plan) else {
        return Ok(None);
    };
    if !scored::has_vector_distance_ordering(plan) {
        return Ok(None);
    }
    let QuerySource::Collection(collection) = &plan.source else {
        return Ok(None);
    };
    let Some(schema) = env.cassie.catalog.get_schema(collection) else {
        return Ok(None);
    };
    if env.cassie.catalog.get_view(collection).is_some()
        || env
            .cassie
            .catalog
            .get_materialized_projection(collection)
            .is_some()
    {
        return Ok(None);
    }
    let Some(candidates) = scored::select_exact_vector_candidates(
        env.cassie,
        env.session,
        env.user_functions,
        env.params,
        plan,
        (&bounds, env.controls),
    )?
    else {
        return Ok(None);
    };
    let scanned_rows = candidates.scanned_rows();
    let (ids, _candidate_memory) = candidates.into_sorted_ids();
    let mut rows = Vec::with_capacity(ids.len());
    for id in ids {
        check_timeout(env.controls)?;
        let document = env
            .cassie
            .get_document_for_session(env.session, collection, &id)
            .map_err(QueryError::from)?;
        let Some(document) = document else {
            // The row vanished between ranking and fetching; the scan decides the answer.
            return Ok(None);
        };
        let row = scan::document_to_row(&document, Some(&schema));
        rows.push(scan::hydrate_character_arrays(row, env.controls)?);
    }
    let mut batches = vec![rows];
    let memory = Arc::new(ensure_query_memory_budget(env.controls, &batches)?);
    for row in batches.iter_mut().flatten() {
        let owned = std::mem::replace(
            row,
            crate::executor::batch::BatchRow::from_projected_values(Vec::new()),
        );
        *row = owned.with_query_memory(Some(Arc::clone(&memory)));
    }
    super::source_shape::attach_types(env, &plan.source, cte_context, &mut batches)?;
    Ok(Some(LoadedSource {
        batches,
        text_fields: env.cassie.catalog.text_fields(collection),
        scanned_rows: Some(scanned_rows),
    }))
}

fn candidate_bounds(
    env: &SourceExecutionEnv<'_>,
    plan: &LogicalPlan,
) -> Option<scored::ExactCandidateBounds> {
    if plan.command.is_some()
        || !plan.ctes.is_empty()
        || plan.distinct
        || !plan.distinct_on.is_empty()
        || !plan.group_by.is_empty()
        || plan.grouping_subtotal
        || plan.having.is_some()
        || plan.set.is_some()
        || plan.order.len() != 1
        || plan.order[0].nulls.is_some()
        || plan_uses_aggregate(plan)
        || plan
            .projection
            .iter()
            .any(|item| matches!(item, SelectItem::WindowFunction { .. }))
    {
        return None;
    }
    let limit = bound_value(plan.limit.as_ref()?, env.params)?;
    let offset = plan
        .offset
        .as_ref()
        .map_or(Some(0), |offset| bound_value(offset, env.params))?;
    (limit > 0).then_some(scored::ExactCandidateBounds { limit, offset })
}

/// A non-negative integer literal or bound integer parameter.
fn bound_value(expr: &crate::sql::ast::Expr, params: &[Value]) -> Option<usize> {
    match expr {
        crate::sql::ast::Expr::IntegerLiteral(value) => usize::try_from(*value).ok(),
        crate::sql::ast::Expr::Param(index) => match params.get(*index)? {
            Value::Int64(value) => usize::try_from(*value).ok(),
            _ => None,
        },
        _ => None,
    }
}
