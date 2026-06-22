use super::*;

pub(super) fn join_strategy(plan: &LogicalPlan) -> Option<String> {
    match &plan.source {
        QuerySource::Join {
            kind: JoinKind::Inner,
            on,
            ..
        } if is_equi_join_predicate(on) => Some("hash".to_string()),
        QuerySource::Join { .. } => Some("nested_loop".to_string()),
        _ if plan.filter.as_ref().is_some_and(expr_contains_not_exists) => Some("anti".to_string()),
        _ if plan.filter.as_ref().is_some_and(expr_contains_exists) => Some("semi".to_string()),
        _ => None,
    }
}

pub(super) fn expr_contains_exists(expr: &Expr) -> bool {
    match expr {
        Expr::Exists(_) => true,
        Expr::Binary { left, right, .. } => {
            expr_contains_exists(left) || expr_contains_exists(right)
        }
        Expr::IsNull { expr, .. } | Expr::Cast { expr, .. } => expr_contains_exists(expr),
        Expr::InList { expr, values, .. } => {
            expr_contains_exists(expr) || values.iter().any(expr_contains_exists)
        }
        Expr::Between {
            expr, low, high, ..
        } => expr_contains_exists(expr) || expr_contains_exists(low) || expr_contains_exists(high),
        Expr::Not { .. }
        | Expr::Column(_)
        | Expr::Param(_)
        | Expr::StringLiteral(_)
        | Expr::NumberLiteral(_)
        | Expr::BoolLiteral(_)
        | Expr::Null
        | Expr::Function(_) => false,
    }
}

pub(super) fn expr_contains_not_exists(expr: &Expr) -> bool {
    expr_contains_not_exists_with_polarity(expr, false)
}

fn expr_contains_not_exists_with_polarity(expr: &Expr, negated: bool) -> bool {
    match expr {
        Expr::Not { expr } => expr_contains_not_exists_with_polarity(expr, !negated),
        Expr::Exists(_) => negated,
        Expr::Binary { left, right, .. } => {
            expr_contains_not_exists_with_polarity(left, negated)
                || expr_contains_not_exists_with_polarity(right, negated)
        }
        Expr::IsNull { expr, .. } | Expr::Cast { expr, .. } => {
            expr_contains_not_exists_with_polarity(expr, negated)
        }
        Expr::InList { expr, values, .. } => {
            expr_contains_not_exists_with_polarity(expr, negated)
                || values
                    .iter()
                    .any(|value| expr_contains_not_exists_with_polarity(value, negated))
        }
        Expr::Between {
            expr, low, high, ..
        } => {
            expr_contains_not_exists_with_polarity(expr, negated)
                || expr_contains_not_exists_with_polarity(low, negated)
                || expr_contains_not_exists_with_polarity(high, negated)
        }
        Expr::Column(_)
        | Expr::Param(_)
        | Expr::StringLiteral(_)
        | Expr::NumberLiteral(_)
        | Expr::BoolLiteral(_)
        | Expr::Null
        | Expr::Function(_) => false,
    }
}
