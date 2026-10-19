//! Flat primitive VALUES rows with admitted buffers and per-expression scratch.

use super::{
    check_timeout, BatchRow, Expr, QueryError, SourceExecution, SourceExecutionEnv, Value,
};
use crate::executor::retained_memory::{add, lookup_bytes, mul, value_clone_bytes};
use std::mem::size_of;
use std::sync::Arc;

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
    let character_types = if count > 0
        && rows.iter().flatten().any(|expr| {
            expr.any_descendant_or_self(&mut |expr| {
                matches!(
                    expr,
                    Expr::Cast {
                        data_type: crate::types::DataType::Char { .. },
                        ..
                    }
                )
            })
        }) {
        let width = rows.first().map_or(0, Vec::len);
        memory.try_grow(add(
            size_of::<Vec<crate::types::DataType>>() + 2 * size_of::<usize>(),
            mul(
                width,
                size_of::<crate::types::FieldSchema>() + size_of::<crate::types::DataType>() + 26,
            )?,
        )?)?;
        let schema = crate::sql::binder::values::schema(rows, env.user_functions, &[])?;
        Some(Arc::new(
            schema
                .fields
                .into_iter()
                .map(|field| field.data_type)
                .collect(),
        ))
    } else {
        None
    };
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
            let value = super::super::filter::evaluate_expr_value(
                &BatchRow::from_projected_values(Vec::new()),
                expression,
                env.params,
                None,
                env.user_functions,
                env.session,
                None,
            )?;
            memory.try_grow(value_clone_bytes(&value)?)?;
            values.push((format!("column{}", index + 1), value));
            drop(scratch);
        }
        output.push(
            BatchRow::from_projected_values(values)
                .with_optional_data_types(character_types.clone()),
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
        Expr::Function(function) if crate::sql::functions::is_temporal_function(&function.name) => {
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
