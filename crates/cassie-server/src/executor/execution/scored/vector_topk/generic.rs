//! Exact candidate selection for general `ORDER BY <vector distance> LIMIT n` queries.
//!
//! The vector top-k fast path only serves a two-column projection. Every other projection over
//! the same ordering used to scan, convert and sort the whole table, charging each row's full
//! decoded size. This module recognizes the ordering alone, streams the table through the exact
//! scorer with a bounded heap, and hands the generic executor the few selected ids; the generic
//! executor still evaluates the projection, the filter and the final ordering on those rows.

use std::collections::HashMap;

use super::exact::{select_exact_candidates, ExactCandidates, ExactVectorRequest};
use super::{
    matching_vector_query, operator_metric_args, vector_distance_args, Cassie, CassieSession, Expr,
    FunctionMeta, LogicalPlan, QueryError, Value, VectorDistanceTopKSpec, VectorOrder,
};
use crate::embeddings::DistanceMetric;
use crate::runtime::QueryExecutionControls;

/// The row count the plan keeps after its offset, as a number of ranked candidates.
pub(crate) struct ExactCandidateBounds {
    pub(crate) limit: usize,
    pub(crate) offset: usize,
}

/// Whether the plan orders by a pgvector distance operator or `vector_distance`, which is cheap
/// to decide and lets other orderings skip candidate selection entirely.
pub(crate) fn has_vector_distance_ordering(plan: &LogicalPlan) -> bool {
    let [order] = plan.order.as_slice() else {
        return false;
    };
    let resolved = crate::executor::sort::alias_expr(&order.expr, &plan.projection);
    ordering_operands(resolved.as_ref().unwrap_or(&order.expr)).is_some()
}

/// Selects the exact top candidates of a vector-distance ordering, or `None` when the generic
/// executor must answer (the ordering is not a vector distance over a fixed-width vector column,
/// the candidate count exceeds the adaptive maximum, or the exact result could differ).
pub(crate) fn select_exact_vector_candidates(
    cassie: &Cassie,
    session: Option<&CassieSession>,
    user_functions: &HashMap<String, FunctionMeta>,
    params: &[Value],
    plan: &LogicalPlan,
    (bounds, controls): (&ExactCandidateBounds, &QueryExecutionControls),
) -> Result<Option<ExactCandidates>, QueryError> {
    let [order] = plan.order.as_slice() else {
        return Ok(None);
    };
    let Some(collection) = crate::sql::physical_collection(&plan.source) else {
        return Ok(None);
    };
    let top_needed = bounds.limit.saturating_add(bounds.offset);
    if top_needed == 0 || top_needed > cassie.runtime.limits().adaptive_candidate_max.max(1) {
        return Ok(None);
    }
    let resolved = crate::executor::sort::alias_expr(&order.expr, &plan.projection);
    let order_expr = resolved.as_ref().unwrap_or(&order.expr);
    let Some((metric, vector_field, operand)) = ordering_operands(order_expr) else {
        return Ok(None);
    };
    let Some(schema) = cassie.catalog.get_schema(collection) else {
        return Ok(None);
    };
    let Some(field) = schema.fields.iter().find(|field| {
        crate::sql::ColumnIdentifierPath::stored_field_key(&field.name)
            == crate::sql::ColumnIdentifierPath::reference_field_key(vector_field)
    }) else {
        return Ok(None);
    };
    let crate::types::DataType::Vector(Some(dimensions)) = &field.data_type else {
        return Ok(None);
    };
    let Some(query) = matching_vector_query(operand, VectorOrder::ScoreColumn, params, controls)?
    else {
        return Ok(None);
    };
    if query.values.len() != *dimensions {
        return Ok(None);
    }
    let spec = VectorDistanceTopKSpec {
        collection: collection.to_string(),
        vector_field: vector_field.to_owned(),
        query: query.values,
        metric,
        id_column: crate::types::row_identity::ROW_IDENTITY_COLUMN.to_string(),
        score_column: "?column?".to_string(),
        direction: order.direction.clone(),
        limit: top_needed,
        offset: 0,
        payload_id_column: None,
        _query_memory: query.memory,
    };
    select_exact_candidates(
        cassie,
        &spec,
        &ExactVectorRequest {
            session,
            stored_vector_field: &field.name,
            user_functions,
            params,
            filter_expr: plan.filter.as_ref(),
            controls,
        },
    )
}

fn ordering_operands(expr: &Expr) -> Option<(DistanceMetric, &str, &Expr)> {
    match expr {
        Expr::Binary { .. } => operator_metric_args(expr),
        Expr::Function(function) if function.name.eq_ignore_ascii_case("vector_distance") => {
            let (field, operand) = vector_distance_args(function)?;
            Some((DistanceMetric::L2, field, operand))
        }
        _ => None,
    }
}
