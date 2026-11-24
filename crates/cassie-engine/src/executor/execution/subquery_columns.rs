//! Correlated subqueries that grouping consumes row by row.
//!
//! A `GROUP BY (SELECT ...)` key and an aggregate argument such as
//! `SUM((SELECT ...))` are evaluated for every input row, before any group
//! exists. Each such subquery is evaluated once per row into a hidden
//! column, and the grouped plan reads that column instead.
use super::source::SourceExecutionEnv;
use super::{check_timeout, filter, Batch, CteContext, Expr, LogicalPlan, QueryError, SelectItem};

const COLUMN_PREFIX: &str = "__cassie_subquery_";

struct FilteredAggregate {
    filter: Expr,
    filter_column: String,
    expressions: Vec<FilteredExpression>,
}

struct FilteredExpression {
    expression: Expr,
    column: String,
}

pub(super) fn materialize(
    env: &SourceExecutionEnv<'_>,
    ctes: &CteContext,
    plan: &LogicalPlan,
    batches: &mut [Batch],
    search: Option<&filter::SearchContext>,
) -> Result<Option<LogicalPlan>, QueryError> {
    let mut rewritten = plan.clone();
    let filtered_aggregates = rewrite_aggregate_filters(&mut rewritten, env.user_functions);

    let mut targets = Vec::new();
    for expr in &rewritten.group_by {
        collect(expr, &mut targets, env.user_functions);
    }
    for expr in plan_exprs(&rewritten) {
        aggregate_arguments(expr, &mut targets, env.user_functions);
    }
    for item in &rewritten.projection {
        if let SelectItem::Function { function, .. } = item {
            if crate::sql::functions::is_aggregate_function(&function.name) {
                for arg in function.expressions() {
                    collect(arg, &mut targets, env.user_functions);
                }
            }
        }
    }
    if targets.is_empty() && filtered_aggregates.is_empty() {
        return Ok(None);
    }
    for batch in batches.iter_mut() {
        for row in batch.iter_mut() {
            check_timeout(env.controls)?;
            for aggregate in &filtered_aggregates {
                let filter_value = super::exists_phase::evaluate(
                    env,
                    ctes,
                    &plan.source,
                    row,
                    &aggregate.filter,
                    search,
                )?;
                row.append_value(aggregate.filter_column.clone(), filter_value.clone());
                let include = match filter_value {
                    crate::types::Value::Bool(include) => include,
                    crate::types::Value::Null => false,
                    _ => {
                        return Err(QueryError::General(
                            "FILTER requires BOOLEAN or SQL NULL".into(),
                        ));
                    }
                };
                for expression in &aggregate.expressions {
                    let value = if include {
                        super::exists_phase::evaluate(
                            env,
                            ctes,
                            &plan.source,
                            row,
                            &expression.expression,
                            search,
                        )?
                    } else {
                        crate::types::Value::Null
                    };
                    row.append_value(expression.column.clone(), value);
                }
            }
            for (index, target) in targets.iter().enumerate() {
                let value =
                    super::exists_phase::evaluate(env, ctes, &plan.source, row, target, search)?;
                row.append_value(format!("{COLUMN_PREFIX}{index}"), value);
            }
        }
    }
    let replace = |expr: &Expr| rewrite(expr, &targets);
    rewritten.group_by = rewritten.group_by.iter().map(replace).collect();
    rewritten.having = rewritten.having.as_ref().map(replace);
    for order in &mut rewritten.order {
        order.expr = replace(&order.expr);
    }
    for item in &mut rewritten.projection {
        match item {
            SelectItem::Expr { expr, .. } => *expr = replace(expr),
            SelectItem::Function { function, .. } => {
                for arg in function.expressions_mut() {
                    *arg = replace(arg);
                }
            }
            SelectItem::WindowFunction { .. }
            | SelectItem::Wildcard
            | SelectItem::Column { .. } => {}
        }
    }
    Ok(Some(rewritten))
}

fn rewrite_aggregate_filters(
    rewritten: &mut LogicalPlan,
    user_functions: &std::collections::HashMap<String, crate::catalog::FunctionMeta>,
) -> Vec<FilteredAggregate> {
    let mut filtered_aggregates = Vec::new();
    let mut aggregate_index = 0;
    for item in &mut rewritten.projection {
        match item {
            SelectItem::Expr { expr, .. } => rewrite_filtered_aggregates(
                expr,
                &mut filtered_aggregates,
                &mut aggregate_index,
                user_functions,
            ),
            SelectItem::Function { function, .. } => {
                let mut expression = Expr::Function(Box::new(function.clone()));
                rewrite_filtered_aggregates(
                    &mut expression,
                    &mut filtered_aggregates,
                    &mut aggregate_index,
                    user_functions,
                );
                if let Expr::Function(rewritten) = expression {
                    *function = *rewritten;
                }
            }
            SelectItem::WindowFunction { .. }
            | SelectItem::Wildcard
            | SelectItem::Column { .. } => {}
        }
    }
    if let Some(having) = &mut rewritten.having {
        rewrite_filtered_aggregates(
            having,
            &mut filtered_aggregates,
            &mut aggregate_index,
            user_functions,
        );
    }
    for order in &mut rewritten.order {
        rewrite_filtered_aggregates(
            &mut order.expr,
            &mut filtered_aggregates,
            &mut aggregate_index,
            user_functions,
        );
    }
    filtered_aggregates
}

fn plan_exprs(plan: &LogicalPlan) -> impl Iterator<Item = &Expr> {
    plan.projection
        .iter()
        .filter_map(|item| match item {
            SelectItem::Expr { expr, .. } => Some(expr),
            _ => None,
        })
        .chain(plan.having.iter())
        .chain(plan.order.iter().map(|order| &order.expr))
}

/// Collects the unresolved subqueries of each aggregate call under `expr`.
fn aggregate_arguments(
    expr: &Expr,
    targets: &mut Vec<Expr>,
    user_functions: &std::collections::HashMap<String, crate::catalog::FunctionMeta>,
) {
    if let Expr::Function(function) = expr {
        if crate::sql::functions::is_aggregate_function(&function.name) {
            for arg in function.expressions() {
                collect(arg, targets, user_functions);
            }
            return;
        }
    }
    expr.for_each_child(|child| aggregate_arguments(child, targets, user_functions));
}

fn collect(
    expr: &Expr,
    targets: &mut Vec<Expr>,
    user_functions: &std::collections::HashMap<String, crate::catalog::FunctionMeta>,
) {
    if let Expr::Subquery(subquery) = expr {
        if subquery.outcome.is_none() {
            if !targets.iter().any(|target| target.structurally_eq(expr)) {
                targets.push(expr.clone());
            }
            return;
        }
    }
    if filter::expression_contains_routine_subquery(expr, user_functions) {
        if !targets.iter().any(|target| target.structurally_eq(expr)) {
            targets.push(expr.clone());
        }
        return;
    }
    expr.for_each_child(|child| collect(child, targets, user_functions));
}

fn rewrite_filtered_aggregates(
    expression: &mut Expr,
    filtered_aggregates: &mut Vec<FilteredAggregate>,
    aggregate_index: &mut usize,
    user_functions: &std::collections::HashMap<String, crate::catalog::FunctionMeta>,
) {
    if let Expr::Function(function) = expression {
        if crate::sql::functions::is_aggregate_function(&function.name) {
            if let Some(filter) = function.filter.clone() {
                let has_subquery_argument = function
                    .args
                    .iter()
                    .chain(function.order_by.iter().map(|order| &order.expr))
                    .any(|argument| {
                        filter::expression_contains_routine_subquery(argument, user_functions)
                            || argument.contains_subquery()
                    });
                if has_subquery_argument {
                    let mut expressions = Vec::new();
                    for argument in function
                        .args
                        .iter()
                        .chain(function.order_by.iter().map(|order| &order.expr))
                    {
                        if matches!(argument, Expr::Column(name) if name == "*") {
                            continue;
                        }
                        if !expressions.iter().any(|target: &FilteredExpression| {
                            target.expression.structurally_eq(argument)
                        }) {
                            expressions.push(FilteredExpression {
                                expression: argument.clone(),
                                column: format!(
                                    "{COLUMN_PREFIX}filtered_{aggregate_index}_{}",
                                    expressions.len()
                                ),
                            });
                        }
                    }
                    let filter_column = format!("{COLUMN_PREFIX}filtered_{aggregate_index}_filter");
                    for argument in &mut function.args {
                        replace_filtered_expression(argument, &expressions);
                    }
                    for order in &mut function.order_by {
                        replace_filtered_expression(&mut order.expr, &expressions);
                    }
                    function.filter = Some(Box::new(Expr::Column(filter_column.clone())));
                    filtered_aggregates.push(FilteredAggregate {
                        filter: *filter,
                        filter_column,
                        expressions,
                    });
                    *aggregate_index = (*aggregate_index).saturating_add(1);
                }
            }
        }
        for child in function.expressions_mut() {
            rewrite_filtered_aggregates(
                child,
                filtered_aggregates,
                aggregate_index,
                user_functions,
            );
        }
    } else {
        *expression = expression.map_children(|child| {
            let mut child = child.clone();
            rewrite_filtered_aggregates(
                &mut child,
                filtered_aggregates,
                aggregate_index,
                user_functions,
            );
            child
        });
    }
}

fn replace_filtered_expression(expression: &mut Expr, targets: &[FilteredExpression]) {
    if let Some(target) = targets
        .iter()
        .find(|target| target.expression.structurally_eq(expression))
    {
        *expression = Expr::Column(target.column.clone());
    } else {
        *expression = expression.map_children(|child| {
            let mut child = child.clone();
            replace_filtered_expression(&mut child, targets);
            child
        });
    }
}

fn rewrite(expr: &Expr, targets: &[Expr]) -> Expr {
    if let Some(index) = targets
        .iter()
        .position(|target| target.structurally_eq(expr))
    {
        return Expr::Column(format!("{COLUMN_PREFIX}{index}"));
    }
    expr.map_children(|child| rewrite(child, targets))
}
