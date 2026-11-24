//! Whole-row references to a derived table (`SELECT x FROM (SELECT ...) x`).
//!
//! The row of a derived table is the anonymous record of its output columns.
//! The rewrite applies only when every output column name is evident from the
//! subquery; any other shape is left to the binder unchanged.
use super::{
    names_output_alias, output_aliases, row_constructor, BindingContext, Catalog, Expr,
    QuerySource, SelectItem, SelectStatement,
};

/// A name that needs no quoting when it is written after `alias.`.
fn is_plain_name(name: &str) -> bool {
    !name.is_empty()
        && name.chars().all(|character| {
            character.is_ascii_lowercase() || character.is_ascii_digit() || character == '_'
        })
}

/// The columns `*` expands to when `source` is one plain table.
fn table_columns(
    source: &QuerySource,
    catalog: &Catalog,
    context: &BindingContext,
) -> Option<Vec<String>> {
    let path = match source {
        QuerySource::Collection(path) => path,
        QuerySource::Aliased { source, .. } => match source.as_ref() {
            QuerySource::Collection(path) => path,
            _ => return None,
        },
        _ => return None,
    };
    let canonical = super::super::resolve_relation_name(path.as_str(), catalog, context).ok()?;
    let row_type = catalog.relation_row_type_unconstrained(&canonical)?;
    Some(
        row_type
            .fields
            .iter()
            .map(|field| field.name.clone())
            .collect(),
    )
}

/// The output column names of `select`, or `None` when one is not evident.
fn output_names(
    select: &SelectStatement,
    catalog: &Catalog,
    context: &BindingContext,
) -> Option<Vec<String>> {
    if select.set.is_some() {
        return None;
    }
    let mut names = Vec::new();
    for item in &select.projection {
        match item {
            SelectItem::Wildcard => {
                names.extend(table_columns(&select.source, catalog, context)?);
            }
            SelectItem::Column { name, alias } => names.push(match alias {
                Some(alias) => alias.clone(),
                None => name.rsplit('.').next()?.to_owned(),
            }),
            SelectItem::Function { alias, .. }
            | SelectItem::Expr { alias, .. }
            | SelectItem::WindowFunction { alias, .. } => names.push(alias.clone()?),
        }
    }
    names
        .iter()
        .all(|name| is_plain_name(name))
        .then_some(names)
}

fn replace_name(expr: &mut Expr, alias: &str, row: &Expr) {
    if matches!(expr, Expr::Column(name) if name.eq_ignore_ascii_case(alias)) {
        *expr = row.clone();
        return;
    }
    let _ = expr.try_visit_children_mut::<()>(|child| {
        replace_name(child, alias, row);
        Ok(())
    });
}

/// Replaces a bare reference to the alias of a derived table with the record
/// of its output columns, in the projection, `WHERE` and `ORDER BY`.
///
/// A derived column named like the alias wins, as in PostgreSQL.
pub(super) fn rewrite(select: &mut SelectStatement, catalog: &Catalog, context: &BindingContext) {
    let QuerySource::Subquery {
        alias,
        select: inner,
        ..
    } = &select.source
    else {
        return;
    };
    let Some(names) = output_names(inner, catalog, context) else {
        return;
    };
    if names.is_empty()
        || !is_plain_name(alias)
        || names.iter().any(|name| name.eq_ignore_ascii_case(alias))
    {
        return;
    }
    let alias = alias.clone();
    let row = row_constructor(
        names
            .iter()
            .map(|name| Expr::Column(format!("{alias}.{name}")))
            .collect(),
    );
    let aliased_orders: Vec<bool> = {
        let aliases = output_aliases(select);
        select
            .order
            .iter()
            .map(|order| names_output_alias(&order.expr, &aliases))
            .collect()
    };
    for item in &mut select.projection {
        let whole_row = match &*item {
            SelectItem::Column {
                name,
                alias: output,
            } if name.eq_ignore_ascii_case(&alias) => Some(SelectItem::Expr {
                expr: row.clone(),
                alias: Some(output.clone().unwrap_or_else(|| name.clone())),
            }),
            _ => None,
        };
        if let Some(whole_row) = whole_row {
            *item = whole_row;
            continue;
        }
        match &mut *item {
            SelectItem::Expr { expr, .. } => replace_name(expr, &alias, &row),
            SelectItem::Function { function, .. } => {
                for argument in &mut function.args {
                    replace_name(argument, &alias, &row);
                }
            }
            _ => {}
        }
    }
    if let Some(filter) = &mut select.filter {
        replace_name(filter, &alias, &row);
    }
    for (order, aliased) in select.order.iter_mut().zip(aliased_orders) {
        if !aliased {
            replace_name(&mut order.expr, &alias, &row);
        }
    }
}
