use crate::types::semantic_index::KeyedMap;
use std::cmp::Ordering as CmpOrdering;
use std::collections::HashMap;

use crate::app::CassieSession;
use crate::catalog::FunctionMeta;
use crate::executor::batch::{self, Batch, BatchRow};
use crate::executor::filter;
use crate::executor::semantic::{SemanticKey, SemanticValue};
use crate::runtime::QueryExecutionControls;
use crate::sql::ast::{
    NullsOrder, SelectItem, SortDirection, WindowFrame, WindowFrameBound, WindowFrameExclusion,
    WindowFrameUnit, WindowFunctionCall,
};
use crate::types::Value;

use super::{check_timeout, QueryError};

mod admitted;
mod aggregate;
mod exists;
mod keys;
#[cfg(test)]
mod keys_owner_tests;
mod stable_order;
use admitted::AdmittedWindowBuffer;
use keys::{
    compare_window_sort_keys, window_peer_key, window_sort_key, WindowSortKey, WindowSortPart,
};
mod ranking;
mod typed;
mod value_functions;
pub(super) use exists::apply_resolving;

pub(super) fn apply_window_functions(
    batches: Vec<Batch>,
    projection: &[SelectItem],
    params: &[Value],
    search_context: Option<&filter::SearchContext>,
    user_functions: &HashMap<String, FunctionMeta>,
    session: Option<&CassieSession>,
    controls: &QueryExecutionControls,
) -> Result<Vec<Batch>, QueryError> {
    typed::apply(
        batches,
        projection,
        &WindowExecutionContext {
            params,
            search_context,
            user_functions,
            session,
            controls,
            evaluate: None,
            output_memory: None,
        },
    )
}

fn apply_scalar_window_functions(
    batches: Vec<Batch>,
    projection: &[SelectItem],
    context: &WindowExecutionContext<'_>,
) -> Result<Vec<Batch>, QueryError> {
    check_timeout(context.controls)?;
    let windows = collect_window_functions(projection);
    if windows.is_empty() {
        return Ok(batches);
    }

    let mut rows = batch::flatten_batches(batches);
    let _rows_memory = context
        .controls
        .reserve_query_memory(batch_rows_bytes(&rows))?;
    for (window_index, function, alias) in windows {
        let output_name = crate::executor::projection::window_value_key(window_index);
        apply_single_window(
            &mut rows,
            function,
            &output_name,
            alias.as_deref().unwrap_or(function.name.as_str()),
            context,
        )?;
    }

    Ok(batch::chunk_rows(rows, batch::DEFAULT_BATCH_SIZE))
}

type PhaseEvaluator<'a> =
    dyn Fn(&BatchRow, &crate::sql::ast::Expr) -> Result<Value, QueryError> + 'a;

#[derive(Clone, Copy)]
struct WindowExecutionContext<'a> {
    params: &'a [Value],
    search_context: Option<&'a filter::SearchContext>,
    user_functions: &'a HashMap<String, FunctionMeta>,
    session: Option<&'a CassieSession>,
    controls: &'a QueryExecutionControls,
    evaluate: Option<&'a PhaseEvaluator<'a>>,
    output_memory: Option<&'a filter::EvaluationMemory>,
}

fn collect_window_functions(
    projection: &[SelectItem],
) -> Vec<(usize, &WindowFunctionCall, &Option<String>)> {
    let mut window_index = 0;
    projection
        .iter()
        .filter_map(|item| match item {
            SelectItem::WindowFunction { function, alias } => {
                let index = window_index;
                window_index += 1;
                Some((index, function, alias))
            }
            _ => None,
        })
        .collect()
}

fn apply_single_window(
    rows: &mut [BatchRow],
    function: &WindowFunctionCall,
    output_name: &str,
    output_alias: &str,
    context: &WindowExecutionContext<'_>,
) -> Result<(), QueryError> {
    let function_name = validated_window_function_name(function)?;
    let mut partitions = partition_window_rows(rows, function, context)?;
    let _partition_memory = context
        .controls
        .reserve_query_memory(window_partition_bytes(&partitions))?;
    let (values, mut values_memory) =
        evaluate_window_values(rows, function, &function_name, &mut partitions, context)?;
    values_memory.try_grow(
        values.len().saturating_mul(
            std::mem::size_of::<(String, Value)>()
                .saturating_add(output_name.len())
                .saturating_add(std::mem::size_of::<(String, usize)>())
                .saturating_add(output_alias.len()),
        ),
    )?;
    let data_type = match function_name.as_str() {
        "ntile" => crate::types::DataType::Int,
        "percent_rank" | "cume_dist" => crate::types::DataType::Float,
        "first_value" | "last_value" | "lag" | "lead" | "nth_value" => rows
            .first()
            .zip(function.args.first())
            .and_then(|(row, expr)| {
                crate::executor::array_order::expression_type(row, expr, context.user_functions)
            })
            .unwrap_or(crate::types::DataType::Null),
        _ if aggregate::selected(function) => aggregate::data_type(rows, function, context),
        _ => crate::types::DataType::BigInt,
    };
    values_memory.try_grow(aggregate::metadata_bytes(rows, &data_type)?)?;
    values_memory.try_grow(
        std::mem::size_of::<crate::runtime::QueryMemoryReservation>()
            + 2 * std::mem::size_of::<usize>(),
    )?;
    let values_memory = std::sync::Arc::new(values_memory);
    append_window_values(rows, output_name, output_alias, values, &data_type)?;
    for row in rows {
        row.attach_operator_memory(context.controls, std::sync::Arc::clone(&values_memory))?;
    }
    Ok(())
}

fn batch_rows_bytes(rows: &[BatchRow]) -> usize {
    rows.iter()
        .map(|row| {
            serde_json::to_vec(row.entries())
                .map(|bytes| bytes.len())
                .unwrap_or_default()
        })
        .sum::<usize>()
        .saturating_add(batch::row_type_bytes(rows))
}

fn window_partition_bytes(partitions: &KeyedMap<SemanticKey, Vec<usize>>) -> usize {
    partitions
        .iter()
        .map(|(key, indices)| {
            key.estimated_bytes()
                .saturating_add(indices.len().saturating_mul(std::mem::size_of::<usize>()))
        })
        .sum()
}

fn validated_window_function_name(function: &WindowFunctionCall) -> Result<String, QueryError> {
    let function_name = function.name.to_ascii_lowercase();
    let function_name = function_name
        .strip_prefix("pg_catalog.")
        .unwrap_or(&function_name)
        .to_owned();
    if aggregate::selected(function) {
        return Ok(function_name);
    }
    if matches!(
        function_name.as_str(),
        "row_number"
            | "rank"
            | "dense_rank"
            | "percent_rank"
            | "cume_dist"
            | "ntile"
            | "lag"
            | "lead"
            | "first_value"
            | "last_value"
            | "nth_value"
            | "count"
            | "sum"
            | "bool_and"
            | "bool_or"
            | "var_pop"
            | "var_samp"
            | "variance"
            | "stddev_pop"
            | "stddev_samp"
            | "stddev"
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
) -> Result<KeyedMap<SemanticKey, Vec<usize>>, QueryError> {
    let mut partitions = KeyedMap::<SemanticKey, Vec<usize>>::new(Some(context.controls))?;
    for (index, row) in rows.iter().enumerate() {
        check_timeout(context.controls)?;
        let key = partition_key(row, function, context)?;
        partitions.entry(key)?.or_default().push(index);
    }
    Ok(partitions)
}

fn partition_key(
    row: &BatchRow,
    function: &WindowFunctionCall,
    context: &WindowExecutionContext<'_>,
) -> Result<SemanticKey, QueryError> {
    if function.partition_by.is_empty() {
        return Ok(SemanticKey::default());
    }
    let values = function
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
        })
        .collect::<Result<Vec<_>, _>>()?;
    let keys = function
        .partition_by
        .iter()
        .zip(&values)
        .map(|(expr, value)| {
            crate::executor::array_order::key(
                row,
                expr,
                value,
                context.user_functions,
                Some(context.controls),
            )
        })
        .collect::<Result<Vec<_>, _>>()?;
    Ok(SemanticKey::from_semantic_values(keys))
}

fn evaluate_window_values(
    rows: &[BatchRow],
    function: &WindowFunctionCall,
    function_name: &str,
    partitions: &mut KeyedMap<SemanticKey, Vec<usize>>,
    context: &WindowExecutionContext<'_>,
) -> Result<(Vec<Value>, crate::runtime::QueryMemoryReservation), QueryError> {
    let memory = filter::EvaluationMemory::new(context.controls)?;
    memory.grow(crate::executor::retained_memory::mul(
        rows.len(),
        std::mem::size_of::<Value>(),
    )?)?;
    let context = WindowExecutionContext {
        output_memory: Some(&memory),
        ..*context
    };
    let mut values = vec![Value::Null; rows.len()];
    for indices in partitions.values_mut() {
        check_timeout(context.controls)?;
        ranking::evaluate_partition(
            rows,
            function,
            function_name,
            indices,
            &mut values,
            &context,
        )?;
    }
    Ok((values, memory.into_reservation()))
}

fn sort_partition(
    rows: &[BatchRow],
    function: &WindowFunctionCall,
    indices: &mut [usize],
    context: &WindowExecutionContext<'_>,
) -> Result<(), QueryError> {
    sort_partition_observing(rows, function, indices, context, |_| {})
}

fn sort_partition_observing(
    rows: &[BatchRow],
    function: &WindowFunctionCall,
    indices: &mut [usize],
    context: &WindowExecutionContext<'_>,
    mut observe_key: impl FnMut(usize),
) -> Result<(), QueryError> {
    let mut keyed = AdmittedWindowBuffer::new(indices.len(), context.controls)?;
    for (position, index) in indices.iter().enumerate() {
        check_timeout(context.controls)?;
        let key = window_sort_key(&rows[*index], &function.order_by, context)?;
        keyed.values.push((key, *index));
        observe_key(position);
        check_timeout(context.controls)?;
    }
    let order = stable_order::order_by(keyed.values.len(), context.controls, |left, right| {
        Ok(compare_window_sort_keys(
            &keyed.values[left].0,
            &keyed.values[right].0,
        ))
    })?;
    for (target, position) in indices.iter_mut().zip(&order.values) {
        *target = keyed.values[*position].1;
    }
    Ok(())
}

#[derive(Clone, Copy)]
struct WindowValuePosition<'a> {
    position: usize,
    rank: i64,
    dense_rank: i64,
    peer_end: usize,
    partition_len: usize,
    ntile_buckets: Option<usize>,
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
        "rank" => Ok(Value::Int64(value_position.rank)),
        "dense_rank" => Ok(Value::Int64(value_position.dense_rank)),
        "percent_rank" => ranking::percent_rank_value(value_position),
        "cume_dist" => ranking::cume_dist_value(value_position),
        "ntile" => ranking::ntile_value_at(value_position),
        "lag" => value_functions::offset_window_arg_value(
            value_position.position,
            indices,
            rows,
            function,
            false,
            context,
        ),
        "lead" => value_functions::offset_window_arg_value(
            value_position.position,
            indices,
            rows,
            function,
            true,
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
        "nth_value" => value_functions::nth_window_arg_value(
            value_position.position,
            indices,
            rows,
            function,
            value_position.frame,
            context,
        ),
        _ if aggregate::selected(function) => {
            let positions = frame_positions(
                value_position.position,
                indices,
                rows,
                function,
                value_position.frame,
                context,
            )?;
            aggregate::evaluate(
                positions.iter().map(|position| &rows[indices[*position]]),
                function,
                context,
            )
        }
        _ => Ok(Value::Null),
    }
}

/// The explicit frame, or PostgreSQL's default `RANGE BETWEEN UNBOUNDED
/// PRECEDING AND CURRENT ROW` (through the last ORDER BY peer), which covers
/// the whole partition when there is no ORDER BY.
fn effective_window_frame(function: &WindowFunctionCall) -> WindowFrame {
    function.frame.clone().unwrap_or(WindowFrame {
        unit: WindowFrameUnit::Range,
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

struct FramePositions {
    positions: Vec<usize>,
    _memory: crate::runtime::QueryMemoryReservation,
}

impl std::ops::Deref for FramePositions {
    type Target = [usize];

    fn deref(&self) -> &Self::Target {
        &self.positions
    }
}

fn frame_positions(
    position: usize,
    indices: &[usize],
    rows: &[BatchRow],
    function: &WindowFunctionCall,
    frame: &WindowFrame,
    context: &WindowExecutionContext<'_>,
) -> Result<FramePositions, QueryError> {
    check_timeout(context.controls)?;
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
        return Ok(FramePositions {
            positions: Vec::new(),
            _memory: context.controls.reserve_query_memory(0)?,
        });
    };
    let count = end
        .checked_sub(start)
        .and_then(|n| n.checked_add(1))
        .ok_or_else(|| {
            crate::app::CassieError::ResourceLimit("window frame size overflow".into())
        })?;
    let admission = context.controls.reserve_query_memory(0)?;
    let (mut positions, memory) = crate::runtime::owned_memory::try_reserve_vec(count, &admission)?;
    let current_key = window_peer_key(&rows[indices[position]], &function.order_by, context)?;
    for candidate in start..=end {
        check_timeout(context.controls)?;
        let is_current = candidate == position;
        let is_peer =
            window_peer_key(&rows[indices[candidate]], &function.order_by, context)? == current_key;
        if match frame.exclusion {
            WindowFrameExclusion::NoOthers => true,
            WindowFrameExclusion::CurrentRow => !is_current,
            WindowFrameExclusion::Group => !is_peer,
            WindowFrameExclusion::Ties => !is_peer || is_current,
        } {
            positions.push(candidate);
        }
        #[cfg(test)]
        frame_resource_tests::after_candidate();
    }
    check_timeout(context.controls)?;
    Ok(FramePositions {
        positions,
        _memory: memory,
    })
}

struct PeerGroups {
    groups: Vec<(usize, usize)>,
    _memory: crate::runtime::QueryMemoryReservation,
}

impl std::ops::Deref for PeerGroups {
    type Target = [(usize, usize)];

    fn deref(&self) -> &Self::Target {
        &self.groups
    }
}

fn peer_groups(
    indices: &[usize],
    rows: &[BatchRow],
    function: &WindowFunctionCall,
    context: &WindowExecutionContext<'_>,
) -> Result<PeerGroups, QueryError> {
    check_timeout(context.controls)?;
    let admission = context.controls.reserve_query_memory(0)?;
    let (mut groups, memory) =
        crate::runtime::owned_memory::try_reserve_vec(indices.len(), &admission)?;
    let mut start = 0;
    for position in 1..indices.len() {
        check_timeout(context.controls)?;
        let previous = window_peer_key(&rows[indices[position - 1]], &function.order_by, context)?;
        let current = window_peer_key(&rows[indices[position]], &function.order_by, context)?;
        if previous != current {
            groups.push((start, position - 1));
            start = position;
        }
        #[cfg(test)]
        frame_resource_tests::after_candidate();
    }
    check_timeout(context.controls)?;
    if !indices.is_empty() {
        groups.push((start, indices.len() - 1));
    }
    Ok(PeerGroups {
        groups,
        _memory: memory,
    })
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
        })
        .collect::<Result<Vec<_>, _>>()?;
    if matches!(values[position], Value::Null) {
        let null_frame = WindowFrame {
            unit: frame.unit,
            start: range_null_bound(frame.start),
            end: range_null_bound(frame.end),
            exclusion: frame.exclusion,
        };
        return peer_range_bounds(position, indices.len(), &groups, &null_frame);
    }
    if values
        .iter()
        .all(|value| matches!(value, Value::Null | Value::Int64(_)))
    {
        let integer_values = values
            .iter()
            .map(|value| match value {
                Value::Int64(value) => Some(i128::from(*value)),
                Value::Null => None,
                _ => unreachable!("integer RANGE values checked above"),
            })
            .collect::<Vec<_>>();
        return range_offset_bounds(
            position,
            indices.len(),
            &groups,
            frame,
            &order.direction,
            &integer_values,
            i128::from,
        );
    }
    let numeric_values = values
        .iter()
        .map(|value| match value {
            Value::Null => Ok(None),
            _ => value.as_f64().map(Some).ok_or_else(|| {
                QueryError::General("RANGE offset ordering expression must be numeric".to_string())
            }),
        })
        .collect::<Result<Vec<_>, _>>()?;
    range_offset_bounds(
        position,
        indices.len(),
        &groups,
        frame,
        &order.direction,
        &numeric_values,
        |offset| offset.to_string().parse::<f64>().unwrap_or(f64::INFINITY),
    )
}

fn range_offset_bounds<T>(
    position: usize,
    partition_len: usize,
    groups: &[(usize, usize)],
    frame: &WindowFrame,
    direction: &SortDirection,
    values: &[Option<T>],
    offset_value: impl Fn(u64) -> T,
) -> Result<Option<(usize, usize)>, QueryError>
where
    T: Copy + PartialOrd + std::ops::Add<Output = T> + std::ops::Sub<Output = T>,
{
    let current = values[position]
        .ok_or_else(|| QueryError::General("window RANGE value is missing".to_string()))?;
    let signed = |bound: WindowFrameBound| -> Option<T> {
        match bound {
            WindowFrameBound::Preceding(offset) => {
                Some(if matches!(direction, SortDirection::Asc) {
                    current - offset_value(offset)
                } else {
                    current + offset_value(offset)
                })
            }
            WindowFrameBound::Following(offset) => {
                Some(if matches!(direction, SortDirection::Asc) {
                    current + offset_value(offset)
                } else {
                    current - offset_value(offset)
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
        bound => range_boundary(values, signed(bound).unwrap_or(current), direction, true),
    };
    let end = match frame.end {
        WindowFrameBound::UnboundedFollowing => partition_len.saturating_sub(1),
        WindowFrameBound::CurrentRow => groups
            .iter()
            .find(|(start, end)| (*start..=*end).contains(&position))
            .map_or(position, |group| group.1),
        bound => range_boundary(values, signed(bound).unwrap_or(current), direction, false),
    };
    Ok((start <= end && start < partition_len)
        .then_some((start, end.min(partition_len.saturating_sub(1)))))
}

const fn range_null_bound(bound: WindowFrameBound) -> WindowFrameBound {
    match bound {
        WindowFrameBound::Preceding(_) | WindowFrameBound::Following(_) => {
            WindowFrameBound::CurrentRow
        }
        other => other,
    }
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

fn range_boundary<T: Copy + PartialOrd>(
    values: &[Option<T>],
    target: T,
    direction: &SortDirection,
    start: bool,
) -> usize {
    if start {
        values
            .iter()
            .position(|value| {
                value.is_some_and(|value| {
                    if matches!(direction, SortDirection::Asc) {
                        value >= target
                    } else {
                        value <= target
                    }
                })
            })
            .unwrap_or(values.len())
    } else {
        values
            .iter()
            .rposition(|value| {
                value.is_some_and(|value| {
                    if matches!(direction, SortDirection::Asc) {
                        value <= target
                    } else {
                        value >= target
                    }
                })
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

fn append_window_values(
    rows: &mut [BatchRow],
    output_name: &str,
    output_alias: &str,
    values: Vec<Value>,
    data_type: &crate::types::DataType,
) -> Result<(), QueryError> {
    let types: Option<std::sync::Arc<Vec<crate::types::DataType>>> = rows.first().and_then(|row| {
        if row.data_types().is_empty()
            && !matches!(
                data_type,
                crate::types::DataType::Array(_)
                    | crate::types::DataType::Jsonb
                    | crate::types::DataType::Char { .. }
            )
        {
            return None;
        }
        let mut types = row.data_types().to_vec();
        types.resize(row.entries().len(), crate::types::DataType::Null);
        types.push(data_type.clone());
        Some(types.into())
    });
    for (row, value) in rows.iter_mut().zip(values) {
        row.append_value_with_alias(output_name.to_string(), value, output_alias)?;
        if let Some(types) = &types {
            row.set_data_types(types.clone());
        }
    }
    Ok(())
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
    evaluate_window_expr(&rows[index], expr, context)
}

fn evaluate_window_expr(
    row: &BatchRow,
    expression: &crate::sql::ast::Expr,
    context: &WindowExecutionContext<'_>,
) -> Result<Value, QueryError> {
    if let Some(evaluate) = context.evaluate {
        return evaluate(row, expression);
    }
    filter::evaluate_expr_value(
        row,
        expression,
        context.params,
        context.search_context,
        context.user_functions,
        context.session,
        None,
    )
}

#[cfg(test)]
mod typed_tests;

#[cfg(test)]
mod frame_resource_tests;

#[cfg(test)]
mod qualification_tests;

#[cfg(test)]
mod statistical_tests;
