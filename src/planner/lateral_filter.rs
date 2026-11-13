//! Early evaluation of WHERE conjuncts for inner joins whose right side is a lateral
//! generated set-returning source.
//!
//! An inner-join `ON` conjunct is equivalent to a `WHERE` conjunct, and one that reads only
//! preserved-side columns can run before the set-returning function expands that row. Only
//! simple, side-effect-free expressions qualify; the original `WHERE` stays in place.
use crate::planner::logical::LogicalPlan;
use crate::sql::ast::{BinaryOp, Expr, JoinKind, QuerySource};

fn is_lateral_generated(source: &QuerySource) -> bool {
    match source {
        QuerySource::Aliased { source, .. } => is_lateral_generated(source),
        QuerySource::TableFunction {
            function, lateral, ..
        } => *lateral && crate::sql::table_sources::is_generated(&function.name),
        _ => false,
    }
}

/// Splits top-level `AND` terms.
pub(crate) fn conjuncts<'a>(expr: &'a Expr, out: &mut Vec<&'a Expr>) {
    if let Expr::Binary {
        left,
        op: BinaryOp::And,
        right,
    } = expr
    {
        conjuncts(left, out);
        conjuncts(right, out);
    } else {
        out.push(expr);
    }
}

/// Collects column names when `expr` is built only from columns, literals, parameters and
/// plain operators; any other form (functions, subqueries, CASE, LIKE, ...) is not eligible.
pub(crate) fn simple_columns<'a>(expr: &'a Expr, out: &mut Vec<&'a str>) -> bool {
    match expr {
        Expr::Column(name) => {
            out.push(name);
            true
        }
        Expr::Param(_)
        | Expr::StringLiteral(_)
        | Expr::NumberLiteral(_)
        | Expr::IntegerLiteral(_)
        | Expr::NumericLiteral(_)
        | Expr::BoolLiteral(_)
        | Expr::Null => true,
        Expr::Binary { .. }
        | Expr::TruthTest { .. }
        | Expr::IsNull { .. }
        | Expr::InList { .. }
        | Expr::Between { .. }
        | Expr::Not { .. }
        | Expr::Cast { .. } => expr
            .try_visit_children(|child| {
                if simple_columns(child, out) {
                    Ok(())
                } else {
                    Err(())
                }
            })
            .is_ok(),
        _ => false,
    }
}

/// Copies eligible `WHERE` conjuncts into the `ON` of a top-level cross or inner join whose
/// right side is a lateral generated source, turning a cross join into an inner join.
pub(crate) fn push_into_lateral_join(plan: &mut LogicalPlan) {
    let Some(filter) = &plan.filter else {
        return;
    };
    let QuerySource::Join {
        left,
        right,
        kind,
        on,
    } = &mut plan.source
    else {
        return;
    };
    if !matches!(kind, JoinKind::Cross | JoinKind::Inner) || !is_lateral_generated(right) {
        return;
    }
    let mut qualifiers = Vec::new();
    collect_qualifiers(&[&**left, &**right], &mut qualifiers);
    let mut terms = Vec::new();
    conjuncts(filter, &mut terms);
    let mut selected = terms
        .into_iter()
        .filter(|term| owned_by(term, &qualifiers))
        .cloned()
        .collect::<Vec<_>>();
    if selected.is_empty() {
        return;
    }
    if !matches!(on, Expr::BoolLiteral(true)) {
        selected.insert(0, on.clone());
    }
    let mut combined = selected.remove(0);
    for term in selected {
        combined = Expr::Binary {
            left: Box::new(combined),
            op: BinaryOp::And,
            right: Box::new(term),
        };
    }
    *on = combined;
    *kind = JoinKind::Inner;
}

/// Names that qualify columns of the given sources.
fn collect_qualifiers(sources: &[&QuerySource], out: &mut Vec<String>) {
    for source in sources {
        match source {
            // An alias hides the names inside the aliased source.
            QuerySource::Aliased { alias, .. } | QuerySource::Subquery { alias, .. } => {
                out.push(alias.clone());
            }
            QuerySource::Collection(path) => {
                let full = path.to_string();
                if let Some((_, last)) = full.rsplit_once('.') {
                    out.push(last.to_owned());
                }
                out.push(full);
            }
            QuerySource::TableSample { source, .. } => collect_qualifiers(&[&**source], out),
            QuerySource::Join { left, right, .. } => collect_qualifiers(&[&**left, &**right], out),
            QuerySource::Cte(name) | QuerySource::TableFunction { name, .. } => {
                out.push(name.clone());
            }
            QuerySource::Values { .. } | QuerySource::SingleRow => {}
        }
    }
}

/// Whether the term is eligible and every column it reads is qualified by one of this join's
/// own sources. Unqualified or foreign (outer-scope) columns are not provably local, and an
/// `ON` clause has no outer scope attached, so such a term must stay in the `WHERE`.
fn owned_by(term: &Expr, qualifiers: &[String]) -> bool {
    let mut names = Vec::new();
    simple_columns(term, &mut names)
        && names.iter().all(|name| {
            name.rsplit_once('.').is_some_and(|(qualifier, _)| {
                qualifiers
                    .iter()
                    .any(|owner| owner.eq_ignore_ascii_case(qualifier))
            })
        })
}
