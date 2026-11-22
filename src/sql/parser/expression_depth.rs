//! Bound the combined operand tree before adding a postfix cast chain.

use crate::sql::ast::{
    CteQuery, Expr, ParsedStatement, QuerySource, QueryStatement, SelectItem, SelectStatement,
};
use crate::sql::SqlError;
use std::cell::Cell;

pub(super) const MAX_POSTFIX_DEPTH: usize = 128;
const MAX_RECURSIVE_EXPRESSION_DEPTH: usize = 128;

thread_local! {
    static NOT_PARSE_DEPTH: Cell<usize> = const { Cell::new(0) };
    static CASE_PARSE_DEPTH: Cell<usize> = const { Cell::new(0) };
}

struct ParseDepthGuard {
    kind: ParseDepthKind,
    previous: usize,
}

#[derive(Clone, Copy)]
enum ParseDepthKind {
    Not,
    Case,
}

impl Drop for ParseDepthGuard {
    fn drop(&mut self) {
        match self.kind {
            ParseDepthKind::Not => NOT_PARSE_DEPTH.with(|depth| depth.set(self.previous)),
            ParseDepthKind::Case => CASE_PARSE_DEPTH.with(|depth| depth.set(self.previous)),
        }
    }
}

pub(super) fn enter_not_parse_depth() -> Result<ParseDepthGuard, SqlError> {
    enter_parse_depth(&NOT_PARSE_DEPTH, "NOT", ParseDepthKind::Not)
}

pub(super) fn enter_case_parse_depth() -> Result<ParseDepthGuard, SqlError> {
    enter_parse_depth(&CASE_PARSE_DEPTH, "CASE", ParseDepthKind::Case)
}

fn enter_parse_depth(
    depth: &'static std::thread::LocalKey<Cell<usize>>,
    expression: &str,
    kind: ParseDepthKind,
) -> Result<ParseDepthGuard, SqlError> {
    depth.with(|depth| {
        let previous = depth.get();
        if previous >= MAX_RECURSIVE_EXPRESSION_DEPTH {
            return Err(SqlError::resource_limit(format!(
                "{expression} nesting exceeds {MAX_RECURSIVE_EXPRESSION_DEPTH}"
            )));
        }
        depth.set(previous + 1);
        Ok(ParseDepthGuard { kind, previous })
    })
}

pub(super) fn limit_error() -> SqlError {
    SqlError::resource_limit(format!(
        "postfix expression nesting exceeds {MAX_POSTFIX_DEPTH}"
    ))
}

fn guard(depth: usize) -> Result<(), SqlError> {
    if depth > MAX_POSTFIX_DEPTH {
        Err(limit_error())
    } else {
        Ok(())
    }
}

pub(super) fn validate(expr: &Expr, depth: usize) -> Result<(), SqlError> {
    guard(depth)?;
    if let Some(query) = expr.subquery_statement() {
        statement(query, depth + 1)?;
    }
    expr.try_visit_children(|child| validate(child, depth + 1))
}

fn statement(query: &ParsedStatement, depth: usize) -> Result<(), SqlError> {
    guard(depth)?;
    if let QueryStatement::Select(query) = &query.statement {
        select(query, depth)?;
    }
    Ok(())
}

fn select(query: &SelectStatement, depth: usize) -> Result<(), SqlError> {
    guard(depth)?;
    source(&query.source, depth + 1)?;
    for cte in &query.ctes {
        match &cte.query {
            CteQuery::Simple(query) => statement(query, depth + 1)?,
            CteQuery::Recursive {
                base, recursive, ..
            } => {
                statement(base, depth + 1)?;
                statement(recursive, depth + 1)?;
            }
        }
    }
    for item in &query.projection {
        match item {
            SelectItem::Expr { expr, .. } => validate(expr, depth + 1)?,
            SelectItem::Function { function, .. } => {
                for arg in function.expressions() {
                    validate(arg, depth + 1)?;
                }
            }
            SelectItem::WindowFunction { function, .. } => {
                for expr in function
                    .args
                    .iter()
                    .chain(function.filter.as_deref())
                    .chain(&function.partition_by)
                {
                    validate(expr, depth + 1)?;
                }
                for order in &function.order_by {
                    validate(&order.expr, depth + 1)?;
                }
            }
            SelectItem::Wildcard | SelectItem::Column { .. } => {}
        }
    }
    for expr in query
        .filter
        .iter()
        .chain(&query.group_by)
        .chain(&query.having)
        .chain(&query.limit)
        .chain(&query.offset)
        .chain(&query.distinct_on)
    {
        validate(expr, depth + 1)?;
    }
    for order in &query.order {
        validate(&order.expr, depth + 1)?;
    }
    if let Some(set) = &query.set {
        select(&set.right, depth + 1)?;
    }
    Ok(())
}

fn source(query: &QuerySource, depth: usize) -> Result<(), SqlError> {
    guard(depth)?;
    match query {
        QuerySource::Values { rows } => {
            for expr in rows.iter().flatten() {
                validate(expr, depth + 1)?;
            }
        }

        QuerySource::Aliased { source: child, .. } => source(child, depth + 1)?,
        QuerySource::TableSample {
            source: child,
            rate,
            repeatable,
            ..
        } => {
            source(child, depth + 1)?;
            validate(rate, depth + 1)?;
            if let Some(seed) = repeatable {
                validate(seed, depth + 1)?;
            }
        }
        QuerySource::TableFunction { function, form, .. } => {
            for arg in function
                .expressions()
                .chain(form.iter().flat_map(|form| form.graph_expressions()))
            {
                validate(arg, depth + 1)?;
            }
        }
        QuerySource::Subquery { select: query, .. } => select(query, depth + 1)?,
        QuerySource::Join {
            left, right, on, ..
        } => {
            source(left, depth + 1)?;
            source(right, depth + 1)?;
            validate(on, depth + 1)?;
        }
        QuerySource::Collection(_) | QuerySource::Cte(_) | QuerySource::SingleRow => {}
    }
    Ok(())
}
