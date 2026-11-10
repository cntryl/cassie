//! Flat primitive VALUES rows with admitted buffers and per-expression scratch.

use super::{
    check_timeout, BatchRow, Expr, QueryError, SourceExecution, SourceExecutionEnv, Value,
};
use crate::executor::retained_memory::{add, lookup_bytes, mul, value_clone_bytes};
use std::mem::size_of;
use std::sync::Arc;

#[cfg(test)]
#[path = "source_values_json_delete_tests.rs"]
mod json_delete_tests;

#[cfg(test)]
thread_local! {
    static CANCEL_AFTER_VALUE: std::cell::RefCell<Option<crate::runtime::QueryCancellationHandle>> = const { std::cell::RefCell::new(None) };
}

#[cfg(test)]
fn cancel_after_value_once(handle: crate::runtime::QueryCancellationHandle) {
    CANCEL_AFTER_VALUE.with(|slot| *slot.borrow_mut() = Some(handle));
}

pub(super) fn execute(
    env: &SourceExecutionEnv<'_>,
    rows: &[Vec<Expr>],
    row_budget: Option<usize>,
) -> SourceExecution {
    check_timeout(env.controls)?;
    let count = rows.len().min(row_budget.unwrap_or(rows.len()));
    let mut memory = env.controls.reserve_query_memory(add(
        size_of::<Vec<BatchRow>>()
            + size_of::<crate::runtime::QueryMemoryReservation>()
            + 2 * size_of::<usize>(),
        mul(count, size_of::<BatchRow>())?,
    )?)?;
    let semantic_types = semantic_types(rows, count, env, &mut memory)?;
    let mut output = Vec::with_capacity(count);
    for expressions in rows.iter().take(count) {
        check_timeout(env.controls)?;
        let names_bytes = mul(expressions.len(), 26)?;
        memory.try_grow(add(
            mul(expressions.len(), size_of::<(String, Value)>())?,
            add(names_bytes, lookup_bytes(expressions.len(), names_bytes)?)?,
        )?)?;
        let mut values = Vec::with_capacity(expressions.len());
        for (index, expression) in expressions.iter().enumerate() {
            check_timeout(env.controls)?;
            let scratch = env
                .controls
                .reserve_query_memory(scratch_bytes(expression, env.params)?)?;
            let evaluation = super::filter::EvaluationMemory::new(env.controls)?;
            let value = super::filter::evaluate_source_argument(
                &BatchRow::from_projected_values(Vec::new()),
                expression,
                env.params,
                env.user_functions,
                env.session,
                env.controls,
                &evaluation,
            )?;
            #[cfg(test)]
            CANCEL_AFTER_VALUE.with(|slot| {
                if let Some(handle) = slot.borrow_mut().take() {
                    handle.cancel();
                }
            });
            check_timeout(env.controls)?;
            memory.try_grow(value_clone_bytes(&value)?)?;
            values.push((format!("column{}", index + 1), value));
            drop(evaluation);
            drop(scratch);
        }
        output.push(
            BatchRow::from_projected_values(values)
                .with_optional_data_types(semantic_types.clone()),
        );
    }
    check_timeout(env.controls)?;
    let memory = Arc::new(memory);
    for row in &mut output {
        let owned = std::mem::replace(row, BatchRow::from_projected_values(Vec::new()));
        *row = owned.with_query_memory(Some(Arc::clone(&memory)));
    }
    Ok((vec![output], Vec::new()))
}

fn semantic_types(
    rows: &[Vec<Expr>],
    count: usize,
    env: &SourceExecutionEnv<'_>,
    memory: &mut crate::runtime::QueryMemoryReservation,
) -> Result<Option<Arc<Vec<crate::types::DataType>>>, QueryError> {
    let needs_types = count > 0
        && rows.iter().flatten().any(|expr| {
            expr.any_descendant_or_self(&mut |expr| {
                if let Expr::Function(function) = expr {
                    if crate::sql::functions::function(&function.name).is_some_and(|metadata| {
                        metadata
                            .return_type
                            .fixed_type()
                            .as_ref()
                            .is_some_and(crate::executor::semantic::jsonb_type)
                    }) || env
                        .user_functions
                        .get(&function.name)
                        .is_some_and(|metadata| {
                            crate::executor::semantic::jsonb_type(&metadata.return_type)
                        })
                    {
                        return true;
                    }
                }
                matches!(
                    expr,
                    Expr::Cast {
                        data_type: crate::types::DataType::Char { .. }
                            | crate::types::DataType::Jsonb
                            | crate::types::DataType::Array(_),
                        ..
                    }
                )
            })
        });
    if !needs_types {
        return Ok(None);
    }
    let width = rows.first().map_or(0, Vec::len);
    memory.try_grow(add(
        size_of::<Vec<crate::types::DataType>>() + 2 * size_of::<usize>(),
        mul(
            width,
            size_of::<crate::types::FieldSchema>() + size_of::<crate::types::DataType>() + 26,
        )?,
    )?)?;
    let schema = crate::sql::binder::values::schema(rows, env.user_functions, &[])?;
    Ok(Some(Arc::new(
        schema
            .fields
            .into_iter()
            .map(|field| field.data_type)
            .collect(),
    )))
}

pub(in crate::executor::execution) fn scratch_bytes(
    expression: &Expr,
    params: &[Value],
) -> Result<usize, QueryError> {
    let own = match expression {
        Expr::StringLiteral(text) => text.len(),
        Expr::Param(index) => params.get(*index).map_or(Ok(0), value_clone_bytes)?,
        Expr::Cast {
            data_type:
                crate::types::DataType::Char {
                    length: Some(length),
                },
            ..
        } => mul(
            usize::try_from(*length).map_err(|_| {
                crate::app::CassieError::ResourceLimit("VALUES character width overflow".into())
            })?,
            4,
        )?,
        Expr::Function(function)
            if crate::sql::functions::is_temporal_function(&function.name)
                || function
                    .name
                    .eq_ignore_ascii_case(crate::sql::ast::ARRAY_CONSTRUCTOR_FUNCTION)
                || function.name.eq_ignore_ascii_case("_cassie_array_value") =>
        {
            64
        }
        // A subquery (`_` below) admits its own rows when it runs.
        Expr::Function(_) | Expr::Column(_) => {
            return Err(crate::app::CassieError::Unsupported(
                "functions and subqueries in VALUES are not yet supported".into(),
            )
            .into());
        }
        _ => 0,
    };
    // Primitive casts/comparisons may retain input, scalar and output carriers
    // together. Integer/float formatting and cast diagnostics fit the fixed tail.
    let mut total = add(mul(own, 32)?, 1024)?;
    expression.try_visit_children(|child| {
        total = add(total, scratch_bytes(child, params)?)?;
        Ok::<_, QueryError>(())
    })?;
    Ok(total)
}
