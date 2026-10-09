//! Selected blocking windows; all eligibility precedes typed conversion.
use std::mem::size_of;
use std::sync::Arc;

use super::{
    apply_scalar_window_functions, batch, check_timeout, collect_window_functions,
    compare_window_sort_keys, frame_row_bounds, Batch, BatchRow, CmpOrdering, QueryError,
    QueryExecutionControls, SelectItem, SemanticValue, Value, WindowExecutionContext,
    WindowFrameExclusion, WindowFrameUnit, WindowFunctionCall, WindowSortKey, WindowSortPart,
};
#[cfg(test)]
use super::{HashMap, SortDirection};
use crate::executor::retained_memory::{
    add, data_type_clone_bytes, lookup_bytes, mul, value_clone_bytes,
};
use crate::executor::sort::accounting;
use crate::executor::typed_batch::{relational_diagnostics, row_bridge, Cell, TypedBatch};
use crate::runtime::QueryMemoryReservation;
use crate::sql::ast::Expr;
use crate::types::DataType;

struct Selection {
    partition: Vec<usize>,
    order: Vec<usize>,
    payload: Option<usize>,
}

struct Keys {
    partition: Vec<SemanticValue>,
    order: WindowSortKey,
}

#[derive(Clone, Copy)]
enum Target {
    Null,
    Rank(i64),
    Row(usize),
}

pub(super) fn apply(
    batches: Vec<Batch>,
    projection: &[SelectItem],
    context: &WindowExecutionContext<'_>,
) -> Result<Vec<Batch>, QueryError> {
    check_timeout(context.controls)?;
    if !projection
        .iter()
        .any(|item| matches!(item, SelectItem::WindowFunction { .. }))
    {
        return Ok(batches);
    }
    if let Some(boundary) = boundary(projection, context.controls) {
        return scalar(batches, projection, context, boundary);
    }
    if batches.iter().all(Vec::is_empty) {
        relational_diagnostics::publish("window", "empty_relation");
        return Ok(Vec::new());
    }
    let (mut rows, mut backing) = flatten(batches, context.controls)?;
    backing.try_grow(input_bytes(&rows, projection)?)?;
    let windows = collect_window_functions(projection);
    let mut selections = Vec::with_capacity(windows.len());
    for (function, _) in &windows {
        let Some(selection) = select(&rows, function, context.controls)? else {
            return scalar(vec![rows], projection, context, "scalar_expression_window");
        };
        selections.push(selection);
    }
    // Only the fully selected projection consumes native input; failures never retry scalar.
    let backing = Arc::new(backing);
    for row in &mut rows {
        row.attach_operator_memory(context.controls, Arc::clone(&backing))?;
    }
    let mut native = true;
    let mut payload = false;
    for ((function, alias), selection) in windows.iter().zip(&selections) {
        let typed = apply_one(&mut rows, function, alias.as_deref(), selection, context)?;
        native &= typed;
        payload |= selection.payload.is_some();
    }
    let output = chunk(rows, context.controls)?;
    check_timeout(context.controls)?;
    relational_diagnostics::publish(
        "window",
        match (native, payload) {
            (true, false) => "native_primitive_keys",
            (true, true) => "native_primitive_keys_scalar_backed_payload",
            (false, false) => "bounded_semantic_keys",
            (false, true) => "bounded_semantic_keys_scalar_backed_payload",
        },
    );
    Ok(output)
}

fn scalar(
    batches: Vec<Batch>,
    projection: &[SelectItem],
    context: &WindowExecutionContext<'_>,
    path: &'static str,
) -> Result<Vec<Batch>, QueryError> {
    let output = apply_scalar_window_functions(batches, projection, context)?;
    relational_diagnostics::publish("window", path);
    Ok(output)
}

fn kind(function: &WindowFunctionCall) -> Option<&'static str> {
    [
        "row_number",
        "rank",
        "dense_rank",
        "lag",
        "lead",
        "first_value",
        "last_value",
    ]
    .into_iter()
    .find(|name| function.name.eq_ignore_ascii_case(name))
}

fn boundary(projection: &[SelectItem], controls: &QueryExecutionControls) -> Option<&'static str> {
    if controls.uses_relational_cte_boundary() {
        return Some("scalar_cte_materialization");
    }
    for item in projection {
        let SelectItem::WindowFunction { function, .. } = item else {
            continue;
        };
        let Some(name) = kind(function) else {
            return Some("scalar_expression_window");
        };
        if function.frame.as_ref().is_some_and(|frame| {
            frame.unit != WindowFrameUnit::Rows || frame.exclusion != WindowFrameExclusion::NoOthers
        }) {
            return Some("scalar_window_frame");
        }
        let value = matches!(name, "first_value" | "last_value" | "lag" | "lead");
        if (value && (function.args.len() != 1 || !matches!(function.args[0], Expr::Column(_))))
            || (!value && !function.args.is_empty())
            || function
                .partition_by
                .iter()
                .any(|expr| !matches!(expr, Expr::Column(_)))
            || function
                .order_by
                .iter()
                .any(|order| !matches!(order.expr, Expr::Column(_)))
        {
            return Some("scalar_expression_window");
        }
    }
    None
}

fn flatten(
    batches: Vec<Batch>,
    controls: &QueryExecutionControls,
) -> Result<(Vec<BatchRow>, QueryMemoryReservation), QueryError> {
    let count = batches
        .iter()
        .try_fold(0, |count, batch| add(count, batch.len()))?;
    let bytes = batches.iter().try_fold(
        mul(batches.capacity(), size_of::<Batch>())?,
        |bytes, batch| add(bytes, mul(batch.capacity(), size_of::<BatchRow>())?),
    )?;
    let bytes = add(
        bytes,
        add(
            mul(count, 2 * size_of::<BatchRow>())?,
            mul(
                count.div_ceil(batch::DEFAULT_BATCH_SIZE),
                size_of::<Batch>(),
            )?,
        )?,
    )?;
    let memory = controls.reserve_query_memory(add(
        bytes,
        size_of::<QueryMemoryReservation>() + 2 * size_of::<usize>(),
    )?)?;
    let mut rows = Vec::with_capacity(count);
    for row in batches.into_iter().flatten() {
        check_timeout(controls)?;
        rows.push(row);
    }
    Ok((rows, memory))
}

fn input_bytes(
    rows: &[BatchRow],
    projection: &[SelectItem],
) -> Result<usize, crate::app::CassieError> {
    let mut bytes = 0;
    for item in projection {
        if let SelectItem::WindowFunction { function, .. } = item {
            bytes = add(bytes, size_of::<Selection>() + 2 * size_of::<usize>())?;
            for expr in function
                .partition_by
                .iter()
                .chain(function.order_by.iter().map(|order| &order.expr))
                .chain(&function.args)
            {
                if let Expr::Column(name) = expr {
                    bytes = add(bytes, mul(rows.len(), add(512, mul(name.len(), 32)?)?)?)?;
                }
                bytes = add(bytes, size_of::<usize>())?;
            }
        }
    }
    for row in rows {
        let names = row
            .entries()
            .iter()
            .try_fold(0, |bytes, (name, _)| add(bytes, name.len()))?;
        let aliases = row
            .aliases()
            .iter()
            .try_fold(0, |bytes, (name, _)| add(bytes, name.len()))?;
        bytes = add(
            bytes,
            add(
                row.unleased_body_bytes()?,
                lookup_bytes(
                    add(row.entries().len(), row.aliases().len())?,
                    add(names, aliases)?,
                )?,
            )?,
        )?;
    }
    Ok(bytes)
}

fn column(
    rows: &[BatchRow],
    expr: &Expr,
    controls: &QueryExecutionControls,
) -> Result<Option<usize>, QueryError> {
    let Expr::Column(name) = expr else {
        return Ok(None);
    };
    let mut previous = None;
    for row in rows {
        check_timeout(controls)?;
        let Some(value) = row.get(name) else {
            return Ok(None);
        };
        let Some(index) = row
            .entries()
            .iter()
            .position(|(_, cell)| std::ptr::eq(cell, value))
        else {
            return Ok(None);
        };
        if previous.is_some_and(|previous| previous != index) {
            return Ok(None);
        }
        previous = Some(index);
    }
    Ok(previous)
}

fn select(
    rows: &[BatchRow],
    function: &WindowFunctionCall,
    controls: &QueryExecutionControls,
) -> Result<Option<Selection>, QueryError> {
    let mut partition = Vec::with_capacity(function.partition_by.len());
    let mut order = Vec::with_capacity(function.order_by.len());
    for expr in &function.partition_by {
        let Some(index) = column(rows, expr, controls)? else {
            return Ok(None);
        };
        partition.push(index);
    }
    for item in &function.order_by {
        let Some(index) = column(rows, &item.expr, controls)? else {
            return Ok(None);
        };
        order.push(index);
    }
    let payload = if let Some(expr) = function.args.first() {
        let Some(index) = column(rows, expr, controls)? else {
            return Ok(None);
        };
        Some(index)
    } else {
        None
    };
    Ok(Some(Selection {
        partition,
        order,
        payload,
    }))
}

fn primitive(cell: Cell<'_>) -> SemanticValue {
    match cell {
        Cell::Null => SemanticValue::Null,
        Cell::Integer(value) => SemanticValue::from_value(&Value::Int64(value)),
        Cell::Float(value) => SemanticValue::from_value(&Value::Float64(value)),
        Cell::Boolean(value) => SemanticValue::Bool(value),
        Cell::Scalar(value) => SemanticValue::from_value(value),
        Cell::Text(_) => unreachable!("primitive bridge eligibility"),
    }
}

fn key_state_bytes(
    rows: &[BatchRow],
    function: &WindowFunctionCall,
    selection: &Selection,
    context: &WindowExecutionContext<'_>,
) -> Result<usize, crate::app::CassieError> {
    let width = add(selection.partition.len(), selection.order.len())?;
    let mut bytes = add(
        64,
        add(
            mul(width, size_of::<usize>())?,
            mul(
                rows.len(),
                add(
                    size_of::<Keys>(),
                    add(
                        mul(
                            width,
                            size_of::<SemanticValue>() + size_of::<WindowSortPart>(),
                        )?,
                        2 * size_of::<usize>() + size_of::<Target>(),
                    )?,
                )?,
            )?,
        )?,
    )?;
    for row in rows {
        // A prior window append invalidates lookup; its new names must be re-admitted.
        let names = row
            .entries()
            .iter()
            .try_fold(0, |bytes, (name, _)| add(bytes, name.len()))?;
        let aliases = row
            .aliases()
            .iter()
            .try_fold(0, |bytes, (name, _)| add(bytes, name.len()))?;
        bytes = add(
            bytes,
            lookup_bytes(
                add(row.entries().len(), row.aliases().len())?,
                add(names, aliases)?,
            )?,
        )?;
        for column in &selection.partition {
            bytes = add(
                bytes,
                accounting::conversion_bytes(&row.entries()[*column].1, None)?,
            )?;
        }
        for (item, column) in function.order_by.iter().zip(&selection.order) {
            bytes = add(
                bytes,
                accounting::type_scratch(row, &item.expr, context.user_functions)?,
            )?;
            bytes = add(
                bytes,
                accounting::conversion_bytes(
                    &row.entries()[*column].1,
                    row.data_types().get(*column),
                )?,
            )?;
        }
        if !function.order_by.is_empty() {
            bytes = add(bytes, accounting::tie_bytes(row)?)?;
        }
    }
    Ok(bytes)
}

fn keys(
    rows: &[BatchRow],
    function: &WindowFunctionCall,
    selection: &Selection,
    context: &WindowExecutionContext<'_>,
) -> Result<(Vec<Keys>, QueryMemoryReservation, bool), QueryError> {
    let memory = context
        .controls
        .reserve_query_memory(key_state_bytes(rows, function, selection, context)?)?;
    let columns = selection
        .partition
        .iter()
        .chain(&selection.order)
        .copied()
        .collect::<Vec<_>>();
    let batch = row_bridge::from_rows(context.controls, rows, &columns)?;
    let native = batch.is_some();
    let mut keys = Vec::with_capacity(rows.len());
    for (lane, row) in rows.iter().enumerate() {
        check_timeout(context.controls)?;
        let mut partition = Vec::with_capacity(selection.partition.len());
        for (column, index) in selection.partition.iter().enumerate() {
            partition.push(if let Some(batch) = &batch {
                primitive(batch.cell(column, lane)?)
            } else {
                SemanticValue::from_value(&row.entries()[*index].1)
            });
        }
        let mut parts = Vec::with_capacity(selection.order.len());
        for (column, (item, index)) in function.order_by.iter().zip(&selection.order).enumerate() {
            let value = if let Some(batch) = &batch {
                primitive(batch.cell(selection.partition.len() + column, lane)?)
            } else {
                crate::executor::array_order::key(
                    row,
                    &item.expr,
                    &row.entries()[*index].1,
                    context.user_functions,
                )
            };
            parts.push(WindowSortPart {
                value,
                direction: item.direction.clone(),
                nulls: item.nulls,
            });
        }
        keys.push(Keys {
            partition,
            order: WindowSortKey {
                parts,
                tie_key: if function.order_by.is_empty() {
                    String::new()
                } else {
                    batch::row_tie_key(row)
                },
            },
        });
    }
    Ok((keys, memory, native))
}

fn sorted_indices(
    keys: &[Keys],
    controls: &QueryExecutionControls,
) -> Result<Vec<usize>, QueryError> {
    let mut input = (0..keys.len()).collect::<Vec<_>>();
    let mut output = Vec::with_capacity(keys.len());
    let mut width = 1;
    while width < input.len() {
        output.clear();
        let step = add(width, width)?;
        for start in (0..input.len()).step_by(step) {
            let middle = add(start, width)?.min(input.len());
            let end = add(start, step)?.min(input.len());
            let (mut left, mut right) = (start, middle);
            while left < middle || right < end {
                check_timeout(controls)?;
                let take_left = right == end
                    || (left < middle && {
                        let (a, b) = (input[left], input[right]);
                        keys[a]
                            .partition
                            .cmp(&keys[b].partition)
                            .then_with(|| compare_window_sort_keys(&keys[a].order, &keys[b].order))
                            .then(a.cmp(&b))
                            != CmpOrdering::Greater
                    });
                if take_left {
                    output.push(input[left]);
                    left += 1;
                } else {
                    output.push(input[right]);
                    right += 1;
                }
            }
        }
        std::mem::swap(&mut input, &mut output);
        width = step;
    }
    Ok(input)
}

fn peers(left: &Keys, right: &Keys) -> bool {
    left.order.parts.len() == right.order.parts.len()
        && left
            .order
            .parts
            .iter()
            .zip(&right.order.parts)
            .all(|(left, right)| left.value == right.value)
}

fn targets(
    keys: &[Keys],
    indices: &[usize],
    function: &WindowFunctionCall,
    controls: &QueryExecutionControls,
) -> Result<Vec<Target>, QueryError> {
    let name = kind(function).expect("supported selection");
    let mut targets = vec![Target::Null; keys.len()];
    let mut start = 0;
    while start < indices.len() {
        let mut end = start + 1;
        while end < indices.len() && keys[indices[start]].partition == keys[indices[end]].partition
        {
            check_timeout(controls)?;
            end += 1;
        }
        let mut peer = start;
        let mut dense = 1;
        while peer < end {
            let mut peer_end = peer + 1;
            while peer_end < end && peers(&keys[indices[peer]], &keys[indices[peer_end]]) {
                check_timeout(controls)?;
                peer_end += 1;
            }
            for position in peer..peer_end {
                check_timeout(controls)?;
                let relative = position - start;
                targets[indices[position]] = match name {
                    "row_number" => Target::Rank(i64::try_from(relative + 1).unwrap_or(i64::MAX)),
                    "rank" => Target::Rank(i64::try_from(peer - start + 1).unwrap_or(i64::MAX)),
                    "dense_rank" => Target::Rank(dense),
                    "lag" => {
                        if relative == 0 {
                            Target::Null
                        } else {
                            Target::Row(indices[position - 1])
                        }
                    }
                    "lead" => {
                        if position + 1 == end {
                            Target::Null
                        } else {
                            Target::Row(indices[position + 1])
                        }
                    }
                    "first_value" | "last_value" => {
                        let bounds = function.frame.as_ref().map_or(
                            Some((
                                0,
                                if function.order_by.is_empty() {
                                    end - start - 1
                                } else {
                                    peer_end - start - 1
                                },
                            )),
                            |frame| frame_row_bounds(relative, end - start, frame),
                        );
                        bounds.map_or(Target::Null, |(first, last)| {
                            Target::Row(
                                indices[start + if name == "first_value" { first } else { last }],
                            )
                        })
                    }
                    _ => unreachable!("validated function"),
                };
            }
            peer = peer_end;
            dense += 1;
        }
        start = end;
    }
    Ok(targets)
}

fn payload(
    rows: &[BatchRow],
    column: usize,
    controls: &QueryExecutionControls,
) -> Result<TypedBatch, QueryError> {
    let bytes = rows
        .iter()
        .try_fold(mul(rows.len(), size_of::<Value>())?, |bytes, row| {
            add(bytes, value_clone_bytes(&row.entries()[column].1)?)
        })?;
    let _memory = controls.reserve_query_memory(bytes)?;
    let mut values = Vec::with_capacity(rows.len());
    for row in rows {
        check_timeout(controls)?;
        values.push(row.entries()[column].1.clone());
    }
    // Existing ScalarBacked transport carries exact original variants; public type stays separate.
    TypedBatch::from_columns(
        controls,
        &[(String::new(), DataType::Json)],
        &[values],
        rows.len(),
        None,
    )
}

fn apply_one(
    rows: &mut [BatchRow],
    function: &WindowFunctionCall,
    alias: Option<&str>,
    selection: &Selection,
    context: &WindowExecutionContext<'_>,
) -> Result<bool, QueryError> {
    let (keys, _state_memory, native) = keys(rows, function, selection, context)?;
    let indices = sorted_indices(&keys, context.controls)?;
    let targets = targets(&keys, &indices, function, context.controls)?;
    let payload = selection
        .payload
        .map(|column| payload(rows, column, context.controls))
        .transpose()?;
    let data_type = match selection.payload {
        Some(column) => rows
            .first()
            .and_then(|row| row.data_types().get(column))
            .unwrap_or(&DataType::Null),
        None => &DataType::BigInt,
    };
    let name = alias.unwrap_or(function.name.as_str());
    let output_bytes = output_bytes(rows, &targets, selection.payload, name, data_type)?;
    let output_memory = Arc::new(context.controls.reserve_query_memory(output_bytes)?);
    let mut values = Vec::with_capacity(rows.len());
    for target in targets {
        check_timeout(context.controls)?;
        values.push(match target {
            Target::Null => Value::Null,
            Target::Rank(value) => Value::Int64(value),
            Target::Row(index) => payload
                .as_ref()
                .expect("value window payload")
                .cell(0, index)?
                .to_owned(),
        });
    }
    let types = rows.first().and_then(|row| {
        if row.data_types().is_empty() && !matches!(data_type, DataType::Array(_)) {
            return None;
        }
        let mut types = Vec::with_capacity(row.entries().len() + 1);
        types.extend(row.data_types().iter().take(row.entries().len()).cloned());
        types.resize(row.entries().len(), DataType::Null);
        types.push(data_type.clone());
        Some(Arc::new(types))
    });
    for (row, value) in rows.iter_mut().zip(values) {
        check_timeout(context.controls)?;
        row.reserve_append_slot()?;
        row.attach_operator_memory(context.controls, Arc::clone(&output_memory))?;
        row.append_value(name.to_owned(), value);
        if let Some(types) = &types {
            row.set_data_types(Arc::clone(types));
        }
    }
    check_timeout(context.controls)?;
    Ok(native)
}

fn output_bytes(
    rows: &[BatchRow],
    targets: &[Target],
    payload: Option<usize>,
    name: &str,
    data_type: &DataType,
) -> Result<usize, crate::app::CassieError> {
    let mut bytes = add(
        mul(rows.len(), size_of::<Value>())?,
        size_of::<QueryMemoryReservation>() + 2 * size_of::<usize>(),
    )?;
    for (row, target) in rows.iter().zip(targets) {
        bytes = add(bytes, add(row.append_slot_backing_bytes()?, name.len())?)?;
        if let (Target::Row(index), Some(column)) = (target, payload) {
            bytes = add(bytes, value_clone_bytes(&rows[*index].entries()[column].1)?)?;
        }
    }
    if let Some(row) = rows.first() {
        if !row.data_types().is_empty() || matches!(data_type, DataType::Array(_)) {
            bytes = add(
                bytes,
                add(
                    size_of::<Vec<DataType>>() + 2 * size_of::<usize>(),
                    mul(add(row.entries().len(), 1)?, size_of::<DataType>())?,
                )?,
            )?;
            for data_type in row
                .data_types()
                .iter()
                .take(row.entries().len())
                .chain(std::iter::once(data_type))
            {
                bytes = add(bytes, data_type_clone_bytes(data_type)?)?;
            }
        }
    }
    Ok(bytes)
}

fn chunk(rows: Vec<BatchRow>, controls: &QueryExecutionControls) -> Result<Vec<Batch>, QueryError> {
    let mut rows = rows.into_iter();
    let mut output = Vec::with_capacity(rows.len().div_ceil(batch::DEFAULT_BATCH_SIZE));
    while rows.len() > 0 {
        check_timeout(controls)?;
        let count = rows.len().min(batch::DEFAULT_BATCH_SIZE);
        let mut batch = Vec::with_capacity(count);
        batch.extend(rows.by_ref().take(count));
        output.push(batch);
    }
    Ok(output)
}

#[cfg(test)]
mod lease_tests;
