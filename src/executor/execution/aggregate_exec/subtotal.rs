//! Freeze row-level aggregate inputs once before computing detail and total.
//! Bare columns already have this lifetime in the source batch. Ordered-set
//! direct arguments are evaluated once per group by their own aggregate.
use std::mem::size_of;
use std::sync::Arc;

use crate::executor::{
    array_order,
    retained_memory::{add, data_type_clone_bytes, lookup_bytes, mul, value_clone_bytes},
};
use crate::types::DataType;

use super::{
    check_timeout, filter, group_memory, ordered_set, state_input, AggregateExecutionContext,
    AggregateSpec, BatchRow, Expr, FunctionCall, GroupMemory, QueryError, Value,
};

struct Inputs<'a> {
    call: &'a FunctionCall,
    filter_name: Option<String>,
    expressions: Vec<(String, &'a Expr)>,
}

pub(super) fn prepare(
    rows: &mut [BatchRow],
    specs: &[AggregateSpec],
    context: &AggregateExecutionContext<'_>,
    memory: &mut GroupMemory,
) -> Result<Vec<AggregateSpec>, QueryError> {
    let mut bound = specs.to_vec();
    let mut inputs = Vec::new();
    let mut slots = 0;
    for (spec, original) in bound.iter_mut().zip(specs) {
        let call = &original.function;
        let mut input = Inputs {
            call,
            filter_name: None,
            expressions: Vec::new(),
        };
        if let Some(filter) = &mut spec.function.filter {
            let name = slot_name(&mut slots);
            **filter = Expr::Column(name.clone());
            input.filter_name = Some(name);
        }
        if !ordered_set::is_ordered_set(call) {
            for (arg, expr) in spec.function.args.iter_mut().zip(&call.args) {
                input.bind(arg, expr, &mut slots);
            }
        }
        for (order, original_order) in spec.function.order_by.iter_mut().zip(&call.order_by) {
            let shared_argument = (!ordered_set::is_ordered_set(call))
                .then(|| {
                    call.args
                        .iter()
                        .position(|arg| arg.structurally_eq(&original_order.expr))
                })
                .flatten();
            if let Some(index) = shared_argument {
                order.expr = spec.function.args[index].clone();
            } else {
                input.bind(&mut order.expr, &original_order.expr, &mut slots);
            }
        }
        if input.filter_name.is_some() || !input.expressions.is_empty() {
            inputs.push(input);
        }
    }
    for row in rows {
        check_timeout(context.controls)?;
        memory.add(group_memory::serial_row_bytes(row, context.controls)?)?;
        if slots > 0 {
            let values = snapshot(row, &inputs, slots, context, memory)?;
            let source = std::mem::replace(row, BatchRow::from_projected_values(Vec::new()));
            *row = values.with_outer_scope(Arc::new(source));
        }
    }
    Ok(bound)
}

// NUL is outside the SQL identifier domain. The slots are local to this
// physical row and their names never enter SELECT output or a stored plan.
fn slot_name(slots: &mut usize) -> String {
    let name = format!("\0subtotal{slots}");
    *slots += 1;
    name
}

impl<'a> Inputs<'a> {
    fn bind(&mut self, target: &mut Expr, expression: &'a Expr, slots: &mut usize) {
        if matches!(
            expression,
            Expr::Column(_)
                | Expr::Param(_)
                | Expr::StringLiteral(_)
                | Expr::IntegerLiteral(_)
                | Expr::NumberLiteral(_)
                | Expr::NumericLiteral(_)
                | Expr::BoolLiteral(_)
                | Expr::Null
        ) {
            return;
        }
        let name = slot_name(slots);
        *target = Expr::Column(name.clone());
        self.expressions.push((name, expression));
    }
}

fn snapshot(
    source: &BatchRow,
    inputs: &[Inputs<'_>],
    slots: usize,
    context: &AggregateExecutionContext<'_>,
    memory: &mut GroupMemory,
) -> Result<BatchRow, QueryError> {
    memory.add(add(
        mul(slots, size_of::<(String, Value)>() + size_of::<DataType>())?,
        size_of::<BatchRow>() + size_of::<Vec<DataType>>() + 4 * size_of::<usize>(),
    )?)?;
    let mut values = Vec::with_capacity(slots);
    let mut types = Vec::with_capacity(slots);
    let mut name_bytes = 0;
    for input in inputs {
        let included = state_input::row_filter_matches(input.call, source, context)?;
        if let Some(name) = &input.filter_name {
            memory.add(name.len())?;
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
            memory.add(add(
                add(name.len(), value_clone_bytes(&value)?)?,
                data_type_clone_bytes(&data_type)?,
            )?)?;
            name_bytes = add(name_bytes, name.len())?;
            values.push((name.clone(), value));
            types.push(data_type);
        }
    }
    memory.add(lookup_bytes(slots, name_bytes)?)?;
    Ok(BatchRow::from_projected_values(values).with_optional_data_types(Some(Arc::new(types))))
}
