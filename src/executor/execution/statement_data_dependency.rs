//! Conservative proof that a SELECT can execute without authoritative Data.
use crate::planner::logical::LogicalPlan;
use crate::sql::ast::{
    CteQuery, Expr, FunctionCall, ParsedStatement, QuerySource, QueryStatement, SelectItem,
    SelectStatement,
};

pub(crate) fn plan_needs_statement_data(plan: &LogicalPlan) -> bool {
    source(&plan.source)
        || plan.projection.iter().any(item)
        || plan.filter.iter().any(expression)
        || plan.distinct_on.iter().any(expression)
        || plan.group_by.iter().any(expression)
        || plan.having.iter().any(expression)
        || plan.order.iter().any(|order| expression(&order.expr))
        || plan.limit.iter().any(expression)
        || plan.offset.iter().any(expression)
        || plan.ctes.iter().any(|cte| cte_query(&cte.query))
        || plan.set.as_ref().is_some_and(|set| select(&set.right))
}

fn select(query: &SelectStatement) -> bool {
    source(&query.source)
        || query.projection.iter().any(item)
        || query.filter.iter().any(expression)
        || query.distinct_on.iter().any(expression)
        || query.group_by.iter().any(expression)
        || query.having.iter().any(expression)
        || query.order.iter().any(|order| expression(&order.expr))
        || query.limit.iter().any(expression)
        || query.offset.iter().any(expression)
        || query.ctes.iter().any(|cte| cte_query(&cte.query))
        || query.set.as_ref().is_some_and(|set| select(&set.right))
}

fn statement(query: &ParsedStatement) -> bool {
    match &query.statement {
        QueryStatement::Select(query) => select(query),
        _ => true,
    }
}

fn cte_query(query: &CteQuery) -> bool {
    match query {
        CteQuery::Simple(query) => statement(query),
        CteQuery::Recursive {
            base, recursive, ..
        } => statement(base) || statement(recursive),
    }
}

fn source(query: &QuerySource) -> bool {
    match query {
        QuerySource::SingleRow => false,
        QuerySource::Aliased { source: query, .. } => source(query),
        QuerySource::Subquery { select: query, .. } => select(query),
        QuerySource::Join {
            left, right, on, ..
        } => source(left) || source(right) || expression(on),
        QuerySource::Collection(_) | QuerySource::Cte(_) | QuerySource::TableFunction { .. } => {
            true
        }
    }
}

fn item(item: &SelectItem) -> bool {
    match item {
        SelectItem::Wildcard | SelectItem::Column { .. } => false,
        SelectItem::Function { function: call, .. } => function(call),
        SelectItem::Expr { expr, .. } => expression(expr),
        SelectItem::WindowFunction { function: call, .. } => {
            call.args.iter().any(expression)
                || call.partition_by.iter().any(expression)
                || call.order_by.iter().any(|order| expression(&order.expr))
        }
    }
}

fn function(call: &FunctionCall) -> bool {
    // This qualified metadata builtin is implemented by existing system dispatch.
    (!call.name.eq_ignore_ascii_case("pg_catalog.version")
        && super::plan_inspection::function_needs_user_functions(call))
        || call.args.iter().any(expression)
}

fn expression(expr: &Expr) -> bool {
    match expr {
        Expr::Exists(query) => statement(query),
        Expr::Function(call) => function(call),
        _ => expr.any_child(expression),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn should_require_data_for_every_nested_query_expression_position() {
        // Arrange
        let parsed = crate::sql::parse_statement("SELECT 1").expect("single row query");
        let QueryStatement::Select(mut query) = parsed.statement else {
            panic!("SELECT");
        };
        let data = Expr::Exists(Box::new(
            crate::sql::parse_statement("SELECT 1 FROM dependency_rows").expect("Data query"),
        ));
        let mut predicates = Vec::new();
        // Act
        query.limit = Some(data.clone());
        predicates.push(select(&query));
        query.limit = None;
        query.offset = Some(data.clone());
        predicates.push(select(&query));
        query.offset = None;
        query.projection = vec![SelectItem::WindowFunction {
            function: crate::sql::ast::WindowFunctionCall {
                name: "row_number".into(),
                args: vec![],
                partition_by: vec![data.clone()],
                order_by: vec![],
                frame: None,
            },
            alias: None,
        }];
        predicates.push(select(&query));
        query.projection = vec![SelectItem::Function {
            function: FunctionCall {
                name: "coalesce".into(),
                args: vec![Expr::BoolLiteral(true), data],
            },
            alias: None,
        }];
        predicates.push(select(&query));
        // Assert
        assert_eq!(predicates, vec![true; 4]);
    }
}
