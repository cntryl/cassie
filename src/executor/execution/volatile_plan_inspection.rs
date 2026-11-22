//! Volatile built-in detection for execution-result-cache admission.

use std::collections::HashSet;

use crate::planner::logical::LogicalPlan;

use crate::sql::ast::{
    CommonTableExpression, CteQuery, Expr, QuerySource, QueryStatement, SelectItem,
    SelectStatement,
};
/// Reports whether a plan contains a built-in whose planner volatility is
/// volatile. Keep this classification in the planner's eligibility module so
/// cache admission and CTE inlining cannot drift into separate function lists.
pub(crate) fn plan_uses_volatile_builtin(plan: &LogicalPlan) -> bool {
    query_source_uses_volatile_builtin(&plan.source)
        || plan.projection.iter().any(select_item_uses_volatile_builtin)
        || plan.filter.as_ref().is_some_and(expr_uses_volatile_builtin)
        || plan.distinct_on.iter().any(expr_uses_volatile_builtin)
        || plan.group_by.iter().any(expr_uses_volatile_builtin)
        || plan.having.as_ref().is_some_and(expr_uses_volatile_builtin)
        || plan
            .order
            .iter()
            .any(|order| expr_uses_volatile_builtin(&order.expr))
        || plan.ctes.iter().any(cte_uses_volatile_builtin)
        || plan
            .set
            .as_ref()
            .is_some_and(|set| select_uses_volatile_builtin(&set.right))
}

fn cte_uses_volatile_builtin(cte: &CommonTableExpression) -> bool {
    match &cte.query {
        CteQuery::Simple(statement) => parsed_statement_uses_volatile_builtin(statement),
        CteQuery::Recursive {
            base, recursive, ..
        } => {
            parsed_statement_uses_volatile_builtin(base)
                || parsed_statement_uses_volatile_builtin(recursive)
        }
    }
}

fn parsed_statement_uses_volatile_builtin(
    statement: &crate::sql::ast::ParsedStatement,
) -> bool {
    match &statement.statement {
        QueryStatement::Select(select) => select_uses_volatile_builtin(select),
        _ => false,
    }
}

fn select_uses_volatile_builtin(select: &SelectStatement) -> bool {
    query_source_uses_volatile_builtin(&select.source)
        || select.projection.iter().any(select_item_uses_volatile_builtin)
        || select.filter.as_ref().is_some_and(expr_uses_volatile_builtin)
        || select.distinct_on.iter().any(expr_uses_volatile_builtin)
        || select.group_by.iter().any(expr_uses_volatile_builtin)
        || select.having.as_ref().is_some_and(expr_uses_volatile_builtin)
        || select
            .order
            .iter()
            .any(|order| expr_uses_volatile_builtin(&order.expr))
        || select.ctes.iter().any(cte_uses_volatile_builtin)
        || select
            .set
            .as_ref()
            .is_some_and(|set| select_uses_volatile_builtin(&set.right))
}

fn select_item_uses_volatile_builtin(item: &SelectItem) -> bool {
    match item {
        SelectItem::Function { function, .. } => function_is_volatile_builtin(function)
            || function.expressions().any(expr_uses_volatile_builtin),
        SelectItem::Expr { expr, .. } => expr_uses_volatile_builtin(expr),
        SelectItem::WindowFunction { function, .. } => function
            .args
            .iter()
            .chain(function.filter.as_deref())
            .chain(&function.partition_by)
            .chain(function.order_by.iter().map(|order| &order.expr))
            .any(expr_uses_volatile_builtin),
        SelectItem::Column { .. } | SelectItem::Wildcard => false,
    }
}

fn query_source_uses_volatile_builtin(source: &QuerySource) -> bool {
    match source {
        QuerySource::Values { rows } => rows
            .iter()
            .flatten()
            .any(expr_uses_volatile_builtin),
        QuerySource::Aliased { source, .. } | QuerySource::TableSample { source, .. } => {
            query_source_uses_volatile_builtin(source)
        }
        QuerySource::Subquery { select, .. } => select_uses_volatile_builtin(select),
        QuerySource::Join {
            left, right, on, ..
        } => {
            query_source_uses_volatile_builtin(left)
                || query_source_uses_volatile_builtin(right)
                || expr_uses_volatile_builtin(on)
        }
        QuerySource::TableFunction { function, .. } => {
            function.expressions().any(expr_uses_volatile_builtin)
        }
        QuerySource::Collection(_) | QuerySource::Cte(_) | QuerySource::SingleRow => false,
    }
}

fn expr_uses_volatile_builtin(expr: &Expr) -> bool {
    if let Expr::Function(function) = expr {
        if function_is_volatile_builtin(function) {
            return true;
        }
    }
    expr.any_child(expr_uses_volatile_builtin)
}

fn function_is_volatile_builtin(function: &crate::sql::ast::FunctionCall) -> bool {
    if crate::planner::cte_inlining::volatile_builtin_name(&function.name) {
        return true;
    }
    if let Some(metadata) = crate::sql::functions::function(&function.name) {
        return crate::planner::cte_inlining::volatile_builtin(&function.name, metadata.id);
    }
    false
}

pub(crate) fn plan_uses_volatile_builtin_including_views(
    plan: &LogicalPlan,
    catalog: &crate::catalog::Catalog,
) -> bool {
    let mut visited_views = HashSet::new();
    plan_uses_volatile_with_views(plan, catalog, &mut visited_views)
}

fn plan_uses_volatile_with_views(
    plan: &LogicalPlan,
    catalog: &crate::catalog::Catalog,
    visited_views: &mut HashSet<String>,
) -> bool {
    source_uses_volatile_with_views(&plan.source, catalog, visited_views)
        || plan
            .projection
            .iter()
            .any(|item| item_uses_volatile_with_views(item, catalog, visited_views))
        || plan
            .filter
            .as_ref()
            .is_some_and(|expr| expr_uses_volatile_with_views(expr, catalog, visited_views))
        || plan
            .distinct_on
            .iter()
            .any(|expr| expr_uses_volatile_with_views(expr, catalog, visited_views))
        || plan
            .group_by
            .iter()
            .any(|expr| expr_uses_volatile_with_views(expr, catalog, visited_views))
        || plan
            .having
            .as_ref()
            .is_some_and(|expr| expr_uses_volatile_with_views(expr, catalog, visited_views))
        || plan.order.iter().any(|order| {
            expr_uses_volatile_with_views(&order.expr, catalog, visited_views)
        })
        || plan
            .limit
            .as_ref()
            .is_some_and(|expr| expr_uses_volatile_with_views(expr, catalog, visited_views))
        || plan
            .offset
            .as_ref()
            .is_some_and(|expr| expr_uses_volatile_with_views(expr, catalog, visited_views))
        || plan
            .ctes
            .iter()
            .any(|cte| cte_uses_volatile_with_views(cte, catalog, visited_views))
        || plan.set.as_ref().is_some_and(|set| {
            select_uses_volatile_with_views(&set.right, catalog, visited_views)
        })
}

fn cte_uses_volatile_with_views(
    cte: &CommonTableExpression,
    catalog: &crate::catalog::Catalog,
    visited_views: &mut HashSet<String>,
) -> bool {
    match &cte.query {
        CteQuery::Simple(statement) => {
            parsed_statement_uses_volatile_with_views(statement, catalog, visited_views)
        }
        CteQuery::Recursive {
            base, recursive, ..
        } => {
            parsed_statement_uses_volatile_with_views(base, catalog, visited_views)
                || parsed_statement_uses_volatile_with_views(recursive, catalog, visited_views)
        }
    }
}

fn parsed_statement_uses_volatile_with_views(
    statement: &crate::sql::ast::ParsedStatement,
    catalog: &crate::catalog::Catalog,
    visited_views: &mut HashSet<String>,
) -> bool {
    match &statement.statement {
        QueryStatement::Select(select) => {
            select_uses_volatile_with_views(select, catalog, visited_views)
        }
        _ => false,
    }
}

fn select_uses_volatile_with_views(
    select: &SelectStatement,
    catalog: &crate::catalog::Catalog,
    visited_views: &mut HashSet<String>,
) -> bool {
    source_uses_volatile_with_views(&select.source, catalog, visited_views)
        || select
            .projection
            .iter()
            .any(|item| item_uses_volatile_with_views(item, catalog, visited_views))
        || select
            .filter
            .as_ref()
            .is_some_and(|expr| expr_uses_volatile_with_views(expr, catalog, visited_views))
        || select
            .distinct_on
            .iter()
            .any(|expr| expr_uses_volatile_with_views(expr, catalog, visited_views))
        || select
            .group_by
            .iter()
            .any(|expr| expr_uses_volatile_with_views(expr, catalog, visited_views))
        || select
            .having
            .as_ref()
            .is_some_and(|expr| expr_uses_volatile_with_views(expr, catalog, visited_views))
        || select.order.iter().any(|order| {
            expr_uses_volatile_with_views(&order.expr, catalog, visited_views)
        })
        || select
            .limit
            .as_ref()
            .is_some_and(|expr| expr_uses_volatile_with_views(expr, catalog, visited_views))
        || select
            .offset
            .as_ref()
            .is_some_and(|expr| expr_uses_volatile_with_views(expr, catalog, visited_views))
        || select
            .ctes
            .iter()
            .any(|cte| cte_uses_volatile_with_views(cte, catalog, visited_views))
        || select.set.as_ref().is_some_and(|set| {
            select_uses_volatile_with_views(&set.right, catalog, visited_views)
        })
}

fn item_uses_volatile_with_views(
    item: &SelectItem,
    catalog: &crate::catalog::Catalog,
    visited_views: &mut HashSet<String>,
) -> bool {
    match item {
        SelectItem::Function { function, .. } => {
            function_is_volatile_builtin(function)
                || function.expressions().any(|expr| {
                    expr_uses_volatile_with_views(expr, catalog, visited_views)
                })
        }
        SelectItem::Expr { expr, .. } => {
            expr_uses_volatile_with_views(expr, catalog, visited_views)
        }
        SelectItem::WindowFunction { function, .. } => function
            .args
            .iter()
            .chain(function.filter.as_deref())
            .chain(&function.partition_by)
            .chain(function.order_by.iter().map(|order| &order.expr))
            .any(|expr| expr_uses_volatile_with_views(expr, catalog, visited_views)),
        SelectItem::Column { .. } | SelectItem::Wildcard => false,
    }
}

fn source_uses_volatile_with_views(
    source: &QuerySource,
    catalog: &crate::catalog::Catalog,
    visited_views: &mut HashSet<String>,
) -> bool {
    match source {
        QuerySource::Values { rows } => rows.iter().flatten().any(|expr| {
            expr_uses_volatile_with_views(expr, catalog, visited_views)
        }),
        QuerySource::TableSample {
            source,
            rate,
            repeatable,
            ..
        } => {
            source_uses_volatile_with_views(source, catalog, visited_views)
                || expr_uses_volatile_with_views(rate, catalog, visited_views)
                || repeatable.as_ref().is_some_and(|expr| {
                    expr_uses_volatile_with_views(expr, catalog, visited_views)
                })
        }
        QuerySource::Aliased { source, .. } => {
            source_uses_volatile_with_views(source, catalog, visited_views)
        }
        QuerySource::Subquery { select, .. } => {
            select_uses_volatile_with_views(select, catalog, visited_views)
        }
        QuerySource::Join {
            left, right, on, ..
        } => {
            source_uses_volatile_with_views(left, catalog, visited_views)
                || source_uses_volatile_with_views(right, catalog, visited_views)
                || expr_uses_volatile_with_views(on, catalog, visited_views)
        }
        QuerySource::TableFunction { function, .. } => function.expressions().any(|expr| {
            expr_uses_volatile_with_views(expr, catalog, visited_views)
        }),
        QuerySource::Collection(name) => {
            let Some(view) = catalog.get_view(name) else {
                return false;
            };
            let key = view.name.to_ascii_lowercase();
            if !visited_views.insert(key.clone()) {
                return false;
            }
            let found = crate::sql::parse_statement(&view.query)
                .ok()
                .and_then(|parsed| match parsed.statement {
                    QueryStatement::Select(select) => Some(select),
                    _ => None,
                })
                .is_some_and(|select| {
                    select_uses_volatile_with_views(&select, catalog, visited_views)
                });
            visited_views.remove(&key);
            found
        }
        QuerySource::Cte(_) | QuerySource::SingleRow => false,
    }
}

fn expr_uses_volatile_with_views(
    expr: &Expr,
    catalog: &crate::catalog::Catalog,
    visited_views: &mut HashSet<String>,
) -> bool {
    if let Expr::Function(function) = expr {
        if function_is_volatile_builtin(function) {
            return true;
        }
    }
    if let Some(statement) = expr.subquery_statement() {
        if parsed_statement_uses_volatile_with_views(statement, catalog, visited_views) {
            return true;
        }
    }
    expr.any_child(|child| expr_uses_volatile_with_views(child, catalog, visited_views))
}


