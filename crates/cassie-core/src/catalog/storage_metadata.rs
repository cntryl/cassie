//! Persisted catalog values shared with storage adapters.

use serde::{Deserialize, Serialize};

use crate::catalog::declarations::CollectionStorageMode;
use crate::types::Value;
pub use crate::catalog::declarations::IndexKind;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CollectionMeta {
    pub name: String,
    #[serde(default)]
    pub storage_id: u64,
    /// Allocator-backed `pg_type.oid` of the table's row type; zero until created.
    #[serde(default)]
    pub row_type_object_id: u64,
    /// Allocator-backed `pg_type.oid` of the row type's `_name` array type.
    #[serde(default)]
    pub row_array_object_id: u64,
    /// In-memory row schema revision; deliberately excluded from persistence.
    #[serde(skip, default = "initial_row_type_revision")]
    pub row_type_revision: u64,
    /// Sticky PostgreSQL `pg_class.relhasindex` compatibility flag.
    #[serde(default)]
    pub has_ever_had_indexes: bool,
    pub description: Option<String>,
    #[serde(default)]
    pub storage_mode: CollectionStorageMode,
    #[serde(default = "default_collection_storage_version")]
    pub storage_version: u16,
}

/// Per-table storage format revision this binary can read and write.
pub const SUPPORTED_COLLECTION_STORAGE_VERSION: u16 = 1;

impl CollectionMeta {
    #[must_use]
    pub fn has_supported_storage_version(&self) -> bool {
        self.storage_version == SUPPORTED_COLLECTION_STORAGE_VERSION
    }

    pub fn new(name: impl Into<String>, description: Option<String>) -> Self {
        Self::new_with_storage_mode(name, description, CollectionStorageMode::RowStore)
    }

    pub fn new_with_storage_mode(
        name: impl Into<String>,
        description: Option<String>,
        storage_mode: CollectionStorageMode,
    ) -> Self {
        Self {
            name: name.into(),
            storage_id: 0,
            row_type_object_id: 0,
            row_array_object_id: 0,
            row_type_revision: 1,
            has_ever_had_indexes: false,
            description,
            storage_mode,
            storage_version: default_collection_storage_version(),
        }
    }
}

fn initial_row_type_revision() -> u64 { 1 }
fn default_collection_storage_version() -> u16 { SUPPORTED_COLLECTION_STORAGE_VERSION }

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NamespaceMeta {
    pub name: String,
    pub description: Option<String>,
    #[serde(default)]
    pub object_id: u64,
    #[serde(default)]
    pub system_owner: Option<String>,
}

impl NamespaceMeta {
    pub fn new(name: impl Into<String>, description: Option<String>) -> Self {
        Self { name: name.into(), description, object_id: 0, system_owner: None }
    }
}

/// Serializable per-collection cardinality and statistics snapshot.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct CollectionCardinalityStats {
    #[serde(default)]
    pub built_generation: u64,
    pub row_count: u64,
    pub hydrated: bool,
    #[serde(default)]
    pub indexes: std::collections::BTreeMap<String, IndexCardinalityStats>,
    #[serde(default)]
    pub fields: std::collections::BTreeMap<String, FieldCardinalityStats>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct IndexCardinalityStats { pub cardinality: u64 }

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
pub struct FieldHistogramBucket { pub lower: String, pub upper: String, pub count: u64 }

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct FieldHeavyHitter { pub value: String, pub count: u64 }

impl CollectionCardinalityStats {
    #[must_use]
    pub fn index_key(kind: &IndexKind, name: &str) -> String {
        let kind = match kind {
            IndexKind::Scalar => "scalar", IndexKind::FullText => "fulltext",
            IndexKind::Vector => "vector", IndexKind::Hybrid => "hybrid",
            IndexKind::Column => "column", IndexKind::TimeSeries => "time_series",
        };
        format!("{kind}:{name}")
    }
    #[must_use]
    pub fn scalar_index_key(name: &str) -> String { Self::index_key(&IndexKind::Scalar, name) }
    #[must_use]
    pub fn fulltext_index_key(name: &str) -> String { Self::index_key(&IndexKind::FullText, name) }
    #[must_use]
    pub fn vector_index_key(field: &str) -> String { Self::index_key(&IndexKind::Vector, field) }
    #[must_use]
    pub fn hybrid_index_key(name: &str) -> String { Self::index_key(&IndexKind::Hybrid, name) }
    #[must_use]
    pub fn time_series_index_key(name: &str) -> String { Self::index_key(&IndexKind::TimeSeries, name) }
    pub fn set_index_cardinality(&mut self, key: String, cardinality: u64) {
        self.indexes.insert(key, IndexCardinalityStats { cardinality });
    }
    #[must_use]
    pub fn index_cardinality(&self, key: &str) -> Option<u64> {
        self.indexes.get(key).map(|stats| stats.cardinality)
    }
    pub fn set_field_stats(&mut self, field: String, stats: FieldCardinalityStats) {
        self.fields.insert(field, stats);
    }
    #[must_use]
    pub fn field_stats(&self, field: &str) -> Option<&FieldCardinalityStats> { self.fields.get(field) }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ColumnBatchMetadata {
    pub metadata_format_version: u32,
    pub summary_format_version: u32,
    pub manifest_revision: u64,
    pub next_segment_id: u64,
    pub collection: String,
    pub index_name: String,
    pub schema_version: u32,
    pub built_generation: u64,
    pub source_row_count: usize,
    pub fields: Vec<String>,
    pub segment_size: usize,
    pub segments: Vec<ColumnBatchSegmentMeta>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ColumnBatchSegmentMeta {
    pub segment_id: u64,
    pub revision: u64,
    pub row_id_start: Option<String>,
    pub row_id_end: Option<String>,
    pub row_count: usize,
    pub null_bitmap_available: bool,
    pub encoding_version: u32,
    pub row_ids: ColumnBatchChunkMeta,
    pub field_chunks: std::collections::BTreeMap<String, ColumnBatchChunkMeta>,
    pub summary_checksum: String,
    pub summaries: std::collections::BTreeMap<String, ColumnBatchFieldSummary>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ColumnBatchFieldSummary {
    pub non_null_count: usize,
    pub numeric_count: usize,
    pub min: Option<Value>,
    pub max: Option<Value>,
    pub sum: ColumnBatchNumericSum,
    pub integer_total: Option<i128>,
    pub integer_prefix_min: Option<i128>,
    pub integer_prefix_max: Option<i128>,
    pub avg_sum: Option<f64>,
    pub distinct_hint: Option<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub enum ColumnBatchNumericSum { Empty, FloatEmpty, Integer(i64), Float(f64), IntegerOverflow }

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ColumnBatchChunkMeta {
    pub logical_type: String,
    pub codec_id: u8,
    pub codec_name: String,
    pub codec_version: u32,
    pub decoded_len: usize,
    pub encoded_len: usize,
    pub value_count: usize,
    pub null_count: usize,
    pub checksum_sha256: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ColumnBatchRow {
    pub row_id: String,
    pub values: std::collections::BTreeMap<String, serde_json::Value>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum IndexKind { Scalar, FullText, Vector, Hybrid, Column, TimeSeries }

