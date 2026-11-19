//! Order equivalence for scalar reads newly admitted by a partial conjunct or joined LIMIT.
use super::{Cassie, IndexMeta, QueryError, QueryExecutionControls, ScalarIndexScanRequest};
use crate::midge::adapter::ScalarIndexScanHit;
use crate::planner::physical::ScalarIndexPlanShape;
use crate::runtime::accounted::AccountedVec;
use crate::types::DataType;

/// Integer and boolean keys have exact order encodings. Unconstrained timestamp order keys
/// require an additional read proof because BC and expanded years also have legal stored text.
pub(super) fn timestamp_fields(
    cassie: &Cassie,
    index: &IndexMeta,
    shape: &ScalarIndexPlanShape,
) -> Option<Vec<String>> {
    if !index.expressions.is_empty() {
        return None;
    }
    let width = (shape.equality_prefix_len + shape.order_columns_used)
        .max(shape.range_field_index.map_or(0, |n| n + 1));
    let mut timestamps = Vec::new();
    for (position, field) in index
        .normalized_fields()
        .into_iter()
        .take(width)
        .enumerate()
    {
        match cassie.catalog.field_type(
            &index.collection,
            &crate::sql::ColumnIdentifierPath::stored_field_key(&field),
        )? {
            DataType::Int | DataType::BigInt | DataType::SmallInt | DataType::Boolean => {}
            DataType::Timestamp
                if position >= shape.equality_prefix_len
                    && shape.range_field_index != Some(position) =>
            {
                timestamps.push(field)
            }
            _ => return None,
        }
    }
    Some(timestamps)
}

/// Keep the actual hits admitted until the caller consumes them. A sentinel protects the
/// LIMIT proof when the ordered prefix contains more than two result windows.
pub(in crate::executor::execution) fn timestamp_page(
    cassie: &Cassie,
    index: &IndexMeta,
    request: &ScalarIndexScanRequest,
    fields: &[String],
    window: usize,
    controls: &QueryExecutionControls,
) -> Result<Option<AccountedVec<ScalarIndexScanHit>>, QueryError> {
    let maximum = window.saturating_mul(2);
    let request = ScalarIndexScanRequest {
        limit: Some(maximum.saturating_add(1)),
        ..request.clone()
    };
    let hits = cassie
        .midge
        .scan_scalar_index_controlled(index, &request, controls)?;
    let reason = if hits.len() > maximum {
        "prefix_exceeds_window"
    } else if hits.as_slice().iter().all(|hit| {
        fields.iter().all(|field| {
            hit.fields
                .get(field)
                .and_then(serde_json::Value::as_str)
                .is_some_and(|s| {
                    s.len() == 27 && crate::types::temporal::is_canonical_timestamp_text(s)
                })
        })
    }) {
        "canonical_timestamp_prefix"
    } else {
        "timestamp_text_requires_sql_order"
    };
    controls.record_order_proof(hits.len(), reason);
    if reason == "canonical_timestamp_prefix" {
        Ok(Some(hits))
    } else {
        cassie.runtime.record_read_path_ordered_bounded_scan(
            &index.collection,
            hits.len(),
            &index.name,
        );
        Ok(None)
    }
}
