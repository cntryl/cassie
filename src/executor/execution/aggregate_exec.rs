use std::collections::btree_map::Entry;
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
#[path = "aggregate_exec/rewrite.rs"]
mod rewrite;
#[path = "aggregate_exec/state.rs"]
mod state;
#[cfg(test)]
#[path = "aggregate_exec/tests.rs"]
mod tests;

use crate::types::numeric::{i64_to_f64, usize_to_f64};
use group_memory::GroupMemory;
pub(super) use rewrite::{
    contains_aggregate, rewrite_aggregate_expr, rewrite_aggregate_projection,
};
use state::{NumericSum, PartialAggregateGroup};

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
    let mut group_memory = GroupMemory::new(context.controls)?;

    for row in rows {
        check_timeout(context.controls)?;
        let group_values = aggregate_group_values(&row, context)?;
        let signature = aggregate_group_signature(&group_values);
        let mut added_bytes = group_memory::serial_row_bytes(&row);
        let group = match groups.entry(signature) {
            Entry::Occupied(entry) => entry.into_mut(),
            Entry::Vacant(entry) => {
                added_bytes = added_bytes
                    .saturating_add(group_memory::serial_group_bytes(entry.key(), &group_values));
                entry.insert((group_values, Vec::new()))
            }
        };
        group.1.push(row);
        group_memory.add(added_bytes)?;
    }

    if groups.is_empty() && context.plan.group_by.is_empty() {
        let signature = SemanticKey::default();
        group_memory.add(group_memory::serial_group_bytes(&signature, &[]))?;
        groups.insert(signature, (Vec::new(), Vec::new()));
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
    let partials = aggregate_partitions(rows, specs, context, workers)?;

    let partitions = partials.len();
    let input_rows = rows.len();
    let mut merged = BTreeMap::<SemanticKey, PartialAggregateGroup>::new();
    // Worker reservations stay alive until the merged output is built: groups
    // moved into `merged` are still accounted by the partition that built
    // them, so only growth that happens during the merge is charged here.
    let mut partition_memory = Vec::with_capacity(partitions);
    let mut merged_memory = GroupMemory::new(context.controls)?;
    for partial in partials {
        partition_memory.push(partial.memory);
        for (signature, group) in partial.groups {
            match merged.entry(signature) {
                Entry::Occupied(mut entry) => {
                    let change = entry.get_mut().merge(&group)?;
                    merged_memory.add(change.after.saturating_sub(change.before))?;
                }
                Entry::Vacant(entry) => {
                    entry.insert(group);
                }
            }
        }
    }

    if merged.is_empty() && context.plan.group_by.is_empty() {
        let signature = SemanticKey::default();
        let group = PartialAggregateGroup::new(Vec::new(), specs);
        merged_memory.add(group_memory::partial_group_bytes(&signature, &group))?;
        merged.insert(signature, group);
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
    drop(partition_memory);

    cassie
        .runtime
        .record_parallel_aggregation(workers, partitions, input_rows, group_count);
    Ok(batch::chunk_rows(out, batch::DEFAULT_BATCH_SIZE))
}

type PartialGroups = BTreeMap<SemanticKey, PartialAggregateGroup>;

/// One worker's partial groups together with the reservation that accounts
/// for them; the reservation must outlive the groups it covers.
struct PartialAggregation {
    groups: PartialGroups,
    memory: GroupMemory,
}

fn aggregate_partitions(
    rows: &[BatchRow],
    specs: &[AggregateSpec],
    context: &AggregateExecutionContext<'_>,
    workers: usize,
) -> Result<Vec<PartialAggregation>, QueryError> {
    let chunk_size = rows.len().div_ceil(workers).max(1);
    thread::scope(|scope| {
        rows.chunks(chunk_size)
            .map(|chunk| scope.spawn(move || aggregate_partition(chunk, specs, context)))
            .collect::<Vec<_>>()
            .into_iter()
            .map(|handle| {
                crate::executor::worker::join_scoped_worker(
                    handle,
                    "parallel aggregation worker panicked",
                )?
            })
            .collect::<Result<Vec<_>, QueryError>>()
    })
}

fn aggregate_partition(
    chunk: &[BatchRow],
    specs: &[AggregateSpec],
    context: &AggregateExecutionContext<'_>,
) -> Result<PartialAggregation, QueryError> {
    let mut groups = PartialGroups::new();
    let mut group_memory = GroupMemory::new(context.controls)?;
    for row in chunk {
        check_timeout(context.controls)?;
        let group_values = aggregate_group_values(row, context)?;
        let signature = aggregate_group_signature(&group_values);
        let group = match groups.entry(signature) {
            Entry::Occupied(entry) => entry.into_mut(),
            Entry::Vacant(entry) => {
                let group = PartialAggregateGroup::new(group_values, specs);
                group_memory.add(group_memory::partial_group_bytes(entry.key(), &group))?;
                entry.insert(group)
            }
        };
        let change = group.update(row, specs, context)?;
        group_memory.resize(change.before, change.after)?;
    }
    Ok(PartialAggregation {
        groups,
        memory: group_memory,
    })
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
        }
        Expr::Exists(_) => return false,
        _ => {}
    }
    expr.all_children(|child| expr_supports_parallel_aggregation(child, user_functions))
}

fn aggregate_specs(plan: &LogicalPlan) -> Vec<AggregateSpec> {
    let mut specs = Vec::<AggregateSpec>::new();
    for item in &plan.projection {
        match item {
            SelectItem::Function { function, alias }
                if crate::sql::functions::is_aggregate_function(&function.name) =>
            {
                register_aggregate_spec(&mut specs, function, alias.clone());
            }
            SelectItem::Function { function, .. } => {
                for arg in &function.args {
                    collect_aggregate_specs_from_expr(arg, &mut specs);
                }
            }
            SelectItem::Expr { expr, .. } => collect_aggregate_specs_from_expr(expr, &mut specs),
            SelectItem::Wildcard
            | SelectItem::Column { .. }
            | SelectItem::WindowFunction { .. } => {}
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
    for name in [output_name, signature] {
        if !output_names.contains(&name) {
            output_names.push(name);
        }
    }
    specs.push(AggregateSpec {
        function: function.clone(),
        output_names,
    });
}

fn collect_aggregate_specs_from_expr(expr: &Expr, specs: &mut Vec<AggregateSpec>) {
    match expr {
        Expr::Function(function)
            if crate::sql::functions::is_aggregate_function(&function.name) =>
        {
            register_aggregate_spec(specs, function, None);
        }
        Expr::Exists(_) => {}
        _ => expr.for_each_child(|child| collect_aggregate_specs_from_expr(child, specs)),
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
                sum.add_float(value);
                seen = true;
            }
            Value::Null => {}
            _ => {
                sum.promote_to_float();
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
                sum += i64_to_f64(value);
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
        Ok(Value::Float64(sum / usize_to_f64(count)))
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
