use std::cmp::Ordering as CmpOrdering;
use std::collections::{BTreeMap, HashMap};

use crate::app::CassieSession;
use crate::catalog::FunctionMeta;
use crate::executor::batch::{self, Batch, BatchRow};
use crate::executor::filter;
use crate::runtime::QueryExecutionControls;
use crate::sql::ast::{
    SelectItem, SortDirection, WindowFrame, WindowFrameBound, WindowFrameUnit, WindowFunctionCall,
};
use crate::types::Value;

use super::{check_timeout, compare_query_values, value_sort_key, QueryError};

pub(super) fn apply_window_functions(
    batches: Vec<Batch>,
    projection: &[SelectItem],
    params: &[Value],
    search_context: Option<&filter::SearchContext>,
    user_functions: &HashMap<String, FunctionMeta>,
    session: Option<&CassieSession>,
    controls: &QueryExecutionControls,
) -> Result<Vec<Batch>, QueryError> {
    check_timeout(controls)?;
    let windows = collect_window_functions(projection);
    if windows.is_empty() {
        return Ok(batches);
    }

    let context = WindowExecutionContext {
        params,
        search_context,
        user_functions,
        session,
        controls,
    };
    let mut rows = batch::flatten_batches(batches);
    let _rows_memory = context
        .controls
        .reserve_query_memory(batch_rows_bytes(&rows))?;
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
    controls: &'a QueryExecutionControls,
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
    let _partition_memory = context
        .controls
        .reserve_query_memory(window_partition_bytes(&partitions))?;
    let values = evaluate_window_values(rows, function, &function_name, &mut partitions, context)?;
    let _value_memory = context
        .controls
        .reserve_query_memory(values.len().saturating_mul(std::mem::size_of::<Value>()))?;
    append_window_values(rows, alias.unwrap_or(function.name.as_str()), values);
    Ok(())
}

fn batch_rows_bytes(rows: &[BatchRow]) -> usize {
    rows.iter()
        .map(|row| {
            serde_json::to_vec(row.entries())
                .map(|bytes| bytes.len())
                .unwrap_or_default()
        })
        .sum()
}

fn window_partition_bytes(partitions: &BTreeMap<String, Vec<usize>>) -> usize {
    partitions
        .iter()
        .map(|(key, indices)| {
            key.len()
                .saturating_add(indices.len().saturating_mul(std::mem::size_of::<usize>()))
        })
        .sum()
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
        check_timeout(context.controls)?;
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
        check_timeout(context.controls)?;
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
    let frame = effective_window_frame(function);
    let mut dense_rank = 1i64;
    let mut previous_peer_key: Option<String> = None;
    for (position, index) in indices.iter().enumerate() {
        check_timeout(context.controls)?;
        let peer_key = window_peer_key(&rows[*index], &function.order_by, context)?;
        if position > 0 && previous_peer_key.as_ref() != Some(&peer_key) {
            dense_rank += 1;
        }
        previous_peer_key = Some(peer_key);
        values[*index] = window_value_at(
            WindowValuePosition {
                position,
                dense_rank,
                frame: &frame,
            },
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

#[derive(Clone, Copy)]
struct WindowValuePosition<'a> {
    position: usize,
    dense_rank: i64,
    frame: &'a WindowFrame,
}

fn window_value_at(
    value_position: WindowValuePosition<'_>,
    rows: &[BatchRow],
    function: &WindowFunctionCall,
    function_name: &str,
    indices: &[usize],
    context: &WindowExecutionContext<'_>,
) -> Result<Value, QueryError> {
    match function_name {
        "row_number" => Ok(Value::Int64(
            i64::try_from(value_position.position + 1).unwrap_or(i64::MAX),
        )),
        "rank" => Ok(rank_value(
            value_position.position,
            rows,
            function,
            indices,
            context,
        )),
        "dense_rank" => Ok(Value::Int64(value_position.dense_rank)),
        "lag" => window_arg_value(
            indices
                .get(value_position.position.wrapping_sub(1))
                .copied(),
            rows,
            function,
            context,
        ),
        "lead" => window_arg_value(
            indices.get(value_position.position + 1).copied(),
            rows,
            function,
            context,
        ),
        "first_value" => framed_window_arg_value(
            value_position.position,
            indices,
            rows,
            function,
            value_position.frame,
            true,
            context,
        ),
        "last_value" => framed_window_arg_value(
            value_position.position,
            indices,
            rows,
            function,
            value_position.frame,
            false,
            context,
        ),
        _ => Ok(Value::Null),
    }
}

fn effective_window_frame(function: &WindowFunctionCall) -> WindowFrame {
    function.frame.clone().unwrap_or(WindowFrame {
        unit: WindowFrameUnit::Rows,
        start: WindowFrameBound::UnboundedPreceding,
        end: if function.order_by.is_empty() {
            WindowFrameBound::UnboundedFollowing
        } else {
            WindowFrameBound::CurrentRow
        },
    })
}

fn framed_window_arg_value(
    position: usize,
    indices: &[usize],
    rows: &[BatchRow],
    function: &WindowFunctionCall,
    frame: &WindowFrame,
    first: bool,
    context: &WindowExecutionContext<'_>,
) -> Result<Value, QueryError> {
    if !matches!(frame.unit, WindowFrameUnit::Rows) {
        return Err(QueryError::General(
            "unsupported window frame unit".to_string(),
        ));
    }
    let Some((start, end)) = frame_row_bounds(position, indices.len(), frame) else {
        return Ok(Value::Null);
    };
    let target = if first { start } else { end };
    window_arg_value(Some(indices[target]), rows, function, context)
}

fn frame_row_bounds(
    position: usize,
    partition_len: usize,
    frame: &WindowFrame,
) -> Option<(usize, usize)> {
    if partition_len == 0 {
        return None;
    }
    let start = resolve_frame_bound(frame.start, position, partition_len);
    let end = resolve_frame_bound(frame.end, position, partition_len);
    let partition_len = i128::try_from(partition_len).ok()?;
    if start > end || start >= partition_len || end < 0 {
        return None;
    }
    let start = usize::try_from(start.max(0)).ok()?;
    let end = usize::try_from(end.min(partition_len - 1)).ok()?;
    Some((start, end))
}

fn resolve_frame_bound(bound: WindowFrameBound, position: usize, partition_len: usize) -> i128 {
    let position = i128::try_from(position).unwrap_or(i128::MAX);
    let partition_len = i128::try_from(partition_len).unwrap_or(i128::MAX);
    match bound {
        WindowFrameBound::UnboundedPreceding => 0,
        WindowFrameBound::Preceding(offset) => position - i128::from(offset),
        WindowFrameBound::CurrentRow => position,
        WindowFrameBound::Following(offset) => position + i128::from(offset),
        WindowFrameBound::UnboundedFollowing => partition_len,
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
