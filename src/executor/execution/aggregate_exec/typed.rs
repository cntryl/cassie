//! Streaming numeric aggregate boundary over the common typed transport.
use super::state::{AvgSum, NumericSum};
use super::QueryError;
use crate::app::{Cassie, CassieSession};
use crate::executor::batch::BatchRow;
use crate::executor::retained_memory::{add, mul};
use crate::executor::scan;
use crate::executor::typed_batch::{check_controls, Cell, TypedBatch};
use crate::planner::logical::LogicalPlan;
use crate::runtime::QueryExecutionControls;
use crate::sql::ast::{Expr, QuerySource, SelectItem};
use crate::types::{DataType, Value};
use std::mem::size_of;
use std::sync::Arc;

mod partials;
mod predicate;
mod workers;

enum State {
    Count(i64),
    Sum { sum: NumericSum, seen: bool },
    Avg { sum: AvgSum, count: usize },
    MinMax { selected: Option<Value>, max: bool },
}

impl State {
    fn new(name: &str) -> Option<Self> {
        if name.eq_ignore_ascii_case("count") {
            Some(Self::Count(0))
        } else if name.eq_ignore_ascii_case("sum") {
            Some(Self::Sum {
                sum: NumericSum::default(),
                seen: false,
            })
        } else if name.eq_ignore_ascii_case("avg") {
            Some(Self::Avg {
                sum: AvgSum::default(),
                count: 0,
            })
        } else if name.eq_ignore_ascii_case("min") || name.eq_ignore_ascii_case("max") {
            Some(Self::MinMax {
                selected: None,
                max: name.eq_ignore_ascii_case("max"),
            })
        } else {
            None
        }
    }

    fn update(&mut self, cell: Cell<'_>) -> Result<(), QueryError> {
        let cell = match cell {
            Cell::Scalar(Value::Int64(value)) => Cell::Integer(*value),
            Cell::Scalar(Value::Float64(value)) => Cell::Float(*value),
            Cell::Scalar(Value::Null) => Cell::Null,
            cell => cell,
        };
        if matches!(cell, Cell::Null) {
            return Ok(());
        }
        match self {
            Self::Count(count) => {
                *count = count.checked_add(1).ok_or_else(count_overflow)?;
            }
            Self::Sum { sum, seen } => {
                match cell {
                    Cell::Integer(value) => sum.add_int(value),
                    Cell::Float(value) => sum.add_float(value),
                    _ => return Err(invalid_input()),
                }
                *seen = true;
            }
            Self::Avg { sum, count } => {
                match cell {
                    Cell::Integer(value) => sum.add_int(value),
                    Cell::Float(value) => sum.add_float(value),
                    _ => return Err(invalid_input()),
                }
                *count = count.checked_add(1).ok_or_else(count_overflow)?;
            }
            Self::MinMax { selected, max } => {
                let value = match cell {
                    Cell::Integer(value) => Value::Int64(value),
                    Cell::Float(value) => Value::Float64(value),
                    _ => return Err(invalid_input()),
                };
                if selected.as_ref().is_none_or(|previous| {
                    let order = crate::executor::semantic::compare_values(&value, previous);
                    if *max {
                        order.is_gt()
                    } else {
                        order.is_lt()
                    }
                }) {
                    *selected = Some(value);
                }
            }
        }
        Ok(())
    }

    fn finish(self) -> Result<Value, QueryError> {
        match self {
            Self::Count(count) => Ok(Value::Int64(count)),
            Self::Sum { sum, seen: true } => sum.finish_value(),
            Self::Sum { seen: false, .. } | Self::Avg { count: 0, .. } => Ok(Value::Null),
            Self::Avg { sum, count } => sum.finish_mean(count).map(Value::Float64),
            Self::MinMax { selected, .. } => Ok(selected.unwrap_or(Value::Null)),
        }
    }
}

struct Spec {
    column: Option<usize>,
    name: String,
    state: State,
    merge_safe: bool,
}

fn count_overflow() -> QueryError {
    QueryError::General("aggregate integer overflow".to_owned())
}

fn invalid_input() -> QueryError {
    QueryError::General("invalid typed aggregate numeric input".to_owned())
}

fn consume(
    state: &mut State,
    column: Option<usize>,
    batch: &TypedBatch,
    controls: &QueryExecutionControls,
) -> Result<(), QueryError> {
    for lane in 0..batch.len() {
        check_controls(controls)?;
        let cell = column.map_or(Ok(Cell::Integer(1)), |column| batch.cell(column, lane))?;
        state.update(cell)?;
    }
    Ok(())
}

pub(in crate::executor::execution) fn try_execute(
    cassie: &Cassie,
    session: Option<&CassieSession>,
    plan: &LogicalPlan,
    controls: &QueryExecutionControls,
) -> Result<Option<Vec<BatchRow>>, QueryError> {
    if !eligible(plan) {
        return Ok(None);
    }
    check_controls(controls)?;
    let floor = mul(
        crate::executor::batch::DEFAULT_BATCH_SIZE,
        size_of::<Value>(),
    )?;
    if controls
        .query_memory_budget_bytes
        .saturating_sub(controls.current_query_memory_bytes())
        < floor
    {
        return Ok(None);
    }
    let QuerySource::Collection(collection) = &plan.source else {
        return Ok(None);
    };
    if crate::catalog::virtual_views::schema(collection).is_some()
        || cassie.catalog.get_view(collection).is_some()
        || !scan::uses_controlled_row_scan(cassie, collection)
    {
        return Ok(None);
    }
    let (schema, _schema_memory) = cassie
        .catalog
        .clone_schema_with_controls(collection, controls)?;
    let Some(schema) = schema else {
        return Ok(None);
    };
    let Some(mut prepared) = prepare_specs(plan, &schema, controls)? else {
        return Ok(None);
    };
    if !predicate::prepare(plan.filter.as_ref(), &schema, &mut prepared)? {
        return Ok(None);
    }
    let Some(mut stream) =
        scan::TypedScanStream::open(cassie, session, collection, &prepared.fields, controls)?
    else {
        return Ok(None);
    };
    let diagnostic = workers::consume_stream(
        cassie,
        &mut stream,
        &mut prepared.specs,
        plan.filter.as_ref(),
        controls,
    )?;
    check_controls(controls)?;
    if controls.max_result_rows == 0 {
        return Err(crate::app::CassieError::ResourceLimit(
            "query result row limit exceeded".to_owned(),
        )
        .into());
    }
    let bytes = prepared.specs.iter().try_fold(
        add(
            add(
                size_of::<BatchRow>(),
                mul(prepared.specs.len(), size_of::<(String, Value)>())?,
            )?,
            add(
                size_of::<crate::runtime::QueryMemoryReservation>(),
                2 * size_of::<usize>(),
            )?,
        )?,
        |bytes, spec| add(bytes, spec.name.capacity()),
    )?;
    let output_memory = Arc::new(controls.reserve_query_memory(bytes)?);
    let mut values = Vec::with_capacity(prepared.specs.len());
    for spec in prepared.specs {
        values.push((spec.name, spec.state.finish()?));
    }
    let output =
        vec![BatchRow::from_projected_values(values).with_query_memory(Some(output_memory))];
    check_controls(controls)?;
    if diagnostic.workers > 1 {
        cassie.runtime.record_parallel_aggregation(
            diagnostic.workers,
            diagnostic.partitions,
            diagnostic.rows,
            1,
        );
    } else {
        cassie
            .runtime
            .record_parallel_aggregation_fallback(diagnostic.fallback);
    }
    Ok(Some(output))
}

struct PreparedSpecs {
    specs: Vec<Spec>,
    fields: Vec<String>,
    memory: crate::runtime::QueryMemoryReservation,
}

fn prepare_specs(
    plan: &LogicalPlan,
    schema: &crate::catalog::CollectionSchema,
    controls: &QueryExecutionControls,
) -> Result<Option<PreparedSpecs>, QueryError> {
    let mut memory = controls.reserve_query_memory(mul(
        plan.projection.len(),
        add(size_of::<Spec>(), size_of::<String>())?,
    )?)?;
    let mut specs = Vec::with_capacity(plan.projection.len());
    let mut fields = Vec::with_capacity(plan.projection.len());
    for item in &plan.projection {
        let SelectItem::Function { function, alias } = item else {
            return Ok(None);
        };
        let Some(state) = State::new(&function.name) else {
            return Ok(None);
        };
        let [Expr::Column(field)] = function.args.as_slice() else {
            return Ok(None);
        };
        // Parsing references and formatting signatures retain bounded temporary strings.
        memory.try_grow(mul(
            add(
                field.len(),
                add(function.name.len(), alias.as_ref().map_or(0, String::len))?,
            )?,
            64,
        )?)?;
        let (column, merge_safe) = if field == "*" && matches!(state, State::Count(_)) {
            (None, true)
        } else {
            let Ok(reference) = crate::sql::ColumnIdentifierPath::parse(field) else {
                return Ok(None);
            };
            if reference.is_qualified() {
                return Ok(None);
            }
            let Some(entry) = schema
                .fields
                .iter()
                .find(|entry| reference.matches_field_name(&entry.name))
            else {
                return Ok(None);
            };
            if !matches!(
                entry.data_type,
                DataType::SmallInt | DataType::Int | DataType::BigInt | DataType::Float
            ) {
                return Ok(None);
            }
            let index = fields
                .iter()
                .position(|name| name == field)
                .unwrap_or(fields.len());
            if index == fields.len() {
                memory.try_grow(field.len())?;
                fields.push(field.clone());
            }
            let merge_safe = !(matches!(state, State::Avg { .. })
                || matches!(state, State::Sum { .. }) && entry.data_type == DataType::Float);
            (Some(index), merge_safe)
        };
        specs.push(Spec {
            column,
            name: alias
                .clone()
                .unwrap_or_else(|| super::super::aggregate_signature(function)),
            state,
            merge_safe,
        });
    }
    Ok(Some(PreparedSpecs {
        specs,
        fields,
        memory,
    }))
}

fn eligible(plan: &LogicalPlan) -> bool {
    !plan.projection.is_empty()
        && plan.command.is_none()
        && plan.ctes.is_empty()
        && plan.group_by.is_empty()
        && plan.having.is_none()
        && plan.order.is_empty()
        && plan.limit.is_none()
        && plan.offset.is_none()
        && !plan.distinct
        && plan.distinct_on.is_empty()
        && plan.set.is_none()
}

#[cfg(test)]
fn fold_sum(batch: &TypedBatch, controls: &QueryExecutionControls) -> Result<Value, QueryError> {
    let mut state = State::new("sum").expect("SUM state");
    consume(&mut state, Some(0), batch, controls)?;
    state.finish()
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod encoded_tests;
