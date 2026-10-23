//! Global DISTINCT and per-aggregate ordering on the controlled scalar fallback.

use std::cmp::Ordering;
use std::collections::HashSet;
use std::mem::size_of;

use crate::app::CassieError;

use crate::executor::batch::{BatchRow, RowAccess};
use crate::executor::filter;
use crate::executor::retained_memory::{
    add, hash_table_bytes, mul, serialized_json_bytes, value_clone_bytes,
};
use crate::executor::semantic::SemanticValue;
use crate::runtime::{QueryExecutionControls, QueryMemoryReservation};
use crate::sql::ast::{Expr, FunctionCall, NullsOrder, SortDirection};
use crate::types::Value;

use super::state::AggregateAccumulator;
use super::{check_timeout, AggregateExecutionContext, QueryError};

struct Seen {
    keys: HashSet<Vec<SemanticValue>>,
    table_bytes: usize,
    memory: QueryMemoryReservation,
}

impl Seen {
    fn new(controls: &QueryExecutionControls) -> Result<Self, QueryError> {
        Ok(Self {
            keys: HashSet::new(),
            table_bytes: 0,
            memory: controls.reserve_query_memory(0)?,
        })
    }

    fn insert(
        &mut self,
        value: &Value,
        array_key: Option<&SemanticValue>,
        additional: Option<&Value>,
        controls: &QueryExecutionControls,
    ) -> Result<bool, QueryError> {
        check_timeout(controls)?;
        let additional_bytes = additional.map(key_bound).transpose()?.unwrap_or_default();
        let bytes = add(add(key_bound(value)?, additional_bytes)?, 64)?;
        let candidate = controls.reserve_query_memory(bytes)?;
        let mut key = vec![array_key.cloned().unwrap_or_else(|| scalar_key(value))];
        if let Some(additional) = additional {
            key.push(scalar_key(additional));
        }
        if self.keys.contains(&key) {
            return Ok(false);
        }
        let growing = self.keys.len() == self.keys.capacity();
        let extra = if growing {
            hash_table_bytes::<Vec<SemanticValue>>(mul(self.keys.capacity().max(2), 2)?)?
        } else {
            0
        };
        self.memory.try_grow(extra)?;
        if growing {
            self.keys
                .try_reserve(1)
                .map_err(|error| allocation_error("DISTINCT aggregate keys", &error))?;
        }
        self.keys.insert(key);
        self.memory.absorb(candidate);
        let table = hash_table_bytes::<Vec<SemanticValue>>(self.keys.capacity())?;
        self.memory
            .shrink_to(add(self.memory.bytes() - extra - self.table_bytes, table)?);
        self.table_bytes = table;
        check_timeout(controls)?;
        Ok(true)
    }
}

fn key_bound(value: &Value) -> Result<usize, QueryError> {
    Ok(match value {
        Value::String(text) => text.len().max(27),
        Value::Vector(vector) => mul(vector.values.len(), size_of::<u32>())?,
        // JSON-backed arrays may retain both semantic slots and escaped element text.
        Value::Json(json) => add(
            mul(value_clone_bytes(value)?, 2)?,
            add(mul(serialized_json_bytes(json)?, 2)?, 128)?,
        )?,
        Value::Null | Value::Bool(_) | Value::Int64(_) | Value::Float64(_) => 0,
        Value::Typed(typed) => typed.heap_bytes(),
    })
}

fn scalar_key(value: &Value) -> SemanticValue {
    match value {
        Value::String(text) => SemanticValue::String(text.clone()),
        _ => SemanticValue::from_value(value),
    }
}

fn allocation_error(owner: &str, error: &impl std::fmt::Display) -> QueryError {
    CassieError::ResourceLimit(format!("unable to allocate {owner}: {error}")).into()
}

struct OrderedInput {
    value: Option<Value>,
    delimiter: Option<Value>,
    array_key: Option<SemanticValue>,
    keys: Vec<SemanticValue>,
}

struct ModifierAccumulator {
    seen: Seen,
    state_memory: QueryMemoryReservation,
    accumulator: AggregateAccumulator,
    memory: QueryMemoryReservation,
    inputs: Vec<OrderedInput>,
}

impl ModifierAccumulator {
    fn new(
        function: &FunctionCall,
        context: &AggregateExecutionContext<'_>,
    ) -> Result<Self, QueryError> {
        Ok(Self {
            seen: Seen::new(context.controls)?,
            state_memory: context.controls.reserve_query_memory(0)?,
            accumulator: AggregateAccumulator::new(function),
            memory: context.controls.reserve_query_memory(0)?,
            inputs: Vec::new(),
        })
    }

    fn push_row(
        &mut self,
        function: &FunctionCall,
        row: &BatchRow,
        context: &AggregateExecutionContext<'_>,
    ) -> Result<(), QueryError> {
        if !super::state_input::row_filter_matches(function, row, context)? {
            return Ok(());
        }
        let input_memory = context
            .controls
            .reserve_query_memory(input_bound(function, row, context)?)?;
        let value = super::state_input::selected_row_input(function, row, context)?;
        let delimiter = crate::sql::functions::function(&function.name)
            .is_some_and(|metadata| metadata.id == crate::sql::functions::FunctionId::StringAgg)
            .then(|| super::state_input::selected_row_argument(function, 1, row, context))
            .transpose()?;
        let key_memory = context.controls.reserve_query_memory(add(
            add(
                key_bound(&value)?,
                delimiter
                    .as_ref()
                    .map(key_bound)
                    .transpose()?
                    .unwrap_or_default(),
            )?,
            64,
        )?)?;
        let array_key = function
            .args
            .first()
            .map(|argument| semantic_key(row, argument, &value, context))
            .filter(|key| matches!(key, SemanticValue::Array(_)));
        if function.is_distinct()
            && !self.seen.insert(
                &value,
                array_key.as_ref(),
                delimiter.as_ref(),
                context.controls,
            )?
        {
            return Ok(());
        }
        if function.order_by.is_empty() {
            transition(
                &mut self.accumulator,
                &mut self.state_memory,
                function,
                value,
                delimiter,
                array_key,
                context.controls,
            )?;
            #[cfg(test)]
            if let Some(observer) = context.after_partition_row {
                observer(self.seen.keys.len());
            }
            return Ok(());
        }
        self.memory.try_grow(add(
            add(
                value_clone_bytes(&value)?,
                delimiter
                    .as_ref()
                    .map(value_clone_bytes)
                    .transpose()?
                    .unwrap_or_default(),
            )?,
            mul(function.order_by.len(), size_of::<SemanticValue>())?,
        )?)?;
        if let Some(key) = &array_key {
            self.memory.try_grow(key.estimated_bytes())?;
        }
        let keys = ordered_keys(
            function,
            row,
            &value,
            array_key.as_ref(),
            context,
            &mut self.memory,
        )?;
        grow_ordered_inputs(&mut self.inputs, &mut self.memory)?;
        self.inputs.push(OrderedInput {
            value: Some(value),
            delimiter,
            array_key,
            keys,
        });
        self.memory.absorb(input_memory);
        self.memory.absorb(key_memory);
        #[cfg(test)]
        if let Some(observer) = context.after_partition_row {
            observer(self.inputs.len());
        }
        Ok(())
    }

    fn finish(
        mut self,
        function: &FunctionCall,
        context: &AggregateExecutionContext<'_>,
    ) -> Result<Value, QueryError> {
        if !self.inputs.is_empty() {
            let (indices, _sort_memory) = sorted_indices(&self.inputs, function, context.controls)?;
            for index in indices {
                check_timeout(context.controls)?;
                let input = &mut self.inputs[index];
                transition(
                    &mut self.accumulator,
                    &mut self.state_memory,
                    function,
                    input
                        .value
                        .take()
                        .expect("each sorted input is visited once"),
                    input.delimiter.take(),
                    input.array_key.take(),
                    context.controls,
                )?;
            }
        }
        check_timeout(context.controls)?;
        self.accumulator.finish()
    }
}

pub(super) fn evaluate(
    function: &FunctionCall,
    rows: &[BatchRow],
    context: &AggregateExecutionContext<'_>,
) -> Result<Value, QueryError> {
    let mut accumulator = ModifierAccumulator::new(function, context)?;
    for row in rows {
        check_timeout(context.controls)?;
        accumulator.push_row(function, row, context)?;
    }
    accumulator.finish(function, context)
}

fn ordered_keys(
    function: &FunctionCall,
    row: &BatchRow,
    value: &Value,
    array_key: Option<&SemanticValue>,
    context: &AggregateExecutionContext<'_>,
    memory: &mut QueryMemoryReservation,
) -> Result<Vec<SemanticValue>, QueryError> {
    let mut keys = Vec::with_capacity(function.order_by.len());
    for order in &function.order_by {
        check_timeout(context.controls)?;
        let same_input = function
            .args
            .first()
            .is_some_and(|input| input.structurally_eq(&order.expr));
        if same_input {
            memory.try_grow(add(key_bound(value)?, 64)?)?;
            keys.push(array_key.cloned().unwrap_or_else(|| scalar_key(value)));
        } else {
            let temporary = context.controls.reserve_query_memory(expression_bound(
                &order.expr,
                row,
                context,
            )?)?;
            let value = filter::evaluate_expr_value(
                row,
                &order.expr,
                context.params,
                context.search_context,
                context.user_functions,
                context.session,
                None,
            )?;
            memory.try_grow(add(key_bound(&value)?, 64)?)?;
            keys.push(semantic_key(row, &order.expr, &value, context));
            drop(value);
            drop(temporary);
        }
    }
    Ok(keys)
}

fn grow_ordered_inputs(
    inputs: &mut Vec<OrderedInput>,
    memory: &mut QueryMemoryReservation,
) -> Result<(), QueryError> {
    if inputs.len() == inputs.capacity() {
        let old = inputs.capacity();
        let next = old.max(1).checked_mul(2).ok_or_else(|| {
            QueryError::from(CassieError::ResourceLimit(
                "aggregate input capacity overflow".into(),
            ))
        })?;
        memory.try_grow(mul(next, size_of::<OrderedInput>())?)?;
        inputs
            .try_reserve_exact(next - inputs.len())
            .map_err(|error| allocation_error("ordered aggregate inputs", &error))?;
        let actual = inputs.capacity();
        if actual > next {
            memory.try_grow(mul(actual - next, size_of::<OrderedInput>())?)?;
        }
        memory.shrink_to(memory.bytes() - mul(old, size_of::<OrderedInput>())?);
    }
    Ok(())
}

fn input_bound(
    function: &FunctionCall,
    row: &BatchRow,
    context: &AggregateExecutionContext<'_>,
) -> Result<usize, QueryError> {
    Ok(function.args.iter().try_fold(0, |bytes, expression| {
        add(bytes, expression_bound(expression, row, context)?)
    })?)
}

fn expression_bound(
    expression: &Expr,
    row: &BatchRow,
    context: &AggregateExecutionContext<'_>,
) -> Result<usize, QueryError> {
    check_timeout(context.controls)?;
    match expression {
        Expr::Column(name) => return Ok(row.get(name).map_or(Ok(0), value_clone_bytes)?),
        Expr::Param(index) => {
            return Ok(context
                .params
                .get(*index)
                .map_or(Ok(0), value_clone_bytes)?)
        }
        Expr::StringLiteral(text) => return Ok(text.len()),
        _ => {}
    }
    let mut children = 0;
    expression.try_visit_children(|child| {
        children = add(children, expression_bound(child, row, context)?)?;
        Ok::<_, QueryError>(())
    })?;
    if let Expr::Function(function) = expression {
        if context
            .user_functions
            .contains_key(&function.name.to_ascii_lowercase())
        {
            return Err(CassieError::Unsupported(
                "SQL-defined function expressions in retained aggregate modifiers require #830/#849 resource qualification".into(),
            ).into());
        }
        if function.name.eq_ignore_ascii_case("upper")
            || function.name.eq_ignore_ascii_case("lower")
        {
            let text = match function.args.first() {
                Some(Expr::Column(name)) => row.get(name).and_then(value_text),
                Some(Expr::Param(index)) => context.params.get(*index).and_then(value_text),
                Some(Expr::StringLiteral(text)) => Some(text.as_str()),
                _ => None,
            };
            if let Some(text) = text {
                let output = text.chars().try_fold(0, |bytes, character| {
                    let next: usize = if function.name.eq_ignore_ascii_case("upper") {
                        character.to_uppercase().map(char::len_utf8).sum()
                    } else {
                        character.to_lowercase().map(char::len_utf8).sum()
                    };
                    add(bytes, next)
                })?;
                return Ok(add(children, output)?);
            }
        }
    }
    // Scalar evaluation can retain its operand backing while creating its result.
    Ok(add(mul(children, 2)?, 64)?)
}

fn value_text(value: &Value) -> Option<&str> {
    match value {
        Value::String(text) => Some(text),
        _ => None,
    }
}

fn semantic_key(
    row: &BatchRow,
    expression: &Expr,
    value: &Value,
    context: &AggregateExecutionContext<'_>,
) -> SemanticValue {
    if row.has_array_types()
        && matches!(
            crate::executor::array_order::expression_type(row, expression, context.user_functions),
            Some(crate::types::DataType::Array(_))
        )
    {
        crate::executor::array_order::key(row, expression, value, context.user_functions)
    } else {
        scalar_key(value)
    }
}

fn transition(
    accumulator: &mut AggregateAccumulator,
    memory: &mut QueryMemoryReservation,
    function: &FunctionCall,
    value: Value,
    delimiter: Option<Value>,
    key: Option<SemanticValue>,
    controls: &QueryExecutionControls,
) -> Result<(), QueryError> {
    let mut retained_bytes = value_clone_bytes(&value)?;
    if let Some(delimiter) = &delimiter {
        retained_bytes = add(retained_bytes, value_clone_bytes(delimiter)?)?;
    }
    let candidate = controls.reserve_query_memory(add(
        retained_bytes,
        key.as_ref().map_or(0, SemanticValue::estimated_bytes),
    )?)?;
    accumulator.update_values(function, value, key, delimiter)?;
    memory.absorb(candidate);
    memory.shrink_to(accumulator.retained_heap_bytes()?);
    controls.aggregate_input_retained()?;
    Ok(())
}

fn sorted_indices(
    inputs: &[OrderedInput],
    function: &FunctionCall,
    controls: &QueryExecutionControls,
) -> Result<(Vec<usize>, QueryMemoryReservation), QueryError> {
    let memory = controls.reserve_query_memory(mul(inputs.len(), 2 * size_of::<usize>())?)?;
    let mut indices = (0..inputs.len()).collect::<Vec<_>>();
    let mut scratch = vec![0; inputs.len()];
    let mut width = 1;
    while width < inputs.len() {
        for start in (0..inputs.len()).step_by(width.saturating_mul(2)) {
            let middle = start.saturating_add(width).min(inputs.len());
            let end = middle.saturating_add(width).min(inputs.len());
            let (mut left, mut right) = (start, middle);
            for output in &mut scratch[start..end] {
                check_timeout(controls)?;
                if right == end
                    || left < middle
                        && compare(&inputs[indices[left]], &inputs[indices[right]], function)
                            != Ordering::Greater
                {
                    *output = indices[left];
                    left += 1;
                } else {
                    *output = indices[right];
                    right += 1;
                }
            }
        }
        std::mem::swap(&mut indices, &mut scratch);
        width = width.saturating_mul(2);
    }
    check_timeout(controls)?;
    Ok((indices, memory))
}

fn compare(left: &OrderedInput, right: &OrderedInput, function: &FunctionCall) -> Ordering {
    for ((left, right), order) in left.keys.iter().zip(&right.keys).zip(&function.order_by) {
        let nulls_first = order
            .nulls
            .map_or(matches!(order.direction, SortDirection::Desc), |nulls| {
                nulls == NullsOrder::First
            });
        let ordering = match (left.is_null(), right.is_null()) {
            (true, false) => {
                if nulls_first {
                    Ordering::Less
                } else {
                    Ordering::Greater
                }
            }
            (false, true) => {
                if nulls_first {
                    Ordering::Greater
                } else {
                    Ordering::Less
                }
            }
            (true, true) => Ordering::Equal,
            (false, false) => {
                let ordering = left.cmp(right);
                if matches!(order.direction, SortDirection::Desc) {
                    ordering.reverse()
                } else {
                    ordering
                }
            }
        };
        if ordering != Ordering::Equal {
            return ordering;
        }
    }
    Ordering::Equal
}
