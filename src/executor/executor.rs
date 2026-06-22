use std::cmp::Ordering as CmpOrdering;
use std::collections::{BTreeMap, BinaryHeap, HashMap, HashSet};
use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::app::{Cassie, CassieSession};
use crate::catalog;
use crate::catalog::virtual_views;
use crate::catalog::{CollectionSchema, FieldMeta, FunctionMeta, ProcedureMeta, Volatility};
use crate::embeddings::{
    DistanceMetric, HnswIndexOptions, VectorIndexMetadata, VectorIndexRecord, VectorIndexType,
};
use crate::executor::batch::{self, Batch, BatchRow, RowAccess};
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

#[derive(Debug, Clone, serde::Serialize, PartialEq, Eq)]
pub struct ColumnMeta {
    pub name: String,
    pub data_type: String,
    pub type_oid: i64,
    pub typlen: i16,
    pub atttypmod: i32,
    pub format_code: i16,
    pub nullable: bool,
}

impl ColumnMeta {
    pub fn text(name: impl Into<String>) -> Self {
        Self::from_data_type(name, DataType::Text)
    }

    pub fn from_data_type(name: impl Into<String>, data_type: DataType) -> Self {
        let data_type_name = data_type.type_name();
        Self {
            name: name.into(),
            data_type: data_type_name,
            type_oid: data_type.type_oid(),
            typlen: data_type.typlen(),
            atttypmod: data_type.atttypmod(),
            format_code: 0,
            nullable: true,
        }
    }
}

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

#[derive(Debug, Clone, serde::Serialize)]
pub struct QueryResult {
    pub columns: Vec<ColumnMeta>,
    pub rows: Vec<Vec<Value>>,
    pub command: String,
}

#[derive(Debug, Clone, Copy, Default, serde::Serialize)]
pub struct ExecutionBreakdownMicros {
    pub scan_us: u64,
    pub row_decode_us: u64,
    pub filter_us: u64,
    pub projection_us: u64,
    pub sort_us: u64,
    pub result_build_us: u64,
    pub stats_us: u64,
}

#[derive(Debug, Clone)]
pub struct ExecutionBreakdownOutput {
    pub result: QueryResult,
    pub breakdown: ExecutionBreakdownMicros,
}

#[derive(Debug, Clone, Copy, Default)]
struct ExecutionBreakdownDurations {
    scan: Duration,
    row_decode: Duration,
    filter: Duration,
    projection: Duration,
    sort: Duration,
    result_build: Duration,
    stats: Duration,
}

impl ExecutionBreakdownDurations {
    fn into_micros(self) -> ExecutionBreakdownMicros {
        ExecutionBreakdownMicros {
            scan_us: duration_micros(self.scan),
            row_decode_us: duration_micros(self.row_decode),
            filter_us: duration_micros(self.filter),
            projection_us: duration_micros(self.projection),
            sort_us: duration_micros(self.sort),
            result_build_us: duration_micros(self.result_build),
            stats_us: duration_micros(self.stats),
        }
    }
}

fn duration_micros(duration: Duration) -> u64 {
    duration.as_micros().try_into().unwrap_or(u64::MAX)
}

#[derive(Debug, thiserror::Error)]
pub enum QueryError {
    #[error("{0}")]
    General(String),

    #[error(transparent)]
    Cassie(#[from] crate::app::CassieError),
}

type CteRows = Vec<Vec<(String, Value)>>;
type CteContext = HashMap<String, CteRows>;
type CteExecution<'a> = Result<CteRows, QueryError>;
type ExprResolution<'a> = Result<Expr, QueryError>;
pub fn run(
    cassie: &Cassie,
    plan: PhysicalPlan,
    params: Vec<Value>,
) -> Result<QueryResult, QueryError> {
    let controls = cassie.runtime.query_controls(std::time::Instant::now());
    run_with_controls(cassie, Arc::new(plan), params, &controls)
}

pub fn run_with_controls(
    cassie: &Cassie,
    plan: Arc<PhysicalPlan>,
    params: Vec<Value>,
    controls: &QueryExecutionControls,
) -> Result<QueryResult, QueryError> {
    run_with_session_controls(cassie, None, plan, params, controls)
}

#[doc(hidden)]
pub fn run_with_execution_breakdown(
    cassie: &Cassie,
    plan: PhysicalPlan,
    params: Vec<Value>,
) -> Result<ExecutionBreakdownOutput, QueryError> {
    let controls = cassie.runtime.query_controls(std::time::Instant::now());
    run_with_execution_breakdown_controls(cassie, Arc::new(plan), params, &controls)
}

fn run_with_execution_breakdown_controls(
    cassie: &Cassie,
    plan: Arc<PhysicalPlan>,
    params: Vec<Value>,
    controls: &QueryExecutionControls,
) -> Result<ExecutionBreakdownOutput, QueryError> {
    let user_functions =
        if plan.logical.command.is_some() || plan_needs_user_functions(&plan.logical) {
            cassie
                .catalog
                .list_functions()
                .into_iter()
                .map(|metadata| (metadata.name.to_ascii_lowercase(), metadata))
                .collect::<HashMap<String, FunctionMeta>>()
        } else {
            HashMap::new()
        };

    if let Some(command) = plan.logical.command.as_ref() {
        let started = Instant::now();
        let result = dml_command::execute_command(
            cassie,
            None,
            command,
            &params,
            &user_functions,
            controls,
        )?;
        let breakdown = ExecutionBreakdownDurations {
            result_build: started.elapsed(),
            ..Default::default()
        };
        return Ok(ExecutionBreakdownOutput {
            result,
            breakdown: breakdown.into_micros(),
        });
    }

    let mut cte_context: CteContext = HashMap::new();
    let (rows, mut breakdown) = execute_plan_with_execution_breakdown(
        cassie,
        None,
        &plan.logical,
        &mut cte_context,
        &user_functions,
        &params,
        controls,
    )?;

    let result_started = Instant::now();
    let collection_schema = cassie.catalog.get_schema(&plan.logical.collection);
    let columns = aggregate::columns_from_projection(
        &plan.logical.projection,
        collection_schema.as_ref(),
        &user_functions,
    );
    let rows: Vec<Vec<Value>> = rows.into_iter().map(BatchRow::into_values).collect();

    if rows.len() > controls.max_result_rows {
        return Err(QueryError::General(format!(
            "query result row limit exceeded: {} > {}",
            rows.len(),
            controls.max_result_rows
        )));
    }

    breakdown.result_build += result_started.elapsed();
    Ok(ExecutionBreakdownOutput {
        result: QueryResult {
            columns,
            rows,
            command: "SELECT".to_string(),
        },
        breakdown: breakdown.into_micros(),
    })
}

pub(crate) fn run_with_session_controls(
    cassie: &Cassie,
    session: Option<&CassieSession>,
    plan: Arc<PhysicalPlan>,
    params: Vec<Value>,
    controls: &QueryExecutionControls,
) -> Result<QueryResult, QueryError> {
    let user_functions =
        if plan.logical.command.is_some() || plan_needs_user_functions(&plan.logical) {
            cassie
                .catalog
                .list_functions()
                .into_iter()
                .map(|metadata| (metadata.name.to_ascii_lowercase(), metadata))
                .collect::<HashMap<String, FunctionMeta>>()
        } else {
            HashMap::new()
        };

    if let Some(command) = plan.logical.command.as_ref() {
        return dml_command::execute_command(
            cassie,
            session,
            command,
            &params,
            &user_functions,
            controls,
        );
    }

    let mut cte_context: CteContext = HashMap::new();
    let rows = execute_plan(
        cassie,
        session,
        &plan.logical,
        &mut cte_context,
        &user_functions,
        &params,
        controls,
    )?;

    let collection_schema = cassie.catalog.get_schema(&plan.logical.collection);
    let columns = aggregate::columns_from_projection(
        &plan.logical.projection,
        collection_schema.as_ref(),
        &user_functions,
    );
    let rows: Vec<Vec<Value>> = rows.into_iter().map(BatchRow::into_values).collect();

    if rows.len() > controls.max_result_rows {
        return Err(QueryError::General(format!(
            "query result row limit exceeded: {} > {}",
            rows.len(),
            controls.max_result_rows
        )));
    }

    Ok(QueryResult {
        columns,
        rows,
        command: "SELECT".to_string(),
    })
}

pub(crate) fn refresh_rollups_for_source_external(
    cassie: &Cassie,
    source: &str,
    controls: &QueryExecutionControls,
) -> Result<(), QueryError> {
    let user_functions = HashMap::new();
    rollups::refresh_rollups_for_source(cassie, source, &user_functions, controls)
}

pub(crate) fn rollup_rewrite_name_for_plan(cassie: &Cassie, plan: &LogicalPlan) -> Option<String> {
    rollups::rewrite_name_for_plan(cassie, plan)
}

#[path = "execution/dml.rs"]
mod dml;
#[path = "execution/dml_command.rs"]
mod dml_command;
#[path = "execution/materialized_projection.rs"]
mod materialized_projection;
#[path = "execution/projection_diff.rs"]
mod projection_diff;
#[path = "execution/retention.rs"]
mod retention;
#[path = "execution/rollups.rs"]
mod rollups;
#[path = "execution/vector_index_command.rs"]
mod vector_index_command;

fn execute_plan(
    cassie: &Cassie,
    session: Option<&CassieSession>,
    plan: &LogicalPlan,
    cte_context: &mut CteContext,
    user_functions: &HashMap<String, FunctionMeta>,
    params: &[Value],
    controls: &QueryExecutionControls,
) -> Result<Vec<BatchRow>, QueryError> {
    execute_plan_with_outer_row(
        cassie,
        session,
        plan,
        cte_context,
        user_functions,
        params,
        controls,
        None,
    )
}

#[allow(clippy::too_many_arguments)]
fn execute_plan_with_outer_row(
    cassie: &Cassie,
    session: Option<&CassieSession>,
    plan: &LogicalPlan,
    cte_context: &mut CteContext,
    user_functions: &HashMap<String, FunctionMeta>,
    params: &[Value],
    controls: &QueryExecutionControls,
    outer_row: Option<&BatchRow>,
) -> Result<Vec<BatchRow>, QueryError> {
    check_timeout(controls)?;
    if plan.command.is_some() {
        return Err(QueryError::General(
            "cannot execute command plans in CTE context".into(),
        ));
    }

    for cte in &plan.ctes {
        let rows = execute_cte(
            cassie,
            session,
            cte,
            cte_context,
            user_functions,
            params,
            controls,
        )?;
        cte_context.insert(cte.name.to_ascii_lowercase(), rows);
    }

    let mixed_execution = mixed_execution_summary(plan);
    if outer_row.is_none() && !source_reads_materialized_projection(cassie, &plan.source) {
        if let Some(rows) =
            scored::execute_vector_distance_top_k(cassie, session, user_functions, params, plan)?
        {
            if mixed_execution.is_some() {
                cassie
                    .runtime
                    .record_mixed_execution_optimized(plan.collection.clone());
            }
            return Ok(rows);
        }

        if let Some(rows) =
            scored::execute_scored_search_top_k(cassie, session, user_functions, params, plan)?
        {
            if mixed_execution.is_some() {
                cassie
                    .runtime
                    .record_mixed_execution_optimized(plan.collection.clone());
            }
            return Ok(rows);
        }

        if let Some(rows) = analytical_projection::try_execute_analytical_projection(
            cassie,
            session,
            plan,
            cte_context,
            user_functions,
            params,
            controls,
        )? {
            return Ok(rows);
        }

        if let Some(rows) = projected_read::execute_ordered_column_top_k(cassie, plan)? {
            if mixed_execution.is_some() {
                cassie
                    .runtime
                    .record_mixed_execution_optimized(plan.collection.clone());
            }
            return Ok(rows);
        }

        if let Some(rows) = projected_read::execute_projected_filtered_read(
            cassie,
            session,
            plan,
            user_functions,
            params,
            controls,
        )? {
            if mixed_execution.is_some() {
                cassie
                    .runtime
                    .record_mixed_execution_optimized(plan.collection.clone());
            }
            return Ok(rows);
        }

        if let Some(rows) =
            rollups::try_execute_rollup_query(cassie, plan, params, user_functions, controls)?
        {
            if mixed_execution.is_some() {
                cassie
                    .runtime
                    .record_mixed_execution_optimized(plan.collection.clone());
            }
            return Ok(rows);
        }
    }

    if let Some(reason) = mixed_execution {
        cassie
            .runtime
            .record_mixed_execution_fallback(plan.collection.clone(), reason);
    }
    source::execute_source_query_with_outer_row(
        cassie,
        session,
        plan,
        cte_context,
        user_functions,
        params,
        controls,
        outer_row,
    )
}

fn mixed_execution_summary(plan: &LogicalPlan) -> Option<String> {
    let uses_fulltext = !plan_inspection::fulltext_query_fields(plan).is_empty();
    let uses_vector = plan_inspection::plan_uses_vector_operator(plan)
        || plan_inspection::plan_uses_function(plan, "vector_score")
        || plan_inspection::plan_uses_function(plan, "vector_distance");
    let uses_hybrid = plan_inspection::plan_uses_function(plan, "hybrid_score");
    let uses_aggregate = !plan.group_by.is_empty()
        || plan.having.is_some()
        || plan.projection.iter().any(select_item_is_aggregate);
    let mixed = (uses_fulltext && uses_vector)
        || (uses_hybrid && uses_aggregate)
        || ((uses_fulltext || uses_vector || uses_hybrid) && uses_aggregate);
    mixed.then(|| {
        let mut stages = Vec::new();
        if uses_fulltext || uses_vector || uses_hybrid {
            stages.push("candidate_generation");
        }
        if plan.filter.is_some() {
            stages.push("metadata_prefilter");
        }
        if uses_fulltext || uses_vector || uses_hybrid {
            stages.push("exact_scoring");
        }
        if uses_aggregate {
            stages.push("analytical_grouping");
        }
        if !plan.order.is_empty() {
            stages.push("ordering");
        }
        if plan.offset.is_some() {
            stages.push("offset");
        }
        if plan.limit.is_some() {
            stages.push("limit");
        }
        format!("source_row_exact_baseline;stages={}", stages.join(">"))
    })
}

fn select_item_is_aggregate(item: &SelectItem) -> bool {
    matches!(
        item,
        SelectItem::Function { function, .. }
            if matches!(
                function.name.to_ascii_lowercase().as_str(),
                "count" | "sum" | "avg" | "min" | "max"
            )
    )
}

fn source_reads_materialized_projection(cassie: &Cassie, source: &QuerySource) -> bool {
    match source {
        QuerySource::Collection(name) => cassie.catalog.is_materialized_projection(name),
        QuerySource::Subquery { select, .. } => {
            source_reads_materialized_projection(cassie, &select.source)
        }
        QuerySource::Join { left, right, .. } => {
            source_reads_materialized_projection(cassie, left)
                || source_reads_materialized_projection(cassie, right)
        }
        QuerySource::Cte(_) | QuerySource::SingleRow => false,
    }
}

fn execute_plan_with_execution_breakdown(
    cassie: &Cassie,
    session: Option<&CassieSession>,
    plan: &LogicalPlan,
    cte_context: &mut CteContext,
    user_functions: &HashMap<String, FunctionMeta>,
    params: &[Value],
    controls: &QueryExecutionControls,
) -> Result<(Vec<BatchRow>, ExecutionBreakdownDurations), QueryError> {
    if let Some(output) = projected_read::execute_projected_filtered_read_with_breakdown(
        cassie,
        session,
        plan,
        user_functions,
        params,
        controls,
    )? {
        return Ok(output);
    }

    let started = Instant::now();
    let rows = execute_plan(
        cassie,
        session,
        plan,
        cte_context,
        user_functions,
        params,
        controls,
    )?;
    let breakdown = ExecutionBreakdownDurations {
        scan: started.elapsed(),
        ..ExecutionBreakdownDurations::default()
    };
    Ok((rows, breakdown))
}

fn execute_cte<'a>(
    cassie: &'a Cassie,
    session: Option<&'a CassieSession>,
    cte: &'a CommonTableExpression,
    cte_context: &'a mut CteContext,
    user_functions: &'a HashMap<String, FunctionMeta>,
    params: &'a [Value],
    controls: &'a QueryExecutionControls,
) -> CteExecution<'a> {
    check_timeout(controls)?;
    let cte_name = cte.name.to_ascii_lowercase();
    let previous = cte_context.remove(&cte_name);

    let output = match &cte.query {
        CteQuery::Simple(statement) => {
            let logical = build_logical_plan(statement.as_ref())?;
            execute_plan(
                cassie,
                session,
                &logical,
                cte_context,
                user_functions,
                params,
                controls,
            )?
            .into_iter()
            .map(BatchRow::into_entries)
            .collect()
        }
        CteQuery::Recursive { base, recursive } => {
            let base_plan = build_logical_plan(base.as_ref())?;
            let mut rows = execute_plan(
                cassie,
                session,
                &base_plan,
                cte_context,
                user_functions,
                params,
                controls,
            )?
            .into_iter()
            .map(BatchRow::into_entries)
            .collect::<Vec<_>>();

            cte_context.insert(cte_name.clone(), rows.clone());

            let mut seen: HashSet<String> = rows.iter().map(row_signature).collect();
            let mut stabilized = false;

            for _ in 0..controls.cte_recursion_depth {
                check_timeout(controls)?;
                let recursive_plan = build_logical_plan(recursive.as_ref())?;
                let recursive_rows = execute_plan(
                    cassie,
                    session,
                    &recursive_plan,
                    cte_context,
                    user_functions,
                    params,
                    controls,
                )?
                .into_iter()
                .map(BatchRow::into_entries)
                .collect::<Vec<_>>();

                let mut new_rows = Vec::new();
                for row in recursive_rows {
                    let signature = row_signature(&row);
                    if seen.insert(signature) {
                        rows.push(row.clone());
                        new_rows.push(row);
                    }
                }

                if new_rows.is_empty() {
                    stabilized = true;
                    break;
                }

                ensure_temp_budget_for_rows(controls, &rows)?;
                cte_context.insert(cte_name.clone(), rows.clone());
            }

            if !stabilized {
                return Err(QueryError::General(format!(
                    "recursive CTE '{}' did not stabilize within {} iterations",
                    cte.name, controls.cte_recursion_depth
                )));
            }

            rows
        }
    };

    if let Some(previous_rows) = previous {
        cte_context.insert(cte_name, previous_rows);
    } else {
        cte_context.remove(&cte_name);
    }

    Ok(output)
}

#[path = "execution/source.rs"]
mod source;
pub(crate) use source::{aggregate_signature, expr_key, group_expr_name, value_sort_key};
#[path = "execution/scored.rs"]
mod scored;
pub(crate) use scored::{vector_prefilter_fallback_reason, vector_prefilter_supported};

#[path = "execution/analytical_projection.rs"]
mod analytical_projection;
#[path = "execution/projected_read.rs"]
mod projected_read;
#[path = "execution/time_series_read.rs"]
mod time_series_read;

fn compare_query_values(left: &Value, right: &Value) -> CmpOrdering {
    if let (Some(left), Some(right)) = (left.as_f64(), right.as_f64()) {
        return left.partial_cmp(&right).unwrap_or(CmpOrdering::Equal);
    }
    if let (Some(left), Some(right)) = (left.as_str(), right.as_str()) {
        return left.cmp(right);
    }
    CmpOrdering::Equal
}

fn resolve_exists_expr<'a>(
    cassie: &'a Cassie,
    session: Option<&'a CassieSession>,
    expr: &'a Expr,
    cte_context: &'a CteContext,
    user_functions: &'a HashMap<String, FunctionMeta>,
    params: &'a [Value],
    controls: &'a QueryExecutionControls,
) -> ExprResolution<'a> {
    match expr {
        Expr::Binary { left, op, right } => Ok(Expr::Binary {
            left: Box::new(resolve_exists_expr(
                cassie,
                session,
                left,
                cte_context,
                user_functions,
                params,
                controls,
            )?),
            op: op.clone(),
            right: Box::new(resolve_exists_expr(
                cassie,
                session,
                right,
                cte_context,
                user_functions,
                params,
                controls,
            )?),
        }),
        Expr::IsNull { expr, negated } => Ok(Expr::IsNull {
            expr: Box::new(resolve_exists_expr(
                cassie,
                session,
                expr,
                cte_context,
                user_functions,
                params,
                controls,
            )?),
            negated: *negated,
        }),
        Expr::InList {
            expr,
            values,
            negated,
        } => {
            let expr = resolve_exists_expr(
                cassie,
                session,
                expr,
                cte_context,
                user_functions,
                params,
                controls,
            )?;
            let mut resolved_values = Vec::with_capacity(values.len());
            for value in values {
                resolved_values.push(resolve_exists_expr(
                    cassie,
                    session,
                    value,
                    cte_context,
                    user_functions,
                    params,
                    controls,
                )?);
            }
            Ok(Expr::InList {
                expr: Box::new(expr),
                values: resolved_values,
                negated: *negated,
            })
        }
        Expr::Between {
            expr,
            low,
            high,
            negated,
        } => Ok(Expr::Between {
            expr: Box::new(resolve_exists_expr(
                cassie,
                session,
                expr,
                cte_context,
                user_functions,
                params,
                controls,
            )?),
            low: Box::new(resolve_exists_expr(
                cassie,
                session,
                low,
                cte_context,
                user_functions,
                params,
                controls,
            )?),
            high: Box::new(resolve_exists_expr(
                cassie,
                session,
                high,
                cte_context,
                user_functions,
                params,
                controls,
            )?),
            negated: *negated,
        }),
        Expr::Not { expr } => Ok(Expr::Not {
            expr: Box::new(resolve_exists_expr(
                cassie,
                session,
                expr,
                cte_context,
                user_functions,
                params,
                controls,
            )?),
        }),
        Expr::Cast { expr, data_type } => Ok(Expr::Cast {
            expr: Box::new(resolve_exists_expr(
                cassie,
                session,
                expr,
                cte_context,
                user_functions,
                params,
                controls,
            )?),
            data_type: data_type.clone(),
        }),
        Expr::Exists(statement) => {
            let logical = build_logical_plan(statement.as_ref())?;
            let logical = bounded_exists_logical(logical);
            let mut subquery_context = cte_context.clone();
            let rows = execute_plan(
                cassie,
                session,
                &logical,
                &mut subquery_context,
                user_functions,
                params,
                controls,
            )?;
            Ok(Expr::BoolLiteral(!rows.is_empty()))
        }
        Expr::Column(_)
        | Expr::Param(_)
        | Expr::StringLiteral(_)
        | Expr::NumberLiteral(_)
        | Expr::BoolLiteral(_)
        | Expr::Null
        | Expr::Function(_) => Ok(expr.clone()),
    }
}

fn build_logical_plan(
    statement: &crate::sql::ast::ParsedStatement,
) -> Result<LogicalPlan, QueryError> {
    let plan = crate::planner::logical::plan(&crate::sql::binder::BoundStatement {
        statement: statement.clone(),
        indexes: Vec::new(),
    })
    .map_err(|error| QueryError::General(error.to_string()))?;

    if plan.command.is_some() {
        return Err(QueryError::General(
            "CTE statements cannot include command statements".into(),
        ));
    }

    Ok(plan)
}

fn bounded_exists_logical(mut logical: LogicalPlan) -> LogicalPlan {
    logical.limit = Some(
        logical
            .limit
            .map(|limit| if limit <= 0 { 0 } else { 1 })
            .unwrap_or(1),
    );
    logical
}

#[path = "execution/fulltext_options.rs"]
mod fulltext_options;
pub(crate) use fulltext_options::load_fulltext_index_options;
#[path = "execution/plan_inspection.rs"]
mod plan_inspection;
pub(crate) use plan_inspection::plan_needs_user_functions;
#[path = "execution/aggregate_accel.rs"]
mod aggregate_accel;
#[path = "execution/aggregate_exec.rs"]
mod aggregate_exec;
#[path = "execution/window_exec.rs"]
mod window_exec;

fn check_timeout(controls: &QueryExecutionControls) -> Result<(), QueryError> {
    if controls.is_timed_out() {
        return Err(QueryError::General("query timeout exceeded".to_string()));
    }

    Ok(())
}

fn ensure_temp_budget(
    controls: &QueryExecutionControls,
    batches: &[batch::Batch],
) -> Result<(), QueryError> {
    let bytes = estimate_batch_bytes(batches);
    if bytes > controls.temp_spill_budget_bytes {
        return Err(QueryError::General(format!(
            "temporary storage budget exceeded: {bytes} > {}",
            controls.temp_spill_budget_bytes
        )));
    }

    Ok(())
}

fn ensure_temp_budget_for_rows(
    controls: &QueryExecutionControls,
    rows: &[Vec<(String, Value)>],
) -> Result<(), QueryError> {
    let bytes = rows
        .iter()
        .map(|row| {
            serde_json::to_vec(row)
                .map(|bytes| bytes.len())
                .unwrap_or_default()
        })
        .sum::<usize>();

    if bytes > controls.temp_spill_budget_bytes {
        return Err(QueryError::General(format!(
            "temporary storage budget exceeded: {bytes} > {}",
            controls.temp_spill_budget_bytes
        )));
    }

    Ok(())
}

fn estimate_batch_bytes(batches: &[batch::Batch]) -> usize {
    batches
        .iter()
        .flat_map(|batch| batch.iter())
        .map(|row| {
            serde_json::to_vec(row.entries())
                .map(|bytes| bytes.len())
                .unwrap_or_default()
        })
        .sum()
}

fn row_signature(row: &impl RowAccess) -> String {
    serde_json::to_string(row.entries()).unwrap_or_else(|_| String::new())
}

fn deduce_text_fields<R: RowAccess>(rows: &[R]) -> Vec<String> {
    let mut fields = HashSet::<String>::new();
    let mut ordered = Vec::new();

    for row in rows {
        for (name, value) in row.entries() {
            if !matches!(value, Value::String(_) | Value::Json(_)) {
                continue;
            }

            let name = name.to_ascii_lowercase();
            if fields.insert(name.clone()) {
                ordered.push(name);
            }
        }
    }

    ordered
}

#[cfg(test)]
#[path = "execution/tests.rs"]
mod tests;
