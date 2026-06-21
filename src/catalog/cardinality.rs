use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::catalog::{IndexKind, IndexMeta};
use crate::embeddings::VectorIndexRecord;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct IndexCardinalityStats {
    pub cardinality: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct FieldCardinalityStats {
    pub non_null_count: u64,
    pub null_count: u64,
    pub missing_count: u64,
    pub distinct_count: u64,
    pub min_value: Option<String>,
    pub max_value: Option<String>,
    #[serde(default)]
    pub sample_count: u64,
    #[serde(default)]
    pub histogram_buckets: Vec<FieldHistogramBucket>,
    #[serde(default)]
    pub heavy_hitters: Vec<FieldHeavyHitter>,
    #[serde(default)]
    pub confidence: u8,
    #[serde(default)]
    pub stale_reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct FieldHistogramBucket {
    pub lower: String,
    pub upper: String,
    pub count: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct FieldHeavyHitter {
    pub value: String,
    pub count: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct CollectionCardinalityStats {
    pub row_count: u64,
    pub hydrated: bool,
    #[serde(default)]
    pub indexes: BTreeMap<String, IndexCardinalityStats>,
    #[serde(default)]
    pub fields: BTreeMap<String, FieldCardinalityStats>,
}

impl CollectionCardinalityStats {
    pub fn index_key(kind: &IndexKind, name: &str) -> String {
        let kind = match kind {
            IndexKind::Scalar => "scalar",
            IndexKind::FullText => "fulltext",
            IndexKind::Vector => "vector",
            IndexKind::Hybrid => "hybrid",
            IndexKind::Column => "column",
            IndexKind::TimeSeries => "time_series",
        };
        format!("{kind}:{name}")
    }

    pub fn scalar_index_key(name: &str) -> String {
        Self::index_key(&IndexKind::Scalar, name)
    }

    pub fn fulltext_index_key(name: &str) -> String {
        Self::index_key(&IndexKind::FullText, name)
    }

    pub fn vector_index_key(field: &str) -> String {
        Self::index_key(&IndexKind::Vector, field)
    }

    pub fn hybrid_index_key(name: &str) -> String {
        Self::index_key(&IndexKind::Hybrid, name)
    }

    pub fn time_series_index_key(name: &str) -> String {
        Self::index_key(&IndexKind::TimeSeries, name)
    }

    pub fn set_index_cardinality(&mut self, key: String, cardinality: u64) {
        self.indexes
            .insert(key, IndexCardinalityStats { cardinality });
    }

    pub fn index_cardinality(&self, key: &str) -> Option<u64> {
        self.indexes.get(key).map(|stats| stats.cardinality)
    }

    pub fn set_field_stats(&mut self, field: String, stats: FieldCardinalityStats) {
        self.fields.insert(field, stats);
    }

    pub fn field_stats(&self, field: &str) -> Option<&FieldCardinalityStats> {
        self.fields.get(field)
    }
}

pub fn index_cardinality_key(index: &IndexMeta) -> String {
    CollectionCardinalityStats::index_key(&index.kind, &index.name)
}

pub fn vector_index_cardinality_key(record: &VectorIndexRecord) -> String {
    CollectionCardinalityStats::vector_index_key(&record.field)
}

pub fn payload_contains_index_membership(payload: &serde_json::Value, index: &IndexMeta) -> bool {
    let fields = index.normalized_fields();
    if fields.is_empty() {
        return false;
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
            .is_some_and(|value| value.is_array()),
    }
}

pub fn payload_contains_vector_membership(
    payload: &serde_json::Value,
    record: &VectorIndexRecord,
) -> bool {
    payload
        .get(&record.source_field)
        .is_some_and(|value| value.is_string() || value.is_array() || value.is_object())
}
