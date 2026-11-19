//! Validate GROUPING at its own SELECT level, before primary-key dependencies
//! can introduce extra executor group keys.
use crate::app::{CassieError, DiagnosticError};
use crate::sql::ast::{Expr, FunctionCall, QuerySource, SelectItem, SelectStatement};

pub(super) fn validate(select: &SelectStatement) -> Result<(), CassieError> {
    let keys = &select.group_by;
    validate_source(&select.source)?;
    for expression in select
        .filter
        .iter()
        .chain(keys)
        .chain(&select.limit)
        .chain(&select.offset)
    {
        validate_expr(expression, None)?;
    }
    for item in &select.projection {
        match item {
            SelectItem::Function { function, .. } => validate_call(function, Some(keys))?,
            SelectItem::Expr { expr, .. } => validate_expr(expr, Some(keys))?,
            SelectItem::WindowFunction { function, .. } => {
                for expr in function
                    .args
                    .iter()
                    .chain(&function.partition_by)
                    .chain(function.filter.as_deref())
                    .chain(function.order_by.iter().map(|key| &key.expr))
                {
                    validate_expr(expr, None)?;
                }
            }
            SelectItem::Wildcard | SelectItem::Column { .. } => {}
        }
    }
    for expr in select
        .having
        .iter()
        .chain(select.order.iter().map(|order| &order.expr))
        .chain(&select.distinct_on)
    {
        validate_expr(expr, Some(keys))?;
    }
    Ok(())
}

fn validate_source(source: &QuerySource) -> Result<(), CassieError> {
    match source {
        QuerySource::Values { rows } => rows
            .iter()
            .flatten()
            .try_for_each(|expr| validate_expr(expr, None)),
        QuerySource::Aliased { source, .. } => validate_source(source),
        QuerySource::Join {
            left, right, on, ..
        } => {
            validate_source(left)?;
            validate_source(right)?;
            validate_expr(on, None)
        }
        QuerySource::TableSample {
            source,
            rate,
            repeatable,
            ..
        } => {
            validate_source(source)?;
            validate_expr(rate, None)?;
            repeatable
                .iter()
                .try_for_each(|expr| validate_expr(expr, None))
        }
        QuerySource::TableFunction { function, .. } => validate_call(function, None),
        QuerySource::Collection(_)
        | QuerySource::Cte(_)
        | QuerySource::SingleRow
        | QuerySource::Subquery { .. } => Ok(()),
    }
}

fn validate_expr(expr: &Expr, keys: Option<&[Expr]>) -> Result<(), CassieError> {
    if let Expr::Function(call) = expr {
        return validate_call(call, keys);
    }
    match expr.find_map_child(|child| validate_expr(child, keys).err()) {
        Some(error) => Err(error),
        None => Ok(()),
    }
}

fn validate_call(call: &FunctionCall, keys: Option<&[Expr]>) -> Result<(), CassieError> {
    if call.name.eq_ignore_ascii_case("grouping") {
        let [key] = call.args.as_slice() else {
            return Err(DiagnosticError::new(
                if call.args.is_empty() {
                    "42601"
                } else {
                    "0A000"
                },
                "GROUPING requires one grouping column",
            )
            .into());
        };
        if call.has_modifiers() {
            return Err(DiagnosticError::new(
                "42601",
                "GROUPING does not accept aggregate modifiers",
            )
            .into());
        }
        if !matches!(key, Expr::Column(_))
            || !keys.is_some_and(|keys| keys.iter().any(|grouped| grouped.structurally_eq(key)))
        {
            return Err(DiagnosticError::new(
                "42803",
                "arguments to GROUPING must be grouping columns of the current query level",
            )
            .into());
        }
        return Ok(());
    }
    let nested_keys = if crate::sql::functions::is_aggregate_function(&call.name) {
        None
    } else {
        keys
    };
    call.expressions()
        .try_for_each(|expr| validate_expr(expr, nested_keys))
}
