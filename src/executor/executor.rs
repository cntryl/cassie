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
use crate::query_cache;
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

#[derive(Debug)]
pub enum QueryError {
    General(String),
}

type CteRows = Vec<Vec<(String, Value)>>;
type CteContext = HashMap<String, CteRows>;
type CteExecution<'a> = Result<CteRows, QueryError>;
type ExprResolution<'a> = Result<Expr, QueryError>;
type SourceExecution<'a> = Result<(Vec<Batch>, Vec<String>), QueryError>;

struct SourceExecutionEnv<'a> {
    cassie: &'a Cassie,
    session: Option<&'a CassieSession>,
    user_functions: &'a HashMap<String, FunctionMeta>,
    params: &'a [Value],
    controls: &'a QueryExecutionControls,
}

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
        let result =
            dml::execute_command(cassie, None, command, &params, &user_functions, controls)?;
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
        return dml::execute_command(cassie, session, command, &params, &user_functions, controls);
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

#[path = "execution/dml.rs"]
mod dml;

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

    if outer_row.is_none() {
        if let Some(rows) =
            scored::execute_vector_distance_top_k(cassie, session, user_functions, params, plan)?
        {
            return Ok(rows);
        }

        if let Some(rows) =
            scored::execute_scored_search_top_k(cassie, session, user_functions, params, plan)?
        {
            return Ok(rows);
        }

        if let Some(rows) = projected_read::execute_ordered_column_top_k(cassie, plan)? {
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
            return Ok(rows);
        }
    }

    execute_source_query_with_outer_row(
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

#[path = "execution/scored.rs"]
mod scored;
pub(crate) use scored::{vector_prefilter_fallback_reason, vector_prefilter_supported};

#[path = "execution/projected_read.rs"]
mod projected_read;

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

fn execute_query_source<'a>(
    env: &'a SourceExecutionEnv<'a>,
    source: &'a QuerySource,
    cte_context: &'a mut CteContext,
    qualify: bool,
    outer_row: Option<&'a BatchRow>,
) -> SourceExecution<'a> {
    match source {
        QuerySource::Collection(name) => {
            if let Some(rows) = virtual_views::rows(&env.cassie.catalog, name) {
                let mut batches = materialize_virtual_rows(rows);
                if qualify {
                    batches = qualify_batches(batches, name);
                }
                ensure_temp_budget(env.controls, &batches)?;
                return Ok((batches, Vec::new()));
            }

            if let Some(view) = env.cassie.catalog.get_view(name) {
                let parsed = crate::sql::parser::parse_statement(&view.query)
                    .map_err(|error| QueryError::General(error.0))?;
                let logical = build_logical_plan(&parsed)?;
                let mut view_cte_context = CteContext::new();
                let rows = execute_plan(
                    env.cassie,
                    env.session,
                    &logical,
                    &mut view_cte_context,
                    env.user_functions,
                    env.params,
                    env.controls,
                )?;
                let rows = project_rows_to_schema(rows, &view.schema, name)?;
                let mut batches = batch::chunk_rows(rows, batch::DEFAULT_BATCH_SIZE);
                if qualify {
                    batches = qualify_batches(batches, name);
                }
                ensure_temp_budget(env.controls, &batches)?;
                let text_fields = view
                    .schema
                    .fields
                    .iter()
                    .filter(|field| field.data_type == DataType::Text)
                    .map(|field| field.name.clone())
                    .collect::<Vec<_>>();
                return Ok((batches, text_fields));
            }

            let mut batches = scan::scan(env.cassie, env.session, name)?;
            if qualify {
                batches = qualify_batches(batches, name);
            }
            ensure_temp_budget(env.controls, &batches)?;
            Ok((batches, env.cassie.catalog.text_fields(name)))
        }
        QuerySource::SingleRow => {
            let batches =
                batch::chunk_rows(vec![BatchRow::new(Vec::new())], batch::DEFAULT_BATCH_SIZE);
            ensure_temp_budget(env.controls, &batches)?;
            Ok((batches, Vec::new()))
        }
        QuerySource::Cte(name) => {
            let key = name.to_ascii_lowercase();
            let rows = cte_context
                .get(&key)
                .cloned()
                .ok_or_else(|| QueryError::General(format!("relation '{name}' does not exist")))?;
            let text_fields = deduce_text_fields(&rows);
            let mut batches = batch::chunk_rows(
                rows.into_iter().map(BatchRow::new).collect::<Vec<_>>(),
                batch::DEFAULT_BATCH_SIZE,
            );
            if qualify {
                batches = qualify_batches(batches, name);
            }
            ensure_temp_budget(env.controls, &batches)?;
            Ok((batches, text_fields))
        }
        QuerySource::Subquery {
            alias,
            select,
            lateral,
        } => {
            let logical = LogicalPlan {
                command: None,
                source: select.source.clone(),
                collection: alias.clone(),
                ctes: select.ctes.clone(),
                distinct: select.distinct,
                distinct_on: select.distinct_on.clone(),
                projection: select.projection.clone(),
                filter: select.filter.clone(),
                group_by: select.group_by.clone(),
                having: select.having.clone(),
                order: select.order.clone(),
                limit: select.limit,
                offset: select.offset,
                set: select.set.clone(),
            };
            let mut subquery_context = cte_context.clone();
            let rows = execute_plan_with_outer_row(
                env.cassie,
                env.session,
                &logical,
                &mut subquery_context,
                env.user_functions,
                env.params,
                env.controls,
                if *lateral { outer_row } else { None },
            )?;
            let text_fields = deduce_text_fields(
                &rows
                    .iter()
                    .map(|row| row.entries().to_vec())
                    .collect::<Vec<_>>(),
            );
            let batches =
                qualify_batches(batch::chunk_rows(rows, batch::DEFAULT_BATCH_SIZE), alias);
            ensure_temp_budget(env.controls, &batches)?;
            Ok((batches, text_fields))
        }
        QuerySource::Join {
            left,
            right,
            kind,
            on,
        } => {
            let (left_batches, _left_text) =
                execute_query_source(env, left, cte_context, true, outer_row)?;
            if source_contains_lateral(right) {
                let left_rows = batch::flatten_batches(left_batches);
                let mut joined = Vec::new();

                for left_row in &left_rows {
                    let (right_batches, _right_text) =
                        execute_query_source(env, right, cte_context, true, Some(left_row))?;
                    let right_rows = batch::flatten_batches(right_batches);
                    let right_columns = row_columns(&right_rows);
                    let mut matched = false;
                    for right_row in &right_rows {
                        let combined = combine_rows(left_row, right_row);
                        let passes = matches!(kind, JoinKind::Cross)
                            || filter::eval_scalar(
                                &combined,
                                on,
                                env.params,
                                None,
                                env.user_functions,
                                None,
                                env.session,
                            )?
                            .as_bool();
                        if passes {
                            matched = true;
                            joined.push(combined);
                        }
                    }

                    if !matched && matches!(kind, JoinKind::Left | JoinKind::Full) {
                        joined.push(combine_row_with_nulls(left_row, &right_columns));
                    }
                }

                let text_fields = deduce_text_fields(
                    &joined
                        .iter()
                        .map(|row| row.entries().to_vec())
                        .collect::<Vec<_>>(),
                );
                let batches = batch::chunk_rows(joined, batch::DEFAULT_BATCH_SIZE);
                ensure_temp_budget(env.controls, &batches)?;
                return Ok((batches, text_fields));
            }

            let (right_batches, _right_text) =
                execute_query_source(env, right, cte_context, true, outer_row)?;
            let left_rows = batch::flatten_batches(left_batches);
            let right_rows = batch::flatten_batches(right_batches);
            let left_columns = row_columns(&left_rows);
            let right_columns = row_columns(&right_rows);
            let mut joined = Vec::new();
            let mut right_matched = vec![false; right_rows.len()];

            for left_row in &left_rows {
                let mut matched = false;
                for (right_index, right_row) in right_rows.iter().enumerate() {
                    let combined = combine_rows(left_row, right_row);
                    let passes = matches!(kind, JoinKind::Cross)
                        || filter::eval_scalar(
                            &combined,
                            on,
                            env.params,
                            None,
                            env.user_functions,
                            None,
                            env.session,
                        )?
                        .as_bool();
                    if passes {
                        matched = true;
                        right_matched[right_index] = true;
                        joined.push(combined);
                    }
                }

                if !matched && matches!(kind, JoinKind::Left | JoinKind::Full) {
                    joined.push(combine_row_with_nulls(left_row, &right_columns));
                }
            }

            if matches!(kind, JoinKind::Right | JoinKind::Full) {
                for (right_index, right_row) in right_rows.iter().enumerate() {
                    if !right_matched[right_index] {
                        joined.push(combine_nulls_with_row(&left_columns, right_row));
                    }
                }
            }

            let text_fields = deduce_text_fields(
                &joined
                    .iter()
                    .map(|row| row.entries().to_vec())
                    .collect::<Vec<_>>(),
            );
            let batches = batch::chunk_rows(joined, batch::DEFAULT_BATCH_SIZE);
            ensure_temp_budget(env.controls, &batches)?;
            Ok((batches, text_fields))
        }
    }
}

fn qualify_batches(batches: Vec<Batch>, qualifier: &str) -> Vec<Batch> {
    batches
        .into_iter()
        .map(|batch| {
            batch
                .into_iter()
                .map(|row| qualify_row(row, qualifier))
                .collect()
        })
        .collect()
}

fn combine_batches_with_outer_row(batches: Vec<Batch>, outer_row: &BatchRow) -> Vec<Batch> {
    batches
        .into_iter()
        .map(|batch| {
            batch
                .into_iter()
                .map(|row| combine_rows(outer_row, &row))
                .collect()
        })
        .collect()
}

fn source_contains_lateral(source: &QuerySource) -> bool {
    match source {
        QuerySource::Subquery { lateral, .. } => *lateral,
        QuerySource::Join { left, right, .. } => {
            source_contains_lateral(left) || source_contains_lateral(right)
        }
        QuerySource::Collection(_) | QuerySource::Cte(_) | QuerySource::SingleRow => false,
    }
}

fn qualify_row(row: BatchRow, qualifier: &str) -> BatchRow {
    let qualifier = qualifier.to_ascii_lowercase();
    let mut values = Vec::new();
    for (name, value) in row.into_entries() {
        values.push((name.clone(), value.clone()));
        values.push((format!("{qualifier}.{name}"), value));
    }
    BatchRow::new(values)
}

fn combine_rows(left: &BatchRow, right: &BatchRow) -> BatchRow {
    let mut values = left.entries().to_vec();
    values.extend(right.entries().iter().cloned());
    BatchRow::new(values)
}

fn combine_row_with_nulls(left: &BatchRow, right_columns: &[String]) -> BatchRow {
    let mut values = left.entries().to_vec();
    values.extend(
        right_columns
            .iter()
            .map(|column| (column.clone(), Value::Null)),
    );
    BatchRow::new(values)
}

fn combine_nulls_with_row(left_columns: &[String], right: &BatchRow) -> BatchRow {
    let mut values = left_columns
        .iter()
        .map(|column| (column.clone(), Value::Null))
        .collect::<Vec<_>>();
    values.extend(right.entries().iter().cloned());
    BatchRow::new(values)
}

fn row_columns(rows: &[BatchRow]) -> Vec<String> {
    let mut columns = Vec::new();
    for row in rows {
        for (column, _) in row.entries() {
            if !columns.contains(column) {
                columns.push(column.clone());
            }
        }
    }
    columns
}

fn materialize_virtual_rows(rows: Vec<virtual_views::VirtualRow>) -> Vec<Batch> {
    batch::chunk_rows(
        rows.into_iter().map(BatchRow::new).collect::<Vec<_>>(),
        batch::DEFAULT_BATCH_SIZE,
    )
}

fn project_rows_to_schema(
    rows: Vec<BatchRow>,
    schema: &Schema,
    relation: &str,
) -> Result<Vec<BatchRow>, QueryError> {
    let mut projected = Vec::with_capacity(rows.len());
    for row in rows {
        let entries = row.into_entries();
        if entries.len() < schema.fields.len() {
            return Err(QueryError::General(format!(
                "view '{}' produced {} columns but schema expects {}",
                relation,
                entries.len(),
                schema.fields.len()
            )));
        }

        let mut values = Vec::with_capacity(schema.fields.len());
        for (field, (_name, value)) in schema.fields.iter().zip(entries) {
            values.push((field.name.clone(), value));
        }
        projected.push(BatchRow::new(values));
    }
    Ok(projected)
}

#[path = "execution/aggregate_exec.rs"]
mod aggregate_exec;

fn distinct_batches(batches: Vec<Batch>) -> Vec<Batch> {
    let mut rows = BTreeMap::<String, BatchRow>::new();
    for row in batch::flatten_batches(batches) {
        rows.entry(row_signature(&row)).or_insert(row);
    }
    batch::chunk_rows(rows.into_values().collect(), batch::DEFAULT_BATCH_SIZE)
}

fn distinct_on_batches(
    batches: Vec<Batch>,
    distinct_on: &[Expr],
    params: &[Value],
    search_context: Option<&filter::SearchContext>,
    user_functions: &HashMap<String, FunctionMeta>,
    session: Option<&CassieSession>,
) -> Result<Vec<Batch>, QueryError> {
    let mut seen = HashSet::<String>::new();
    let mut rows = Vec::new();
    for row in batch::flatten_batches(batches) {
        let key = distinct_on
            .iter()
            .map(|expr| {
                filter::evaluate_expr_value(
                    &row,
                    expr,
                    params,
                    search_context,
                    user_functions,
                    session,
                    None,
                )
                .map(|value| value_sort_key(&value))
            })
            .collect::<Result<Vec<_>, _>>()?
            .join("|");
        if seen.insert(key) {
            rows.push(row);
        }
    }
    Ok(batch::chunk_rows(rows, batch::DEFAULT_BATCH_SIZE))
}

fn apply_set_operation(
    left: Vec<BatchRow>,
    right: Vec<BatchRow>,
    set: &SelectSet,
) -> Result<Vec<BatchRow>, QueryError> {
    validate_set_width(&left, &right)?;
    match set.operator {
        SetOperator::UnionAll => {
            let mut rows = left;
            rows.extend(right);
            rows.sort_by_key(row_signature);
            Ok(rows)
        }
        SetOperator::Union => {
            let mut rows = left;
            rows.extend(right);
            let mut unique = BTreeMap::<String, BatchRow>::new();
            for row in rows {
                unique.entry(row_signature(&row)).or_insert(row);
            }
            Ok(unique.into_values().collect())
        }
        SetOperator::Intersect => {
            let right_signatures = right.iter().map(row_signature).collect::<HashSet<_>>();
            let mut unique = BTreeMap::<String, BatchRow>::new();
            for row in left {
                let signature = row_signature(&row);
                if right_signatures.contains(&signature) {
                    unique.entry(signature).or_insert(row);
                }
            }
            Ok(unique.into_values().collect())
        }
        SetOperator::Except => {
            let right_signatures = right.iter().map(row_signature).collect::<HashSet<_>>();
            let mut unique = BTreeMap::<String, BatchRow>::new();
            for row in left {
                let signature = row_signature(&row);
                if !right_signatures.contains(&signature) {
                    unique.entry(signature).or_insert(row);
                }
            }
            Ok(unique.into_values().collect())
        }
    }
}

fn slice_rows(rows: Vec<BatchRow>, offset: Option<i64>, limit: Option<i64>) -> Vec<BatchRow> {
    let offset = offset
        .and_then(|value| usize::try_from(value.max(0)).ok())
        .unwrap_or(0);
    let limit = limit.and_then(|value| usize::try_from(value.max(0)).ok());
    let iter = rows.into_iter().skip(offset);
    match limit {
        Some(limit) => iter.take(limit).collect(),
        None => iter.collect(),
    }
}

fn validate_set_width(left: &[BatchRow], right: &[BatchRow]) -> Result<(), QueryError> {
    let left_width = left.first().map(|row| row.entries().len());
    let right_width = right.first().map(|row| row.entries().len());
    if let (Some(left_width), Some(right_width)) = (left_width, right_width) {
        if left_width != right_width {
            return Err(QueryError::General(format!(
                "set operation column count mismatch: {left_width} != {right_width}"
            )));
        }
    }
    Ok(())
}

fn plan_uses_aggregate(plan: &LogicalPlan) -> bool {
    !plan.group_by.is_empty()
        || plan.having.is_some()
        || plan.projection.iter().any(|item| match item {
            SelectItem::Function { function, .. } => {
                crate::sql::functions::is_aggregate_function(&function.name)
            }
            SelectItem::Wildcard
            | SelectItem::Column { .. }
            | SelectItem::Expr { .. }
            | SelectItem::WindowFunction { .. } => false,
        })
}

fn group_expr_name(expr: &Expr) -> String {
    match expr {
        Expr::Column(name) => name.clone(),
        _ => expr_key(expr),
    }
}

fn aggregate_signature(function: &FunctionCall) -> String {
    format!(
        "{}({})",
        function.name.to_ascii_lowercase(),
        function
            .args
            .iter()
            .map(expr_key)
            .collect::<Vec<_>>()
            .join(",")
    )
}

fn expr_key(expr: &Expr) -> String {
    match expr {
        Expr::Column(name) => name.clone(),
        Expr::Param(index) => format!("${}", index + 1),
        Expr::Null => "null".to_string(),
        Expr::BoolLiteral(value) => value.to_string(),
        Expr::NumberLiteral(value) => value.to_string(),
        Expr::StringLiteral(value) => format!("'{value}'"),
        Expr::Function(function) => aggregate_signature(function),
        Expr::Binary { left, op, right } => {
            format!("{}{:?}{}", expr_key(left), op, expr_key(right))
        }
        Expr::IsNull { expr, negated } => {
            format!(
                "{} is{} null",
                expr_key(expr),
                if *negated { " not" } else { "" }
            )
        }
        Expr::InList {
            expr,
            values,
            negated,
        } => format!(
            "{}{} in ({})",
            expr_key(expr),
            if *negated { " not" } else { "" },
            values.iter().map(expr_key).collect::<Vec<_>>().join(",")
        ),
        Expr::Between {
            expr,
            low,
            high,
            negated,
        } => format!(
            "{}{} between {} and {}",
            expr_key(expr),
            if *negated { " not" } else { "" },
            expr_key(low),
            expr_key(high)
        ),
        Expr::Not { expr } => format!("not {}", expr_key(expr)),
        Expr::Cast { expr, data_type } => format!("{}::{data_type:?}", expr_key(expr)),
        Expr::Exists(_) => "exists".to_string(),
    }
}

fn value_sort_key(value: &Value) -> String {
    match value {
        Value::Null => "0:null".to_string(),
        Value::Bool(value) => format!("1:{value}"),
        Value::Int64(value) => format!("2:{value:020}"),
        Value::Float64(value) => format!("3:{value:020.12}"),
        Value::String(value) => format!("4:{value}"),
        Value::Vector(value) => format!("5:{:?}", value.values),
        Value::Json(value) => format!("6:{value}"),
    }
}

#[allow(clippy::too_many_arguments)]
fn execute_source_query_with_outer_row(
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
    let started_at = Instant::now();
    let env = SourceExecutionEnv {
        cassie,
        session,
        user_functions,
        params,
        controls,
    };
    let (mut batches, text_fields) =
        execute_query_source(&env, &plan.source, cte_context, false, outer_row)?;
    if let Some(outer_row) = outer_row {
        batches = combine_batches_with_outer_row(batches, outer_row);
    }
    let candidate_rows = batches.iter().map(|batch| batch.len()).sum::<usize>();

    let fulltext_fields = plan_inspection::fulltext_query_fields(plan);
    let uses_hybrid = plan_inspection::plan_uses_function(plan, "hybrid_score");
    let uses_vector = plan_inspection::plan_uses_vector_operator(plan);
    let search_context = if fulltext_fields.is_empty() {
        None
    } else {
        let (field_boost, field_k1, field_b, field_analyzer) =
            if let QuerySource::Collection(name) = &plan.source {
                let fields = cassie.catalog.text_fields(name);
                let mut boost = HashMap::with_capacity(fields.len());
                for field in fields {
                    if let Some(value) = cassie.catalog.get_field_boost(name, &field) {
                        boost.insert(field, value as f64);
                    }
                }

                let index_options = load_fulltext_index_options(cassie, name, &fulltext_fields)?;
                for (field, value) in index_options.field_boost {
                    boost.insert(field, value);
                }

                (
                    boost,
                    index_options.field_k1,
                    index_options.field_b,
                    index_options.field_analyzer,
                )
            } else {
                (
                    HashMap::new(),
                    HashMap::new(),
                    HashMap::new(),
                    HashMap::new(),
                )
            };

        Some(filter::SearchContext::from_rows(
            batches.iter().flat_map(|batch| batch.iter()),
            &text_fields,
            &field_boost,
            &field_k1,
            &field_b,
            &field_analyzer,
        ))
    };

    let resolved_filter = if let Some(filter_expr) = &plan.filter {
        Some(resolve_exists_expr(
            cassie,
            session,
            filter_expr,
            cte_context,
            user_functions,
            params,
            controls,
        )?)
    } else {
        None
    };

    if let Some(filter_expr) = &resolved_filter {
        batches = filter::filter_batches(
            batches,
            filter_expr,
            params,
            search_context.as_ref(),
            user_functions,
            session,
        )?;
        ensure_temp_budget(controls, &batches)?;
    }

    if plan_uses_aggregate(plan) {
        batches = aggregate_exec::aggregate_query_batches(
            cassie,
            batches,
            plan,
            params,
            search_context.as_ref(),
            user_functions,
            session,
            controls,
        )?;
        ensure_temp_budget(controls, &batches)?;
        if let Some(having) = &plan.having {
            let having = aggregate_exec::rewrite_aggregate_expr(having);
            batches = filter::filter_batches(
                batches,
                &having,
                params,
                search_context.as_ref(),
                user_functions,
                session,
            )?;
            ensure_temp_budget(controls, &batches)?;
        }
    }

    batches = aggregate_exec::apply_window_functions(
        batches,
        &plan.projection,
        params,
        search_context.as_ref(),
        user_functions,
        session,
    )?;
    ensure_temp_budget(controls, &batches)?;

    if !plan.distinct_on.is_empty() {
        batches = sort::sort_batches(
            batches,
            &plan.order,
            &plan.projection,
            params,
            search_context.as_ref(),
            user_functions,
            session,
        )?;
        ensure_temp_budget(controls, &batches)?;
        batches = distinct_on_batches(
            batches,
            &plan.distinct_on,
            params,
            search_context.as_ref(),
            user_functions,
            session,
        )?;
        ensure_temp_budget(controls, &batches)?;
    } else if plan.set.is_none() && !plan.order.is_empty() {
        batches = sort::sort_batches(
            batches,
            &plan.order,
            &plan.projection,
            params,
            search_context.as_ref(),
            user_functions,
            session,
        )?;
        ensure_temp_budget(controls, &batches)?;
    }

    batches = projection::project_batches(
        batches,
        &plan.projection,
        params,
        search_context.as_ref(),
        user_functions,
        session,
    )?;
    ensure_temp_budget(controls, &batches)?;

    if plan.distinct {
        batches = distinct_batches(batches);
        ensure_temp_budget(controls, &batches)?;
    }

    let mut rows = batch::flatten_batches(batches);
    if let Some(set) = &plan.set {
        let right_plan = plan_inspection::logical_plan_from_select(&set.right);
        let right_rows = execute_plan(
            cassie,
            session,
            &right_plan,
            cte_context,
            user_functions,
            params,
            controls,
        )?;
        rows = apply_set_operation(rows, right_rows, set)?;
    }
    if plan.set.is_some() && !plan.order.is_empty() {
        rows = sort::sort_rows(
            rows,
            &plan.order,
            &plan.projection,
            params,
            search_context.as_ref(),
            user_functions,
            session,
        )?;
    }
    rows = slice_rows(rows, plan.offset, plan.limit);

    let elapsed = started_at.elapsed();
    if !fulltext_fields.is_empty() {
        cassie
            .runtime
            .record_search_execution(elapsed, candidate_rows, rows.len());
    }
    if uses_hybrid {
        cassie
            .runtime
            .record_hybrid_execution(elapsed, candidate_rows, rows.len());
    }
    if uses_vector {
        cassie
            .runtime
            .record_vector_execution(elapsed, candidate_rows, rows.len());
    }

    Ok(rows)
}

fn load_fulltext_index_options(
    cassie: &Cassie,
    collection: &str,
    requested_fields: &HashSet<String>,
) -> Result<FulltextIndexOptions, QueryError> {
    let cache_key = FulltextIndexOptionsCacheKey::new(
        cassie.runtime.schema_epoch(),
        collection,
        requested_fields.iter().cloned(),
    );
    if let Some(options) = cassie.runtime.fulltext_index_options_lookup(&cache_key) {
        return Ok(options);
    }

    let mut field_boost = HashMap::new();
    let mut field_k1 = HashMap::new();
    let mut field_b = HashMap::new();
    let mut field_analyzer = HashMap::new();
    let mut seen_fields = HashSet::new();

    for index in cassie.catalog.list_indexes(collection) {
        if index.kind != catalog::IndexKind::FullText {
            continue;
        }

        let field = index.field.to_ascii_lowercase();
        if !requested_fields.contains(&field) {
            continue;
        }
        if !seen_fields.insert(field.clone()) {
            return Err(QueryError::General(format!(
                "fulltext indexes on field '{}' already exist on collection '{}'",
                index.field, collection
            )));
        }

        let boost = parse_index_float_option(
            &index,
            &index.field,
            "boost",
            index.options.get("boost").map(String::as_str),
            crate::search::bm25::DEFAULT_FULLTEXT_BOOST,
            0.0,
            None,
        )?;

        let k1 = parse_index_float_option(
            &index,
            &index.field,
            "k1",
            index.options.get("k1").map(String::as_str),
            crate::search::bm25::DEFAULT_BM25_K1,
            0.0,
            None,
        )?;

        let b = parse_index_float_option(
            &index,
            &index.field,
            "b",
            index.options.get("b").map(String::as_str),
            crate::search::bm25::DEFAULT_BM25_B,
            0.0,
            Some(1.0),
        )?;
        let analyzer = AnalyzerConfig::from_index_options(&index.options)
            .map_err(|error| QueryError::General(error.to_string()))?;

        field_boost.insert(field.clone(), boost);
        field_k1.insert(field.clone(), k1);
        field_b.insert(field.clone(), b);
        field_analyzer.insert(field, analyzer);
    }

    let options = FulltextIndexOptions {
        field_boost,
        field_k1,
        field_b,
        field_analyzer,
    };
    cassie
        .runtime
        .store_fulltext_index_options(cache_key, options.clone());
    Ok(options)
}

#[path = "execution/plan_inspection.rs"]
mod plan_inspection;
pub(crate) use plan_inspection::plan_needs_user_functions;

fn parse_index_float_option(
    index: &catalog::IndexMeta,
    field: &str,
    key: &str,
    value: Option<&str>,
    default: f64,
    min: f64,
    max: Option<f64>,
) -> Result<f64, QueryError> {
    let value = value.unwrap_or("").trim();
    if value.is_empty() {
        return Ok(default);
    }

    let parsed = value.parse::<f64>().map_err(|_| {
        QueryError::General(format!(
            "fulltext index option '{key}' on '{field}' for collection '{}' must be numeric",
            index.collection
        ))
    })?;

    if !parsed.is_finite() {
        return Err(QueryError::General(format!(
            "fulltext index option '{key}' on '{field}' for collection '{}' must be finite",
            index.collection
        )));
    }

    let valid = if let Some(max) = max {
        parsed >= min && parsed <= max
    } else {
        parsed >= min
    };

    if !valid {
        if let Some(max) = max {
            return Err(QueryError::General(format!(
                "fulltext index option '{key}' on '{field}' for collection '{}' must be in [{min}, {max}]",
                index.collection
            )));
        }

        return Err(QueryError::General(format!(
            "fulltext index option '{key}' on '{field}' for collection '{}' must be at least {min}",
            index.collection
        )));
    }

    Ok(parsed)
}

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
mod tests {
    use super::*;

    fn plan_for_sql(sql: &str) -> LogicalPlan {
        let parsed = crate::sql::parse_statement(sql).expect("parse statement");
        build_logical_plan(&parsed).expect("build logical plan")
    }

    #[test]
    fn should_detect_unordered_fulltext_fast_path_for_matching_search_query() {
        // Arrange
        let plan = plan_for_sql(
            "SELECT id, search_score(body, 'alpha') AS score FROM bench_documents WHERE search(body, 'alpha')",
        );

        // Act
        let spec = scored::fulltext_filtered_read_spec(&plan);

        // Assert
        let spec = spec.expect("unordered fulltext fast path");
        assert_eq!(spec.collection, "bench_documents");
        assert_eq!(spec.text_field, "body");
        assert_eq!(spec.query, "alpha");
        assert_eq!(spec.score_column, "score");
        assert_eq!(spec.columns.len(), 1);
        assert_eq!(spec.columns[0].name, "id");
        assert_eq!(spec.columns[0].output_name, "id");
    }

    #[test]
    fn should_reject_unordered_fulltext_fast_path_for_mismatched_search_query() {
        // Arrange
        let plan = plan_for_sql(
            "SELECT id, search_score(body, 'alpha') AS score FROM bench_documents WHERE search(body, 'bravo')",
        );

        // Act
        let spec = scored::fulltext_filtered_read_spec(&plan);

        // Assert
        assert!(spec.is_none());
    }

    #[test]
    fn should_reject_unordered_fulltext_fast_path_for_additional_filters() {
        // Arrange
        let plan = plan_for_sql(
            "SELECT id, search_score(body, 'alpha') AS score FROM bench_documents WHERE search(body, 'alpha') AND status = 'approved'",
        );

        // Act
        let spec = scored::fulltext_filtered_read_spec(&plan);

        // Assert
        assert!(spec.is_none());
    }

    #[test]
    fn should_reject_unordered_fulltext_fast_path_for_wildcard_projection() {
        // Arrange
        let plan = plan_for_sql("SELECT * FROM bench_documents WHERE search(body, 'alpha')");

        // Act
        let spec = scored::fulltext_filtered_read_spec(&plan);

        // Assert
        assert!(spec.is_none());
    }

    #[test]
    fn should_build_projected_read_spec_without_filter() {
        // Arrange
        let plan = plan_for_sql("SELECT id, title FROM bench_documents LIMIT 20");

        // Act
        let spec = projected_read::projected_filtered_read_spec(&plan);

        // Assert
        let spec = spec.expect("projected read spec");
        assert_eq!(spec.collection, "bench_documents");
        assert_eq!(spec.scan_fields, vec!["title".to_string()]);
    }

    #[test]
    fn should_push_limit_into_projected_read_spec_without_filter() {
        // Arrange
        let plan = plan_for_sql("SELECT id, title FROM bench_documents LIMIT 20");

        // Act
        let spec = projected_read::projected_filtered_read_spec(&plan);

        // Assert
        let spec = spec.expect("projected read spec");
        assert_eq!(spec.scan_limit, Some(20));
    }

    #[test]
    fn should_include_offset_in_projected_read_spec_scan_limit() {
        // Arrange
        let plan = plan_for_sql("SELECT id, title FROM bench_documents LIMIT 20 OFFSET 5");

        // Act
        let spec = projected_read::projected_filtered_read_spec(&plan);

        // Assert
        let spec = spec.expect("projected read spec");
        assert_eq!(spec.scan_limit, Some(25));
    }

    #[test]
    fn should_not_push_limit_into_projected_read_spec_when_filter_is_present() {
        // Arrange
        let plan = plan_for_sql(
            "SELECT id, title FROM bench_documents WHERE status = 'approved' LIMIT 20",
        );

        // Act
        let spec = projected_read::projected_filtered_read_spec(&plan);

        // Assert
        let spec = spec.expect("projected read spec");
        assert_eq!(spec.scan_limit, None);
    }

    #[test]
    fn should_detect_projected_scan_pushdown_for_literal_equality() {
        // Arrange
        let plan = plan_for_sql("SELECT id, title FROM bench_documents WHERE title = 'alpha'");
        let filter = plan.filter.as_ref().expect("filter");

        // Act
        let pushdown = projected_read::projected_scan_pushdown_filter(filter);

        // Assert
        let pushdown = pushdown.expect("pushdown filter");
        assert_eq!(pushdown.field, "title");
        assert_eq!(pushdown.value, Value::String("alpha".to_string()));
    }

    #[test]
    fn should_reject_projected_scan_pushdown_for_row_id_equality() {
        // Arrange
        let plan = plan_for_sql("SELECT id, title FROM bench_documents WHERE id = 'doc-1'");
        let filter = plan.filter.as_ref().expect("filter");

        // Act
        let pushdown = projected_read::projected_scan_pushdown_filter(filter);

        // Assert
        assert!(pushdown.is_none());
    }

    #[test]
    fn should_skip_user_function_catalog_for_builtin_only_plan() {
        // Arrange
        let plan =
            plan_for_sql("SELECT id FROM bench_documents WHERE score >= 10 ORDER BY id LIMIT 20");

        // Act
        let needs_user_functions = plan_needs_user_functions(&plan);

        // Assert
        assert!(!needs_user_functions);
    }

    #[test]
    fn should_require_user_function_catalog_for_user_defined_function_plan() {
        // Arrange
        let plan =
            plan_for_sql("SELECT my_udf(title) AS normalized_title FROM bench_documents LIMIT 20");

        // Act
        let needs_user_functions = plan_needs_user_functions(&plan);

        // Assert
        assert!(needs_user_functions);
    }

    #[test]
    fn should_report_execution_breakdown_for_projected_filtered_read() {
        // Arrange
        std::env::set_var("CASSIE_MIDGE_ALLOW_FALLBACK", "1");
        let mut path = std::env::temp_dir();
        path.push(format!(
            "cassie-execution-breakdown-{}",
            uuid::Uuid::new_v4()
        ));
        let cassie = Cassie::new_with_data_dir(&path).expect("cassie");
        let collection = "breakdown_documents";
        let schema = Schema {
            fields: vec![FieldSchema {
                name: "title".to_string(),
                data_type: DataType::Text,
                nullable: true,
            }],
        };
        cassie
            .midge
            .create_collection(collection, schema.clone())
            .expect("create collection");
        cassie.register_collection(collection, schema);
        cassie
            .midge
            .put_document(
                collection,
                Some("doc-1".to_string()),
                serde_json::json!({"title": "alpha"}),
            )
            .expect("put document");

        let logical =
            plan_for_sql("SELECT id, title FROM breakdown_documents WHERE title = 'alpha'");
        let physical = crate::planner::physical::build(logical);

        // Act
        let output =
            run_with_execution_breakdown(&cassie, physical, vec![]).expect("execution breakdown");

        // Assert
        assert_eq!(output.result.rows.len(), 1);
        assert_eq!(
            output.result.rows[0],
            vec![
                Value::String("doc-1".to_string()),
                Value::String("alpha".to_string()),
            ]
        );
        assert!(output.breakdown.scan_us > 0 || output.breakdown.row_decode_us > 0);
        assert_eq!(output.breakdown.filter_us, 0);
        assert!(output.breakdown.projection_us > 0);
        assert!(output.breakdown.result_build_us > 0);

        let _ = std::fs::remove_dir_all(path);
    }
}
