use std::cmp::Ordering as CmpOrdering;
use std::collections::{BTreeMap, BinaryHeap, HashMap, HashSet};
use std::sync::Arc;
use std::time::Instant;

use crate::app::{Cassie, CassieSession};
use crate::catalog;
use crate::catalog::virtual_views;
use crate::catalog::{CollectionSchema, FieldMeta, FunctionMeta, ProcedureMeta, Volatility};
use crate::embeddings::{
    DistanceMetric, HnswIndexOptions, VectorIndexMetadata, VectorIndexRecord, VectorIndexType,
};
use crate::executor::batch::{self, Batch, BatchRow};
use crate::executor::{aggregate, filter, projection, scan, sort};
use crate::midge::adapter::RowDecode;
use crate::planner::logical::{LogicalCommand, LogicalPlan};
use crate::planner::physical::PhysicalPlan;
use crate::runtime::query_cache;
use crate::runtime::{FulltextIndexOptions, FulltextIndexOptionsCacheKey, QueryExecutionControls};
use crate::search::analyzer::AnalyzerConfig;
use crate::sql::ast::{
    BinaryOp, CommonTableExpression, CteQuery, Expr, FunctionCall, InsertSource, JoinKind,
    QuerySource, QueryStatement, SelectItem, SelectSet, SetOperator, SortDirection,
};
use crate::types::{DataType, FieldSchema, Schema, Value};

fn primary_key_indexes(
    table: &str,
    constraints: &[catalog::FieldConstraint],
) -> Vec<catalog::IndexMeta> {
    constraints
        .iter()
        .filter(|constraint| constraint.primary_key)
        .map(|constraint| catalog::IndexMeta {
            collection: table.to_string(),
            name: format!("{table}_pkey"),
            field: constraint.field.clone(),
            fields: vec![constraint.field.clone()],
            expressions: Vec::new(),
            include_fields: Vec::new(),
            predicate: None,
            kind: catalog::IndexKind::Scalar,
            unique: true,
            options: BTreeMap::new(),
        })
        .collect()
}

mod types;
use types::ExecutionBreakdownDurations;
pub use types::{
    ColumnMeta, ExecutionBreakdownMicros, ExecutionBreakdownOutput, QueryError, QueryResult,
};

mod cte;
mod dispatch;
mod entrypoints;
mod result;

pub(crate) use entrypoints::{
    mark_source_projections_stale_external, refresh_rollups_for_source_external,
    rollup_rewrite_name_for_plan, run_with_session_controls,
};
pub use entrypoints::{run, run_with_controls, run_with_execution_breakdown};

use cte::{execute_cte, CteContext};
use dispatch::{
    build_logical_plan, check_timeout, ensure_temp_budget, ensure_temp_budget_for_rows,
    execute_physical_plan, execute_plan, execute_plan_with_execution_breakdown,
    execute_plan_with_outer_row, resolve_exists_expr,
};
#[cfg(test)]
use dispatch::{preferred_access_path_route, AccessPathRoute};
use result::{build_select_result, compare_query_values, deduce_text_fields, row_signature};

mod dml;
mod dml_command;
mod dml_referential_actions;
mod graph_command;
mod materialized_projection;
mod projection_diff;
mod projection_repair;
mod retention;
mod rollups;
mod schema_command;
mod sequence_command;
mod session_command;
mod vector_index_command;

mod source;
pub(crate) use source::{aggregate_signature, expr_key, group_expr_name, value_sort_key};
mod scored;
pub(crate) use scored::{vector_prefilter_fallback_reason, vector_prefilter_supported};

mod analytical_projection;
mod index_read;
mod ordered_read;
mod projected_read;
mod time_series_read;

mod fulltext_options;
pub(crate) use fulltext_options::load_fulltext_index_options;
mod graph;
mod plan_inspection;
pub(crate) use plan_inspection::plan_needs_user_functions;
mod aggregate_accel;
mod aggregate_exec;
mod window_exec;

#[cfg(test)]
mod tests;
