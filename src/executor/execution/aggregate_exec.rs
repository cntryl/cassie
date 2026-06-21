use std::cmp::Ordering as CmpOrdering;
use std::collections::{BTreeMap, HashMap};
use std::thread;

use crate::app::{Cassie, CassieSession};
use crate::catalog::FunctionMeta;
use crate::executor::batch::{self, Batch, BatchRow};
use crate::executor::filter;
use crate::planner::logical::LogicalPlan;
use crate::runtime::QueryExecutionControls;
use crate::sql::ast::{Expr, FunctionCall, SelectItem, SortDirection};
use crate::types::Value;

use super::{
    aggregate_signature, check_timeout, compare_query_values, group_expr_name, value_sort_key,
    QueryError,
};

#[derive(Clone)]
struct AggregateSpec {
    function: FunctionCall,
    output_names: Vec<String>,
}

pub(super) fn aggregate_query_batches(
    cassie: &Cassie,
    batches: Vec<Batch>,
    plan: &LogicalPlan,
    params: &[Value],
    search_context: Option<&filter::SearchContext>,
    user_functions: &HashMap<String, FunctionMeta>,
    session: Option<&CassieSession>,
    controls: &QueryExecutionControls,
) -> Result<Vec<Batch>, QueryError> {
    let rows = batch::flatten_batches(batches);
    let specs = aggregate_specs(plan);
    let worker_limit = cassie.runtime.limits().parallel_aggregation_workers.max(1);
    let eligibility = parallel_aggregation_eligibility(plan, &specs, user_functions);
    if worker_limit > 1 && rows.len() >= batch::DEFAULT_BATCH_SIZE {
        if let Ok(()) = eligibility {
            let workers = worker_limit.min(rows.len().div_ceil(batch::DEFAULT_BATCH_SIZE).max(1));
            if workers > 1 {
                return aggregate_query_batches_parallel(
                    cassie,
                    rows,
                    plan,
                    &specs,
                    params,
                    search_context,
                    user_functions,
                    session,
                    controls,
                    workers,
                );
            }
        }
    }

    let fallback_reason = if worker_limit == 1 {
        "worker-limit-one".to_string()
    } else if rows.len() < batch::DEFAULT_BATCH_SIZE {
        "small-input".to_string()
    } else {
        eligibility
            .err()
            .unwrap_or_else(|| "single-partition".to_string())
    };
    cassie
        .runtime
        .record_parallel_aggregation_fallback(fallback_reason);
    aggregate_query_batches_serial(
        rows,
        plan,
        &specs,
        params,
        search_context,
        user_functions,
        session,
    )
}

fn aggregate_query_batches_serial(
    rows: Vec<BatchRow>,
    plan: &LogicalPlan,
    specs: &[AggregateSpec],
    params: &[Value],
    search_context: Option<&filter::SearchContext>,
    user_functions: &HashMap<String, FunctionMeta>,
    session: Option<&CassieSession>,
) -> Result<Vec<Batch>, QueryError> {
    let mut groups = BTreeMap::<String, (Vec<(String, Value)>, Vec<BatchRow>)>::new();

    for row in rows {
        let group_values =
            aggregate_group_values(&row, plan, params, search_context, user_functions, session)?;
        let signature = aggregate_group_signature(&group_values);
        groups
            .entry(signature)
            .or_insert_with(|| (group_values, Vec::new()))
            .1
            .push(row);
    }

    if groups.is_empty() && plan.group_by.is_empty() {
        groups.insert("__all__".to_string(), (Vec::new(), Vec::new()));
    }

    let mut out = Vec::with_capacity(groups.len());
    for (_signature, (group_values, group_rows)) in groups {
        let mut values = group_values;
        for spec in specs {
            let value = evaluate_aggregate(
                &spec.function,
                &group_rows,
                params,
                search_context,
                user_functions,
                session,
            )?;
            for name in &spec.output_names {
                values.push((name.clone(), value.clone()));
            }
        }
        out.push(BatchRow::new(values));
    }

    Ok(batch::chunk_rows(out, batch::DEFAULT_BATCH_SIZE))
}

#[allow(clippy::too_many_arguments)]
fn aggregate_query_batches_parallel(
    cassie: &Cassie,
    rows: Vec<BatchRow>,
    plan: &LogicalPlan,
    specs: &[AggregateSpec],
    params: &[Value],
    search_context: Option<&filter::SearchContext>,
    user_functions: &HashMap<String, FunctionMeta>,
    session: Option<&CassieSession>,
    controls: &QueryExecutionControls,
    workers: usize,
) -> Result<Vec<Batch>, QueryError> {
    let chunk_size = rows.len().div_ceil(workers).max(1);
    let mut partials = thread::scope(|scope| {
        rows.chunks(chunk_size)
            .map(|chunk| {
                scope.spawn(move || {
                    let mut groups = BTreeMap::<String, PartialAggregateGroup>::new();
                    for row in chunk {
                        check_timeout(controls)?;
                        let group_values = aggregate_group_values(
                            row,
                            plan,
                            params,
                            search_context,
                            user_functions,
                            session,
                        )?;
                        let signature = aggregate_group_signature(&group_values);
                        let group = groups
                            .entry(signature)
                            .or_insert_with(|| PartialAggregateGroup::new(group_values, specs));
                        group.update(
                            row,
                            specs,
                            params,
                            search_context,
                            user_functions,
                            session,
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
    let mut merged = BTreeMap::<String, PartialAggregateGroup>::new();
    for partial in partials.drain(..) {
        for (signature, group) in partial {
            merged
                .entry(signature)
                .and_modify(|existing| existing.merge(&group))
                .or_insert(group);
        }
    }

    if merged.is_empty() && plan.group_by.is_empty() {
        merged.insert(
            "__all__".to_string(),
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
        params: &[Value],
        search_context: Option<&filter::SearchContext>,
        user_functions: &HashMap<String, FunctionMeta>,
        session: Option<&CassieSession>,
    ) -> Result<(), QueryError> {
        for (accumulator, spec) in self.accumulators.iter_mut().zip(specs) {
            accumulator.update(
                &spec.function,
                row,
                params,
                search_context,
                user_functions,
                session,
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
    Sum { sum: f64, all_int: bool, seen: bool },
    Avg { sum: f64, count: f64 },
    MinMax { selected: Option<Value>, max: bool },
}

impl AggregateAccumulator {
    fn new(function: &FunctionCall) -> Self {
        match function.name.to_ascii_lowercase().as_str() {
            "count" => Self::Count { count: 0 },
            "sum" => Self::Sum {
                sum: 0.0,
                all_int: true,
                seen: false,
            },
            "avg" => Self::Avg {
                sum: 0.0,
                count: 0.0,
            },
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
        match self {
            Self::Count { count } => {
                if matches!(function.args.as_slice(), [Expr::Column(name)] if name == "*") {
                    *count += 1;
                    return Ok(());
                }
                let Some(expr) = function.args.first() else {
                    return Ok(());
                };
                let value = filter::evaluate_expr_value(
                    row,
                    expr,
                    params,
                    search_context,
                    user_functions,
                    session,
                    None,
                )?;
                if !matches!(value, Value::Null) {
                    *count += 1;
                }
            }
            Self::Sum { sum, all_int, seen } => {
                let Some(expr) = function.args.first() else {
                    return Ok(());
                };
                match filter::evaluate_expr_value(
                    row,
                    expr,
                    params,
                    search_context,
                    user_functions,
                    session,
                    None,
                )? {
                    Value::Int64(value) => {
                        *sum += value as f64;
                        *seen = true;
                    }
                    Value::Float64(value) => {
                        *sum += value;
                        *all_int = false;
                        *seen = true;
                    }
                    Value::Null => {}
                    _ => *all_int = false,
                }
            }
            Self::Avg { sum, count } => {
                let Some(expr) = function.args.first() else {
                    return Ok(());
                };
                match filter::evaluate_expr_value(
                    row,
                    expr,
                    params,
                    search_context,
                    user_functions,
                    session,
                    None,
                )? {
                    Value::Int64(value) => {
                        *sum += value as f64;
                        *count += 1.0;
                    }
                    Value::Float64(value) => {
                        *sum += value;
                        *count += 1.0;
                    }
                    _ => {}
                }
            }
            Self::MinMax { selected, max } => {
                let Some(expr) = function.args.first() else {
                    return Ok(());
                };
                let value = filter::evaluate_expr_value(
                    row,
                    expr,
                    params,
                    search_context,
                    user_functions,
                    session,
                    None,
                )?;
                if matches!(value, Value::Null) {
                    return Ok(());
                }
                let replace = selected
                    .as_ref()
                    .map(|current| {
                        let current_key = value_sort_key(current);
                        let value_key = value_sort_key(&value);
                        if *max {
                            value_key > current_key
                        } else {
                            value_key < current_key
                        }
                    })
                    .unwrap_or(true);
                if replace {
                    *selected = Some(value);
                }
            }
        }
        Ok(())
    }

    fn merge(&mut self, other: &Self) {
        match (self, other) {
            (Self::Count { count }, Self::Count { count: other }) => *count += other,
            (
                Self::Sum { sum, all_int, seen },
                Self::Sum {
                    sum: other_sum,
                    all_int: other_all_int,
                    seen: other_seen,
                },
            ) => {
                *sum += other_sum;
                *all_int = *all_int && *other_all_int;
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
                    selected: other,
                    max: _,
                },
            ) => {
                if let Some(value) = other {
                    let replace = selected
                        .as_ref()
                        .map(|current| {
                            let current_key = value_sort_key(current);
                            let value_key = value_sort_key(value);
                            if *max {
                                value_key > current_key
                            } else {
                                value_key < current_key
                            }
                        })
                        .unwrap_or(true);
                    if replace {
                        *selected = Some(value.clone());
                    }
                }
            }
            _ => {}
        }
    }

    fn finish(self) -> Value {
        match self {
            Self::Count { count } => Value::Int64(count),
            Self::Sum { sum, all_int, seen } => {
                if !seen {
                    Value::Null
                } else if all_int {
                    Value::Int64(sum as i64)
                } else {
                    Value::Float64(sum)
                }
            }
            Self::Avg { sum, count } => {
                if count == 0.0 {
                    Value::Null
                } else {
                    Value::Float64(sum / count)
                }
            }
            Self::MinMax { selected, .. } => selected.unwrap_or(Value::Null),
        }
    }
}

fn aggregate_group_values(
    row: &BatchRow,
    plan: &LogicalPlan,
    params: &[Value],
    search_context: Option<&filter::SearchContext>,
    user_functions: &HashMap<String, FunctionMeta>,
    session: Option<&CassieSession>,
) -> Result<Vec<(String, Value)>, QueryError> {
    plan.group_by
        .iter()
        .map(|expr| {
            let name = group_expr_name(expr);
            let value = filter::evaluate_expr_value(
                row,
                expr,
                params,
                search_context,
                user_functions,
                session,
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
) -> Result<(), String> {
    if plan.distinct || !plan.distinct_on.is_empty() {
        return Err("distinct".to_string());
    }
    if plan.set.is_some() {
        return Err("set-operation".to_string());
    }
    if plan
        .projection
        .iter()
        .any(|item| matches!(item, SelectItem::WindowFunction { .. }))
    {
        return Err("window-function".to_string());
    }
    if specs.iter().any(|spec| {
        !matches!(
            spec.function.name.to_ascii_lowercase().as_str(),
            "count" | "sum" | "avg" | "min" | "max"
        )
    }) {
        return Err("unsupported-aggregate".to_string());
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
        return Err("unsupported-expression".to_string());
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

pub(super) fn apply_window_functions(
    batches: Vec<Batch>,
    projection: &[SelectItem],
    params: &[Value],
    search_context: Option<&filter::SearchContext>,
    user_functions: &HashMap<String, FunctionMeta>,
    session: Option<&CassieSession>,
) -> Result<Vec<Batch>, QueryError> {
    let windows = projection
        .iter()
        .filter_map(|item| match item {
            SelectItem::WindowFunction { function, alias } => Some((function, alias)),
            _ => None,
        })
        .collect::<Vec<_>>();
    if windows.is_empty() {
        return Ok(batches);
    }

    let mut rows = batch::flatten_batches(batches);
    for (function, alias) in windows {
        let function_name = function.name.to_ascii_lowercase();
        if !matches!(
            function_name.as_str(),
            "row_number" | "rank" | "dense_rank" | "lag" | "lead" | "first_value" | "last_value"
        ) {
            return Err(QueryError::General(format!(
                "unsupported window function '{}'",
                function.name
            )));
        }
        let output_name = alias
            .as_deref()
            .unwrap_or(function.name.as_str())
            .to_string();
        let mut partitions = BTreeMap::<String, Vec<usize>>::new();
        for (index, row) in rows.iter().enumerate() {
            let key = if function.partition_by.is_empty() {
                "__all__".to_string()
            } else {
                function
                    .partition_by
                    .iter()
                    .map(|expr| {
                        filter::evaluate_expr_value(
                            row,
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
                    .join("|")
            };
            partitions.entry(key).or_default().push(index);
        }

        let mut values = vec![Value::Null; rows.len()];
        for indices in partitions.values_mut() {
            indices.sort_by(|left, right| {
                compare_window_rows(
                    &rows[*left],
                    &rows[*right],
                    &function.order_by,
                    params,
                    search_context,
                    user_functions,
                    session,
                )
            });
            let mut dense_rank = 1i64;
            let mut previous_peer_key: Option<String> = None;
            for (position, index) in indices.iter().enumerate() {
                let peer_key = window_peer_key(
                    &rows[*index],
                    &function.order_by,
                    params,
                    search_context,
                    user_functions,
                    session,
                )?;
                if position > 0 && previous_peer_key.as_ref() != Some(&peer_key) {
                    dense_rank += 1;
                }
                previous_peer_key = Some(peer_key);

                values[*index] = match function_name.as_str() {
                    "row_number" => Value::Int64(i64::try_from(position + 1).unwrap_or(i64::MAX)),
                    "rank" => {
                        let peer_position = indices[..=position]
                            .iter()
                            .position(|candidate| {
                                window_peer_key(
                                    &rows[*candidate],
                                    &function.order_by,
                                    params,
                                    search_context,
                                    user_functions,
                                    session,
                                )
                                .ok()
                                    == previous_peer_key
                            })
                            .unwrap_or(position);
                        Value::Int64(i64::try_from(peer_position + 1).unwrap_or(i64::MAX))
                    }
                    "dense_rank" => Value::Int64(dense_rank),
                    "lag" => window_arg_value(
                        indices.get(position.wrapping_sub(1)).copied(),
                        &rows,
                        function,
                        params,
                        search_context,
                        user_functions,
                        session,
                    )?,
                    "lead" => window_arg_value(
                        indices.get(position + 1).copied(),
                        &rows,
                        function,
                        params,
                        search_context,
                        user_functions,
                        session,
                    )?,
                    "first_value" => window_arg_value(
                        indices.first().copied(),
                        &rows,
                        function,
                        params,
                        search_context,
                        user_functions,
                        session,
                    )?,
                    "last_value" => window_arg_value(
                        indices.last().copied(),
                        &rows,
                        function,
                        params,
                        search_context,
                        user_functions,
                        session,
                    )?,
                    _ => Value::Null,
                };
            }
        }

        for (row, value) in rows.iter_mut().zip(values) {
            let mut entries = row.clone().into_entries();
            entries.push((output_name.clone(), value));
            *row = BatchRow::new(entries);
        }
    }

    Ok(batch::chunk_rows(rows, batch::DEFAULT_BATCH_SIZE))
}

fn window_arg_value(
    index: Option<usize>,
    rows: &[BatchRow],
    function: &crate::sql::ast::WindowFunctionCall,
    params: &[Value],
    search_context: Option<&filter::SearchContext>,
    user_functions: &HashMap<String, FunctionMeta>,
    session: Option<&CassieSession>,
) -> Result<Value, QueryError> {
    let Some(index) = index else {
        return Ok(Value::Null);
    };
    let Some(expr) = function.args.first() else {
        return Ok(Value::Null);
    };
    filter::evaluate_expr_value(
        &rows[index],
        expr,
        params,
        search_context,
        user_functions,
        session,
        None,
    )
}

fn window_peer_key(
    row: &BatchRow,
    order_by: &[crate::sql::ast::OrderExpr],
    params: &[Value],
    search_context: Option<&filter::SearchContext>,
    user_functions: &HashMap<String, FunctionMeta>,
    session: Option<&CassieSession>,
) -> Result<String, QueryError> {
    if order_by.is_empty() {
        return Ok("__all__".to_string());
    }
    order_by
        .iter()
        .map(|order| {
            filter::evaluate_expr_value(
                row,
                &order.expr,
                params,
                search_context,
                user_functions,
                session,
                None,
            )
            .map(|value| value_sort_key(&value))
        })
        .collect::<Result<Vec<_>, _>>()
        .map(|parts| parts.join("|"))
}

fn compare_window_rows(
    left: &BatchRow,
    right: &BatchRow,
    order_by: &[crate::sql::ast::OrderExpr],
    params: &[Value],
    search_context: Option<&filter::SearchContext>,
    user_functions: &HashMap<String, FunctionMeta>,
    session: Option<&CassieSession>,
) -> CmpOrdering {
    for order in order_by {
        let left_value = filter::evaluate_expr_value(
            left,
            &order.expr,
            params,
            search_context,
            user_functions,
            session,
            None,
        )
        .unwrap_or(Value::Null);
        let right_value = filter::evaluate_expr_value(
            right,
            &order.expr,
            params,
            search_context,
            user_functions,
            session,
            None,
        )
        .unwrap_or(Value::Null);
        let cmp = compare_query_values(&left_value, &right_value);
        if cmp != CmpOrdering::Equal {
            return match order.direction {
                SortDirection::Asc => cmp,
                SortDirection::Desc => cmp.reverse(),
            };
        }
    }
    batch::row_tie_key(left).cmp(&batch::row_tie_key(right))
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
    params: &[Value],
    search_context: Option<&filter::SearchContext>,
    user_functions: &HashMap<String, FunctionMeta>,
    session: Option<&CassieSession>,
) -> Result<Value, QueryError> {
    let name = function.name.to_ascii_lowercase();
    match name.as_str() {
        "count" => Ok(Value::Int64(count_aggregate(
            function,
            rows,
            params,
            search_context,
            user_functions,
            session,
        )?)),
        "sum" => sum_aggregate(
            function,
            rows,
            params,
            search_context,
            user_functions,
            session,
        ),
        "avg" => avg_aggregate(
            function,
            rows,
            params,
            search_context,
            user_functions,
            session,
        ),
        "min" => minmax_aggregate(
            function,
            rows,
            params,
            search_context,
            user_functions,
            false,
            session,
        ),
        "max" => minmax_aggregate(
            function,
            rows,
            params,
            search_context,
            user_functions,
            true,
            session,
        ),
        _ => Ok(Value::Null),
    }
}

fn count_aggregate(
    function: &FunctionCall,
    rows: &[BatchRow],
    params: &[Value],
    search_context: Option<&filter::SearchContext>,
    user_functions: &HashMap<String, FunctionMeta>,
    session: Option<&CassieSession>,
) -> Result<i64, QueryError> {
    if matches!(function.args.as_slice(), [Expr::Column(name)] if name == "*") {
        return Ok(rows.len() as i64);
    }
    let mut count = 0i64;
    for row in rows {
        let value = filter::evaluate_expr_value(
            row,
            &function.args[0],
            params,
            search_context,
            user_functions,
            session,
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
    params: &[Value],
    search_context: Option<&filter::SearchContext>,
    user_functions: &HashMap<String, FunctionMeta>,
    session: Option<&CassieSession>,
) -> Result<Value, QueryError> {
    let mut sum = 0.0;
    let mut all_int = true;
    let mut seen = false;
    for row in rows {
        match filter::evaluate_expr_value(
            row,
            &function.args[0],
            params,
            search_context,
            user_functions,
            session,
            None,
        )? {
            Value::Int64(value) => {
                sum += value as f64;
                seen = true;
            }
            Value::Float64(value) => {
                sum += value;
                all_int = false;
                seen = true;
            }
            Value::Null => {}
            _ => all_int = false,
        }
    }
    if !seen {
        return Ok(Value::Null);
    }
    if all_int {
        Ok(Value::Int64(sum as i64))
    } else {
        Ok(Value::Float64(sum))
    }
}

fn avg_aggregate(
    function: &FunctionCall,
    rows: &[BatchRow],
    params: &[Value],
    search_context: Option<&filter::SearchContext>,
    user_functions: &HashMap<String, FunctionMeta>,
    session: Option<&CassieSession>,
) -> Result<Value, QueryError> {
    let mut sum = 0.0;
    let mut count = 0.0;
    for row in rows {
        match filter::evaluate_expr_value(
            row,
            &function.args[0],
            params,
            search_context,
            user_functions,
            session,
            None,
        )? {
            Value::Int64(value) => {
                sum += value as f64;
                count += 1.0;
            }
            Value::Float64(value) => {
                sum += value;
                count += 1.0;
            }
            _ => {}
        }
    }
    if count == 0.0 {
        Ok(Value::Null)
    } else {
        Ok(Value::Float64(sum / count))
    }
}

fn minmax_aggregate(
    function: &FunctionCall,
    rows: &[BatchRow],
    params: &[Value],
    search_context: Option<&filter::SearchContext>,
    user_functions: &HashMap<String, FunctionMeta>,
    max: bool,
    session: Option<&CassieSession>,
) -> Result<Value, QueryError> {
    let mut selected: Option<Value> = None;
    for row in rows {
        let value = filter::evaluate_expr_value(
            row,
            &function.args[0],
            params,
            search_context,
            user_functions,
            session,
            None,
        )?;
        if matches!(value, Value::Null) {
            continue;
        }
        let replace = selected
            .as_ref()
            .map(|current| {
                let current_key = value_sort_key(current);
                let value_key = value_sort_key(&value);
                if max {
                    value_key > current_key
                } else {
                    value_key < current_key
                }
            })
            .unwrap_or(true);
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
