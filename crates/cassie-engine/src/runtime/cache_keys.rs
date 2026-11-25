//! Stable query/cache identity records shared by planning, execution and codecs.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ExecutionMode {
    SimpleQuery,
    DescribeQuery,
    ExtendedQuery,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct PlanCacheKey {
    pub sql_fingerprint: u64,
    pub schema_epoch: u64,
    pub data_epoch: u64,
    pub index_feedback_epoch: u64,
    pub cost_model_version: u32,
    #[serde(default)]
    pub adaptive_config_hash: u64,
    pub parameter_shape: Vec<ParameterShape>,
    pub mode: ExecutionMode,
    pub database: Option<String>,
    #[serde(default)]
    pub search_path: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ParameterShape {
    Null,
    Bool,
    Int64,
    Float64,
    String,
    Vector(usize),
    Array,
    Json,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ExecutionResultCacheKey {
    pub sql_fingerprint: u64,
    pub params_hash: u64,
    pub schema_epoch: u64,
    pub data_epoch: u64,
    pub user: String,
    pub database: Option<String>,
    pub search_path: Vec<String>,
    pub settings_fingerprint: u64,
    pub mode: ExecutionMode,
}
