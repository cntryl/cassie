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
                if let Expr::Column(name) = &item.expr {
                    let matches = projection
                        .iter()
                        .filter(|output| output.output_name().eq_ignore_ascii_case(name))
                        .count();
                    if matches > 1 {
                        return Err(SqlError::diagnostic(
                            "42702",
                            format!("ORDER BY \"{name}\" is ambiguous"),
                        ));
                    }
                }
                return Ok(item);
            };
            let expr = output_column_at(position, projection)?;
            if !ordinal_survives_alias_capture(position, &expr, projection) {
                return Ok(item);
            }
            Ok(OrderExpr { expr, ..item })
        })
        .collect()
}

/// The compound sorter sees the output tuple after both operands have run.
/// Keep validated positions instead of substituting a first-operand expression.
pub(super) fn validate_set_order_ordinals(
    order: &[OrderExpr],
    projection: &[SelectItem],
) -> Result<(), SqlError> {
    for item in order {
        if let Expr::IntegerLiteral(position) = item.expr {
            output_column_at(position, projection)?;
        }
    }
    Ok(())
}

/// ORDER BY column references resolve to the first select-list alias with
/// that name, so a rewritten position must not be captured by a different
/// item's alias (for example `SELECT g, v AS g ... ORDER BY 1`).
fn ordinal_survives_alias_capture(position: i64, expr: &Expr, projection: &[SelectItem]) -> bool {
    let Expr::Column(name) = expr else {
        return true;
    };
    let captures = projection
        .iter()
        .enumerate()
        .filter(|(_, item)| {
            alias_of(item).is_some_and(|alias| {
                crate::sql::ColumnIdentifierPath::stored_field_key(alias)
                    == crate::sql::ColumnIdentifierPath::reference_field_key(name)
            })
        })
        .map(|(index, _)| index)
        .collect::<Vec<_>>();
    let target = usize::try_from(position - 1).ok();
    captures.is_empty() || captures.len() == 1 && Some(captures[0]) == target
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
    if projection.iter().take(index).any(is_wildcard) {
        return Err(wildcard());
    }
    match projection.get(index).ok_or_else(not_in_list)? {
        SelectItem::Wildcard => Err(wildcard()),
        SelectItem::Column { name, .. }
            if crate::sql::ColumnIdentifierPath::is_qualified_wildcard(name) =>
        {
            Err(wildcard())
        }
        SelectItem::Column { name, alias } => Ok(Expr::Column(alias.as_deref().map_or_else(
            || name.clone(),
            crate::sql::ColumnIdentifierPath::stored_field_key,
        ))),
        SelectItem::Function {
            alias: Some(alias), ..
        }
        | SelectItem::Expr {
            alias: Some(alias), ..
        }
        | SelectItem::WindowFunction {
            alias: Some(alias), ..
        } => Ok(Expr::Column(
            crate::sql::ColumnIdentifierPath::stored_field_key(alias),
        )),
        SelectItem::Function {
            function,
            alias: None,
        } => Ok(Expr::Function(Box::new(function.clone()))),
        SelectItem::Expr { expr, alias: None } => Ok(expr.clone()),
        SelectItem::WindowFunction { alias: None, .. } => Err(SqlError::unsupported(
            "ORDER BY position of an unaliased window function is not supported".into(),
        )),
    }
}

fn is_wildcard(item: &SelectItem) -> bool {
    matches!(item, SelectItem::Wildcard)
        || matches!(item, SelectItem::Column { name, .. } if crate::sql::ColumnIdentifierPath::is_qualified_wildcard(name))
}
