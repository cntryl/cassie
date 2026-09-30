use super::{Expr, OrderExpr, SelectItem, SqlError};

/// Rewrites each `ORDER BY <integer>` item to the output column at that
/// 1-based position of `projection`, as PostgreSQL does. Any other ORDER BY
/// expression is left unchanged.
///
/// # Errors
///
/// Returns an error when a position is outside the select list, or when it
/// cannot be resolved because the select list contains `*`.
pub(super) fn resolve_order_ordinals(
    order: Vec<OrderExpr>,
    projection: &[SelectItem],
) -> Result<Vec<OrderExpr>, SqlError> {
    order
        .into_iter()
        .map(|item| {
            let Expr::IntegerLiteral(position) = item.expr else {
                return Ok(item);
            };
            let expr = output_column_at(position, projection)?;
            ensure_unambiguous(position, &expr, projection)?;
            Ok(OrderExpr { expr, ..item })
        })
        .collect()
}

/// ORDER BY column references resolve to the first select-list alias with
/// that name, so a rewritten position must not be captured by a different
/// item's alias (for example `SELECT g, v AS g ... ORDER BY 1`).
fn ensure_unambiguous(
    position: i64,
    expr: &Expr,
    projection: &[SelectItem],
) -> Result<(), SqlError> {
    let Expr::Column(name) = expr else {
        return Ok(());
    };
    let captured_by = projection
        .iter()
        .position(|item| alias_of(item).is_some_and(|alias| alias.eq_ignore_ascii_case(name)));
    let target = usize::try_from(position - 1).ok();
    match captured_by {
        Some(index) if Some(index) != target => Err(SqlError::unsupported(format!(
            "ORDER BY position {position} is ambiguous: output name \"{name}\" is used by more than one select-list item"
        ))),
        _ => Ok(()),
    }
}

fn alias_of(item: &SelectItem) -> Option<&str> {
    match item {
        SelectItem::Wildcard => None,
        SelectItem::Column { alias, .. }
        | SelectItem::Function { alias, .. }
        | SelectItem::Expr { alias, .. }
        | SelectItem::WindowFunction { alias, .. } => alias.as_deref(),
    }
}

fn output_column_at(position: i64, projection: &[SelectItem]) -> Result<Expr, SqlError> {
    let not_in_list = || {
        SqlError::syntax(format!(
            "ORDER BY position {position} is not in select list"
        ))
    };
    let index = usize::try_from(position)
        .ok()
        .and_then(|position| position.checked_sub(1))
        .ok_or_else(not_in_list)?;
    let wildcard =
        || SqlError::unsupported("ORDER BY position is not supported with SELECT *".into());
    if projection
        .iter()
        .take(index)
        .any(|item| matches!(item, SelectItem::Wildcard))
    {
        return Err(wildcard());
    }
    match projection.get(index).ok_or_else(not_in_list)? {
        SelectItem::Wildcard => Err(wildcard()),
        SelectItem::Column { name, alias } => {
            Ok(Expr::Column(alias.clone().unwrap_or_else(|| name.clone())))
        }
        SelectItem::Function {
            alias: Some(alias), ..
        }
        | SelectItem::Expr {
            alias: Some(alias), ..
        }
        | SelectItem::WindowFunction {
            alias: Some(alias), ..
        } => Ok(Expr::Column(alias.clone())),
        SelectItem::Function {
            function,
            alias: None,
        } => Ok(Expr::Function(function.clone())),
        SelectItem::Expr { expr, alias: None } => Ok(expr.clone()),
        SelectItem::WindowFunction { alias: None, .. } => Err(SqlError::unsupported(
            "ORDER BY position of an unaliased window function is not supported".into(),
        )),
    }
}
