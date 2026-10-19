//! A target alias that a mutation drops while binding must still name the
//! target inside its correlated subqueries: `UPDATE t AS x SET v =
//! (SELECT ... WHERE s.id = x.id)` reads `t.id` from the target row.
use super::{Expr, ParsedStatement, QuerySource, QueryStatement, SelectItem, SelectStatement};
use crate::sql::ast::CteQuery;
use crate::sql::ColumnIdentifierPath;

/// `UPDATE t AS alias`: the alias names `t` inside the statement's subqueries.
pub(super) fn rename_update_alias(statement: &mut crate::sql::ast::UpdateStatement, alias: &str) {
    let target = crate::catalog::local_name(statement.table.as_ref()).to_ascii_lowercase();
    rename_in_subqueries(
        statement
            .assignments
            .iter_mut()
            .map(|(_, expr)| expr)
            .chain(statement.filter.iter_mut())
            .chain(item_exprs(&mut statement.returning)),
        alias,
        &target,
    );
}

/// `DELETE FROM t AS alias`: the alias names `t` inside the subqueries.
pub(super) fn rename_delete_alias(statement: &mut crate::sql::ast::DeleteStatement, alias: &str) {
    let target = crate::catalog::local_name(statement.table.as_ref()).to_ascii_lowercase();
    rename_in_subqueries(
        statement
            .filter
            .iter_mut()
            .chain(item_exprs(&mut statement.returning)),
        alias,
        &target,
    );
}

/// Renames `from.column` to `to.column` in every subquery of `exprs`.
fn rename_in_subqueries<'a>(exprs: impl IntoIterator<Item = &'a mut Expr>, from: &str, to: &str) {
    let from = ColumnIdentifierPath::parse(from)
        .map_or_else(|_| from.to_ascii_lowercase(), |path| path.declared_name());
    for expr in exprs {
        subqueries(expr, &from, to);
    }
}

/// Every expression of a RETURNING list.
fn item_exprs(items: &mut [SelectItem]) -> impl Iterator<Item = &mut Expr> {
    items.iter_mut().flat_map(|item| match item {
        SelectItem::Expr { expr, .. } => vec![expr],
        SelectItem::Function { function, .. } => function.expressions_mut().collect(),
        SelectItem::Wildcard | SelectItem::Column { .. } | SelectItem::WindowFunction { .. } => {
            Vec::new()
        }
    })
}

fn subqueries(expr: &mut Expr, from: &str, to: &str) {
    if let Some(statement) = expr.subquery_statement_mut() {
        self::statement(statement, from, to);
    }
    let _ = expr.try_visit_children_mut(|child| {
        subqueries(child, from, to);
        Ok::<_, ()>(())
    });
}

fn statement(statement: &mut ParsedStatement, from: &str, to: &str) {
    if let QueryStatement::Select(select) = &mut statement.statement {
        self::select(select, from, to);
    }
}

fn select(select: &mut SelectStatement, from: &str, to: &str) {
    if shadows(&select.source, from) {
        return;
    }
    for cte in &mut select.ctes {
        match &mut cte.query {
            CteQuery::Simple(body) => statement(body, from, to),
            CteQuery::Recursive {
                base, recursive, ..
            } => {
                statement(base, from, to);
                statement(recursive, from, to);
            }
        }
    }
    source(&mut select.source, from, to);
    for item in &mut select.projection {
        if let SelectItem::Column { name, alias } = item {
            if let Some(renamed) = renamed(name, from, to) {
                if alias.is_none() {
                    *alias = ColumnIdentifierPath::parse(name)
                        .ok()
                        .map(|path| path.implicit_output_label());
                }
                *name = renamed;
            }
        }
    }
    for expr in item_exprs(&mut select.projection)
        .chain(select.filter.iter_mut())
        .chain(select.having.iter_mut())
        .chain(&mut select.group_by)
        .chain(&mut select.distinct_on)
        .chain(select.order.iter_mut().map(|order| &mut order.expr))
    {
        self::expr(expr, from, to);
    }
    if let Some(set) = &mut select.set {
        self::select(&mut set.right, from, to);
    }
}

fn source(source: &mut QuerySource, from: &str, to: &str) {
    match source {
        QuerySource::Aliased { source, .. } => self::source(source, from, to),
        QuerySource::Subquery { select, .. } => self::select(select, from, to),
        QuerySource::Join {
            left, right, on, ..
        } => {
            self::source(left, from, to);
            self::source(right, from, to);
            expr(on, from, to);
        }
        QuerySource::TableFunction { function, .. } => {
            for arg in function.expressions_mut() {
                expr(arg, from, to);
            }
        }
        QuerySource::Values { rows } => {
            for value in rows.iter_mut().flatten() {
                expr(value, from, to);
            }
        }
        QuerySource::Collection(_) | QuerySource::Cte(_) | QuerySource::SingleRow => {}
    }
}

fn expr(expr: &mut Expr, from: &str, to: &str) {
    if let Expr::Column(name) = expr {
        if let Some(renamed) = renamed(name, from, to) {
            *name = renamed;
        }
        return;
    }
    if let Some(statement) = expr.subquery_statement_mut() {
        self::statement(statement, from, to);
    }
    let _ = expr.try_visit_children_mut(|child| {
        self::expr(child, from, to);
        Ok::<_, ()>(())
    });
}

fn renamed(name: &str, from: &str, to: &str) -> Option<String> {
    let path = ColumnIdentifierPath::parse(name).ok()?;
    (path.namespace_qualifier()? == from).then(|| format!("{to}.{}", path.field_lookup_key()))
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
        QuerySource::Values { .. } | QuerySource::SingleRow => false,
    }
}
