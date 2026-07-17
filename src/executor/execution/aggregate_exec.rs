use std::collections::{BTreeMap, HashMap};
use std::thread;

use crate::app::{Cassie, CassieSession};
use crate::catalog::FunctionMeta;
use crate::executor::batch::{self, Batch, BatchRow};
use crate::executor::filter;
use crate::executor::semantic::{compare_values, SemanticKey};
use crate::planner::logical::LogicalPlan;
use crate::runtime::QueryExecutionControls;
use crate::sql::ast::{Expr, FunctionCall, SelectItem};
use crate::types::Value;

use super::{aggregate_signature, check_timeout, group_expr_name, QueryError};

#[path = "aggregate_exec/group_memory.rs"]
mod group_memory;
#[path = "aggregate_exec/state.rs"]
mod state;

use state::{i64_to_f64, usize_to_f64, NumericSum, PartialAggregateGroup};

pub(super) struct AggregateExecutionContext<'a> {
    pub(super) plan: &'a LogicalPlan,
    pub(super) params: &'a [Value],
    pub(super) search_context: Option<&'a filter::SearchContext>,
    pub(super) user_functions: &'a HashMap<String, FunctionMeta>,
    pub(super) session: Option<&'a CassieSession>,
    pub(super) controls: &'a QueryExecutionControls,
}

#[derive(Clone)]
struct AggregateSpec {
    function: FunctionCall,
    output_names: Vec<String>,
}

pub(super) fn aggregate_query_batches(
    cassie: &Cassie,
    batches: Vec<Batch>,
    context: &AggregateExecutionContext<'_>,
) -> Result<Vec<Batch>, QueryError> {
    let rows = batch::flatten_batches(batches);
    let specs = aggregate_specs(context.plan);
    let worker_limit = aggregation_worker_limit(cassie, rows.len());
    let eligibility =
        parallel_aggregation_eligibility(context.plan, &specs, context.user_functions);
    if worker_limit > 1 && rows.len() >= batch::DEFAULT_BATCH_SIZE {
        if let Ok(()) = eligibility {
            let requested =
                worker_limit.min(partition_count(rows.len(), batch::DEFAULT_BATCH_SIZE));
            if let Some(worker_guard) = cassie.runtime.try_acquire_operator_workers(requested) {
                let workers = worker_guard.workers().min(requested);
                return aggregate_query_batches_parallel(cassie, &rows, &specs, context, workers);
            }
        }
    }

    let fallback_reason = if worker_limit == 1 {
        "worker-limit-one"
    } else if rows.len() < batch::DEFAULT_BATCH_SIZE {
        "small-input"
    } else {
        eligibility.err().unwrap_or("single-partition")
    };
    cassie
        .runtime
        .record_parallel_aggregation_fallback(fallback_reason.to_owned());
    aggregate_query_batches_serial(rows, &specs, context)
}

fn aggregate_query_batches_serial(
    rows: Vec<BatchRow>,
    specs: &[AggregateSpec],
    context: &AggregateExecutionContext<'_>,
) -> Result<Vec<Batch>, QueryError> {
    let mut groups = BTreeMap::<SemanticKey, (Vec<(String, Value)>, Vec<BatchRow>)>::new();
    let mut group_memory = group_memory::replace_serial(None, context.controls, &groups)?;

    for row in rows {
        check_timeout(context.controls)?;
        let group_values = aggregate_group_values(&row, context)?;
        let signature = aggregate_group_signature(&group_values);
        groups
            .entry(signature)
            .or_insert_with(|| (group_values, Vec::new()))
            .1
            .push(row);
        group_memory = group_memory::replace_serial(Some(group_memory), context.controls, &groups)?;
    }

    if groups.is_empty() && context.plan.group_by.is_empty() {
        groups.insert(SemanticKey::default(), (Vec::new(), Vec::new()));
        group_memory = group_memory::replace_serial(Some(group_memory), context.controls, &groups)?;
    }

    let mut out = Vec::with_capacity(groups.len());
    for (_signature, (group_values, group_rows)) in groups {
        check_timeout(context.controls)?;
        let mut values = group_values;
        for spec in specs {
            let value = evaluate_aggregate(&spec.function, &group_rows, context)?;
            for name in &spec.output_names {
                values.push((name.clone(), value.clone()));
            }
        }
        out.push(BatchRow::new(values));
    }
    drop(group_memory);

    Ok(batch::chunk_rows(out, batch::DEFAULT_BATCH_SIZE))
}

fn aggregate_query_batches_parallel(
    cassie: &Cassie,
    rows: &[BatchRow],
    specs: &[AggregateSpec],
    context: &AggregateExecutionContext<'_>,
    workers: usize,
) -> Result<Vec<Batch>, QueryError> {
    let chunk_size = rows.len().div_ceil(workers).max(1);
    let mut partials = thread::scope(|scope| {
        rows.chunks(chunk_size)
            .map(|chunk| {
                scope.spawn(move || {
                    let mut groups = BTreeMap::<SemanticKey, PartialAggregateGroup>::new();
                    let mut group_memory =
                        group_memory::replace_partial(None, context.controls, &groups)?;
                    for row in chunk {
                        check_timeout(context.controls)?;
                        let group_values = aggregate_group_values(row, context)?;
                        let signature = aggregate_group_signature(&group_values);
                        let group = groups
                            .entry(signature)
                            .or_insert_with(|| PartialAggregateGroup::new(group_values, specs));
                        group.update(row, specs, context)?;
                        group_memory = group_memory::replace_partial(
                            Some(group_memory),
                            context.controls,
                            &groups,
                        )?;
                    }
                    Ok::<_, QueryError>(groups)
                })
            })
            .collect::<Vec<_>>()
            .into_iter()
            .map(|handle| {
                handle.join().map_err(|_| {
                    QueryError::General("parallel aggregation worker panicked".into())
                })?
            })
            .collect::<Result<Vec<_>, QueryError>>()
    })?;

    let partitions = partials.len();
    let input_rows = rows.len();
    let mut merged = BTreeMap::<SemanticKey, PartialAggregateGroup>::new();
    let mut merged_memory = group_memory::replace_partial(None, context.controls, &merged)?;
    for partial in partials.drain(..) {
        for (signature, group) in partial {
            merged
                .entry(signature)
                .and_modify(|existing| existing.merge(&group))
                .or_insert(group);
            merged_memory =
                group_memory::replace_partial(Some(merged_memory), context.controls, &merged)?;
        }
    }

    if merged.is_empty() && context.plan.group_by.is_empty() {
        merged.insert(
            SemanticKey::default(),
            PartialAggregateGroup::new(Vec::new(), specs),
        );
        merged_memory =
            group_memory::replace_partial(Some(merged_memory), context.controls, &merged)?;
    }

    let group_count = merged.len();
    let mut out = Vec::with_capacity(group_count);
    for (_signature, group) in merged {
        let mut values = group.group_values;
        for (spec, accumulator) in specs.iter().zip(group.accumulators) {
            let value = accumulator.finish();
            for name in &spec.output_names {
                values.push((name.clone(), value.clone()));
            }
        }
        out.push(BatchRow::new(values));
    }
    drop(merged_memory);

    cassie
        .runtime
        .record_parallel_aggregation(workers, partitions, input_rows, group_count);
    Ok(batch::chunk_rows(out, batch::DEFAULT_BATCH_SIZE))
}

fn aggregate_group_values(
    row: &BatchRow,
    context: &AggregateExecutionContext<'_>,
) -> Result<Vec<(String, Value)>, QueryError> {
    context
        .plan
        .group_by
        .iter()
        .map(|expr| {
            let name = group_expr_name(expr);
            let value = filter::evaluate_expr_value(
                row,
                expr,
                context.params,
                context.search_context,
                context.user_functions,
                context.session,
                None,
            )?;
            Ok((name, value))
        })
        .collect::<Result<Vec<_>, QueryError>>()
}

fn aggregate_group_signature(group_values: &[(String, Value)]) -> SemanticKey {
    SemanticKey::from_values(group_values.iter().map(|(_, value)| value))
}

fn parallel_aggregation_eligibility(
    plan: &LogicalPlan,
    specs: &[AggregateSpec],
    user_functions: &HashMap<String, FunctionMeta>,
) -> Result<(), &'static str> {
    if plan.distinct || !plan.distinct_on.is_empty() {
        return Err("distinct");
    }
    if plan.set.is_some() {
        return Err("set-operation");
    }
    if plan
        .projection
        .iter()
        .any(|item| matches!(item, SelectItem::WindowFunction { .. }))
    {
        return Err("window-function");
    }
    if specs.iter().any(|spec| {
        !matches!(
            spec.function.name.to_ascii_lowercase().as_str(),
            "count" | "sum" | "avg" | "min" | "max"
        )
    }) {
        return Err("unsupported-aggregate");
    }
    if plan
        .group_by
        .iter()
        .chain(plan.having.iter())
        .chain(plan.order.iter().map(|order| &order.expr))
        .any(|expr| !expr_supports_parallel_aggregation(expr, user_functions))
        || specs.iter().any(|spec| {
            spec.function
                .args
                .iter()
                .any(|expr| !expr_supports_parallel_aggregation(expr, user_functions))
        })
    {
        return Err("unsupported-expression");
    }
    Ok(())
}

fn expr_supports_parallel_aggregation(
    expr: &Expr,
    user_functions: &HashMap<String, FunctionMeta>,
) -> bool {
    match expr {
        Expr::Function(function) => {
            let name = function.name.to_ascii_lowercase();
            if user_functions.contains_key(&name) {
                return false;
            }
            if matches!(
                name.as_str(),
                "search"
                    | "search_score"
                    | "snippet"
                    | "vector_distance"
                    | "vector_score"
                    | "hybrid_score"
            ) {
                return false;
            }
            if crate::sql::functions::is_aggregate_function(&function.name)
                && !matches!(name.as_str(), "count" | "sum" | "avg" | "min" | "max")
            {
                return false;
            }
            function
                .args
                .iter()
                .all(|expr| expr_supports_parallel_aggregation(expr, user_functions))
        }
        Expr::Binary { left, right, .. } => {
            expr_supports_parallel_aggregation(left, user_functions)
                && expr_supports_parallel_aggregation(right, user_functions)
        }
        Expr::IsNull { expr, .. } | Expr::Cast { expr, .. } | Expr::Not { expr } => {
            expr_supports_parallel_aggregation(expr, user_functions)
        }
        Expr::InList { expr, values, .. } => {
            expr_supports_parallel_aggregation(expr, user_functions)
                && values
                    .iter()
                    .all(|value| expr_supports_parallel_aggregation(value, user_functions))
        }
        Expr::Between {
            expr, low, high, ..
        } => {
            expr_supports_parallel_aggregation(expr, user_functions)
                && expr_supports_parallel_aggregation(low, user_functions)
                && expr_supports_parallel_aggregation(high, user_functions)
        }
        Expr::Exists(_) => false,
        Expr::Column(_)
        | Expr::Param(_)
        | Expr::Null
        | Expr::BoolLiteral(_)
        | Expr::NumberLiteral(_)
        | Expr::StringLiteral(_) => true,
    }
}

fn aggregate_specs(plan: &LogicalPlan) -> Vec<AggregateSpec> {
    let mut specs = Vec::<AggregateSpec>::new();
    for item in &plan.projection {
        if let SelectItem::Function { function, alias } = item {
            register_aggregate_spec(&mut specs, function, alias.clone());
        }
    }
    if let Some(having) = &plan.having {
        collect_aggregate_specs_from_expr(having, &mut specs);
    }
    for order in &plan.order {
        collect_aggregate_specs_from_expr(&order.expr, &mut specs);
    }
    specs
}

fn register_aggregate_spec(
    specs: &mut Vec<AggregateSpec>,
    function: &FunctionCall,
    alias: Option<String>,
) {
    if !crate::sql::functions::is_aggregate_function(&function.name) {
        return;
    }
    let signature = aggregate_signature(function);
    let output_name = alias.unwrap_or_else(|| function.name.clone());
    if let Some(existing) = specs
        .iter_mut()
        .find(|spec| aggregate_signature(&spec.function) == signature)
    {
        if !existing.output_names.contains(&output_name) {
            existing.output_names.push(output_name);
        }
        return;
    }
    let mut output_names = vec![function.name.clone()];
    if !output_names.contains(&output_name) {
        output_names.push(output_name);
    }
    specs.push(AggregateSpec {
        function: function.clone(),
        output_names,
    });
}

fn collect_aggregate_specs_from_expr(expr: &Expr, specs: &mut Vec<AggregateSpec>) {
    match expr {
        Expr::Function(function) => register_aggregate_spec(specs, function, None),
        Expr::Binary { left, right, .. } => {
            collect_aggregate_specs_from_expr(left, specs);
            collect_aggregate_specs_from_expr(right, specs);
        }
        Expr::IsNull { expr, .. } | Expr::Cast { expr, .. } => {
            collect_aggregate_specs_from_expr(expr, specs);
        }
        Expr::InList { expr, values, .. } => {
            collect_aggregate_specs_from_expr(expr, specs);
            for value in values {
                collect_aggregate_specs_from_expr(value, specs);
            }
        }
        Expr::Between {
            expr, low, high, ..
        } => {
            collect_aggregate_specs_from_expr(expr, specs);
            collect_aggregate_specs_from_expr(low, specs);
            collect_aggregate_specs_from_expr(high, specs);
        }
        Expr::Not { expr } => collect_aggregate_specs_from_expr(expr, specs),
        Expr::Exists(_)
        | Expr::Column(_)
        | Expr::Param(_)
        | Expr::Null
        | Expr::BoolLiteral(_)
        | Expr::NumberLiteral(_)
        | Expr::StringLiteral(_) => {}
    }
}

fn evaluate_aggregate(
    function: &FunctionCall,
    rows: &[BatchRow],
    context: &AggregateExecutionContext<'_>,
) -> Result<Value, QueryError> {
    let name = function.name.to_ascii_lowercase();
    match name.as_str() {
        "count" => Ok(Value::Int64(count_aggregate(function, rows, context)?)),
        "sum" => sum_aggregate(function, rows, context),
        "avg" => avg_aggregate(function, rows, context),
        "min" => minmax_aggregate(function, rows, context, false),
        "max" => minmax_aggregate(function, rows, context, true),
        _ => Ok(Value::Null),
    }
}

fn count_aggregate(
    function: &FunctionCall,
    rows: &[BatchRow],
    context: &AggregateExecutionContext<'_>,
) -> Result<i64, QueryError> {
    if matches!(function.args.as_slice(), [Expr::Column(name)] if name == "*") {
        return i64::try_from(rows.len())
            .map_err(|_| QueryError::General(String::from("aggregate row count overflow")));
    }
    let mut count = 0i64;
    for row in rows {
        let value = filter::evaluate_expr_value(
            row,
            &function.args[0],
            context.params,
            context.search_context,
            context.user_functions,
            context.session,
            None,
        )?;
        if !matches!(value, Value::Null) {
            count += 1;
        }
    }
    Ok(count)
}

fn sum_aggregate(
    function: &FunctionCall,
    rows: &[BatchRow],
    context: &AggregateExecutionContext<'_>,
) -> Result<Value, QueryError> {
    let mut sum = NumericSum::Int(0);
    let mut seen = false;
    for row in rows {
        match filter::evaluate_expr_value(
            row,
            &function.args[0],
            context.params,
            context.search_context,
            context.user_functions,
            context.session,
            None,
        )? {
            Value::Int64(value) => {
                sum.add_int(value)?;
                seen = true;
            }
            Value::Float64(value) => {
                sum.add_float(value)?;
                seen = true;
            }
            Value::Null => {}
            _ => {
                sum.promote_to_float()?;
            }
        }
    }
    if !seen {
        return Ok(Value::Null);
    }
    Ok(sum.finish_value())
}

fn avg_aggregate(
    function: &FunctionCall,
    rows: &[BatchRow],
    context: &AggregateExecutionContext<'_>,
) -> Result<Value, QueryError> {
    let mut sum = 0.0;
    let mut count = 0usize;
    for row in rows {
        match filter::evaluate_expr_value(
            row,
            &function.args[0],
            context.params,
            context.search_context,
            context.user_functions,
            context.session,
            None,
        )? {
            Value::Int64(value) => {
                sum += i64_to_f64(value)?;
                count += 1;
            }
            Value::Float64(value) => {
                sum += value;
                count += 1;
            }
            _ => {}
        }
    }
    if count == 0 {
        Ok(Value::Null)
    } else {
        Ok(Value::Float64(
            sum / usize_to_f64(count)
                .map_err(|_| QueryError::General(String::from("aggregate count overflow")))?,
        ))
    }
}

fn minmax_aggregate(
    function: &FunctionCall,
    rows: &[BatchRow],
    context: &AggregateExecutionContext<'_>,
    max: bool,
) -> Result<Value, QueryError> {
    let mut selected: Option<Value> = None;
    for row in rows {
        let value = filter::evaluate_expr_value(
            row,
            &function.args[0],
            context.params,
            context.search_context,
            context.user_functions,
            context.session,
            None,
        )?;
        if matches!(value, Value::Null) {
            continue;
        }
        let replace = selected.as_ref().is_none_or(|current| {
            let ordering = compare_values(&value, current);
            if max {
                ordering.is_gt()
            } else {
                ordering.is_lt()
            }
        });
        if replace {
            selected = Some(value);
        }
    }
    Ok(selected.unwrap_or(Value::Null))
}

pub(super) fn rewrite_aggregate_expr(expr: &Expr) -> Expr {
    match expr {
        Expr::Function(function)
            if crate::sql::functions::is_aggregate_function(&function.name) =>
        {
            Expr::Column(function.name.clone())
        }
        Expr::Binary { left, op, right } => Expr::Binary {
            left: Box::new(rewrite_aggregate_expr(left)),
            op: op.clone(),
            right: Box::new(rewrite_aggregate_expr(right)),
        },
        Expr::IsNull { expr, negated } => Expr::IsNull {
            expr: Box::new(rewrite_aggregate_expr(expr)),
            negated: *negated,
        },
        Expr::InList {
            expr,
            values,
            negated,
        } => Expr::InList {
            expr: Box::new(rewrite_aggregate_expr(expr)),
            values: values.iter().map(rewrite_aggregate_expr).collect(),
            negated: *negated,
        },
        Expr::Between {
            expr,
            low,
            high,
            negated,
        } => Expr::Between {
            expr: Box::new(rewrite_aggregate_expr(expr)),
            low: Box::new(rewrite_aggregate_expr(low)),
            high: Box::new(rewrite_aggregate_expr(high)),
            negated: *negated,
        },
        Expr::Not { expr } => Expr::Not {
            expr: Box::new(rewrite_aggregate_expr(expr)),
        },
        Expr::Cast { expr, data_type } => Expr::Cast {
            expr: Box::new(rewrite_aggregate_expr(expr)),
            data_type: data_type.clone(),
        },
        Expr::Exists(_)
        | Expr::Function(_)
        | Expr::Column(_)
        | Expr::Param(_)
        | Expr::Null
        | Expr::BoolLiteral(_)
        | Expr::NumberLiteral(_)
        | Expr::StringLiteral(_) => expr.clone(),
    }
}

fn aggregation_worker_limit(cassie: &Cassie, row_count: usize) -> usize {
    cassie
        .runtime
        .limits()
        .parallel_aggregation_workers
        .max(1)
        .min(partition_count(row_count, batch::DEFAULT_BATCH_SIZE))
}

fn partition_count(row_count: usize, partition_size: usize) -> usize {
    row_count.div_ceil(partition_size).max(1)
}
