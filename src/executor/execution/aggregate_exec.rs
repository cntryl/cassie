use std::collections::{BTreeMap, HashMap};
use std::thread;

use crate::app::{Cassie, CassieSession};
use crate::catalog::FunctionMeta;
use crate::executor::batch::{self, Batch, BatchRow};
use crate::executor::filter;
use crate::planner::logical::LogicalPlan;
use crate::runtime::QueryExecutionControls;
use crate::sql::ast::{Expr, FunctionCall, SelectItem};
use crate::types::Value;

use super::{aggregate_signature, check_timeout, group_expr_name, value_sort_key, QueryError};

#[path = "aggregate_exec/memory.rs"]
mod memory;

pub(super) struct AggregateExecutionContext<'a> {
    pub(super) plan: &'a LogicalPlan,
    pub(super) params: &'a [Value],
    pub(super) search_context: Option<&'a filter::SearchContext>,
    pub(super) user_functions: &'a HashMap<String, FunctionMeta>,
    pub(super) session: Option<&'a CassieSession>,
    pub(super) controls: &'a QueryExecutionControls,
}

struct AggregateValueContext<'a> {
    params: &'a [Value],
    search_context: Option<&'a filter::SearchContext>,
    user_functions: &'a HashMap<String, FunctionMeta>,
    session: Option<&'a CassieSession>,
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
    let mut groups = BTreeMap::<String, (Vec<(String, Value)>, Vec<BatchRow>)>::new();
    let mut group_memory = memory::replace_serial(None, context.controls, &groups)?;

    for row in rows {
        check_timeout(context.controls)?;
        let group_values = aggregate_group_values(&row, context)?;
        let signature = aggregate_group_signature(&group_values);
        groups
            .entry(signature)
            .or_insert_with(|| (group_values, Vec::new()))
            .1
            .push(row);
        group_memory = memory::replace_serial(Some(group_memory), context.controls, &groups)?;
    }

    if groups.is_empty() && context.plan.group_by.is_empty() {
        groups.insert(String::from("__all__"), (Vec::new(), Vec::new()));
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
                    let mut groups = BTreeMap::<String, PartialAggregateGroup>::new();
                    let mut group_memory =
                        memory::replace_partial(None, context.controls, &groups)?;
                    for row in chunk {
                        check_timeout(context.controls)?;
                        let group_values = aggregate_group_values(row, context)?;
                        let signature = aggregate_group_signature(&group_values);
                        let group = groups
                            .entry(signature)
                            .or_insert_with(|| PartialAggregateGroup::new(group_values, specs));
                        group.update(row, specs, context)?;
                        group_memory =
                            memory::replace_partial(Some(group_memory), context.controls, &groups)?;
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
    let mut merged = BTreeMap::<String, PartialAggregateGroup>::new();
    let mut merged_memory = memory::replace_partial(None, context.controls, &merged)?;
    for partial in partials.drain(..) {
        for (signature, group) in partial {
            merged
                .entry(signature)
                .and_modify(|existing| existing.merge(&group))
                .or_insert(group);
            merged_memory =
                memory::replace_partial(Some(merged_memory), context.controls, &merged)?;
        }
    }

    if merged.is_empty() && context.plan.group_by.is_empty() {
        merged.insert(
            String::from("__all__"),
            PartialAggregateGroup::new(Vec::new(), specs),
        );
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

    cassie
        .runtime
        .record_parallel_aggregation(workers, partitions, input_rows, group_count);
    Ok(batch::chunk_rows(out, batch::DEFAULT_BATCH_SIZE))
}

#[derive(Clone)]
struct PartialAggregateGroup {
    group_values: Vec<(String, Value)>,
    accumulators: Vec<AggregateAccumulator>,
}

impl PartialAggregateGroup {
    fn new(group_values: Vec<(String, Value)>, specs: &[AggregateSpec]) -> Self {
        Self {
            group_values,
            accumulators: specs
                .iter()
                .map(|spec| AggregateAccumulator::new(&spec.function))
                .collect(),
        }
    }

    fn update(
        &mut self,
        row: &BatchRow,
        specs: &[AggregateSpec],
        context: &AggregateExecutionContext<'_>,
    ) -> Result<(), QueryError> {
        for (accumulator, spec) in self.accumulators.iter_mut().zip(specs) {
            accumulator.update(
                &spec.function,
                row,
                context.params,
                context.search_context,
                context.user_functions,
                context.session,
            )?;
        }
        Ok(())
    }

    fn merge(&mut self, other: &Self) {
        for (left, right) in self.accumulators.iter_mut().zip(&other.accumulators) {
            left.merge(right);
        }
    }
}

#[derive(Clone)]
enum AggregateAccumulator {
    Count { count: i64 },
    Sum { sum: NumericSum, seen: bool },
    Avg { sum: f64, count: usize },
    MinMax { selected: Option<Value>, max: bool },
}

impl AggregateAccumulator {
    fn new(function: &FunctionCall) -> Self {
        match function.name.to_ascii_lowercase().as_str() {
            "count" => Self::Count { count: 0 },
            "sum" => Self::Sum {
                sum: NumericSum::Int(0),
                seen: false,
            },
            "avg" => Self::Avg { sum: 0.0, count: 0 },
            "max" => Self::MinMax {
                selected: None,
                max: true,
            },
            _ => Self::MinMax {
                selected: None,
                max: false,
            },
        }
    }

    fn update(
        &mut self,
        function: &FunctionCall,
        row: &BatchRow,
        params: &[Value],
        search_context: Option<&filter::SearchContext>,
        user_functions: &HashMap<String, FunctionMeta>,
        session: Option<&CassieSession>,
    ) -> Result<(), QueryError> {
        let value_context = AggregateValueContext {
            params,
            search_context,
            user_functions,
            session,
        };
        match self {
            Self::Count { count } => Self::update_count(function, row, &value_context, count)?,
            Self::Sum { sum, seen } => {
                Self::update_sum(function, row, &value_context, sum, seen)?;
            }
            Self::Avg { sum, count } => {
                Self::update_avg(function, row, &value_context, sum, count)?;
            }
            Self::MinMax { selected, max } => {
                Self::update_minmax(function, row, &value_context, selected, *max)?;
            }
        }
        Ok(())
    }

    fn merge(&mut self, other: &Self) {
        match (self, other) {
            (Self::Count { count }, Self::Count { count: other }) => *count += other,
            (
                Self::Sum { sum, seen },
                Self::Sum {
                    sum: other_sum,
                    seen: other_seen,
                },
            ) => {
                sum.merge(other_sum)
                    .expect("numeric aggregate state should merge");
                *seen = *seen || *other_seen;
            }
            (
                Self::Avg { sum, count },
                Self::Avg {
                    sum: other_sum,
                    count: other_count,
                },
            ) => {
                *sum += other_sum;
                *count += other_count;
            }
            (
                Self::MinMax { selected, max },
                Self::MinMax {
                    selected: Some(value),
                    max: _,
                },
            ) => {
                let replace = selected.as_ref().is_none_or(|current| {
                    let current_key = value_sort_key(current);
                    let value_key = value_sort_key(value);
                    if *max {
                        value_key > current_key
                    } else {
                        value_key < current_key
                    }
                });
                if replace {
                    *selected = Some(value.clone());
                }
            }
            _ => {}
        }
    }

    fn finish(self) -> Value {
        match self {
            Self::Count { count } => Value::Int64(count),
            Self::Sum { sum, seen } => {
                if seen {
                    sum.finish_value()
                } else {
                    Value::Null
                }
            }
            Self::Avg { sum, count } => {
                if count == 0 {
                    Value::Null
                } else {
                    let count = usize_to_f64(count).expect("aggregate count should fit in f64");
                    Value::Float64(sum / count)
                }
            }
            Self::MinMax { selected, .. } => selected.unwrap_or(Value::Null),
        }
    }
}

impl AggregateAccumulator {
    fn evaluate_input(
        function: &FunctionCall,
        row: &BatchRow,
        context: &AggregateValueContext<'_>,
    ) -> Result<Option<Value>, QueryError> {
        let Some(expr) = function.args.first() else {
            return Ok(None);
        };
        filter::evaluate_expr_value(
            row,
            expr,
            context.params,
            context.search_context,
            context.user_functions,
            context.session,
            None,
        )
        .map(Some)
    }

    fn update_count(
        function: &FunctionCall,
        row: &BatchRow,
        context: &AggregateValueContext<'_>,
        count: &mut i64,
    ) -> Result<(), QueryError> {
        if matches!(function.args.as_slice(), [Expr::Column(name)] if name == "*") {
            *count += 1;
            return Ok(());
        }
        let Some(value) = Self::evaluate_input(function, row, context)? else {
            return Ok(());
        };
        if !matches!(value, Value::Null) {
            *count += 1;
        }
        Ok(())
    }

    fn update_sum(
        function: &FunctionCall,
        row: &BatchRow,
        context: &AggregateValueContext<'_>,
        sum: &mut NumericSum,
        seen: &mut bool,
    ) -> Result<(), QueryError> {
        let Some(value) = Self::evaluate_input(function, row, context)? else {
            return Ok(());
        };
        match value {
            Value::Int64(value) => {
                sum.add_int(value)?;
                *seen = true;
            }
            Value::Float64(value) => {
                sum.add_float(value)?;
                *seen = true;
            }
            Value::Null => {}
            _ => sum.promote_to_float()?,
        }
        Ok(())
    }

    fn update_avg(
        function: &FunctionCall,
        row: &BatchRow,
        context: &AggregateValueContext<'_>,
        sum: &mut f64,
        count: &mut usize,
    ) -> Result<(), QueryError> {
        let Some(value) = Self::evaluate_input(function, row, context)? else {
            return Ok(());
        };
        match value {
            Value::Int64(value) => {
                *sum += i64_to_f64(value)?;
                *count += 1;
            }
            Value::Float64(value) => {
                *sum += value;
                *count += 1;
            }
            _ => {}
        }
        Ok(())
    }

    fn update_minmax(
        function: &FunctionCall,
        row: &BatchRow,
        context: &AggregateValueContext<'_>,
        selected: &mut Option<Value>,
        max: bool,
    ) -> Result<(), QueryError> {
        let Some(value) = Self::evaluate_input(function, row, context)? else {
            return Ok(());
        };
        if matches!(value, Value::Null) {
            return Ok(());
        }
        let replace = selected.as_ref().is_none_or(|current| {
            let current_key = value_sort_key(current);
            let value_key = value_sort_key(&value);
            if max {
                value_key > current_key
            } else {
                value_key < current_key
            }
        });
        if replace {
            *selected = Some(value);
        }
        Ok(())
    }
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

fn aggregate_group_signature(group_values: &[(String, Value)]) -> String {
    if group_values.is_empty() {
        "__all__".to_string()
    } else {
        group_values
            .iter()
            .map(|(_, value)| value_sort_key(value))
            .collect::<Vec<_>>()
            .join("|")
    }
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
            let current_key = value_sort_key(current);
            let value_key = value_sort_key(&value);
            if max {
                value_key > current_key
            } else {
                value_key < current_key
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

#[derive(Clone)]
enum NumericSum {
    Int(i64),
    Float(f64),
}

impl NumericSum {
    fn add_int(&mut self, value: i64) -> Result<(), QueryError> {
        match self {
            Self::Int(sum) => {
                *sum = sum.checked_add(value).ok_or_else(|| {
                    QueryError::General(String::from("aggregate integer overflow"))
                })?;
            }
            Self::Float(sum) => *sum += i64_to_f64(value)?,
        }
        Ok(())
    }

    fn add_float(&mut self, value: f64) -> Result<(), QueryError> {
        self.promote_to_float()?;
        if let Self::Float(sum) = self {
            *sum += value;
        }
        Ok(())
    }

    fn promote_to_float(&mut self) -> Result<(), QueryError> {
        if let Self::Int(sum) = self {
            *self = Self::Float(i64_to_f64(*sum)?);
        }
        Ok(())
    }

    fn merge(&mut self, other: &Self) -> Result<(), QueryError> {
        match other {
            Self::Int(value) => self.add_int(*value),
            Self::Float(value) => self.add_float(*value),
        }
    }

    fn finish_value(self) -> Value {
        match self {
            Self::Int(sum) => Value::Int64(sum),
            Self::Float(sum) => Value::Float64(sum),
        }
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

fn i64_to_f64(value: i64) -> Result<f64, QueryError> {
    value
        .to_string()
        .parse::<f64>()
        .map_err(|_| QueryError::General(String::from("aggregate integer conversion failed")))
}

fn usize_to_f64(value: usize) -> Result<f64, QueryError> {
    value
        .to_string()
        .parse::<f64>()
        .map_err(|_| QueryError::General(String::from("aggregate count conversion failed")))
}
