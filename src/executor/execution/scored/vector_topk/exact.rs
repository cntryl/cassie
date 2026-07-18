use std::collections::HashMap;
use std::time::Instant;

use crate::app::{Cassie, CassieSession};
use crate::catalog::FunctionMeta;
use crate::executor::batch::{self, BatchRow};
use crate::runtime::QueryExecutionControls;
use crate::sql::ast::Expr;
use crate::types::Value;

use super::candidate::{
    candidate_sort_value, vector_rows_from_top, AccountedVectorTopK, SqlVectorCandidate,
};
use super::{
    adaptive_candidate_decision, record_adaptive_candidate_decision, vector_from_json,
    VectorDistanceTopKSpec,
};
use super::{filter, scan, value_to_vector, vector_prefilter_supported, QueryError};

pub(super) struct ExactVectorRequest<'a> {
    pub(super) session: Option<&'a CassieSession>,
    pub(super) user_functions: &'a HashMap<String, FunctionMeta>,
    pub(super) params: &'a [Value],
    pub(super) filter_expr: Option<&'a Expr>,
    pub(super) controls: &'a QueryExecutionControls,
}

pub(super) fn execute_exact_vector_top_k(
    cassie: &Cassie,
    spec: &VectorDistanceTopKSpec,
    request: &ExactVectorRequest<'_>,
    started_at: Instant,
) -> Result<Option<Vec<BatchRow>>, QueryError> {
    let top_needed = spec.top_needed();
    let adaptive = adaptive_candidate_decision(cassie, &spec.collection, top_needed)?;
    if let Some((top, top_memory, final_candidate_count)) =
        stream_exact_vector_candidates(cassie, spec, request)?
    {
        let rows = vector_rows_from_top(top, spec);
        drop(top_memory);
        cassie.runtime.record_vector_execution(
            started_at.elapsed(),
            final_candidate_count,
            rows.len(),
        );
        record_adaptive_candidate_decision(cassie, &adaptive, final_candidate_count, rows.len());
        return Ok(Some(rows));
    }

    let schema = cassie.catalog.get_schema(&spec.collection).ok_or_else(|| {
        QueryError::General(format!("collection '{}' not found", spec.collection))
    })?;
    let mut candidates = batch::flatten_batches(scan::scan(
        cassie,
        request.session,
        &spec.collection,
        request.controls,
    )?);
    if let Some(filter_expr) = request.filter_expr {
        if !vector_prefilter_supported(filter_expr, &schema) {
            return Ok(None);
        }
        let before = candidates.len();
        candidates = filter::filter_rows(
            candidates,
            filter_expr,
            request.params,
            None,
            request.user_functions,
            request.session,
        )?;
        cassie
            .runtime
            .record_vector_prefilter_usage(before, candidates.len(), None);
    }

    let final_candidate_count = candidates.len();
    let mut top = AccountedVectorTopK::try_new(request.controls)?;
    for candidate in candidates {
        super::super::super::check_timeout(request.controls)?;
        let vector = candidate
            .get(&spec.vector_field)
            .and_then(value_to_vector)
            .unwrap_or_default();
        validate_exact_vector(spec, &vector)?;
        let score = crate::vector::l2_distance(&vector, &spec.query);
        top.try_push(
            SqlVectorCandidate {
                sort_value: candidate_sort_value(&spec.direction, score),
                score,
                id: candidate
                    .get("id")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string(),
            },
            top_needed,
        )?;
    }
    let (top, top_memory) = top.into_parts();
    let rows = vector_rows_from_top(top, spec);
    drop(top_memory);
    cassie
        .runtime
        .record_vector_execution(started_at.elapsed(), final_candidate_count, rows.len());
    record_adaptive_candidate_decision(cassie, &adaptive, final_candidate_count, rows.len());
    Ok(Some(rows))
}

fn stream_exact_vector_candidates(
    cassie: &Cassie,
    spec: &VectorDistanceTopKSpec,
    request: &ExactVectorRequest<'_>,
) -> Result<
    Option<(
        std::collections::BinaryHeap<SqlVectorCandidate>,
        crate::runtime::QueryMemoryReservation,
        usize,
    )>,
    QueryError,
> {
    if request
        .session
        .is_some_and(|session| !session.collection_changes(&spec.collection).is_empty())
    {
        return Ok(None);
    }
    let decode = if request.filter_expr.is_some() {
        crate::midge::adapter::RowDecode::Full
    } else {
        crate::midge::adapter::RowDecode::ProjectedHistorical(vec![spec.vector_field.clone()])
    };
    let Some(mut cursor) = cassie.midge.open_row_cursor(&spec.collection, decode)? else {
        return Ok(None);
    };
    let mut top = AccountedVectorTopK::try_new(request.controls)?;
    let mut candidate_count = 0usize;
    let mut scanned_count = 0usize;
    loop {
        let documents =
            cursor.next_documents(&cassie.midge, batch::DEFAULT_BATCH_SIZE, request.controls)?;
        if documents.is_empty() {
            break;
        }
        for document in documents {
            scanned_count = scanned_count.saturating_add(1);
            if !document_matches_filter(
                &document,
                request.filter_expr,
                request.params,
                request.user_functions,
                request.session,
            )? {
                continue;
            }
            let vector =
                vector_from_json(&document.payload[&spec.vector_field]).ok_or_else(|| {
                    QueryError::General("exact vector candidate is invalid".to_string())
                })?;
            validate_exact_vector(spec, &vector)?;
            let score = crate::vector::l2_distance(&vector, &spec.query);
            candidate_count = candidate_count.saturating_add(1);
            top.try_push(
                SqlVectorCandidate {
                    sort_value: candidate_sort_value(&spec.direction, score),
                    score,
                    id: document.id.clone(),
                },
                spec.top_needed(),
            )?;
        }
    }
    if request.filter_expr.is_some() {
        cassie
            .runtime
            .record_vector_prefilter_usage(scanned_count, candidate_count, None);
    }
    let (top, memory) = top.into_parts();
    Ok(Some((top, memory, candidate_count)))
}

fn document_matches_filter(
    document: &crate::midge::adapter::DocumentRef,
    filter_expr: Option<&Expr>,
    params: &[Value],
    user_functions: &HashMap<String, FunctionMeta>,
    session: Option<&CassieSession>,
) -> Result<bool, QueryError> {
    let Some(filter_expr) = filter_expr else {
        return Ok(true);
    };
    let mut entries = vec![("id".to_string(), Value::String(document.id.clone()))];
    if let Some(payload) = document.payload.as_object() {
        entries.extend(payload.iter().map(|(name, value)| {
            (
                name.clone(),
                super::super::super::projected_read::json_to_query_value(value),
            )
        }));
    }
    filter::filter_rows(
        vec![BatchRow::new(entries)],
        filter_expr,
        params,
        None,
        user_functions,
        session,
    )
    .map(|rows| !rows.is_empty())
}

fn validate_exact_vector(spec: &VectorDistanceTopKSpec, vector: &[f32]) -> Result<(), QueryError> {
    if vector.len() != spec.query.len() || vector.is_empty() {
        return Err(QueryError::General(format!(
            "vector field '{}' on collection '{}' does not match query dimensions {}",
            spec.vector_field,
            spec.collection,
            spec.query.len()
        )));
    }
    Ok(())
}
