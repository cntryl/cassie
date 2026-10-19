//! Native volatile and recursive CTEs retain sharing even with NOT MATERIALIZED.
use crate::app::CassieError;
use crate::catalog::{Catalog, Volatility};
use crate::runtime::QueryExecutionControls;
use crate::sql::ast::{Expr, QuerySource, SelectItem, SelectStatement};
use crate::sql::functions::{self, FunctionId};

pub(super) fn select(
    select: &SelectStatement,
    catalog: &Catalog,
    controls: &QueryExecutionControls,
) -> Result<bool, CassieError> {
    controls.check_interruption()?;
    if !select.ctes.is_empty() {
        return Err(CassieError::Unsupported(
            "nested NOT MATERIALIZED CTE scope expansion remains pending under #801".into(),
        ));
    }
    let mut pure = source(&select.source, catalog, controls)?;
    for item in &select.projection {
        match item {
            SelectItem::Expr { expr, .. } => pure &= expression(expr, catalog, controls)?,
            SelectItem::Function { function, .. } => {
                pure &= function_pure(function, catalog, controls)?;
            }
            SelectItem::WindowFunction { function, .. } => {
                for expr in function
                    .args
                    .iter()
                    .chain(&function.partition_by)
                    .chain(function.order_by.iter().map(|order| &order.expr))
                {
                    pure &= expression(expr, catalog, controls)?;
                }
            }
            _ => {}
        }
    }
    for expr in select
        .filter
        .iter()
        .chain(select.having.iter())
        .chain(&select.group_by)
        .chain(&select.distinct_on)
        .chain(select.limit.iter())
        .chain(select.offset.iter())
        .chain(select.order.iter().map(|order| &order.expr))
    {
        pure &= expression(expr, catalog, controls)?;
    }
    if let Some(set) = &select.set {
        pure &= self::select(&set.right, catalog, controls)?;
    }
    Ok(pure)
}

fn function_pure(
    function: &crate::sql::ast::FunctionCall,
    catalog: &Catalog,
    controls: &QueryExecutionControls,
) -> Result<bool, CassieError> {
    controls.check_interruption()?;
    let mut pure = if let Some(definition) = functions::function(&function.name) {
        definition.id != FunctionId::SetConfig
    } else {
        match catalog.function_volatility(&function.name) {
            Some(Volatility::Volatile) => false,
            Some(Volatility::Immutable | Volatility::Stable) => true,
            None => return Err(CassieError::Unsupported(
                "NOT MATERIALIZED routine volatility resolution remains pending under #801/#810"
                    .into(),
            )),
        }
    };
    for expr in function.expressions() {
        pure &= expression(expr, catalog, controls)?;
    }
    Ok(pure)
}

fn expression(
    expr: &Expr,
    catalog: &Catalog,
    controls: &QueryExecutionControls,
) -> Result<bool, CassieError> {
    controls.check_interruption()?;
    if let Expr::Function(function) = expr {
        return function_pure(function, catalog, controls);
    }
    if expr.is_subquery_node() {
        return Err(CassieError::Unsupported(
            "NOT MATERIALIZED EXISTS scope expansion remains pending under #801/#800".into(),
        ));
    }
    let mut pure = true;
    expr.try_visit_children(|child| {
        pure &= expression(child, catalog, controls)?;
        Ok::<_, CassieError>(())
    })?;
    Ok(pure)
}

fn source(
    source: &QuerySource,
    catalog: &Catalog,
    controls: &QueryExecutionControls,
) -> Result<bool, CassieError> {
    controls.check_interruption()?;
    match source {
        QuerySource::Subquery { select, .. } => self::select(select, catalog, controls),
        QuerySource::Aliased { source: inner, .. } => self::source(inner, catalog, controls),
        QuerySource::Join {
            left, right, on, ..
        } => Ok(self::source(left, catalog, controls)?
            & self::source(right, catalog, controls)?
            & expression(on, catalog, controls)?),
        QuerySource::TableFunction { function, .. } => function_pure(function, catalog, controls),
        QuerySource::Values { rows } => {
            let mut pure = true;
            for expr in rows.iter().flatten() {
                pure &= expression(expr, catalog, controls)?;
            }
            Ok(pure)
        }
        _ => Ok(true),
    }
}

pub(super) fn flat(select: &SelectStatement) -> bool {
    !select.distinct && select.distinct_on.is_empty() && select.group_by.is_empty()
        && select.having.is_none() && select.order.is_empty() && select.limit.is_none()
        && select.offset.is_none() && select.set.is_none()
        && select.projection.iter().all(|item| match item {
            SelectItem::WindowFunction { .. } => false,
            SelectItem::Function { function, .. } => !functions::is_aggregate_function(&function.name),
            SelectItem::Expr { expr, .. } => !expr.any_descendant_or_self(&mut |expr| matches!(expr, Expr::Function(function) if functions::is_aggregate_function(&function.name))),
            _ => true,
        })
}
