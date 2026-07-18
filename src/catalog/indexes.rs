use serde::{Deserialize, Serialize};

use crate::types::Value;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum IndexKind {
    Scalar,
    FullText,
    Vector,
    Hybrid,
    Column,
    TimeSeries,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct IndexMeta {
    pub collection: String,
    pub name: String,
    pub field: String,
    #[serde(default)]
    pub fields: Vec<String>,
    #[serde(default)]
    pub expressions: Vec<String>,
    #[serde(default)]
    pub include_fields: Vec<String>,
    #[serde(default)]
    pub predicate: Option<String>,
    pub kind: IndexKind,
    pub unique: bool,
    pub options: std::collections::BTreeMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ColumnBatchMetadata {
    pub metadata_format_version: u32,
    pub summary_format_version: u32,
    pub collection: String,
    pub index_name: String,
    pub schema_version: u32,
    /// Durable collection generation represented by the batch payloads.
    pub built_generation: u64,
    pub source_row_count: usize,
    pub fields: Vec<String>,
    pub segment_size: usize,
    pub segments: Vec<ColumnBatchSegmentMeta>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ColumnBatchSegmentMeta {
    pub segment_id: u64,
    pub row_id_start: Option<String>,
    pub row_id_end: Option<String>,
    pub row_count: usize,
    pub null_bitmap_available: bool,
    pub encoding_version: u32,
    pub codec: ColumnBatchCodecMeta,
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
pub enum ColumnBatchNumericSum {
    Empty,
    FloatEmpty,
    Integer(i64),
    Float(f64),
    IntegerOverflow,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ColumnBatchCodecMeta {
    pub codec_name: String,
    pub codec_version: u32,
    pub uncompressed_len: usize,
    pub compressed_len: usize,
    pub value_count: usize,
    pub null_bitmap_encoding: String,
    pub checksum: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ColumnBatchPayload {
    pub encoding_version: u32,
    pub codec_name: String,
    pub codec_version: u32,
    #[serde(default)]
    pub row_ids: Vec<String>,
    #[serde(default)]
    pub rows: Vec<ColumnBatchRow>,
    #[serde(default)]
    pub columns: Vec<ColumnBatchColumn>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ColumnBatchRow {
    pub row_id: String,
    pub values: std::collections::BTreeMap<String, serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ColumnBatchColumn {
    pub field: String,
    pub runs: Vec<ColumnBatchValueRun>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ColumnBatchValueRun {
    pub value: serde_json::Value,
    pub len: usize,
}

impl IndexMeta {
    const STORAGE_ID_OPTION: &'static str = "__cassie_storage_id";
    const RELATION_ID_OPTION: &'static str = "__cassie_relation_id";

    #[must_use]
    pub(crate) fn storage_id(&self) -> Option<u64> {
        self.options
            .get(Self::STORAGE_ID_OPTION)
            .and_then(|value| value.parse().ok())
    }

    #[must_use]
    pub(crate) fn relation_id(&self) -> Option<u64> {
        self.options
            .get(Self::RELATION_ID_OPTION)
            .and_then(|value| value.parse().ok())
    }

    pub(crate) fn set_storage_ids(&mut self, relation_id: u64, storage_id: u64) {
        self.options.insert(
            Self::RELATION_ID_OPTION.to_string(),
            relation_id.to_string(),
        );
        self.options
            .insert(Self::STORAGE_ID_OPTION.to_string(), storage_id.to_string());
    }

    pub(crate) fn clear_storage_ids(&mut self) {
        self.options.remove(Self::RELATION_ID_OPTION);
        self.options.remove(Self::STORAGE_ID_OPTION);
    }

    #[must_use]
    pub fn normalized_fields(&self) -> Vec<String> {
        if self.fields.is_empty() && !self.field.is_empty() {
            vec![self.field.clone()]
        } else {
            self.fields.clone()
        }
    }

    #[must_use]
    pub fn normalized_include_fields(&self) -> Vec<String> {
        self.include_fields.clone()
    }

    #[must_use]
    pub fn normalized_expressions(&self) -> Vec<String> {
        self.expressions.clone()
    }

    #[must_use]
    pub fn primary_field(&self) -> String {
        self.normalized_fields()
            .into_iter()
            .next()
            .unwrap_or_else(|| self.field.clone())
    }

    pub fn rename_field(&mut self, current: &str, next: &str) -> bool {
        let mut changed = false;
        let mut fields = self.normalized_fields();

        for field in &mut fields {
            if field.eq_ignore_ascii_case(current) {
                *field = next.to_string();
                changed = true;
            }
        }

        if changed {
            self.field = fields.first().cloned().unwrap_or_else(|| next.to_string());
            self.fields = fields;
        }

        changed
    }
}
