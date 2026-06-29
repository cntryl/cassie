use serde::{Deserialize, Serialize};

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
    pub collection: String,
    pub index_name: String,
    pub schema_epoch: u64,
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
    #[serde(default)]
    pub summaries: std::collections::BTreeMap<String, ColumnBatchFieldSummary>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ColumnBatchFieldSummary {
    pub non_null_count: usize,
    pub min: Option<serde_json::Value>,
    pub max: Option<serde_json::Value>,
    pub sum: Option<f64>,
    pub all_int: bool,
    pub distinct_hint: Option<usize>,
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
