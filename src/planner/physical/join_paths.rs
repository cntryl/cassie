use super::{
    is_equi_join_predicate, source_contains_join, BinaryOp, Expr, JoinKind, LogicalPlan,
    QuerySource,
};

pub(super) fn join_strategy(plan: &LogicalPlan) -> Option<String> {
    match &plan.source {
        QuerySource::Join { kind, on, .. } if merge_join_preferred(plan, *kind, on) => {
            Some("merge".to_string())
        }
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

pub(super) fn join_keys(plan: &LogicalPlan) -> Vec<String> {
    let QuerySource::Join { on, .. } = &plan.source else {
        return Vec::new();
    };
    equi_join_columns(on)
        .map(|(left, right)| vec![format!("{left}={right}")])
        .unwrap_or_default()
}

pub(super) fn join_sort_required(plan: &LogicalPlan, strategy: Option<&str>) -> bool {
    matches!(strategy, Some("merge")) && source_contains_join(&plan.source)
}

pub(super) fn join_fallback_reason(plan: &LogicalPlan, strategy: Option<&str>) -> Option<String> {
    let QuerySource::Join { kind, on, .. } = &plan.source else {
        return None;
    };
    if matches!(strategy, Some("merge")) {
        return None;
    }
    if matches!(kind, JoinKind::Cross) {
        return Some("cross_join".to_string());
    }
    if !is_equi_join_predicate(on) {
        return Some("non_equi_predicate".to_string());
    }
    Some("ordering_not_beneficial".to_string())
}

pub(super) fn vectorized_join_candidate(plan: &LogicalPlan) -> bool {
    matches!(
        &plan.source,
        QuerySource::Join {
            kind: JoinKind::Inner | JoinKind::Left,
            on,
            ..
        } if is_equi_join_predicate(on)
    )
}

pub(super) fn vectorized_join_fallback_reason(plan: &LogicalPlan) -> Option<String> {
    let QuerySource::Join { kind, on, .. } = &plan.source else {
        return None;
    };
    if matches!(kind, JoinKind::Inner | JoinKind::Left) && is_equi_join_predicate(on) {
        return None;
    }
    if !matches!(kind, JoinKind::Inner | JoinKind::Left) {
        return Some("unsupported_join_type".to_string());
    }
    Some("non_equi_predicate".to_string())
}

fn merge_join_preferred(plan: &LogicalPlan, kind: JoinKind, on: &Expr) -> bool {
    matches!(
        kind,
        JoinKind::Inner | JoinKind::Left | JoinKind::Right | JoinKind::Full
    ) && is_equi_join_predicate(on)
        && equi_join_columns(on).is_some_and(|(left, right)| {
            plan.order.iter().any(|order| {
                matches!(
                    &order.expr,
                    Expr::Column(column) if column == &left || column == &right
                )
            })
        })
}

fn equi_join_columns(expr: &Expr) -> Option<(String, String)> {
    let Expr::Binary {
        left,
        op: BinaryOp::Eq,
        right,
    } = expr
    else {
        return None;
    };
    let (Expr::Column(left), Expr::Column(right)) = (left.as_ref(), right.as_ref()) else {
        return None;
    };
    Some((left.clone(), right.clone()))
}

pub(super) fn expr_contains_exists(expr: &Expr) -> bool {
    match expr {
        Expr::Exists(_) => true,
        // A negated EXISTS is an anti-join, handled by
        // `expr_contains_not_exists`, and a function argument never carries a
        // join-planning EXISTS.
        Expr::Not { .. } | Expr::Function(_) => false,
        _ => expr.any_child(expr_contains_exists),
    }
}

pub(super) fn expr_contains_not_exists(expr: &Expr) -> bool {
    expr_contains_not_exists_with_polarity(expr, false)
}

fn expr_contains_not_exists_with_polarity(expr: &Expr, negated: bool) -> bool {
    match expr {
        Expr::Not { expr } => expr_contains_not_exists_with_polarity(expr, !negated),
        Expr::Exists(_) => negated,
        Expr::Function(_) => false,
        _ => expr.any_child(|child| expr_contains_not_exists_with_polarity(child, negated)),
    }
}
