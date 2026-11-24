//! Borrow-only graph-call detection across expression and nested query boundaries.

use std::ops::ControlFlow;

use crate::sql::ast::{
    CteQuery, Expr, ParsedStatement, QuerySource, QueryStatement, SelectItem, SelectStatement,
};
use crate::sql::functions::{self, FunctionId};

fn registered(name: &str) -> bool {
    functions::function(name).is_some_and(|metadata| {
        matches!(
            metadata.id,
            FunctionId::GraphKeyPart
                | FunctionId::GraphKeyPartTagged
                | FunctionId::GraphVertexRef
                | FunctionId::GraphEdgeRef
                | FunctionId::GraphPathToJsonb
        )
    })
}

pub(super) fn contains(expression: &Expr) -> bool {
    match expression {
        Expr::Function(function) if registered(&function.name) => return true,
        Expr::Subquery(subquery) if statement(&subquery.query) => return true,
        Expr::Exists(query) if statement(query) => return true,
        _ => {}
    }
    matches!(
        expression.try_for_each_child(|child| {
            if contains(child) {
                ControlFlow::Break(())
            } else {
                ControlFlow::Continue(())
            }
        }),
        ControlFlow::Break(())
    )
}

fn statement(query: &ParsedStatement) -> bool {
    matches!(&query.statement, QueryStatement::Select(select) if select_contains(select))
}

fn select_contains(select: &SelectStatement) -> bool {
    source(&select.source)
        || select.projection.iter().any(item)
        || select
            .filter
            .iter()
            .chain(&select.distinct_on)
            .chain(&select.group_by)
            .chain(&select.having)
            .chain(&select.limit)
            .chain(&select.offset)
            .chain(select.order.iter().map(|order| &order.expr))
            .any(contains)
        || select
            .set
            .as_ref()
            .is_some_and(|set| select_contains(&set.right))
        || select.ctes.iter().any(|cte| {
            let query = match &cte.query {
                CteQuery::Simple(query) => statement(query),
                CteQuery::Recursive {
                    base, recursive, ..
                } => statement(base) || statement(recursive),
            };
            query
                || cte.cycle.as_ref().is_some_and(|cycle| {
                    contains(&cycle.mark_value) || contains(&cycle.default_value)
                })
        })
}

fn item(item: &SelectItem) -> bool {
    match item {
        SelectItem::Function { function, .. } => {
            registered(&function.name) || function.expressions().any(contains)
        }
        SelectItem::Expr { expr, .. } => contains(expr),
        SelectItem::WindowFunction { function, .. } => function
            .args
            .iter()
            .chain(function.filter.as_deref())
            .chain(&function.partition_by)
            .chain(function.order_by.iter().map(|order| &order.expr))
            .any(contains),
        SelectItem::Wildcard | SelectItem::Column { .. } => false,
    }
}

fn source(source: &QuerySource) -> bool {
    match source {
        QuerySource::Values { rows } => rows.iter().flatten().any(contains),
        QuerySource::Aliased { source: inner, .. } => self::source(inner),
        QuerySource::TableSample {
            source: inner,
            rate,
            repeatable,
            ..
        } => self::source(inner) || contains(rate) || repeatable.as_ref().is_some_and(contains),
        QuerySource::TableFunction { function, form, .. } => function
            .expressions()
            .chain(form.iter().flat_map(|form| form.graph_expressions()))
            .any(contains),
        QuerySource::Subquery { select, .. } => select_contains(select),
        QuerySource::Join {
            left, right, on, ..
        } => self::source(left) || self::source(right) || contains(on),
        QuerySource::Collection(_) | QuerySource::Cte(_) | QuerySource::SingleRow => false,
    }
}
