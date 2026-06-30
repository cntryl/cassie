use std::cmp::Ordering as CmpOrdering;
use std::collections::{BTreeMap, HashMap};

use crate::app::CassieSession;
use crate::catalog::FunctionMeta;
use crate::executor::batch::{self, Batch, BatchRow};
use crate::executor::filter;
use crate::sql::ast::{SelectItem, SortDirection, WindowFunctionCall};
use crate::types::Value;

use super::{compare_query_values, value_sort_key, QueryError};

pub(super) fn apply_window_functions(
    batches: Vec<Batch>,
    projection: &[SelectItem],
    params: &[Value],
    search_context: Option<&filter::SearchContext>,
    user_functions: &HashMap<String, FunctionMeta>,
    session: Option<&CassieSession>,
) -> Result<Vec<Batch>, QueryError> {
    let windows = collect_window_functions(projection);
    if windows.is_empty() {
        return Ok(batches);
    }

    let context = WindowExecutionContext {
        params,
        search_context,
        user_functions,
        session,
    };
    let mut rows = batch::flatten_batches(batches);
    for (function, alias) in windows {
        apply_single_window(&mut rows, function, alias.as_deref(), &context)?;
    }

    Ok(batch::chunk_rows(rows, batch::DEFAULT_BATCH_SIZE))
}

struct WindowExecutionContext<'a> {
    params: &'a [Value],
    search_context: Option<&'a filter::SearchContext>,
    user_functions: &'a HashMap<String, FunctionMeta>,
    session: Option<&'a CassieSession>,
}

fn collect_window_functions(
    projection: &[SelectItem],
) -> Vec<(&WindowFunctionCall, &Option<String>)> {
    projection
        .iter()
        .filter_map(|item| match item {
            SelectItem::WindowFunction { function, alias } => Some((function, alias)),
            _ => None,
        })
        .collect()
}

fn apply_single_window(
    rows: &mut [BatchRow],
    function: &WindowFunctionCall,
    alias: Option<&str>,
    context: &WindowExecutionContext<'_>,
) -> Result<(), QueryError> {
    let function_name = validated_window_function_name(function)?;
    let mut partitions = partition_window_rows(rows, function, context)?;
    let values = evaluate_window_values(rows, function, &function_name, &mut partitions, context)?;
    append_window_values(rows, alias.unwrap_or(function.name.as_str()), values);
    Ok(())
}

fn validated_window_function_name(function: &WindowFunctionCall) -> Result<String, QueryError> {
    let function_name = function.name.to_ascii_lowercase();
    if matches!(
        function_name.as_str(),
        "row_number" | "rank" | "dense_rank" | "lag" | "lead" | "first_value" | "last_value"
    ) {
        return Ok(function_name);
    }
    Err(QueryError::General(format!(
        "unsupported window function '{}'",
        function.name
    )))
}

fn partition_window_rows(
    rows: &[BatchRow],
    function: &WindowFunctionCall,
    context: &WindowExecutionContext<'_>,
) -> Result<BTreeMap<String, Vec<usize>>, QueryError> {
    let mut partitions = BTreeMap::<String, Vec<usize>>::new();
    for (index, row) in rows.iter().enumerate() {
        let key = partition_key(row, function, context)?;
        partitions.entry(key).or_default().push(index);
    }
    Ok(partitions)
}

fn partition_key(
    row: &BatchRow,
    function: &WindowFunctionCall,
    context: &WindowExecutionContext<'_>,
) -> Result<String, QueryError> {
    if function.partition_by.is_empty() {
        return Ok("__all__".to_string());
    }
    function
        .partition_by
        .iter()
        .map(|expr| {
            filter::evaluate_expr_value(
                row,
                expr,
                context.params,
                context.search_context,
                context.user_functions,
                context.session,
                None,
            )
            .map(|value| value_sort_key(&value))
        })
        .collect::<Result<Vec<_>, _>>()
        .map(|parts| parts.join("|"))
}

fn evaluate_window_values(
    rows: &[BatchRow],
    function: &WindowFunctionCall,
    function_name: &str,
    partitions: &mut BTreeMap<String, Vec<usize>>,
    context: &WindowExecutionContext<'_>,
) -> Result<Vec<Value>, QueryError> {
    let mut values = vec![Value::Null; rows.len()];
    for indices in partitions.values_mut() {
        evaluate_window_partition(rows, function, function_name, indices, &mut values, context)?;
    }
    Ok(values)
}

fn evaluate_window_partition(
    rows: &[BatchRow],
    function: &WindowFunctionCall,
    function_name: &str,
    indices: &mut [usize],
    values: &mut [Value],
    context: &WindowExecutionContext<'_>,
) -> Result<(), QueryError> {
    sort_partition(rows, function, indices, context);
    let mut dense_rank = 1i64;
    let mut previous_peer_key: Option<String> = None;
    for (position, index) in indices.iter().enumerate() {
        let peer_key = window_peer_key(&rows[*index], &function.order_by, context)?;
        if position > 0 && previous_peer_key.as_ref() != Some(&peer_key) {
            dense_rank += 1;
        }
        previous_peer_key = Some(peer_key);
        values[*index] = window_value_at(
            position,
            dense_rank,
            rows,
            function,
            function_name,
            indices,
            context,
        )?;
    }
    Ok(())
}

fn sort_partition(
    rows: &[BatchRow],
    function: &WindowFunctionCall,
    indices: &mut [usize],
    context: &WindowExecutionContext<'_>,
) {
    indices.sort_by(|left, right| {
        compare_window_rows(&rows[*left], &rows[*right], &function.order_by, context)
    });
}

fn window_value_at(
    position: usize,
    dense_rank: i64,
    rows: &[BatchRow],
    function: &WindowFunctionCall,
    function_name: &str,
    indices: &[usize],
    context: &WindowExecutionContext<'_>,
) -> Result<Value, QueryError> {
    match function_name {
        "row_number" => Ok(Value::Int64(
            i64::try_from(position + 1).unwrap_or(i64::MAX),
        )),
        "rank" => Ok(rank_value(position, rows, function, indices, context)),
        "dense_rank" => Ok(Value::Int64(dense_rank)),
        "lag" => window_arg_value(
            indices.get(position.wrapping_sub(1)).copied(),
            rows,
            function,
            context,
        ),
        "lead" => window_arg_value(indices.get(position + 1).copied(), rows, function, context),
        "first_value" => window_arg_value(indices.first().copied(), rows, function, context),
        "last_value" => window_arg_value(indices.last().copied(), rows, function, context),
        _ => Ok(Value::Null),
    }
}

fn rank_value(
    position: usize,
    rows: &[BatchRow],
    function: &WindowFunctionCall,
    indices: &[usize],
    context: &WindowExecutionContext<'_>,
) -> Value {
    let current_peer_key =
        window_peer_key(&rows[indices[position]], &function.order_by, context).ok();
    let peer_position = indices[..=position]
        .iter()
        .position(|candidate| {
            window_peer_key(&rows[*candidate], &function.order_by, context).ok() == current_peer_key
        })
        .unwrap_or(position);
    Value::Int64(i64::try_from(peer_position + 1).unwrap_or(i64::MAX))
}

fn append_window_values(rows: &mut [BatchRow], output_name: &str, values: Vec<Value>) {
    for (row, value) in rows.iter_mut().zip(values) {
        let mut entries = row.clone().into_entries();
        entries.push((output_name.to_string(), value));
        *row = BatchRow::new(entries);
    }
}

fn window_arg_value(
    index: Option<usize>,
    rows: &[BatchRow],
    function: &WindowFunctionCall,
    context: &WindowExecutionContext<'_>,
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
        context.params,
        context.search_context,
        context.user_functions,
        context.session,
        None,
    )
}

fn window_peer_key(
    row: &BatchRow,
    order_by: &[crate::sql::ast::OrderExpr],
    context: &WindowExecutionContext<'_>,
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
                context.params,
                context.search_context,
                context.user_functions,
                context.session,
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
    context: &WindowExecutionContext<'_>,
) -> CmpOrdering {
    for order in order_by {
        let left_value = filter::evaluate_expr_value(
            left,
            &order.expr,
            context.params,
            context.search_context,
            context.user_functions,
            context.session,
            None,
        )
        .unwrap_or(Value::Null);
        let right_value = filter::evaluate_expr_value(
            right,
            &order.expr,
            context.params,
            context.search_context,
            context.user_functions,
            context.session,
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
