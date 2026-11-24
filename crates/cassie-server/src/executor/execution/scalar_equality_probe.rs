//! Equality probes over eligible scalar indexes, shared by indexed full-text reads
//! and keyed UPDATE/DELETE row lookups.

use super::{Cassie, CassieSession, Expr, HashSet, QueryError, Value};
use crate::runtime::{QueryExecutionControls, QueryMemoryReservation};

/// Row ids returned by a scalar index probe, with the memory that holds them.
pub(super) struct ControlledScalarCandidates {
    pub(super) ids: HashSet<String>,
    pub(super) memory: Vec<QueryMemoryReservation>,
}

/// Probes the eligible scalar index of `field` for rows whose key equals `value`.
///
/// Returns `None` when no index can answer the comparison exactly.
pub(super) fn probe_scalar_equality(
    cassie: &Cassie,
    session: Option<&CassieSession>,
    collection: &str,
    (field, mut value): (&str, serde_json::Value),
    controls: &QueryExecutionControls,
) -> Result<Option<ControlledScalarCandidates>, QueryError> {
    if !super::index_probe_canonicalization::probe_comparison_is_exact(
        cassie, collection, field, &value,
    ) {
        return Ok(None);
    }
    let Some(index) = cassie
        .catalog
        .list_indexes(collection)
        .into_iter()
        .find(|index| {
            index.kind == crate::catalog::IndexKind::Scalar
                && index.predicate.is_none()
                && index.normalized_fields().first().is_some_and(|indexed| {
                    crate::sql::ColumnIdentifierPath::stored_field_key(indexed)
                        == crate::sql::ColumnIdentifierPath::reference_field_key(field)
                })
                && super::index_read::index_trailing_keys_not_null(cassie, collection, index)
        })
    else {
        return Ok(None);
    };
    if session.is_some_and(|session| !session.transaction_index_visible(&index, &cassie.catalog)) {
        return Ok(None);
    }
    let signed_zero_counterpart =
        super::index_read::signed_zero_probe_counterpart(cassie, collection, field, &value);
    super::index_probe_canonicalization::canonicalize_index_probe_value(
        cassie, collection, field, &mut value,
    );
    let mut probe_values = vec![value];
    if let Some(mut counterpart) = signed_zero_counterpart {
        super::index_probe_canonicalization::canonicalize_index_probe_value(
            cassie,
            collection,
            field,
            &mut counterpart,
        );
        probe_values.push(counterpart);
    }
    let mut ids = HashSet::new();
    let mut memory = Vec::with_capacity(probe_values.len() * 2);
    for value in probe_values {
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
        ids.extend(hits.into_iter().map(|hit| hit.id));
        memory.push(hit_memory);
        memory.push(id_memory);
    }
    Ok(Some(ControlledScalarCandidates { ids, memory }))
}

/// The JSON form of a literal or bound parameter, when it has one.
pub(super) fn expression_literal(expr: &Expr, params: &[Value]) -> Option<serde_json::Value> {
    match expr {
        Expr::StringLiteral(value) => Some(serde_json::Value::String(value.clone())),
        Expr::NumberLiteral(value) => serde_json::Number::from_f64(*value).map(Into::into),
        Expr::IntegerLiteral(value) => Some(serde_json::Value::Number((*value).into())),
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
        Value::Typed(value) => Some(value.to_json()),
        Value::Vector(_) | Value::Array(_) => None,
    }
}
