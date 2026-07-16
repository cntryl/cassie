use std::cmp::Ordering as CmpOrdering;
use std::collections::{BTreeMap, HashMap};

use crate::app::CassieSession;
use crate::catalog::FunctionMeta;
use crate::executor::batch::{self, Batch, BatchRow};
use crate::executor::filter;
use crate::runtime::QueryExecutionControls;
use crate::sql::ast::{
    SelectItem, SortDirection, WindowFrame, WindowFrameBound, WindowFrameExclusion,
    WindowFrameUnit, WindowFunctionCall,
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
        exclusion: WindowFrameExclusion::NoOthers,
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
    let positions = frame_positions(position, indices, rows, function, frame, context)?;
    let Some(target) = (if first {
        positions.first()
    } else {
        positions.last()
    }) else {
        return Ok(Value::Null);
    };
    window_arg_value(Some(indices[*target]), rows, function, context)
}

fn frame_positions(
    position: usize,
    indices: &[usize],
    rows: &[BatchRow],
    function: &WindowFunctionCall,
    frame: &WindowFrame,
    context: &WindowExecutionContext<'_>,
) -> Result<Vec<usize>, QueryError> {
    let bounds = match frame.unit {
        WindowFrameUnit::Rows => frame_row_bounds(position, indices.len(), frame),
        WindowFrameUnit::Groups => {
            frame_group_bounds(position, indices, rows, function, frame, context)?
        }
        WindowFrameUnit::Range => {
            frame_range_bounds(position, indices, rows, function, frame, context)?
        }
    };
    let Some((start, end)) = bounds else {
        return Ok(Vec::new());
    };
    let current_key = window_peer_key(&rows[indices[position]], &function.order_by, context)?;
    Ok((start..=end)
        .filter(|candidate| {
            let is_current = *candidate == position;
            let is_peer = window_peer_key(&rows[indices[*candidate]], &function.order_by, context)
                .is_ok_and(|key| key == current_key);
            match frame.exclusion {
                WindowFrameExclusion::NoOthers => true,
                WindowFrameExclusion::CurrentRow => !is_current,
                WindowFrameExclusion::Group => !is_peer,
                WindowFrameExclusion::Ties => !is_peer || is_current,
            }
        })
        .collect())
}

fn peer_groups(
    indices: &[usize],
    rows: &[BatchRow],
    function: &WindowFunctionCall,
    context: &WindowExecutionContext<'_>,
) -> Result<Vec<(usize, usize)>, QueryError> {
    let mut groups = Vec::new();
    let mut start = 0;
    for position in 1..indices.len() {
        let previous = window_peer_key(&rows[indices[position - 1]], &function.order_by, context)?;
        let current = window_peer_key(&rows[indices[position]], &function.order_by, context)?;
        if previous != current {
            groups.push((start, position - 1));
            start = position;
        }
    }
    if !indices.is_empty() {
        groups.push((start, indices.len() - 1));
    }
    Ok(groups)
}

fn frame_group_bounds(
    position: usize,
    indices: &[usize],
    rows: &[BatchRow],
    function: &WindowFunctionCall,
    frame: &WindowFrame,
    context: &WindowExecutionContext<'_>,
) -> Result<Option<(usize, usize)>, QueryError> {
    let groups = peer_groups(indices, rows, function, context)?;
    let current = groups
        .iter()
        .position(|(start, end)| (*start..=*end).contains(&position))
        .ok_or_else(|| QueryError::General("window peer group is missing".to_string()))?;
    let group_frame = WindowFrame {
        unit: WindowFrameUnit::Rows,
        start: frame.start,
        end: frame.end,
        exclusion: WindowFrameExclusion::NoOthers,
    };
    Ok(frame_row_bounds(current, groups.len(), &group_frame)
        .map(|(start, end)| (groups[start].0, groups[end].1)))
}

fn frame_range_bounds(
    position: usize,
    indices: &[usize],
    rows: &[BatchRow],
    function: &WindowFunctionCall,
    frame: &WindowFrame,
    context: &WindowExecutionContext<'_>,
) -> Result<Option<(usize, usize)>, QueryError> {
    let groups = peer_groups(indices, rows, function, context)?;
    if !matches!(
        frame.start,
        WindowFrameBound::Preceding(_) | WindowFrameBound::Following(_)
    ) && !matches!(
        frame.end,
        WindowFrameBound::Preceding(_) | WindowFrameBound::Following(_)
    ) {
        return peer_range_bounds(position, indices.len(), &groups, frame);
    }
    if function.order_by.len() != 1 {
        return Err(QueryError::General(
            "RANGE offset frames require exactly one ordering expression".to_string(),
        ));
    }
    let order = &function.order_by[0];
    let values = indices
        .iter()
        .map(|index| {
            filter::evaluate_expr_value(
                &rows[*index],
                &order.expr,
                context.params,
                context.search_context,
                context.user_functions,
                context.session,
                None,
            )
            .and_then(|value| {
                value.as_f64().ok_or_else(|| {
                    QueryError::General(
                        "RANGE offset ordering expression must be numeric".to_string(),
                    )
                })
            })
        })
        .collect::<Result<Vec<_>, _>>()?;
    let current = values[position];
    let signed = |bound: WindowFrameBound| -> Option<f64> {
        match bound {
            WindowFrameBound::Preceding(offset) => {
                offset.to_string().parse::<f64>().ok().map(|offset| {
                    if matches!(order.direction, SortDirection::Asc) {
                        current - offset
                    } else {
                        current + offset
                    }
                })
            }
            WindowFrameBound::Following(offset) => {
                offset.to_string().parse::<f64>().ok().map(|offset| {
                    if matches!(order.direction, SortDirection::Asc) {
                        current + offset
                    } else {
                        current - offset
                    }
                })
            }
            _ => None,
        }
    };
    let start = match frame.start {
        WindowFrameBound::UnboundedPreceding => 0,
        WindowFrameBound::CurrentRow => groups
            .iter()
            .find(|(start, end)| (*start..=*end).contains(&position))
            .map_or(position, |group| group.0),
        bound => range_boundary(
            &values,
            signed(bound).unwrap_or(current),
            &order.direction,
            true,
        ),
    };
    let end = match frame.end {
        WindowFrameBound::UnboundedFollowing => indices.len().saturating_sub(1),
        WindowFrameBound::CurrentRow => groups
            .iter()
            .find(|(start, end)| (*start..=*end).contains(&position))
            .map_or(position, |group| group.1),
        bound => range_boundary(
            &values,
            signed(bound).unwrap_or(current),
            &order.direction,
            false,
        ),
    };
    Ok((start <= end && start < indices.len())
        .then_some((start, end.min(indices.len().saturating_sub(1)))))
}

fn peer_range_bounds(
    position: usize,
    partition_len: usize,
    groups: &[(usize, usize)],
    frame: &WindowFrame,
) -> Result<Option<(usize, usize)>, QueryError> {
    let current = groups
        .iter()
        .position(|(start, end)| (*start..=*end).contains(&position))
        .ok_or_else(|| QueryError::General("window peer group is missing".to_string()))?;
    let start = match frame.start {
        WindowFrameBound::UnboundedPreceding => 0,
        WindowFrameBound::CurrentRow => groups[current].0,
        WindowFrameBound::UnboundedFollowing => partition_len,
        _ => unreachable!(),
    };
    let end = match frame.end {
        WindowFrameBound::UnboundedFollowing => partition_len.saturating_sub(1),
        WindowFrameBound::CurrentRow => groups[current].1,
        WindowFrameBound::UnboundedPreceding => 0,
        _ => unreachable!(),
    };
    Ok((start <= end && start < partition_len).then_some((start, end)))
}

fn range_boundary(values: &[f64], target: f64, direction: &SortDirection, start: bool) -> usize {
    if start {
        values
            .iter()
            .position(|value| {
                if matches!(direction, SortDirection::Asc) {
                    *value >= target
                } else {
                    *value <= target
                }
            })
            .unwrap_or(values.len())
    } else {
        values
            .iter()
            .rposition(|value| {
                if matches!(direction, SortDirection::Asc) {
                    *value <= target
                } else {
                    *value >= target
                }
            })
            .unwrap_or(0)
    }
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
