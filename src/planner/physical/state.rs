use super::{AdaptivePlanDiagnostics, Operator, OperatorFeedbackPlanDiagnostics};
use crate::planner::logical::LogicalPlan;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ReadAccessPath {
    #[default]
    Unknown,
    CollectionScan,
    PointLookup,
    IndexSeek,
    PrefixScan,
    RangeScan,
    OrderedBoundedScan,
    GraphAdjacency,
    RuntimeJoin,
}

impl ReadAccessPath {
    pub(crate) fn as_str(&self) -> &'static str {
        match self {
            Self::Unknown => "unknown",
            Self::CollectionScan => "collection_scan",
            Self::PointLookup => "point_lookup",
            Self::IndexSeek => "index_seek",
            Self::PrefixScan => "prefix_scan",
            Self::RangeScan => "range_scan",
            Self::OrderedBoundedScan => "ordered_bounded_scan",
            Self::GraphAdjacency => "graph_adjacency",
            Self::RuntimeJoin => "runtime_join",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PaginationStrategy {
    #[default]
    None,
    Limit,
    Offset,
    Keyset,
    DegradedOffset,
}

impl PaginationStrategy {
    pub(crate) fn as_str(&self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Limit => "limit",
            Self::Offset => "offset",
            Self::Keyset => "keyset",
            Self::DegradedOffset => "degraded_offset",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TopKMode {
    #[default]
    None,
    Heap,
    Storage,
}

impl TopKMode {
    pub(crate) fn as_str(&self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Heap => "heap",
            Self::Storage => "storage",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ProjectionShape {
    #[default]
    Unknown,
    RuntimeJoinDegraded,
    Collection,
    MaterializedProjection,
    Other,
}

impl ProjectionShape {
    pub(crate) fn as_str(&self) -> &'static str {
        match self {
            Self::Unknown => "unknown",
            Self::RuntimeJoinDegraded => "runtime_join_degraded",
            Self::Collection => "collection",
            Self::MaterializedProjection => "materialized_projection",
            Self::Other => "other",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum EarlyStopMode {
    #[default]
    None,
    PointLookup,
    ScanLimit,
    Exists,
    StorageTopK,
    Keyset,
}

impl EarlyStopMode {
    pub(crate) fn as_str(&self) -> &'static str {
        match self {
            Self::None => "none",
            Self::PointLookup => "point_lookup",
            Self::ScanLimit => "scan_limit",
            Self::Exists => "exists",
            Self::StorageTopK => "storage_top_k",
            Self::Keyset => "keyset",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PhysicalPlan {
    pub collection: String,
    pub operators: Vec<Operator>,
    pub logical: LogicalPlan,
    #[serde(default)]
    pub collection_schema: Option<crate::catalog::CollectionSchema>,
    pub estimates: PlanEstimates,
    #[serde(default)]
    pub operator_feedback: OperatorFeedbackPlanDiagnostics,
    #[serde(default)]
    pub adaptive_plan: AdaptivePlanDiagnostics,
    pub read: PhysicalReadPlan,
    pub top_k: PhysicalTopKPlan,
    pub join: PhysicalJoinPlan,
    pub aggregate: PhysicalAggregatePlan,
    pub projection: PhysicalProjectionPlan,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct PhysicalReadPlan {
    pub predicate_pushdown: bool,
    pub projected_scan_fields: Vec<String>,
    pub scan_limit: Option<usize>,
    pub selected_index: Option<String>,
    pub covered_index: bool,
    pub column_batch_index: Option<String>,
    #[serde(default)]
    pub access_path: ReadAccessPath,
    #[serde(default)]
    pub access_path_reason: String,
    #[serde(default)]
    pub fallback_reason: Option<String>,
    #[serde(default)]
    pub pagination_strategy: PaginationStrategy,
    #[serde(default)]
    pub early_stop: EarlyStopMode,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct PhysicalTopKPlan {
    pub enabled: bool,
    pub limit: Option<usize>,
    #[serde(default)]
    pub mode: TopKMode,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct PhysicalJoinPlan {
    pub strategy: Option<String>,
    #[serde(default)]
    pub keys: Vec<String>,
    #[serde(default)]
    pub sort_required: bool,
    #[serde(default)]
    pub fallback_reason: Option<String>,
    #[serde(default)]
    pub vectorized: PhysicalVectorizedJoinPlan,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct PhysicalVectorizedJoinPlan {
    #[serde(default)]
    pub candidate: bool,
    #[serde(default)]
    pub fallback_reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct PhysicalAggregatePlan {
    pub parallel_candidate: bool,
    pub acceleration: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct PhysicalProjectionPlan {
    #[serde(default)]
    pub shape: ProjectionShape,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct PlanEstimates {
    pub cost_model_version: u32,
    pub scan_rows: u64,
    pub index_rows: u64,
    pub join_rows: u64,
    pub search_rows: u64,
    pub vector_rows: u64,
    pub aggregate_rows: u64,
    pub scan_cost: u64,
    pub index_cost: u64,
    pub selected_cost: u64,
    pub cost_source: String,
    pub rejected_alternatives: Vec<String>,
}
