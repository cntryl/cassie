//! Identify qualified free enclosing names inside existing nested statements.
use super::{check_timeout, ExistsResolutionContext, HashSet, QueryError, QuerySource, SelectItem};
use crate::sql::ast::{CteQuery, Expr, ParsedStatement, QueryStatement, SelectStatement};

struct Namespaces<'a> {
    source: &'a QuerySource,
    parent: Option<&'a Self>,
}
impl Namespaces<'_> {
    fn claims(&self, qualifier: &str) -> bool {
        source_claims(self.source, qualifier)
            || self.parent.is_some_and(|parent| parent.claims(qualifier))
    }
}
fn source_claims(source: &QuerySource, qualifier: &str) -> bool {
    match source {
        QuerySource::Collection(name) => crate::catalog::qualifier_variants(name)
            .iter()
            .any(|name| name == qualifier),
        QuerySource::Aliased { alias, .. } => {
            crate::sql::binder::alias_row_qualifier(alias) == qualifier
        }
        QuerySource::Subquery { alias, .. } => alias.to_ascii_lowercase() == qualifier,
        QuerySource::Cte(name) | QuerySource::TableFunction { name, .. } => {
            name.to_ascii_lowercase() == qualifier
        }
        QuerySource::Join { left, right, .. } => {
            source_claims(left, qualifier) || source_claims(right, qualifier)
        }
        QuerySource::SingleRow => false,
    }
}

pub(super) fn references(
    context: &ExistsResolutionContext<'_>,
    statement: &ParsedStatement,
    fields: &HashSet<String>,
) -> Result<bool, QueryError> {
    let _scratch = super::reserve_clone(statement, context)?;
    statement_references(context, statement, fields, None)
}
fn statement_references(
    context: &ExistsResolutionContext<'_>,
    statement: &ParsedStatement,
    fields: &HashSet<String>,
    parent: Option<&Namespaces<'_>>,
) -> Result<bool, QueryError> {
    if let QueryStatement::Select(query) = &statement.statement {
        query_references(context, query, fields, parent)
    } else {
        Ok(false)
    }
}
fn query_references(
    context: &ExistsResolutionContext<'_>,
    query: &SelectStatement,
    fields: &HashSet<String>,
    parent: Option<&Namespaces<'_>>,
) -> Result<bool, QueryError> {
    check_timeout(context.controls)?;
    let scope = Namespaces {
        source: &query.source,
        parent,
    };
    for item in &query.projection {
        let found = match item {
            SelectItem::Column { name, .. } => column_references(name, fields, &scope),
            SelectItem::Expr { expr, .. } => expression(context, expr, fields, &scope)?,
            SelectItem::Function { function, .. } => {
                expressions(context, &function.args, fields, &scope)?
            }
            SelectItem::WindowFunction { function, .. } => {
                expressions(context, &function.args, fields, &scope)?
                    || expressions(context, &function.partition_by, fields, &scope)?
                    || function.order_by.iter().try_fold(false, |found, order| {
                        Ok::<_, QueryError>(
                            found || expression(context, &order.expr, fields, &scope)?,
                        )
                    })?
            }
            SelectItem::Wildcard => false,
        };
        if found {
            return Ok(true);
        }
    }
    for expr in query
        .filter
        .iter()
        .chain(&query.distinct_on)
        .chain(&query.group_by)
        .chain(&query.having)
        .chain(query.order.iter().map(|order| &order.expr))
        .chain(&query.limit)
        .chain(&query.offset)
    {
        if expression(context, expr, fields, &scope)? {
            return Ok(true);
        }
    }
    for cte in &query.ctes {
        let found = match &cte.query {
            CteQuery::Simple(statement) => {
                statement_references(context, statement, fields, Some(&scope))?
            }
            CteQuery::Recursive {
                base, recursive, ..
            } => {
                statement_references(context, base, fields, Some(&scope))?
                    || statement_references(context, recursive, fields, Some(&scope))?
            }
        };
        if found {
            return Ok(true);
        }
    }
    if source_references(context, &query.source, fields, &scope)? {
        return Ok(true);
    }
    if let Some(set) = &query.set {
        return query_references(context, &set.right, fields, parent);
    }
    Ok(false)
}
fn source_references(
    context: &ExistsResolutionContext<'_>,
    source: &QuerySource,
    fields: &HashSet<String>,
    scope: &Namespaces<'_>,
) -> Result<bool, QueryError> {
    check_timeout(context.controls)?;
    match source {
        QuerySource::Aliased { source, .. } => source_references(context, source, fields, scope),
        QuerySource::Join {
            left, right, on, ..
        } => Ok(source_references(context, left, fields, scope)?
            || source_references(context, right, fields, scope)?
            || expression(context, on, fields, scope)?),
        QuerySource::Subquery { select, .. } => {
            query_references(context, select, fields, Some(scope))
        }
        QuerySource::TableFunction { function, .. } => {
            expressions(context, &function.args, fields, scope)
        }
        _ => Ok(false),
    }
}
fn expressions(
    context: &ExistsResolutionContext<'_>,
    exprs: &[Expr],
    fields: &HashSet<String>,
    scope: &Namespaces<'_>,
) -> Result<bool, QueryError> {
    for expr in exprs {
        if expression(context, expr, fields, scope)? {
            return Ok(true);
        }
    }
    Ok(false)
}
fn expression(
    context: &ExistsResolutionContext<'_>,
    expr: &Expr,
    fields: &HashSet<String>,
    scope: &Namespaces<'_>,
) -> Result<bool, QueryError> {
    check_timeout(context.controls)?;
    match expr {
        Expr::Column(name) => Ok(column_references(name, fields, scope)),
        Expr::Exists(statement) => statement_references(context, statement, fields, Some(scope)),
        _ => {
            let mut found = false;
            expr.try_visit_children(&mut |child| {
                found |= expression(context, child, fields, scope)?;
                Ok::<_, QueryError>(())
            })?;
            Ok(found)
        }
    }
}
fn column_references(name: &str, fields: &HashSet<String>, scope: &Namespaces<'_>) -> bool {
    let Ok(path) = crate::sql::ColumnIdentifierPath::parse(name) else {
        return false;
    };
    path.namespace_qualifier()
        .is_some_and(|qualifier| !scope.claims(&qualifier) && fields.contains(&path.lookup_key()))
}

/// Ordinary first-level occurrences keep their existing classification admission.
pub(super) fn contains_nested(statement: &ParsedStatement) -> bool {
    fn expr(value: &Expr) -> bool {
        value.any_descendant_or_self(&mut |value| matches!(value, Expr::Exists(_)))
    }
    fn source(value: &QuerySource) -> bool {
        match value {
            QuerySource::Aliased { source: value, .. } => source(value),
            QuerySource::Subquery { select: value, .. } => query(value),
            QuerySource::Join {
                left, right, on, ..
            } => source(left) || source(right) || expr(on),
            QuerySource::TableFunction { function, .. } => function.args.iter().any(expr),
            _ => false,
        }
    }
    fn query(value: &SelectStatement) -> bool {
        source(&value.source)
            || value.projection.iter().any(|item| match item {
                SelectItem::Expr { expr: value, .. } => expr(value),
                SelectItem::Function { function, .. } => function.args.iter().any(expr),
                SelectItem::WindowFunction { function, .. } => {
                    function.args.iter().chain(&function.partition_by).any(expr)
                        || function.order_by.iter().any(|order| expr(&order.expr))
                }
                _ => false,
            })
            || value
                .filter
                .iter()
                .chain(&value.distinct_on)
                .chain(&value.group_by)
                .chain(&value.having)
                .chain(value.order.iter().map(|order| &order.expr))
                .chain(&value.limit)
                .chain(&value.offset)
                .any(expr)
            || value.ctes.iter().any(|cte| match &cte.query {
                CteQuery::Simple(statement) => contains_nested(statement),
                CteQuery::Recursive {
                    base, recursive, ..
                } => contains_nested(base) || contains_nested(recursive),
            })
            || value.set.as_ref().is_some_and(|set| query(&set.right))
    }
    match &statement.statement {
        QueryStatement::Select(value) => query(value),
        _ => false,
    }
}

#[cfg(test)]
#[path = "ancestor_tests.rs"]
mod tests;
