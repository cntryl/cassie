//! Streaming grouped hash aggregate over typed batches from any scan source.
//!
//! Each batch folds into per-group aggregate states and is released before the
//! next one is read, so live memory follows the number of groups, not input
//! rows. Lanes fold serially in row order, so SUM and AVG over floats and
//! integer overflow behave exactly like the row path, and groups finish in the
//! row path's semantic key order.
use super::group_key::{group_bytes, lane_key, supported_count_input, supported_key};
use super::{predicate, PreparedSpecs, State};
use crate::app::{Cassie, CassieSession};
use crate::catalog::CollectionSchema;
use crate::executor::batch::{BatchRow, DEFAULT_BATCH_SIZE};
use crate::executor::execution::aggregate_exec::group_memory::GroupMemory;
use crate::executor::execution::aggregate_exec::output_owner;
use crate::executor::retained_memory::{add, mul, value_clone_bytes};
use crate::executor::scan::TypedScanStream;
use crate::executor::typed_batch::{check_controls, Cell, TypedBatch};
use crate::executor::QueryError;
use crate::planner::logical::LogicalPlan;
use crate::runtime::QueryExecutionControls;
use crate::sql::ast::{Expr, SelectItem};
use crate::types::semantic::SemanticKey;
use crate::types::semantic_index::{Entry, KeyedMap};
use crate::types::{DataType, Value};
use std::mem::size_of;

/// Dispatch label recorded when the grouped kernel serves a query.
pub(super) const DISPATCH: &str = "typed-grouped-row-order-fold";

struct Aggregate {
    column: Option<usize>,
    template: State,
}

enum Output {
    Key { index: usize, name: String },
    Aggregate { index: usize, name: String },
}

struct Shape {
    keys: Vec<usize>,
    aggregates: Vec<Aggregate>,
    outputs: Vec<Output>,
}

struct Group {
    values: Vec<Value>,
    states: Vec<State>,
}

// The map must retire before the reservation that covers its groups.
struct Groups {
    map: KeyedMap<SemanticKey, Group>,
    memory: GroupMemory,
}

pub(super) fn execute(
    cassie: &Cassie,
    session: Option<&CassieSession>,
    plan: &LogicalPlan,
    collection: &str,
    schema: &CollectionSchema,
    params: &[Value],
    controls: &QueryExecutionControls,
) -> Result<Option<Vec<BatchRow>>, QueryError> {
    let Some((shape, mut prepared)) = prepare(plan, schema, controls)? else {
        return Ok(None);
    };
    if !predicate::prepare(plan.filter.as_ref(), schema, params, &mut prepared)? {
        return Ok(None);
    }
    let Some(mut stream) =
        TypedScanStream::open(cassie, session, collection, &prepared.fields, controls)?
    else {
        return Ok(None);
    };
    let mut groups = Groups {
        map: KeyedMap::new(Some(controls))?,
        memory: GroupMemory::new(controls)?,
    };
    while let Some(batch) = stream.next_batch_bounded(DEFAULT_BATCH_SIZE)? {
        let batch = predicate::apply(batch, plan.filter.as_ref(), params, controls)?;
        shape.fold(&batch, &mut groups, controls)?;
        drop(batch);
        #[cfg(test)]
        super::workers::cancel_after_completed_wave();
    }
    check_controls(controls)?;
    let rows = shape.finish(groups, controls)?;
    check_controls(controls)?;
    cassie
        .runtime
        .record_parallel_aggregation_fallback(DISPATCH);
    tracing::trace!(target: "cassie::typed_profile", event = "typed_grouped_aggregate_complete",
        output_groups = rows.len(),
        current_admitted_query_bytes = controls.current_query_memory_bytes(),
        peak_admitted_query_bytes = controls.peak_query_memory_bytes(),
    );
    Ok(Some(rows))
}

/// The scan fields a grouped plan reads, or why it declines to the row path.
pub(super) fn explain_shape(
    plan: &LogicalPlan,
    schema: &CollectionSchema,
    params: &[Value],
    controls: &QueryExecutionControls,
) -> Result<super::PlanShape, QueryError> {
    let Some((_, mut prepared)) = prepare(plan, schema, controls)? else {
        return Ok(super::PlanShape::Aggregate);
    };
    if !predicate::prepare(plan.filter.as_ref(), schema, params, &mut prepared)? {
        return Ok(super::PlanShape::Expression);
    }
    Ok(super::PlanShape::Fields(std::mem::take(
        &mut prepared.fields,
    )))
}

impl Shape {
    fn group_fixed_bytes(&self) -> Result<usize, QueryError> {
        Ok(add(
            size_of::<(SemanticKey, Group)>(),
            mul(self.aggregates.len(), size_of::<State>())?,
        )?)
    }

    fn fold(
        &self,
        batch: &TypedBatch,
        groups: &mut Groups,
        controls: &QueryExecutionControls,
    ) -> Result<(), QueryError> {
        let fixed = self.group_fixed_bytes()?;
        for lane in 0..batch.len() {
            check_controls(controls)?;
            let (key, values) = lane_key(&self.keys, batch, lane)?;
            let mut added = 0;
            let group = match groups.map.entry(key)? {
                Entry::Occupied(entry) => entry.into_mut(),
                Entry::Vacant(entry) => {
                    added = group_bytes(entry.key(), &values, fixed)?;
                    entry.insert(Group {
                        values,
                        states: self
                            .aggregates
                            .iter()
                            .map(|aggregate| aggregate.template.empty_partial())
                            .collect(),
                    })
                }
            };
            for (aggregate, state) in self.aggregates.iter().zip(&mut group.states) {
                let cell = aggregate
                    .column
                    .map_or(Ok(Cell::Integer(1)), |column| batch.cell(column, lane))?;
                state.update(cell)?;
            }
            controls.aggregate_input_retained()?;
            check_controls(controls)?;
            groups.memory.add(added)?;
        }
        Ok(())
    }

    fn finish(
        &self,
        groups: Groups,
        controls: &QueryExecutionControls,
    ) -> Result<Vec<BatchRow>, QueryError> {
        let Groups { map, mut memory } = groups;
        let ordered = map.into_sorted_entries()?.into_iter();
        let names = self.outputs.iter().try_fold(0, |bytes, output| {
            let (Output::Key { name, .. } | Output::Aggregate { name, .. }) = output;
            add(bytes, name.len())
        })?;
        let row_bytes = add(
            add(
                size_of::<BatchRow>(),
                mul(self.outputs.len(), size_of::<(String, Value)>())?,
            )?,
            names,
        )?;
        memory.add(mul(ordered.len(), row_bytes)?)?;
        let mut rows = Vec::with_capacity(ordered.len());
        for (_key, group) in ordered {
            check_controls(controls)?;
            let Group { values, states } = group;
            let finished = states
                .into_iter()
                .map(State::finish)
                .collect::<Result<Vec<_>, _>>()?;
            let mut entries = Vec::with_capacity(self.outputs.len());
            for output in &self.outputs {
                let (name, value) = match output {
                    Output::Key { index, name } => (name, &values[*index]),
                    Output::Aggregate { index, name } => (name, &finished[*index]),
                };
                memory.add(value_clone_bytes(value)?)?;
                entries.push((name.clone(), value.clone()));
            }
            rows.push(BatchRow::from_projected_values(entries));
        }
        output_owner::retain_outputs(rows, memory, Vec::new(), controls)
    }
}

/// Resolves an unqualified column reference to its schema field.
fn resolve<'a>(schema: &'a CollectionSchema, name: &str) -> Option<&'a crate::catalog::FieldMeta> {
    let reference = crate::sql::ColumnIdentifierPath::parse(name).ok()?;
    if reference.is_qualified() {
        return None;
    }
    schema
        .fields
        .iter()
        .find(|entry| reference.matches_field_name(&entry.name))
}

fn scan_index(prepared: &mut PreparedSpecs, field: &str) -> Result<usize, QueryError> {
    if let Some(index) = prepared.fields.iter().position(|name| name == field) {
        return Ok(index);
    }
    prepared
        .memory
        .try_grow(add(size_of::<String>(), field.len())?)?;
    prepared.fields.push(field.to_owned());
    Ok(prepared.fields.len() - 1)
}

/// Accepts GROUP BY bare native/text columns with COUNT, SUM, MIN, MAX and AVG.
///
/// Anything else (expressions, qualified names, other key types, aggregate
/// modifiers, other aggregates) declines to the row path.
fn prepare(
    plan: &LogicalPlan,
    schema: &CollectionSchema,
    controls: &QueryExecutionControls,
) -> Result<Option<(Shape, PreparedSpecs)>, QueryError> {
    let memory = controls.reserve_query_memory(mul(
        add(plan.projection.len(), plan.group_by.len())?,
        add(size_of::<Aggregate>(), size_of::<Output>())?,
    )?)?;
    let mut prepared = PreparedSpecs {
        specs: Vec::new(),
        fields: Vec::new(),
        memory,
    };
    let mut keys = Vec::with_capacity(plan.group_by.len());
    let mut key_fields = Vec::with_capacity(plan.group_by.len());
    for expr in &plan.group_by {
        let Expr::Column(name) = expr else {
            return Ok(None);
        };
        let Some(field) = resolve(schema, name) else {
            return Ok(None);
        };
        if !supported_key(&field.data_type) {
            return Ok(None);
        }
        keys.push(scan_index(&mut prepared, name)?);
        key_fields.push(field.name.as_str());
    }
    let mut aggregates = Vec::new();
    let mut outputs = Vec::with_capacity(plan.projection.len());
    for item in &plan.projection {
        match item {
            SelectItem::Column { name, alias } => {
                let Some(field) = resolve(schema, name) else {
                    return Ok(None);
                };
                let Some(index) = key_fields.iter().position(|key| *key == field.name) else {
                    return Ok(None);
                };
                let name = alias.clone().unwrap_or_else(|| {
                    crate::sql::ColumnIdentifierPath::parse(name)
                        .map_or_else(|_| name.clone(), |column| column.implicit_output_label())
                });
                prepared.memory.try_grow(name.len())?;
                outputs.push(Output::Key { index, name });
            }
            SelectItem::Function { function, alias } => {
                if function.is_distinct()
                    || function.filter.is_some()
                    || !function.order_by.is_empty()
                {
                    return Ok(None);
                }
                let Some(template) = State::new(&function.name) else {
                    return Ok(None);
                };
                let [Expr::Column(argument)] = function.args.as_slice() else {
                    return Ok(None);
                };
                let column = match template {
                    State::Boolean(_) => return Ok(None),
                    State::Count(_) if argument == "*" => None,
                    _ => {
                        let Some(field) = resolve(schema, argument) else {
                            return Ok(None);
                        };
                        let supported = if matches!(template, State::Count(_)) {
                            supported_count_input(&field.data_type)
                        } else {
                            matches!(
                                field.data_type,
                                DataType::SmallInt
                                    | DataType::Int
                                    | DataType::BigInt
                                    | DataType::Float
                            )
                        };
                        if !supported {
                            return Ok(None);
                        }
                        Some(scan_index(&mut prepared, argument)?)
                    }
                };
                let name = alias.clone().unwrap_or_else(|| function.name.clone());
                prepared.memory.try_grow(name.len())?;
                outputs.push(Output::Aggregate {
                    index: aggregates.len(),
                    name,
                });
                aggregates.push(Aggregate { column, template });
            }
            _ => return Ok(None),
        }
    }
    Ok(Some((
        Shape {
            keys,
            aggregates,
            outputs,
        },
        prepared,
    )))
}
