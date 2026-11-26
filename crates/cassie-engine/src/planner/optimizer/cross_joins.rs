//! Evaluate qualified equijoin filters while joining comma-separated relations.
use super::{combine_predicates, BinaryOp, Expr, JoinKind, LogicalPlan, QuerySource};
use crate::planner::lateral_filter::{collect_qualifiers, conjuncts};

/// Copy an equality into the cross join that first brings both of its
/// qualified columns into scope. Keep the entire WHERE as a residual.
/// Outer joins are opaque operands: their nullable rows already exist
/// before any equality on the enclosing cross join is evaluated.
pub(super) fn bind_equalities(plan: &mut LogicalPlan) {
    if !matches!(
        plan.source,
        QuerySource::Join {
            kind: JoinKind::Cross,
            ..
        }
    ) {
        return;
    }
    let Some(filter) = &plan.filter else { return };
    let mut terms = Vec::new();
    conjuncts(filter, &mut terms);
    bind(&mut plan.source, &terms);
}

fn bind(source: &mut QuerySource, terms: &[&Expr]) {
    let QuerySource::Join {
        kind,
        left,
        right,
        on,
    } = source
    else {
        return;
    };
    if *kind != JoinKind::Cross || !matches!(on, Expr::BoolLiteral(true)) {
        return;
    }
    bind(left, terms);
    bind(right, terms);
    let mut left_names = Vec::new();
    let mut right_names = Vec::new();
    collect_qualifiers(&[left], &mut left_names);
    collect_qualifiers(&[right], &mut right_names);
    let mut equalities = Vec::new();
    for term in terms {
        let Expr::Binary {
            left,
            op: BinaryOp::Eq,
            right,
        } = term
        else {
            continue;
        };
        let (Expr::Column(left), Expr::Column(right)) = (&**left, &**right) else {
            continue;
        };
        let (Some((a, _)), Some((b, _))) = (left.rsplit_once('.'), right.rsplit_once('.')) else {
            continue;
        };
        let own = |names: &[String], qualifier: &str| names.iter().any(|name| name == qualifier);
        let opposite = |a: &str, b: &str| {
            own(&left_names, a)
                && !own(&right_names, a)
                && own(&right_names, b)
                && !own(&left_names, b)
        };
        if opposite(a, b) || opposite(b, a) {
            equalities.push((*term).clone());
        }
    }
    if !equalities.is_empty() {
        *kind = JoinKind::Inner;
        *on = combine_predicates(equalities);
    }
}
