//! Query dependencies that bypass execution-result-cache admission.

use std::collections::HashSet;

use crate::planner::logical::LogicalPlan;

use crate::sql::ast::{
    CommonTableExpression, CteQuery, Expr, QuerySource, QueryStatement, SelectItem, SelectStatement,
};

#[derive(Clone, Copy)]
enum ResultCacheBypassDependency {
    VirtualCatalog,
    Volatile,
    ClockDependent,
}

impl ResultCacheBypassDependency {
    fn matches_function(self, function: &crate::sql::ast::FunctionCall) -> bool {
        match self {
            Self::VirtualCatalog => false,
            Self::Volatile => {
                crate::planner::cte_inlining::volatile_builtin_name(&function.name)
                    || crate::sql::functions::function(&function.name).is_some_and(|metadata| {
                        crate::planner::cte_inlining::volatile_builtin(&function.name, metadata.id)
                    })
            }
            Self::ClockDependent => {
                crate::sql::functions::function(&function.name).is_some_and(|metadata| {
                    metadata.id == crate::sql::functions::FunctionId::Temporal
                        && clock_function(&function.name, function.args.len())
                })
            }
        }
    }

    fn matches_expression(self, expression: &Expr) -> bool {
        if !matches!(self, Self::ClockDependent) {
            return false;
        }
        let Expr::Cast {
            expr, data_type, ..
        } = expression
        else {
            return false;
        };
        is_temporal_type(data_type) && contains_clock_input(expr)
    }
}

fn clock_function(name: &str, argument_count: usize) -> bool {
    let name = name.rsplit('.').next().unwrap_or(name);
    match name.to_ascii_lowercase().as_str() {
        "now"
        | "transaction_timestamp"
        | "statement_timestamp"
        | "clock_timestamp"
        | "current_date"
        | "timeofday" => argument_count == 0,
        "current_timestamp" | "localtimestamp" | "current_time" | "localtime" => {
            argument_count <= 1
        }
        "age" => argument_count <= 1,
        _ => false,
    }
}

fn is_temporal_type(data_type: &crate::types::DataType) -> bool {
    use crate::types::{extended::ExtendedType, DataType};
    matches!(
        data_type,
        DataType::Date
            | DataType::Time
            | DataType::Timestamp
            | DataType::Extended(
                ExtendedType::Date
                    | ExtendedType::Time { .. }
                    | ExtendedType::TimeTz { .. }
                    | ExtendedType::Timestamp { .. }
                    | ExtendedType::TimestampTz { .. }
            )
    )
}

fn contains_clock_input(expression: &Expr) -> bool {
    if let Expr::StringLiteral(value) = expression {
        if ["now", "today", "tomorrow", "yesterday"]
            .iter()
            .any(|clock_input| value.eq_ignore_ascii_case(clock_input))
        {
            return true;
        }
    }
    expression.any_child(contains_clock_input)
}

pub(crate) fn plan_uses_virtual_catalog_including_views(
    plan: &LogicalPlan,
    catalog: &crate::catalog::Catalog,
) -> bool {
    plan_uses_with_views(
        plan,
        catalog,
        &mut HashSet::new(),
        ResultCacheBypassDependency::VirtualCatalog,
    )
}

pub(crate) fn plan_uses_volatile_builtin_including_views(
    plan: &LogicalPlan,
    catalog: &crate::catalog::Catalog,
) -> bool {
    let mut visited_views = HashSet::new();
    plan_uses_with_views(
        plan,
        catalog,
        &mut visited_views,
        ResultCacheBypassDependency::Volatile,
    )
}

pub(crate) fn plan_uses_clock_dependent_builtin_including_views(
    plan: &LogicalPlan,
    catalog: &crate::catalog::Catalog,
) -> bool {
    let mut visited_views = HashSet::new();
    plan_uses_with_views(
        plan,
        catalog,
        &mut visited_views,
        ResultCacheBypassDependency::ClockDependent,
    )
}

fn plan_uses_with_views(
    plan: &LogicalPlan,
    catalog: &crate::catalog::Catalog,
    visited_views: &mut HashSet<String>,
    builtin: ResultCacheBypassDependency,
) -> bool {
    source_uses_with_views(&plan.source, catalog, visited_views, builtin)
        || plan
            .projection
            .iter()
            .any(|item| item_uses_with_views(item, catalog, visited_views, builtin))
        || plan
            .filter
            .as_ref()
            .is_some_and(|expr| expr_uses_with_views(expr, catalog, visited_views, builtin))
        || plan
            .distinct_on
            .iter()
            .any(|expr| expr_uses_with_views(expr, catalog, visited_views, builtin))
        || plan
            .group_by
            .iter()
            .any(|expr| expr_uses_with_views(expr, catalog, visited_views, builtin))
        || plan
            .having
            .as_ref()
            .is_some_and(|expr| expr_uses_with_views(expr, catalog, visited_views, builtin))
        || plan
            .order
            .iter()
            .any(|order| expr_uses_with_views(&order.expr, catalog, visited_views, builtin))
        || plan
            .limit
            .as_ref()
            .is_some_and(|expr| expr_uses_with_views(expr, catalog, visited_views, builtin))
        || plan
            .offset
            .as_ref()
            .is_some_and(|expr| expr_uses_with_views(expr, catalog, visited_views, builtin))
        || plan
            .ctes
            .iter()
            .any(|cte| cte_uses_with_views(cte, catalog, visited_views, builtin))
        || plan
            .set
            .as_ref()
            .is_some_and(|set| select_uses_with_views(&set.right, catalog, visited_views, builtin))
}

fn cte_uses_with_views(
    cte: &CommonTableExpression,
    catalog: &crate::catalog::Catalog,
    visited_views: &mut HashSet<String>,
    builtin: ResultCacheBypassDependency,
) -> bool {
    match &cte.query {
        CteQuery::Simple(statement) => {
            parsed_statement_uses_with_views(statement, catalog, visited_views, builtin)
        }
        CteQuery::Recursive {
            base, recursive, ..
        } => {
            parsed_statement_uses_with_views(base, catalog, visited_views, builtin)
                || parsed_statement_uses_with_views(recursive, catalog, visited_views, builtin)
        }
    }
}

fn parsed_statement_uses_with_views(
    statement: &crate::sql::ast::ParsedStatement,
    catalog: &crate::catalog::Catalog,
    visited_views: &mut HashSet<String>,
    builtin: ResultCacheBypassDependency,
) -> bool {
    match &statement.statement {
        QueryStatement::Select(select) => {
            select_uses_with_views(select, catalog, visited_views, builtin)
        }
        _ => false,
    }
}

fn select_uses_with_views(
    select: &SelectStatement,
    catalog: &crate::catalog::Catalog,
    visited_views: &mut HashSet<String>,
    builtin: ResultCacheBypassDependency,
) -> bool {
    source_uses_with_views(&select.source, catalog, visited_views, builtin)
        || select
            .projection
            .iter()
            .any(|item| item_uses_with_views(item, catalog, visited_views, builtin))
        || select
            .filter
            .as_ref()
            .is_some_and(|expr| expr_uses_with_views(expr, catalog, visited_views, builtin))
        || select
            .distinct_on
            .iter()
            .any(|expr| expr_uses_with_views(expr, catalog, visited_views, builtin))
        || select
            .group_by
            .iter()
            .any(|expr| expr_uses_with_views(expr, catalog, visited_views, builtin))
        || select
            .having
            .as_ref()
            .is_some_and(|expr| expr_uses_with_views(expr, catalog, visited_views, builtin))
        || select
            .order
            .iter()
            .any(|order| expr_uses_with_views(&order.expr, catalog, visited_views, builtin))
        || select
            .limit
            .as_ref()
            .is_some_and(|expr| expr_uses_with_views(expr, catalog, visited_views, builtin))
        || select
            .offset
            .as_ref()
            .is_some_and(|expr| expr_uses_with_views(expr, catalog, visited_views, builtin))
        || select
            .ctes
            .iter()
            .any(|cte| cte_uses_with_views(cte, catalog, visited_views, builtin))
        || select
            .set
            .as_ref()
            .is_some_and(|set| select_uses_with_views(&set.right, catalog, visited_views, builtin))
}

fn item_uses_with_views(
    item: &SelectItem,
    catalog: &crate::catalog::Catalog,
    visited_views: &mut HashSet<String>,
    builtin: ResultCacheBypassDependency,
) -> bool {
    match item {
        SelectItem::Function { function, .. } => {
            builtin.matches_function(function)
                || function
                    .expressions()
                    .any(|expr| expr_uses_with_views(expr, catalog, visited_views, builtin))
        }
        SelectItem::Expr { expr, .. } => {
            expr_uses_with_views(expr, catalog, visited_views, builtin)
        }
        SelectItem::WindowFunction { function, .. } => function
            .args
            .iter()
            .chain(function.filter.as_deref())
            .chain(&function.partition_by)
            .chain(function.order_by.iter().map(|order| &order.expr))
            .any(|expr| expr_uses_with_views(expr, catalog, visited_views, builtin)),
        SelectItem::Column { .. } | SelectItem::Wildcard => false,
    }
}

fn source_uses_with_views(
    source: &QuerySource,
    catalog: &crate::catalog::Catalog,
    visited_views: &mut HashSet<String>,
    builtin: ResultCacheBypassDependency,
) -> bool {
    match source {
        QuerySource::Values { rows } => rows
            .iter()
            .flatten()
            .any(|expr| expr_uses_with_views(expr, catalog, visited_views, builtin)),
        QuerySource::TableSample {
            source,
            rate,
            repeatable,
            ..
        } => {
            source_uses_with_views(source, catalog, visited_views, builtin)
                || expr_uses_with_views(rate, catalog, visited_views, builtin)
                || repeatable
                    .as_ref()
                    .is_some_and(|expr| expr_uses_with_views(expr, catalog, visited_views, builtin))
        }
        QuerySource::Aliased { source, .. } => {
            source_uses_with_views(source, catalog, visited_views, builtin)
        }
        QuerySource::Subquery { select, .. } => {
            select_uses_with_views(select, catalog, visited_views, builtin)
        }
        QuerySource::Join {
            left, right, on, ..
        } => {
            source_uses_with_views(left, catalog, visited_views, builtin)
                || source_uses_with_views(right, catalog, visited_views, builtin)
                || expr_uses_with_views(on, catalog, visited_views, builtin)
        }
        QuerySource::TableFunction { function, .. } => function
            .expressions()
            .any(|expr| expr_uses_with_views(expr, catalog, visited_views, builtin)),
        QuerySource::Collection(name) => {
            if matches!(builtin, ResultCacheBypassDependency::VirtualCatalog)
                && crate::catalog::virtual_views::schema(name).is_some()
            {
                return true;
            }
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
                    select_uses_with_views(&select, catalog, visited_views, builtin)
                });
            visited_views.remove(&key);
            found
        }
        QuerySource::Cte(_) | QuerySource::SingleRow => false,
    }
}

fn expr_uses_with_views(
    expr: &Expr,
    catalog: &crate::catalog::Catalog,
    visited_views: &mut HashSet<String>,
    builtin: ResultCacheBypassDependency,
) -> bool {
    if builtin.matches_expression(expr) {
        return true;
    }
    if let Expr::Function(function) = expr {
        if builtin.matches_function(function) {
            return true;
        }
    }
    if let Some(statement) = expr.subquery_statement() {
        if parsed_statement_uses_with_views(statement, catalog, visited_views, builtin) {
            return true;
        }
    }
    expr.any_child(|child| expr_uses_with_views(child, catalog, visited_views, builtin))
}

#[cfg(test)]
mod tests {
    use super::super::plan_inspection::plan_uses_function_including_views;
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
        let parsed = crate::sql::parse_statement("SELECT value FROM volatile_cache_udf_view")
            .expect("outer view query");
        let bound = crate::sql::bind(parsed, &catalog).expect("bound view query");
        let plan = crate::planner::logical::plan(&bound).expect("logical view plan");

        // Act
        let contains_volatile_udf =
            plan_uses_function_including_views(&plan, "volatile_cache_random_value", &catalog);

        // Assert
        assert_eq!(
            catalog
                .get_function("volatile_cache_random_value")
                .expect("function metadata")
                .volatility,
            Volatility::Volatile
        );
        assert!(contains_volatile_udf);
    }
}
