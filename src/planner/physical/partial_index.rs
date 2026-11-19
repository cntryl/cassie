//! An exact conjunct can be discharged by membership in a partial scalar index.
use std::borrow::Cow;

use super::{BinaryOp, Expr, IndexMeta, LogicalPlan};

pub(super) fn matches(filter: Option<&Expr>, predicate: Option<&String>) -> bool {
    let Some(predicate) = predicate else {
        return true;
    };
    filter.is_some_and(|filter| {
        is_predicate(filter, predicate) || without_conjunct(filter, predicate).is_some()
    })
}

fn is_predicate(expr: &Expr, predicate: &str) -> bool {
    serde_json::to_string(expr).is_ok_and(|serialized| serialized == predicate)
}

// Only descend through AND. In particular, neither OR nor a negation proves membership.
fn without_conjunct(expr: &Expr, predicate: &str) -> Option<Expr> {
    let Expr::Binary {
        left,
        op: BinaryOp::And,
        right,
    } = expr
    else {
        return None;
    };
    for (term, sibling) in [(left, right), (right, left)] {
        if is_predicate(term, predicate) {
            return Some((**sibling).clone());
        }
    }
    if let Some(reduced) = without_conjunct(left, predicate) {
        return Some(Expr::Binary {
            left: Box::new(reduced),
            op: BinaryOp::And,
            right: right.clone(),
        });
    }
    without_conjunct(right, predicate).map(|reduced| Expr::Binary {
        left: left.clone(),
        op: BinaryOp::And,
        right: Box::new(reduced),
    })
}

pub(crate) fn access_plan<'a>(plan: &'a LogicalPlan, index: &IndexMeta) -> Cow<'a, LogicalPlan> {
    let residual = index.predicate.as_deref().and_then(|predicate| {
        // Keep a predicate on an index key available for prefix or range construction.
        let Expr::Binary {
            left,
            op: BinaryOp::Eq,
            right,
        } = serde_json::from_str::<Expr>(predicate).ok()?
        else {
            return None;
        };
        let Expr::Column(field) = left.as_ref() else {
            return None;
        };
        if !matches!(
            right.as_ref(),
            Expr::StringLiteral(_) | Expr::IntegerLiteral(_) | Expr::BoolLiteral(_)
        ) || index
            .normalized_fields()
            .iter()
            .any(|key| crate::sql::ColumnIdentifierPath::matches_stored_field(field, key))
        {
            return None;
        }
        without_conjunct(plan.filter.as_ref()?, predicate)
    });
    if let Some(filter) = residual {
        let mut plan = plan.clone();
        plan.filter = Some(filter);
        Cow::Owned(plan)
    } else {
        Cow::Borrowed(plan)
    }
}
