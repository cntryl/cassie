//! Rewrite only qualified outer references across an existing nested scope.

use super::{CassieError, ColumnIdentifierPath, Expr, Namespace, QuerySource, SelectItem};
use crate::sql::ast::{CteQuery, ParsedStatement, QueryStatement, SelectStatement};

pub(super) fn rewrite(
    statement: &mut ParsedStatement,
    outer: &[Namespace],
) -> Result<(), CassieError> {
    if let QueryStatement::Select(select) = &mut statement.statement {
        query(select, outer)?;
    }
    Ok(())
}

fn shadows(source: &QuerySource, name: &str) -> bool {
    match source {
        QuerySource::Aliased { alias, .. } | QuerySource::Subquery { alias, .. } => {
            ColumnIdentifierPath::parse(alias).is_ok_and(|path| path.declared_name() == name)
        }
        QuerySource::Collection(path) => {
            crate::catalog::local_name(path).eq_ignore_ascii_case(name)
        }
        QuerySource::Cte(alias) | QuerySource::TableFunction { name: alias, .. } => {
            alias.eq_ignore_ascii_case(name)
        }
        QuerySource::Join { left, right, .. } => shadows(left, name) || shadows(right, name),
        QuerySource::SingleRow => false,
    }
}

fn expression(
    expr: &mut Expr,
    source: &QuerySource,
    outer: &[Namespace],
) -> Result<(), CassieError> {
    if let Expr::Column(name) = expr {
        let path = ColumnIdentifierPath::parse(name).map_err(CassieError::Planner)?;
        if let Some(qualifier) = path.namespace_qualifier() {
            if !shadows(source, &qualifier) && outer.iter().any(|space| space.name == qualifier) {
                *name = super::reference(name, outer, false)?;
            }
        }
        return Ok(());
    }
    if let Expr::Exists(statement) = expr {
        return rewrite(statement, outer);
    }
    let mut failure = None;
    *expr = expr.map_children(|child| {
        let mut child = child.clone();
        if let Err(error) = expression(&mut child, source, outer) {
            failure = Some(error);
        }
        child
    });
    failure.map_or(Ok(()), Err)
}

pub(super) fn query(select: &mut SelectStatement, outer: &[Namespace]) -> Result<(), CassieError> {
    let scope = select.source.clone();
    source(&mut select.source, &scope, outer)?;
    for item in &mut select.projection {
        match item {
            SelectItem::Column { name, .. } => {
                let mut expr = Expr::Column(name.clone());
                expression(&mut expr, &select.source, outer)?;
                if let Expr::Column(next) = expr {
                    *name = next;
                }
            }
            SelectItem::Expr { expr, .. } => expression(expr, &select.source, outer)?,
            SelectItem::Function { function, .. } => {
                for expr in &mut function.args {
                    expression(expr, &select.source, outer)?;
                }
            }
            SelectItem::WindowFunction { function, .. } => {
                for expr in function.args.iter_mut().chain(&mut function.partition_by) {
                    expression(expr, &select.source, outer)?;
                }
                for order in &mut function.order_by {
                    expression(&mut order.expr, &select.source, outer)?;
                }
            }
            SelectItem::Wildcard => {}
        }
    }
    for expr in select
        .filter
        .iter_mut()
        .chain(&mut select.having)
        .chain(&mut select.group_by)
        .chain(&mut select.distinct_on)
        .chain(&mut select.limit)
        .chain(&mut select.offset)
    {
        expression(expr, &select.source, outer)?;
    }
    for order in &mut select.order {
        expression(&mut order.expr, &select.source, outer)?;
    }
    for cte in &mut select.ctes {
        match &mut cte.query {
            CteQuery::Simple(statement) => rewrite(statement, outer)?,
            CteQuery::Recursive {
                base, recursive, ..
            } => {
                rewrite(base, outer)?;
                rewrite(recursive, outer)?;
            }
        }
    }
    if let Some(set) = &mut select.set {
        query(&mut set.right, outer)?;
    }
    Ok(())
}

fn source(
    source: &mut QuerySource,
    scope: &QuerySource,
    outer: &[Namespace],
) -> Result<(), CassieError> {
    match source {
        QuerySource::Join {
            left, right, on, ..
        } => {
            expression(on, scope, outer)?;
            self::source(left, scope, outer)?;
            self::source(right, scope, outer)?;
        }
        QuerySource::Subquery {
            select,
            lateral: true,
            ..
        } => query(select, outer)?,
        QuerySource::TableFunction {
            function,
            lateral: true,
            ..
        } => {
            for expr in &mut function.args {
                expression(expr, scope, outer)?;
            }
        }
        QuerySource::Aliased { source, .. } => self::source(source, scope, outer)?,
        _ => {}
    }
    Ok(())
}
