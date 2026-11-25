//! Membership and keys for persisted collection cardinality statistics.

use cassie_core::catalog::CollectionCardinalityStats;
use cassie_sql::catalog_declarations::{IndexKind, IndexMeta};
use cassie_vector::types::VectorIndexRecord;

#[must_use]
pub fn index_cardinality_key(index: &IndexMeta) -> String {
    CollectionCardinalityStats::index_key(&index.kind, &index.name)
}

#[must_use]
pub fn vector_index_cardinality_key(record: &VectorIndexRecord) -> String {
    CollectionCardinalityStats::vector_index_key(&record.field)
}

#[must_use]
pub fn payload_contains_index_membership(payload: &serde_json::Value, index: &IndexMeta) -> bool {
    let fields = index.normalized_fields();
    if fields.is_empty() {
        return matches!(index.kind, IndexKind::Scalar)
            && !index.normalized_expressions().is_empty();
    }

    match index.kind {
        IndexKind::Scalar | IndexKind::Hybrid | IndexKind::Column | IndexKind::TimeSeries => fields
            .iter()
            .all(|field| payload.get(field).is_some_and(|value| !value.is_null())),
        IndexKind::FullText => fields
            .iter()
            .any(|field| payload.get(field).is_some_and(|value| !value.is_null())),
        IndexKind::Vector => payload
            .get(index.primary_field())
            .is_some_and(serde_json::Value::is_array),
    }
}

#[must_use]
pub fn payload_contains_vector_membership(
    payload: &serde_json::Value,
    record: &VectorIndexRecord,
) -> bool {
    payload
        .get(&record.source_field)
        .is_some_and(|value| value.is_string() || value.is_array() || value.is_object())
}
