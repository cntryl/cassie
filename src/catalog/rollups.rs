use serde::{Deserialize, Serialize};

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
    pub source_epoch: u64,
    pub last_refresh_ms: Option<u64>,
    pub source_row_count: u64,
    pub lag_rows: u64,
}

impl RollupMeta {
    pub fn new(
        name: String,
        source_collection: String,
        timestamp_field: String,
        bucket_width: String,
        origin: Option<String>,
        bucket_expr: String,
        group_keys: Vec<String>,
        aggregates: Vec<RollupAggregateMeta>,
        filter_expr: Option<String>,
    ) -> Self {
        Self {
            output_collection: output_collection_name(&name),
            name,
            source_collection,
            timestamp_field,
            bucket_width,
            origin,
            bucket_expr,
            group_keys,
            aggregates,
            filter_expr,
            version: 1,
            state: RollupState::Building,
            refresh_cursor: RollupRefreshCursor::default(),
        }
    }

    pub fn is_fresh(&self) -> bool {
        self.state == RollupState::Ready && self.refresh_cursor.lag_rows == 0
    }
}

pub fn output_collection_name(name: &str) -> String {
    format!("__cassie_rollup_{}", name.trim().to_ascii_lowercase())
}
