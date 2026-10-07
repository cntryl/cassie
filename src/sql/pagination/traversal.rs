//! Visit bounds in the complete statement tree before planning any source.

use crate::app::CassieError;
use crate::sql::ast::{
    CteQuery, Expr, InsertSource, ParsedStatement, QuerySource, QueryStatement, SelectItem,
    SelectStatement,
};

type Visitor<'a> = dyn FnMut(&mut Option<Expr>, bool) -> Result<(), CassieError> + 'a;

mod read;
pub(super) use read::plan_needs_resolution;

pub(super) fn visit_statement(
    statement: &mut ParsedStatement,
    visitor: &mut Visitor<'_>,
) -> Result<(), CassieError> {
    match &mut statement.statement {
        QueryStatement::Select(select) => visit_select(select, visitor),
        QueryStatement::Explain(explain) => visit_statement(&mut explain.statement, visitor),
        QueryStatement::Insert(insert) => {
            match &mut insert.source {
                InsertSource::Select(select) => visit_select(select, visitor)?,
                InsertSource::Values(rows) => {
                    for value in rows.iter_mut().flatten() {
                        visit_expr(value, visitor)?;
                    }
                }
            }
            if let Some(conflict) = &mut insert.on_conflict {
                if let crate::sql::ast::InsertConflictAction::DoUpdate {
                    assignments,
                    filter,
                } = &mut conflict.action
                {
                    for (_, expr) in assignments {
                        visit_expr(expr, visitor)?;
                    }
                    if let Some(expr) = filter {
                        visit_expr(expr, visitor)?;
                    }
                }
            }
            visit_items(&mut insert.returning, visitor)?;
            Ok(())
        }
        QueryStatement::Update(update) => {
            for (_, expr) in &mut update.assignments {
                visit_expr(expr, visitor)?;
            }
            if let Some(expr) = &mut update.filter {
                visit_expr(expr, visitor)?;
            }
            visit_items(&mut update.returning, visitor)?;
            Ok(())
        }
        QueryStatement::Delete(delete) => {
            if let Some(expr) = &mut delete.filter {
                visit_expr(expr, visitor)?;
            }
            visit_items(&mut delete.returning, visitor)?;
            Ok(())
        }
        _ => Ok(()),
    }
}

fn visit_select(
    select: &mut SelectStatement,
    visitor: &mut Visitor<'_>,
) -> Result<(), CassieError> {
    visitor(&mut select.limit, false)?;
    visitor(&mut select.offset, true)?;
    for cte in &mut select.ctes {
        match &mut cte.query {
            CteQuery::Simple(statement) => visit_statement(statement, visitor)?,
            CteQuery::Recursive {
                base, recursive, ..
            } => {
                visit_statement(base, visitor)?;
                visit_statement(recursive, visitor)?;
            }
        }
    }
    visit_source(&mut select.source, visitor)?;
    if let Some(set) = &mut select.set {
        visit_select(&mut set.right, visitor)?;
    }
    visit_items(&mut select.projection, visitor)?;
    for expr in select
        .filter
        .iter_mut()
        .chain(&mut select.having)
        .chain(&mut select.distinct_on)
        .chain(&mut select.group_by)
    {
        visit_expr(expr, visitor)?;
    }
    for order in &mut select.order {
        visit_expr(&mut order.expr, visitor)?;
    }
    Ok(())
}

fn visit_items(items: &mut [SelectItem], visitor: &mut Visitor<'_>) -> Result<(), CassieError> {
    for item in items {
        match item {
            SelectItem::Expr { expr, .. } => visit_expr(expr, visitor)?,
            SelectItem::Function { function, .. } => {
                for expr in &mut function.args {
                    visit_expr(expr, visitor)?;
                }
            }
            SelectItem::WindowFunction { function, .. } => {
                for expr in function.args.iter_mut().chain(&mut function.partition_by) {
                    visit_expr(expr, visitor)?;
                }
                for order in &mut function.order_by {
                    visit_expr(&mut order.expr, visitor)?;
                }
            }
            _ => {}
        }
    }
    Ok(())
}

fn visit_source(source: &mut QuerySource, visitor: &mut Visitor<'_>) -> Result<(), CassieError> {
    match source {
        QuerySource::Aliased { source, .. } => visit_source(source, visitor)?,
        QuerySource::Subquery { select, .. } => visit_select(select, visitor)?,
        QuerySource::Join {
            left, right, on, ..
        } => {
            visit_source(left, visitor)?;
            visit_source(right, visitor)?;
            visit_expr(on, visitor)?;
        }
        QuerySource::TableFunction { function, .. } => {
            for expr in &mut function.args {
                visit_expr(expr, visitor)?;
            }
        }
        _ => {}
    }
    Ok(())
}

fn visit_expr(expr: &mut Expr, visitor: &mut Visitor<'_>) -> Result<(), CassieError> {
    match expr {
        Expr::Exists(statement) => visit_statement(statement, visitor)?,
        Expr::Binary { left, right, .. } => {
            visit_expr(left, visitor)?;
            visit_expr(right, visitor)?;
        }
        Expr::Cast { expr, .. } | Expr::IsNull { expr, .. } | Expr::Not { expr } => {
            visit_expr(expr, visitor)?;
        }
        Expr::InList { expr, values, .. } => {
            visit_expr(expr, visitor)?;
            for value in values {
                visit_expr(value, visitor)?;
            }
        }
        Expr::Between {
            expr, low, high, ..
        } => {
            visit_expr(expr, visitor)?;
            visit_expr(low, visitor)?;
            visit_expr(high, visitor)?;
        }
        Expr::Case {
            operand,
            branches,
            else_expr,
        } => {
            for expr in operand.iter_mut().chain(else_expr) {
                visit_expr(expr, visitor)?;
            }
            for (when, then) in branches {
                visit_expr(when, visitor)?;
                visit_expr(then, visitor)?;
            }
        }
        Expr::Function(function) => {
            for expr in &mut function.args {
                visit_expr(expr, visitor)?;
            }
        }
        _ => {}
    }
    Ok(())
}
