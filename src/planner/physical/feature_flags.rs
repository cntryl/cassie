use super::{BinaryOp, Expr, FunctionCall, LogicalPlan, SelectItem, WindowFunctionCall};

pub(super) fn plan_uses_fulltext(plan: &LogicalPlan) -> bool {
    plan.projection.iter().any(select_item_uses_fulltext)
        || plan_expressions(plan).any(expr_uses_fulltext)
}

pub(super) fn plan_uses_vector(plan: &LogicalPlan) -> bool {
    plan.projection.iter().any(select_item_uses_vector)
        || plan_expressions(plan).any(expr_uses_vector)
}

pub(super) fn plan_expressions(plan: &LogicalPlan) -> impl Iterator<Item = &Expr> {
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

pub(super) fn function_uses_fulltext(function: &FunctionCall) -> bool {
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

pub(super) fn function_uses_vector(function: &FunctionCall) -> bool {
    matches!(
        function.name.to_ascii_lowercase().as_str(),
        "vector_distance" | "vector_score" | "cosine_distance" | "dot_product"
    ) || function.args.iter().any(expr_uses_vector)
}
