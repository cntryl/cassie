//! Finite direct-key ordering with admitted blocking backing and retained row owners.
use std::mem::size_of;
use std::sync::Arc;

use super::*;
use crate::executor::batch::BatchRow;
use crate::executor::retained_memory::{add, lookup_bytes, mul};
use crate::executor::typed_batch::{relational_diagnostics, row_bridge, Cell, TypedBatch};
use crate::runtime::QueryMemoryReservation;

pub(super) fn sort_batches(
    batches: Vec<Batch>,
    eval: &EvalInput<'_>,
    controls: &QueryExecutionControls,
) -> Result<Vec<Batch>, crate::executor::QueryError> {
    check_query_controls(controls)?;
    if eval.order.is_empty() {
        return Ok(batches);
    }
    let (rows, _flatten_memory) = flatten(batches, controls)?;
    let (rows, path) = ordered_rows(rows, eval, controls)?;
    let output = chunk_rows_controlled(rows.into_iter(), controls)?;
    relational_diagnostics::publish("sort", path);
    Ok(output)
}

pub(super) fn sort_rows(
    rows: Vec<BatchRow>,
    eval: &EvalInput<'_>,
    controls: &QueryExecutionControls,
) -> Result<Vec<BatchRow>, crate::executor::QueryError> {
    let (rows, path) = ordered_rows(rows, eval, controls)?;
    relational_diagnostics::publish("sort", path);
    Ok(rows)
}

fn ordered_rows(
    rows: Vec<BatchRow>,
    eval: &EvalInput<'_>,
    controls: &QueryExecutionControls,
) -> Result<(Vec<BatchRow>, &'static str), crate::executor::QueryError> {
    check_query_controls(controls)?;
    if rows.is_empty() || eval.order.is_empty() {
        return Ok((rows, "empty_relation"));
    }
    if controls.uses_relational_cte_boundary() {
        return sort_rows_with_controls(rows, eval, controls)
            .map(|rows| (rows, "scalar_cte_materialization"));
    }
    let Some(order) = DirectOrder::new(eval, controls)? else {
        return sort_rows_with_controls(rows, eval, controls)
            .map(|rows| (rows, "scalar_expression_order"));
    };
    let memory = admit_rows(&rows, rows.len(), &order.order, controls)?;
    let Some(columns) = direct_columns(&rows, &order.order, controls)? else {
        return sort_rows_with_controls(rows, eval, controls)
            .map(|rows| (rows, "scalar_expression_order"));
    };
    let batch = row_bridge::from_rows(controls, &rows, &columns)?;
    let path = key_path(batch.as_ref());
    let mut lane = 0;
    let rows = sort_rows_by_key(rows, controls, &SortRetentionContext::default(), |row| {
        let key = direct_key(row, &order.order, batch.as_ref(), lane, eval, controls)?;
        lane += 1;
        Ok(key)
    })?;
    let memory = Arc::new(memory);
    let rows = retain_rows(rows, &memory, controls)?;
    check_query_controls(controls)?;
    Ok((rows, path))
}

pub(super) fn top_k_batches(
    batches: Vec<Batch>,
    eval: &EvalInput<'_>,
    count: usize,
    controls: &QueryExecutionControls,
) -> Result<Vec<Batch>, crate::executor::QueryError> {
    check_query_controls(controls)?;
    if count == 0 || eval.order.is_empty() {
        relational_diagnostics::publish("top_k", "empty_relation");
        return Ok(Vec::new());
    }
    if controls.uses_relational_cte_boundary() {
        let output = top_k_batches_with_context(
            batches,
            eval,
            count,
            controls,
            &SortRetentionContext::default(),
        )?;
        relational_diagnostics::publish("top_k", "scalar_cte_materialization");
        return Ok(output);
    }
    let Some(order) = DirectOrder::new(eval, controls)? else {
        let output = top_k_batches_with_context(
            batches,
            eval,
            count,
            controls,
            &SortRetentionContext::default(),
        )?;
        relational_diagnostics::publish("top_k", "scalar_expression_order");
        return Ok(output);
    };
    let (rows, _flatten_memory) = flatten(batches, controls)?;
    if rows.is_empty() {
        relational_diagnostics::publish("top_k", "empty_relation");
        return Ok(Vec::new());
    }
    let memory = admit_rows(&rows, count.min(rows.len()), &order.order, controls)?;
    let Some(columns) = direct_columns(&rows, &order.order, controls)? else {
        let output = top_k_batches_with_context(
            vec![rows],
            eval,
            count,
            controls,
            &SortRetentionContext::default(),
        )?;
        relational_diagnostics::publish("top_k", "scalar_expression_order");
        return Ok(output);
    };
    let batch = row_bridge::from_rows(controls, &rows, &columns)?;
    let path = key_path(batch.as_ref());
    let mut lane = 0;
    let mut output = top_k_by_key(
        vec![rows],
        count,
        controls,
        &SortRetentionContext::default(),
        |row| {
            let key = direct_key(row, &order.order, batch.as_ref(), lane, eval, controls)?;
            lane += 1;
            Ok(key)
        },
    )?;
    let memory = Arc::new(memory);
    for row in output.iter_mut().flatten() {
        row.attach_operator_memory(controls, Arc::clone(&memory))?;
    }
    check_query_controls(controls)?;
    relational_diagnostics::publish("top_k", path);
    Ok(output)
}

fn key_path(batch: Option<&TypedBatch>) -> &'static str {
    if batch.is_some() {
        "native_primitive_keys"
    } else {
        "bounded_semantic_keys"
    }
}

fn retain_rows(
    mut rows: Vec<BatchRow>,
    memory: &Arc<QueryMemoryReservation>,
    controls: &QueryExecutionControls,
) -> Result<Vec<BatchRow>, crate::executor::QueryError> {
    for row in &mut rows {
        row.attach_operator_memory(controls, Arc::clone(memory))?;
    }
    Ok(rows)
}

fn flatten(
    batches: Vec<Batch>,
    controls: &QueryExecutionControls,
) -> Result<(Vec<BatchRow>, QueryMemoryReservation), crate::executor::QueryError> {
    check_query_controls(controls)?;
    let count = batches
        .iter()
        .try_fold(0, |count, batch| add(count, batch.len()))?;
    let old = batches.iter().try_fold(
        mul(batches.capacity(), size_of::<Batch>())?,
        |bytes, batch| add(bytes, mul(batch.capacity(), size_of::<BatchRow>())?),
    )?;
    let memory = controls.reserve_query_memory(add(old, mul(count, size_of::<BatchRow>())?)?)?;
    let mut rows = Vec::with_capacity(count);
    for row in batches.into_iter().flatten() {
        check_query_controls(controls)?;
        rows.push(row);
    }
    Ok((rows, memory))
}

fn admit_rows(
    rows: &[BatchRow],
    output: usize,
    order: &[OrderExpr],
    controls: &QueryExecutionControls,
) -> Result<QueryMemoryReservation, crate::executor::QueryError> {
    let slots = add(
        mul(rows.len(), size_of::<BatchRow>())?,
        mul(output, size_of::<BatchRow>())?,
    )?;
    let headers = mul(
        output.div_ceil(DEFAULT_BATCH_SIZE).max(1),
        size_of::<Batch>(),
    )?;
    let lookup_scratch = order.iter().try_fold(0, |bytes, order| {
        let Expr::Column(name) = &order.expr else {
            unreachable!("resolved direct order")
        };
        add(bytes, add(512, mul(name.len(), 32)?)?)
    })?;
    let bytes = rows.iter().try_fold(
        add(
            slots,
            add(
                headers,
                size_of::<QueryMemoryReservation>() + 2 * size_of::<usize>(),
            )?,
        )?,
        |bytes, row| {
            let names = row
                .entries()
                .iter()
                .try_fold(0, |bytes, (name, _)| add(bytes, name.len()))?;
            let aliases = row
                .aliases()
                .iter()
                .try_fold(0, |bytes, (name, _)| add(bytes, name.len()))?;
            add(
                bytes,
                add(
                    row.unleased_body_bytes()?,
                    add(
                        lookup_bytes(
                            add(row.entries().len(), row.aliases().len())?,
                            add(names, aliases)?,
                        )?,
                        lookup_scratch,
                    )?,
                )?,
            )
        },
    )?;
    controls
        .reserve_query_memory(bytes)
        .map_err(crate::executor::QueryError::from)
}

struct DirectOrder {
    order: Vec<OrderExpr>,
    _memory: QueryMemoryReservation,
}

impl DirectOrder {
    fn new(
        eval: &EvalInput<'_>,
        controls: &QueryExecutionControls,
    ) -> Result<Option<Self>, crate::executor::QueryError> {
        if eval
            .order
            .iter()
            .any(|order| !matches!(order.expr, Expr::Column(_)))
        {
            return Ok(None);
        }
        let names = eval.order.iter().try_fold(0, |bytes, order| {
            let Expr::Column(name) = &order.expr else {
                unreachable!("column checked")
            };
            add(bytes, name.len())
        })?;
        let names = eval
            .projection
            .iter()
            .try_fold(names, |bytes, item| match item {
                SelectItem::Column { name, alias, .. } => add(
                    bytes,
                    add(name.len(), alias.as_ref().map_or(0, String::len))?,
                ),
                SelectItem::Expr {
                    expr: Expr::Column(name),
                    alias,
                } => add(
                    bytes,
                    add(name.len(), alias.as_ref().map_or(0, String::len))?,
                ),
                SelectItem::Expr { alias, .. } | SelectItem::Function { alias, .. } => {
                    add(bytes, alias.as_ref().map_or(0, String::len))
                }
                _ => Ok(bytes),
            })?;
        let memory = controls.reserve_query_memory(add(
            mul(eval.order.len(), size_of::<OrderExpr>() + 512)?,
            mul(names, 32)?,
        )?)?;
        let mut order = Vec::with_capacity(eval.order.len());
        for item in eval.order {
            check_query_controls(controls)?;
            let Expr::Column(name) = &item.expr else {
                unreachable!("column checked")
            };
            let reference = crate::sql::ColumnIdentifierPath::reference_field_key(name);
            let mut resolved = name;
            for projected in eval.projection {
                let alias = match projected {
                    SelectItem::Column { alias, .. }
                    | SelectItem::Expr { alias, .. }
                    | SelectItem::Function { alias, .. } => alias.as_ref(),
                    _ => None,
                };
                if alias.is_some_and(|alias| {
                    crate::sql::ColumnIdentifierPath::stored_field_key(alias) == reference
                }) {
                    resolved = match projected {
                        SelectItem::Column { name, .. }
                        | SelectItem::Expr {
                            expr: Expr::Column(name),
                            ..
                        } => name,
                        _ => return Ok(None),
                    };
                    break;
                }
            }
            order.push(OrderExpr {
                expr: Expr::Column(resolved.clone()),
                direction: item.direction.clone(),
                nulls: item.nulls,
            });
        }
        Ok(Some(Self {
            order,
            _memory: memory,
        }))
    }
}

fn direct_columns(
    rows: &[BatchRow],
    order: &[OrderExpr],
    controls: &QueryExecutionControls,
) -> Result<Option<Vec<usize>>, crate::executor::QueryError> {
    let mut columns = Vec::with_capacity(order.len());
    for order in order {
        let Expr::Column(name) = &order.expr else {
            unreachable!("direct order")
        };
        let mut previous = None;
        for row in rows {
            check_query_controls(controls)?;
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
        columns.push(previous.expect("nonempty rows checked"));
    }
    Ok(Some(columns))
}

fn direct_key(
    row: &BatchRow,
    order: &[OrderExpr],
    batch: Option<&TypedBatch>,
    lane: usize,
    eval: &EvalInput<'_>,
    controls: &QueryExecutionControls,
) -> Result<RowKey, crate::executor::QueryError> {
    let mut memory = controls.reserve_query_memory(add(
        size_of::<RowKey>(),
        add(mul(order.len(), size_of::<KeyPart>())?, 64)?,
    )?)?;
    let mut parts = Vec::with_capacity(order.len());
    for (column, order) in order.iter().enumerate() {
        check_query_controls(controls)?;
        let semantic = if let Some(batch) = batch {
            match batch.cell(column, lane)? {
                Cell::Null => SemanticValue::Null,
                Cell::Integer(value) => SemanticValue::from_value(&Value::Int64(value)),
                Cell::Float(value) => SemanticValue::from_value(&Value::Float64(value)),
                Cell::Boolean(value) => SemanticValue::Bool(value),
                Cell::Scalar(value) => SemanticValue::from_value(value),
                Cell::Text(_) => unreachable!("primitive bridge eligibility"),
            }
        } else {
            let Expr::Column(name) = &order.expr else {
                unreachable!("direct order")
            };
            let value = row.get(name).expect("borrowed direct key eligibility");
            memory.try_grow(accounting::type_scratch(
                row,
                &order.expr,
                eval.user_functions,
            )?)?;
            let data_type = row
                .has_array_types()
                .then(|| {
                    crate::executor::array_order::expression_type(
                        row,
                        &order.expr,
                        eval.user_functions,
                    )
                })
                .flatten();
            memory.try_grow(accounting::conversion_bytes(value, data_type.as_ref())?)?;
            // This borrowed-value bound precedes every rich clone/semantic adaptation.
            crate::executor::array_order::key(row, &order.expr, value, eval.user_functions)
        };
        parts.push(KeyPart {
            value: semantic,
            direction: order.direction.clone(),
            nulls: order.nulls,
        });
    }
    memory.try_grow(accounting::tie_bytes(row)?)?;
    let tie_key = row_tie_key(row);
    check_query_controls(controls)?;
    Ok(RowKey {
        parts,
        tie_key,
        _memory: Some(memory),
    })
}
