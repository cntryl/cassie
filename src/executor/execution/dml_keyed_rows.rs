//! Row selection for single-table UPDATE and DELETE.
//!
//! A `WHERE` clause with a `_id = value` conjunct, or with `column = value`
//! conjuncts over scalar-indexed columns, names a bounded candidate set. Only those
//! rows are fetched and charged to the query memory budget; the caller still applies
//! the full original filter to each of them. Every other shape scans the table.

use super::super::scalar_equality_probe::{
    expression_literal, probe_scalar_equality, ControlledScalarCandidates,
};
use super::{
    batch, check_timeout, ensure_query_memory_budget, scan, BatchRow, Cassie, CassieSession,
    CollectionSchema, Expr, QueryError, QueryExecutionControls, Value,
};
use crate::sql::ast::BinaryOp;
use crate::types::row_identity::is_row_identity_column;
use crate::types::DataType;

/// The statement whose target rows are selected.
pub(super) struct DmlRowRequest<'a> {
    pub(super) cassie: &'a Cassie,
    pub(super) session: Option<&'a CassieSession>,
    pub(super) table: &'a str,
    pub(super) filter: Option<&'a Expr>,
    pub(super) params: &'a [Value],
}

/// Rows that may satisfy the statement filter: fetched by key when the filter
/// provably names a bounded candidate set, otherwise every row of the table.
pub(super) fn candidate_rows(
    request: &DmlRowRequest<'_>,
    controls: &QueryExecutionControls,
) -> Result<Vec<BatchRow>, QueryError> {
    if let Some(rows) = keyed_rows(request, controls)? {
        request.cassie.runtime.record_dml_keyed_row_lookup();
        return Ok(rows);
    }
    request.cassie.runtime.record_dml_full_scan();
    let batches = scan::scan(request.cassie, request.session, request.table, controls)?;
    ensure_query_memory_budget(controls, &batches)?;
    Ok(batch::flatten_batches(batches))
}

fn keyed_rows(
    request: &DmlRowRequest<'_>,
    controls: &QueryExecutionControls,
) -> Result<Option<Vec<BatchRow>>, QueryError> {
    let Some(filter) = request.filter else {
        return Ok(None);
    };
    let cassie = request.cassie;
    let Some(schema) = cassie.catalog.get_schema(request.table) else {
        return Ok(None);
    };
    // A column-store target and an open-transaction overlay change how rows are
    // read; the scan merges both, so they keep the scan.
    if cassie
        .catalog
        .collection_storage_mode(request.table)
        .is_some_and(crate::catalog::collections::CollectionStorageMode::uses_column_store_storage)
        || request
            .session
            .is_some_and(|session| session.has_collection_changes(request.table))
    {
        return Ok(None);
    }
    let mut rewritten = filter.clone();
    crate::planner::logical::rewrite_expr_for_schema(&mut rewritten, schema.declares_id());
    let mut conjuncts = Vec::new();
    collect_conjuncts(&rewritten, &mut conjuncts);
    if let Some(id) = conjuncts
        .iter()
        .find_map(|conjunct| identity_key(conjunct, request.params))
    {
        return fetch_rows(request, &schema, vec![id], controls, true);
    }
    let mut probes = Vec::new();
    for conjunct in &conjuncts {
        let Some((field, value)) = indexable_equality(cassie, request, conjunct) else {
            continue;
        };
        if let Some(candidates) = probe_scalar_equality(
            cassie,
            request.session,
            request.table,
            (&field, value),
            controls,
        )? {
            probes.push(candidates);
        }
    }
    let Some(ids) = intersect_ids(&probes) else {
        return Ok(None);
    };
    let rows = fetch_rows(request, &schema, ids, controls, false);
    drop(probes);
    rows
}

fn intersect_ids(probes: &[ControlledScalarCandidates]) -> Option<Vec<String>> {
    let (first, rest) = probes.split_first()?;
    let mut ids = first
        .ids
        .iter()
        .filter(|id| rest.iter().all(|other| other.ids.contains(*id)))
        .cloned()
        .collect::<Vec<_>>();
    // Scans yield rows in key order; keep that order for RETURNING and triggers.
    ids.sort_unstable();
    Some(ids)
}

/// Fetches `ids` as the same rows the scan produces. A stale index entry whose
/// row is gone makes the result unprovable, so the caller scans instead.
fn fetch_rows(
    request: &DmlRowRequest<'_>,
    schema: &CollectionSchema,
    ids: Vec<String>,
    controls: &QueryExecutionControls,
    missing_is_absent: bool,
) -> Result<Option<Vec<BatchRow>>, QueryError> {
    let mut rows = Vec::with_capacity(ids.len());
    for id in ids {
        check_timeout(controls)?;
        let document = request
            .cassie
            .get_document_for_session(request.session, request.table, &id)
            .map_err(QueryError::from)?;
        let Some(document) = document else {
            if missing_is_absent {
                continue;
            }
            return Ok(None);
        };
        let row = scan::document_to_row(&document, Some(schema));
        rows.push(scan::hydrate_character_arrays(row, controls)?);
    }
    let fetched = [rows];
    ensure_query_memory_budget(controls, &fetched)?;
    let [rows] = fetched;
    Ok(Some(rows))
}

fn collect_conjuncts<'a>(expr: &'a Expr, conjuncts: &mut Vec<&'a Expr>) {
    if let Expr::Binary {
        left,
        op: BinaryOp::And,
        right,
    } = expr
    {
        collect_conjuncts(left, conjuncts);
        collect_conjuncts(right, conjuncts);
    } else {
        conjuncts.push(expr);
    }
}

/// Splits `column = value` (either operand order) into its column and bound value.
fn column_equality(expr: &Expr, params: &[Value]) -> Option<(String, serde_json::Value)> {
    let Expr::Binary {
        left,
        op: BinaryOp::Eq,
        right,
    } = expr
    else {
        return None;
    };
    let ((Expr::Column(field), literal) | (literal, Expr::Column(field))) =
        (left.as_ref(), right.as_ref())
    else {
        return None;
    };
    let value = expression_literal(literal, params)?;
    (!value.is_null()).then(|| (crate::catalog::local_name(field), value))
}

fn identity_key(expr: &Expr, params: &[Value]) -> Option<String> {
    let (field, value) = column_equality(expr, params)?;
    if !is_row_identity_column(&field) {
        return None;
    }
    match value {
        serde_json::Value::String(id) => Some(id),
        _ => None,
    }
}

/// An equality whose value has the exact JSON shape of the column's storage, so a
/// key probe cannot disagree with the SQL comparison the filter applies.
fn indexable_equality(
    cassie: &Cassie,
    request: &DmlRowRequest<'_>,
    expr: &Expr,
) -> Option<(String, serde_json::Value)> {
    let (field, value) = column_equality(expr, request.params)?;
    let shape_matches = match cassie.catalog.field_type(request.table, &field)? {
        DataType::SmallInt | DataType::Int | DataType::BigInt => value.is_i64(),
        DataType::Text | DataType::Varchar { .. } => value.is_string(),
        DataType::Boolean => value.is_boolean(),
        DataType::Float => value.is_number(),
        _ => false,
    };
    shape_matches.then_some((field, value))
}
