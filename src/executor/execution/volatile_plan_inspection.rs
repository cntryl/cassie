//! Volatile built-in detection for execution-result-cache admission.

use std::collections::HashSet;

use crate::planner::logical::LogicalPlan;

use crate::sql::ast::{
    CommonTableExpression, CteQuery, Expr, QuerySource, QueryStatement, SelectItem,
    SelectStatement,
};

fn function_is_volatile_builtin(function: &crate::sql::ast::FunctionCall) -> bool {
    if crate::planner::cte_inlining::volatile_builtin_name(&function.name) {
        return true;
    }
    crate::sql::functions::function(&function.name).is_some_and(|metadata| {
        crate::planner::cte_inlining::volatile_builtin(&function.name, metadata.id)
    })
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
            // Stored view bodies resolve type names from the view's own
            // database and schema, including enum/domain/composite types.
            let context = crate::sql::binder::BindingContext::unscoped(
                crate::catalog::relation_database_name(&view.name)
                    .unwrap_or_else(|| crate::sql::binder::BindingContext::default().database),
                vec![
                    crate::catalog::relation_schema_name(&view.name),
                    "public".to_string(),
                ],
            );
            let found = crate::sql::parse_statement_in_catalog(catalog, &context, &view.query)
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

#[cfg(test)]
mod tests {
    use super::plan_uses_volatile_builtin_including_views;
    use crate::catalog::{Catalog, FunctionArgMeta, FunctionMeta, ViewMeta, Volatility};
    use crate::types::{DataType, Schema};

    #[test]
    fn should_find_non_immutable_udf_in_a_stored_view_body() {
        // Arrange
        let catalog = Catalog::new();
        catalog.register_function(FunctionMeta {
            name: "volatile_cache_random_value".to_owned(),
            args: Vec::<FunctionArgMeta>::new(),
            return_type: DataType::Float,
            volatility: Volatility::Volatile,
            body: "random()".to_owned(),
            object_id: 0,
        });
        catalog.register_view(ViewMeta::new(
            "volatile_cache_udf_view",
            "SELECT volatile_cache_random_value() AS value",
            Schema::from_iter([(String::from("value"), DataType::Float)]),
        ));
        let parsed = crate::sql::parse_statement(
            "SELECT value FROM volatile_cache_udf_view",
        )
        .expect("outer view query");
        let bound = crate::sql::bind(parsed, &catalog).expect("bound view query");
        let plan = crate::planner::logical::plan(&bound).expect("logical view plan");

        // Act
        let contains_volatile_udf = plan_uses_volatile_builtin_including_views(&plan, &catalog);

        // Assert
        assert!(contains_volatile_udf);
    }
}
