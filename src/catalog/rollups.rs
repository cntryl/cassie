use serde::{Deserialize, Serialize};

use crate::catalog::derive_scoped_name;
use crate::types::DataType;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RollupMeta {
    pub name: String,
    pub source_collection: String,
    pub output_collection: String,
    pub timestamp_field: String,
    pub bucket_width: String,
    pub origin: Option<String>,
    pub bucket_expr: String,
    pub group_keys: Vec<String>,
    pub aggregates: Vec<RollupAggregateMeta>,
    pub filter_expr: Option<String>,
    pub version: u64,
    pub state: RollupState,
    pub refresh_cursor: RollupRefreshCursor,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RollupAggregateMeta {
    pub alias: String,
    pub function: String,
    pub expression: String,
    pub data_type: DataType,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum RollupState {
    Building,
    Ready,
    Stale,
}

impl RollupState {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Building => "building",
            Self::Ready => "ready",
            Self::Stale => "stale",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct RollupRefreshCursor {
    #[serde(default)]
    pub source_generation: u64,
    pub source_epoch: u64,
    pub last_refresh_ms: Option<u64>,
    pub source_row_count: u64,
    pub lag_rows: u64,
}

impl RollupMeta {
    #[must_use]
    pub fn new(definition: RollupDefinition) -> Self {
        Self {
            output_collection: output_collection_name(&definition.name),
            name: definition.name,
            source_collection: definition.source_collection,
            timestamp_field: definition.timestamp_field,
            bucket_width: definition.bucket_width,
            origin: definition.origin,
            bucket_expr: definition.bucket_expr,
            group_keys: definition.group_keys,
            aggregates: definition.aggregates,
            filter_expr: definition.filter_expr,
            version: 1,
            state: RollupState::Building,
            refresh_cursor: RollupRefreshCursor::default(),
        }
    }

    #[must_use]
    pub fn is_fresh(&self, source_generation: u64) -> bool {
        self.state == RollupState::Ready
            && self.refresh_cursor.lag_rows == 0
            && self.refresh_cursor.source_generation == source_generation
    }
}

#[derive(Debug, Clone)]
pub struct RollupDefinition {
    pub name: String,
    pub source_collection: String,
    pub timestamp_field: String,
    pub bucket_width: String,
    pub origin: Option<String>,
    pub bucket_expr: String,
    pub group_keys: Vec<String>,
    pub aggregates: Vec<RollupAggregateMeta>,
    pub filter_expr: Option<String>,
}

#[must_use]
pub fn output_collection_name(name: &str) -> String {
    derive_scoped_name(name, |local| {
        format!("__cassie_rollup_{}", local.trim().to_ascii_lowercase())
    })
}
