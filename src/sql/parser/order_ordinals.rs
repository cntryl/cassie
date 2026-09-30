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
            Ok(OrderExpr {
                expr: output_column_at(position, projection)?,
                ..item
            })
        })
        .collect()
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
