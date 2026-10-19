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
    visit_statement_with_controls(statement, visitor, None)
}

pub(super) fn visit_reached_statement(
    statement: &mut ParsedStatement,
    visitor: &mut Visitor<'_>,
    controls: &crate::runtime::QueryExecutionControls,
) -> Result<(), CassieError> {
    visit_statement_with_controls(statement, visitor, Some(controls))
}

fn visit_statement_with_controls(
    statement: &mut ParsedStatement,
    visitor: &mut Visitor<'_>,
    controls: Option<&crate::runtime::QueryExecutionControls>,
) -> Result<(), CassieError> {
    match &mut statement.statement {
        QueryStatement::Select(select) => visit_select(select, visitor, controls),
        QueryStatement::Explain(explain) => {
            visit_statement_with_controls(&mut explain.statement, visitor, controls)
        }
        QueryStatement::Insert(insert) => {
            match &mut insert.source {
                InsertSource::Select(select) => visit_select(select, visitor, controls)?,
                InsertSource::Values(rows) => {
                    for value in rows.iter_mut().flatten() {
                        visit_expr(value, visitor, controls)?;
                    }
                }
            }
            if let Some(conflict) = &mut insert.on_conflict {
                if let crate::sql::ast::InsertConflictAction::DoUpdate {
                    assignments,
                    filter,
                    ..
                } = &mut conflict.action
                {
                    for (_, expr) in assignments {
                        visit_expr(expr, visitor, controls)?;
                    }
                    if let Some(expr) = filter {
                        visit_expr(expr, visitor, controls)?;
                    }
                }
            }
            visit_items(&mut insert.returning, visitor, controls)?;
            Ok(())
        }
        QueryStatement::Update(update) => {
            for (_, expr) in &mut update.assignments {
                visit_expr(expr, visitor, controls)?;
            }
            if let Some(expr) = &mut update.filter {
                visit_expr(expr, visitor, controls)?;
            }
            visit_items(&mut update.returning, visitor, controls)?;
            Ok(())
        }
        QueryStatement::Delete(delete) => {
            if let Some(expr) = &mut delete.filter {
                visit_expr(expr, visitor, controls)?;
            }
            visit_items(&mut delete.returning, visitor, controls)?;
            Ok(())
        }
        _ => Ok(()),
    }
}

fn visit_select(
    select: &mut SelectStatement,
    visitor: &mut Visitor<'_>,
    controls: Option<&crate::runtime::QueryExecutionControls>,
) -> Result<(), CassieError> {
    visitor(&mut select.limit, false)?;
    visitor(&mut select.offset, true)?;
    let live = controls
        .filter(|_| !select.ctes.is_empty())
        .map(|controls| crate::executor::reached_ctes(select, controls))
        .transpose()?;
    for (index, cte) in select.ctes.iter_mut().enumerate() {
        if live.as_ref().is_some_and(|mask| !mask.contains(index)) {
            continue;
        }
        match &mut cte.query {
            CteQuery::Simple(statement) => {
                visit_statement_with_controls(statement, visitor, controls)?;
            }
            CteQuery::Recursive {
                base, recursive, ..
            } => {
                visit_statement_with_controls(base, visitor, controls)?;
                visit_statement_with_controls(recursive, visitor, controls)?;
            }
        }
    }
    visit_source(&mut select.source, visitor, controls)?;
    if let Some(set) = &mut select.set {
        visit_select(&mut set.right, visitor, controls)?;
    }
    visit_items(&mut select.projection, visitor, controls)?;
    for expr in select
        .filter
        .iter_mut()
        .chain(&mut select.having)
        .chain(&mut select.distinct_on)
        .chain(&mut select.group_by)
    {
        visit_expr(expr, visitor, controls)?;
    }
    for order in &mut select.order {
        visit_expr(&mut order.expr, visitor, controls)?;
    }
    Ok(())
}

fn visit_items(
    items: &mut [SelectItem],
    visitor: &mut Visitor<'_>,
    controls: Option<&crate::runtime::QueryExecutionControls>,
) -> Result<(), CassieError> {
    for item in items {
        match item {
            SelectItem::Expr { expr, .. } => visit_expr(expr, visitor, controls)?,
            SelectItem::Function { function, .. } => {
                for expr in function.expressions_mut() {
                    visit_expr(expr, visitor, controls)?;
                }
            }
            SelectItem::WindowFunction { function, .. } => {
                for expr in function.args.iter_mut().chain(&mut function.partition_by) {
                    visit_expr(expr, visitor, controls)?;
                }
                for order in &mut function.order_by {
                    visit_expr(&mut order.expr, visitor, controls)?;
                }
            }
            _ => {}
        }
    }
    Ok(())
}

fn visit_source(
    source: &mut QuerySource,
    visitor: &mut Visitor<'_>,
    controls: Option<&crate::runtime::QueryExecutionControls>,
) -> Result<(), CassieError> {
    match source {
        QuerySource::Values { rows } => {
            for expr in rows.iter_mut().flatten() {
                visit_expr(expr, visitor, controls)?;
            }
        }

        QuerySource::Aliased { source, .. } => visit_source(source, visitor, controls)?,
        QuerySource::Subquery { select, .. } => visit_select(select, visitor, controls)?,
        QuerySource::Join {
            left, right, on, ..
        } => {
            visit_source(left, visitor, controls)?;
            visit_source(right, visitor, controls)?;
            visit_expr(on, visitor, controls)?;
        }
        QuerySource::TableFunction { function, .. } => {
            for expr in function.expressions_mut() {
                visit_expr(expr, visitor, controls)?;
            }
        }
        _ => {}
    }
    Ok(())
}

fn visit_expr(
    expr: &mut Expr,
    visitor: &mut Visitor<'_>,
    controls: Option<&crate::runtime::QueryExecutionControls>,
) -> Result<(), CassieError> {
    match expr {
        Expr::Exists(statement) => visit_statement_with_controls(statement, visitor, controls)?,
        Expr::RowCompare(comparison) => {
            for field in comparison.left.iter_mut().chain(&mut comparison.right) {
                visit_expr(field, visitor, controls)?;
            }
        }
        Expr::Binary { left, right, .. } => {
            visit_expr(left, visitor, controls)?;
            visit_expr(right, visitor, controls)?;
        }
        Expr::Cast { expr, .. }
        | Expr::UnaryNumeric { expr, .. }
        | Expr::IntegerNegate { expr, .. }
        | Expr::IsNull { expr, .. }
        | Expr::TruthTest { expr, .. }
        | Expr::Not { expr } => {
            visit_expr(expr, visitor, controls)?;
        }
        Expr::InList { expr, values, .. } => {
            visit_expr(expr, visitor, controls)?;
            for value in values {
                visit_expr(value, visitor, controls)?;
            }
        }
        Expr::Between {
            expr, low, high, ..
        }
        | Expr::Like {
            expr,
            pattern: low,
            escape: high,
        } => {
            visit_expr(expr, visitor, controls)?;
            visit_expr(low, visitor, controls)?;
            visit_expr(high, visitor, controls)?;
        }
        Expr::Case {
            operand,
            branches,
            else_expr,
        } => {
            for expr in operand.iter_mut().chain(else_expr) {
                visit_expr(expr, visitor, controls)?;
            }
            for (when, then) in branches {
                visit_expr(when, visitor, controls)?;
                visit_expr(then, visitor, controls)?;
            }
        }
        Expr::Function(function) => {
            for expr in function.expressions_mut() {
                visit_expr(expr, visitor, controls)?;
            }
        }
        _ => {}
    }
    Ok(())
}
