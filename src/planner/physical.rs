use crate::planner::logical::LogicalPlan;
use crate::sql::ast::{BinaryOp, Expr, FunctionCall, QuerySource, SelectItem, WindowFunctionCall};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Operator {
    Scan,
    Filter,
    Project,
    Sort,
    Limit,
    Offset,
    VectorSearch,
    FullTextSearch,
    Join,
    Aggregate,
    Distinct,
    SetOperation,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PhysicalPlan {
    pub collection: String,
    pub operators: Vec<Operator>,
    pub logical: LogicalPlan,
    pub predicate_pushdown: bool,
    pub projected_scan_fields: Vec<String>,
}

pub fn build(plan: LogicalPlan) -> PhysicalPlan {
    if plan.command.is_some() {
        return PhysicalPlan {
            collection: plan.collection.clone(),
            operators: Vec::new(),
            logical: plan,
            predicate_pushdown: false,
            projected_scan_fields: Vec::new(),
        };
    }

    let predicate_pushdown = plan_supports_predicate_pushdown(&plan);
    let projected_scan_fields = projected_scan_fields(&plan).unwrap_or_default();
    let mut operators = vec![Operator::Scan];
    if source_contains_join(&plan.source) {
        operators.push(Operator::Join);
    }
    if plan_uses_fulltext(&plan) {
        operators.push(Operator::FullTextSearch);
    }
    if plan_uses_vector(&plan) {
        operators.push(Operator::VectorSearch);
    }
    if plan.filter.is_some() {
        operators.push(Operator::Filter);
    }
    if plan_uses_aggregate(&plan) {
        operators.push(Operator::Aggregate);
    }
    if plan.distinct || !plan.distinct_on.is_empty() {
        operators.push(Operator::Distinct);
    }
    if plan.set.is_some() {
        operators.push(Operator::SetOperation);
    }
    if !plan.order.is_empty() {
        operators.push(Operator::Sort);
    }
    if !plan.projection.is_empty() {
        operators.push(Operator::Project);
    }
    if plan.offset.is_some() {
        operators.push(Operator::Offset);
    }
    if plan.limit.is_some() {
        operators.push(Operator::Limit);
    }
    PhysicalPlan {
        collection: plan.collection.clone(),
        operators,
        logical: plan,
        predicate_pushdown,
        projected_scan_fields,
    }
}

fn plan_supports_predicate_pushdown(plan: &LogicalPlan) -> bool {
    if plan.command.is_some()
        || !plan.ctes.is_empty()
        || plan.distinct
        || !plan.distinct_on.is_empty()
        || !plan.group_by.is_empty()
        || plan.having.is_some()
        || plan.set.is_some()
        || !plan.order.is_empty()
    {
        return false;
    }

    if !matches!(plan.source, QuerySource::Collection(_)) {
        return false;
    }

    if plan.projection.is_empty()
        || !plan
            .projection
            .iter()
            .all(|item| matches!(item, SelectItem::Column { .. }))
    {
        return false;
    }

    plan.filter
        .as_ref()
        .is_some_and(filter_supports_predicate_pushdown)
}

fn projected_scan_fields(plan: &LogicalPlan) -> Option<Vec<String>> {
    if plan.command.is_some()
        || !plan.ctes.is_empty()
        || plan.distinct
        || !plan.distinct_on.is_empty()
        || !plan.group_by.is_empty()
        || plan.having.is_some()
        || plan.set.is_some()
        || !plan.order.is_empty()
    {
        return None;
    }

    if !matches!(plan.source, QuerySource::Collection(_)) {
        return None;
    }

    let projection_columns = plan
        .projection
        .iter()
        .map(|item| match item {
            SelectItem::Column { name, .. } => Some(name.clone()),
            _ => None,
        })
        .collect::<Option<Vec<_>>>()?;
    if projection_columns.is_empty() {
        return None;
    }

    let filter_columns = match plan.filter.as_ref() {
        Some(filter) => projected_filter_columns(filter)?,
        None => Vec::new(),
    };

    let mut fields = Vec::new();
    for column in projection_columns.into_iter().chain(filter_columns) {
        if is_row_id_column(&column) || fields.iter().any(|field: &String| field == &column) {
            continue;
        }
        fields.push(column);
    }
    Some(fields)
}

fn projected_filter_columns(expr: &Expr) -> Option<Vec<String>> {
    let mut fields = Vec::new();
    collect_projected_filter_columns(expr, &mut fields)?;
    Some(fields)
}

fn collect_projected_filter_columns(expr: &Expr, fields: &mut Vec<String>) -> Option<()> {
    match expr {
        Expr::Column(name) => {
            if !fields.iter().any(|field| field.eq_ignore_ascii_case(name)) {
                fields.push(name.clone());
            }
            Some(())
        }
        Expr::Param(_)
        | Expr::StringLiteral(_)
        | Expr::NumberLiteral(_)
        | Expr::BoolLiteral(_)
        | Expr::Null => Some(()),
        Expr::Binary { left, op, right } => {
            match op {
                BinaryOp::Eq
                | BinaryOp::NotEq
                | BinaryOp::Lt
                | BinaryOp::Lte
                | BinaryOp::Gt
                | BinaryOp::Gte
                | BinaryOp::And
                | BinaryOp::Or
                | BinaryOp::Like => {}
                _ => return None,
            }
            collect_projected_filter_columns(left, fields)?;
            collect_projected_filter_columns(right, fields)
        }
        Expr::IsNull { expr, .. } => collect_projected_filter_columns(expr, fields),
        Expr::InList { expr, values, .. } => {
            collect_projected_filter_columns(expr, fields)?;
            for value in values {
                collect_projected_filter_columns(value, fields)?;
            }
            Some(())
        }
        Expr::Between {
            expr, low, high, ..
        } => {
            collect_projected_filter_columns(expr, fields)?;
            collect_projected_filter_columns(low, fields)?;
            collect_projected_filter_columns(high, fields)
        }
        Expr::Not { expr } | Expr::Cast { expr, .. } => {
            collect_projected_filter_columns(expr, fields)
        }
        Expr::Function(_) | Expr::Exists(_) => None,
    }
}

fn filter_supports_predicate_pushdown(expr: &Expr) -> bool {
    let Expr::Binary {
        left,
        op: BinaryOp::Eq,
        right,
    } = expr
    else {
        return false;
    };

    match (left.as_ref(), right.as_ref()) {
        (Expr::Column(field), literal) | (literal, Expr::Column(field)) => {
            !is_row_id_column(field) && expr_is_pushdown_literal(literal)
        }
        _ => false,
    }
}

fn expr_is_pushdown_literal(expr: &Expr) -> bool {
    matches!(
        expr,
        Expr::StringLiteral(_) | Expr::BoolLiteral(_) | Expr::Null
    )
}

fn is_row_id_column(field: &str) -> bool {
    field == "_id" || field.eq_ignore_ascii_case("id")
}

fn source_contains_join(source: &QuerySource) -> bool {
    match source {
        QuerySource::Join { .. } => true,
        QuerySource::Subquery { select, .. } => source_contains_join(&select.source),
        QuerySource::Collection(_) | QuerySource::Cte(_) | QuerySource::SingleRow => false,
    }
}

fn plan_uses_aggregate(plan: &LogicalPlan) -> bool {
    !plan.group_by.is_empty()
        || plan.having.is_some()
        || plan.projection.iter().any(|item| match item {
            SelectItem::Function { function, .. } => {
                crate::sql::functions::is_aggregate_function(&function.name)
            }
            SelectItem::Wildcard
            | SelectItem::Column { .. }
            | SelectItem::Expr { .. }
            | SelectItem::WindowFunction { .. } => false,
        })
}

fn plan_uses_fulltext(plan: &LogicalPlan) -> bool {
    plan.projection.iter().any(select_item_uses_fulltext)
        || plan_expressions(plan).any(expr_uses_fulltext)
}

fn plan_uses_vector(plan: &LogicalPlan) -> bool {
    plan.projection.iter().any(select_item_uses_vector)
        || plan_expressions(plan).any(expr_uses_vector)
}

fn plan_expressions(plan: &LogicalPlan) -> impl Iterator<Item = &Expr> {
    plan.projection
        .iter()
        .flat_map(select_item_expressions)
        .chain(plan.filter.iter())
        .chain(plan.group_by.iter())
        .chain(plan.having.iter())
        .chain(plan.order.iter().map(|order| &order.expr))
        .chain(plan.distinct_on.iter())
}

fn select_item_expressions(item: &SelectItem) -> Vec<&Expr> {
    match item {
        SelectItem::Wildcard | SelectItem::Column { .. } => Vec::new(),
        SelectItem::Function { function, .. } => function.args.iter().collect(),
        SelectItem::Expr { expr, .. } => vec![expr],
        SelectItem::WindowFunction { function, .. } => window_function_expressions(function),
    }
}

fn window_function_expressions(function: &WindowFunctionCall) -> Vec<&Expr> {
    function
        .args
        .iter()
        .chain(function.partition_by.iter())
        .chain(function.order_by.iter().map(|order| &order.expr))
        .collect()
}

fn select_item_uses_fulltext(item: &SelectItem) -> bool {
    match item {
        SelectItem::Wildcard | SelectItem::Column { .. } => false,
        SelectItem::Function { function, .. } => function_uses_fulltext(function),
        SelectItem::Expr { expr, .. } => expr_uses_fulltext(expr),
        SelectItem::WindowFunction { function, .. } => window_function_expressions(function)
            .into_iter()
            .any(expr_uses_fulltext),
    }
}

fn select_item_uses_vector(item: &SelectItem) -> bool {
    match item {
        SelectItem::Wildcard | SelectItem::Column { .. } => false,
        SelectItem::Function { function, .. } => function_uses_vector(function),
        SelectItem::Expr { expr, .. } => expr_uses_vector(expr),
        SelectItem::WindowFunction { function, .. } => window_function_expressions(function)
            .into_iter()
            .any(expr_uses_vector),
    }
}

fn expr_uses_fulltext(expr: &Expr) -> bool {
    match expr {
        Expr::Function(function) => function_uses_fulltext(function),
        Expr::Binary { left, right, .. } => expr_uses_fulltext(left) || expr_uses_fulltext(right),
        Expr::IsNull { expr, .. } | Expr::Not { expr } | Expr::Cast { expr, .. } => {
            expr_uses_fulltext(expr)
        }
        Expr::InList { expr, values, .. } => {
            expr_uses_fulltext(expr) || values.iter().any(expr_uses_fulltext)
        }
        Expr::Between {
            expr, low, high, ..
        } => expr_uses_fulltext(expr) || expr_uses_fulltext(low) || expr_uses_fulltext(high),
        Expr::Column(_)
        | Expr::Param(_)
        | Expr::StringLiteral(_)
        | Expr::NumberLiteral(_)
        | Expr::BoolLiteral(_)
        | Expr::Null
        | Expr::Exists(_) => false,
    }
}

fn function_uses_fulltext(function: &FunctionCall) -> bool {
    matches!(
        function.name.to_ascii_lowercase().as_str(),
        "search" | "search_score" | "snippet"
    ) || function.args.iter().any(expr_uses_fulltext)
}

fn expr_uses_vector(expr: &Expr) -> bool {
    match expr {
        Expr::Function(function) => function_uses_vector(function),
        Expr::Binary { left, op, right } => {
            matches!(
                op,
                BinaryOp::PgvectorCosine | BinaryOp::PgvectorL2 | BinaryOp::PgvectorDot
            ) || expr_uses_vector(left)
                || expr_uses_vector(right)
        }
        Expr::IsNull { expr, .. } | Expr::Not { expr } | Expr::Cast { expr, .. } => {
            expr_uses_vector(expr)
        }
        Expr::InList { expr, values, .. } => {
            expr_uses_vector(expr) || values.iter().any(expr_uses_vector)
        }
        Expr::Between {
            expr, low, high, ..
        } => expr_uses_vector(expr) || expr_uses_vector(low) || expr_uses_vector(high),
        Expr::Column(_)
        | Expr::Param(_)
        | Expr::StringLiteral(_)
        | Expr::NumberLiteral(_)
        | Expr::BoolLiteral(_)
        | Expr::Null
        | Expr::Exists(_) => false,
    }
}

fn function_uses_vector(function: &FunctionCall) -> bool {
    matches!(
        function.name.to_ascii_lowercase().as_str(),
        "vector_distance" | "vector_score" | "cosine_distance" | "dot_product"
    ) || function.args.iter().any(expr_uses_vector)
}
