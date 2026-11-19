//! Snapshot row-level aggregate arguments before the group evaluator addresses its states.
//! Each call occurrence owns its computed inputs. A sort key naming a projected expression
//! refers to that same occurrence. Plain columns already have statement lifetime.
use super::{
    check_timeout, filter, group_memory, ordered_set, state_input, AggregateExecutionContext,
    Batch, BatchRow, Expr, QueryError, Value,
};
use crate::executor::{
    array_order,
    retained_memory::{add, data_type_clone_bytes, lookup_bytes, mul, value_clone_bytes},
};
use crate::planner::logical::{plan_clone_bytes, LogicalPlan};
use crate::runtime::{owned_memory::grow_vec, QueryMemoryReservation};
use crate::sql::ast::{FunctionCall, SelectItem};
use crate::sql::functions::is_aggregate_function;
use crate::types::DataType;
use std::{mem::size_of, sync::Arc};

pub(in crate::executor::execution) struct PreparedPlan {
    pub(in crate::executor::execution) plan: LogicalPlan,
    _memory: QueryMemoryReservation,
}

#[derive(Default)]
struct Inputs {
    filter: Option<(String, Expr)>,
    expressions: Vec<(String, Expr)>,
}

struct Layout {
    inputs: Vec<Inputs>,
    slots: usize,
    memory: QueryMemoryReservation,
}

pub(in crate::executor::execution) fn prepare_plan(
    batches: &mut [Batch],
    context: &AggregateExecutionContext<'_>,
) -> Result<PreparedPlan, QueryError> {
    let original = context.plan;
    let memory = context
        .controls
        .reserve_query_memory(plan_clone_bytes(original, context.controls)?)?;
    let mut layout = Layout {
        inputs: Vec::new(),
        slots: 0,
        memory,
    };
    let mut plan = original.clone();
    for item in &mut plan.projection {
        context.controls.check_interruption()?;
        match item {
            SelectItem::Function { function, .. } => layout.call(function)?,
            SelectItem::Expr { expr, .. } => layout.expression(expr)?,
            SelectItem::WindowFunction { function, .. } => {
                for expr in function
                    .args
                    .iter_mut()
                    .chain(function.filter.as_deref_mut())
                    .chain(&mut function.partition_by)
                    .chain(function.order_by.iter_mut().map(|order| &mut order.expr))
                {
                    layout.expression(expr)?;
                }
            }
            SelectItem::Column { .. } | SelectItem::Wildcard => {}
        }
    }
    if let Some(having) = &mut plan.having {
        layout.expression(having)?;
    }
    for (order, source) in plan.order.iter_mut().zip(&original.order) {
        context.controls.check_interruption()?;
        if let Some(bound) = project_reference(&source.expr, original, &plan.projection) {
            order.expr = bound;
        } else {
            layout.expression(&mut order.expr)?;
        }
    }
    for (key, source) in plan.distinct_on.iter_mut().zip(&original.distinct_on) {
        context.controls.check_interruption()?;
        if let Some(index) = original
            .order
            .iter()
            .position(|order| order.expr.structurally_eq(source))
        {
            *key = plan.order[index].expr.clone();
        } else if let Some(bound) = project_reference(source, original, &plan.projection) {
            *key = bound;
        } else {
            layout.expression(key)?;
        }
    }
    if layout.slots > 0 {
        for row in batches.iter_mut().flatten() {
            check_timeout(context.controls)?;
            // The wrapper owns these original entries until the group has consumed its rows.
            layout
                .memory
                .try_grow(group_memory::serial_row_bytes(row, context.controls)?)?;
            let values = snapshot(
                row,
                &layout.inputs,
                layout.slots,
                context,
                &mut layout.memory,
            )?;
            let source = std::mem::replace(row, BatchRow::from_projected_values(Vec::new()));
            *row = values.with_outer_scope(Arc::new(source));
        }
    }
    Ok(PreparedPlan {
        plan,
        _memory: layout.memory,
    })
}

fn project_reference(source: &Expr, original: &LogicalPlan, bound: &[SelectItem]) -> Option<Expr> {
    let index = original
        .projection
        .iter()
        .position(|item| matches_projection(source, item))?;
    match &bound[index] {
        SelectItem::Function { function, .. } => Some(Expr::Function(Box::new(function.clone()))),
        SelectItem::Expr { expr, .. } => Some(expr.clone()),
        _ => None,
    }
}

fn matches_projection(expr: &Expr, item: &SelectItem) -> bool {
    match (expr, item) {
        (Expr::Function(call), SelectItem::Function { function, .. }) => {
            call.structurally_eq(function)
        }
        (
            _,
            SelectItem::Expr {
                expr: projected, ..
            },
        ) => expr.structurally_eq(projected),
        _ => false,
    }
}

impl Layout {
    fn expression(&mut self, expr: &mut Expr) -> Result<(), QueryError> {
        match expr {
            Expr::Function(call) => self.call(call),
            _ => expr.try_visit_children_mut(|child| self.expression(child)),
        }
    }

    fn call(&mut self, call: &mut FunctionCall) -> Result<(), QueryError> {
        if !is_aggregate_function(&call.name) {
            return call
                .expressions_mut()
                .try_for_each(|expr| self.expression(expr));
        }
        let mut inputs = Inputs::default();
        if let Some(filter) = &mut call.filter {
            inputs.filter = Some(self.take_slot(filter)?);
        }
        if !ordered_set::is_ordered_set(call) {
            for arg in &mut call.args {
                self.bind(arg, &mut inputs)?;
            }
        }
        let argument_slots = inputs.expressions.len();
        for order in &mut call.order_by {
            if let Some((name, _)) = inputs.expressions[..argument_slots]
                .iter()
                .find(|(_, expr)| expr.structurally_eq(&order.expr))
            {
                self.memory.try_grow(name.len())?;
                order.expr = Expr::Column(name.clone());
            } else {
                self.bind(&mut order.expr, &mut inputs)?;
            }
        }
        if inputs.filter.is_some() || !inputs.expressions.is_empty() {
            grow_vec(&mut self.inputs, 1, &mut self.memory)?;
            self.inputs.push(inputs);
        }
        Ok(())
    }

    fn bind(&mut self, expr: &mut Expr, input: &mut Inputs) -> Result<(), QueryError> {
        if matches!(
            expr,
            Expr::Column(_)
                | Expr::Param(_)
                | Expr::StringLiteral(_)
                | Expr::IntegerLiteral(_)
                | Expr::NumberLiteral(_)
                | Expr::NumericLiteral(_)
                | Expr::BoolLiteral(_)
                | Expr::Null
        ) {
            return Ok(());
        }
        grow_vec(&mut input.expressions, 1, &mut self.memory)?;
        input.expressions.push(self.take_slot(expr)?);
        Ok(())
    }

    fn take_slot(&mut self, expr: &mut Expr) -> Result<(String, Expr), QueryError> {
        // PostgreSQL identifiers cannot contain NUL. Each name is retained in the input
        // description and in the executable plan; allow the full decimal usize suffix.
        const MAX_NAME: usize = 9 + usize::BITS as usize;
        self.memory.try_grow(2 * MAX_NAME)?;
        let name = format!("\0subtotal{}", self.slots);
        self.slots += 1;
        let source = std::mem::replace(expr, Expr::Column(name.clone()));
        Ok((name, source))
    }
}

fn snapshot(
    source: &BatchRow,
    inputs: &[Inputs],
    slots: usize,
    context: &AggregateExecutionContext<'_>,
    memory: &mut QueryMemoryReservation,
) -> Result<BatchRow, QueryError> {
    memory.try_grow(add(
        mul(slots, size_of::<(String, Value)>() + size_of::<DataType>())?,
        size_of::<BatchRow>() + size_of::<Vec<DataType>>() + 4 * size_of::<usize>(),
    )?)?;
    let mut values = Vec::with_capacity(slots);
    let mut types = Vec::with_capacity(slots);
    let mut name_bytes = 0;
    for input in inputs {
        let included = state_input::row_predicate_matches(
            input.filter.as_ref().map(|(_, expr)| expr),
            source,
            context,
        )?;
        if let Some((name, _)) = &input.filter {
            memory.try_grow(name.len())?;
            name_bytes = add(name_bytes, name.len())?;
            types.push(DataType::Boolean);
            values.push((name.clone(), Value::Bool(included)));
        }
        for (name, expr) in &input.expressions {
            check_timeout(context.controls)?;
            let data_type = array_order::expression_type_with_parameter_oids(
                source,
                expr,
                context.user_functions,
                context.parameter_type_oids,
            )
            .unwrap_or(DataType::Null);
            let value =
                if included {
                    let _temporary = context.controls.reserve_query_memory(
                        super::modifiers::expression_bound(expr, source, context)?,
                    )?;
                    filter::evaluate_expr_value(
                        source,
                        expr,
                        context.params,
                        context.search_context,
                        context.user_functions,
                        context.session,
                        None,
                    )?
                } else {
                    Value::Null
                };
            memory.try_grow(add(
                add(name.len(), value_clone_bytes(&value)?)?,
                data_type_clone_bytes(&data_type)?,
            )?)?;
            name_bytes = add(name_bytes, name.len())?;
            values.push((name.clone(), value));
            types.push(data_type);
        }
    }
    memory.try_grow(lookup_bytes(slots, name_bytes)?)?;
    Ok(BatchRow::from_projected_values(values).with_optional_data_types(Some(Arc::new(types))))
}
