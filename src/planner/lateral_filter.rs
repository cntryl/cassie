//! Early evaluation of WHERE conjuncts for inner joins whose right side is a lateral
//! generated set-returning source.
//!
//! An inner-join `ON` conjunct is equivalent to a `WHERE` conjunct, and one that reads only
//! preserved-side columns can run before the set-returning function expands that row. A left
//! join may receive a conjunct that reads only its preserved side: a row that fails it is
//! null-extended without expansion and the `WHERE` then removes it. Only simple,
//! side-effect-free expressions qualify; the original `WHERE` stays in place.
use crate::planner::logical::LogicalPlan;
use crate::sql::ast::{BinaryOp, Expr, FunctionCall, JoinKind, QuerySource};

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

/// Deterministic scalar functions that commonly guard a set-returning call.
const GUARD_FUNCTIONS: &[&str] = &[
    "coalesce",
    "nullif",
    "greatest",
    "least",
    "lower",
    "upper",
    "length",
    "abs",
    "jsonb_typeof",
    "json_typeof",
    "jsonb_array_length",
    "json_array_length",
];

/// Whether a function call may run before the lateral expansion: a plain call of a selected
/// deterministic builtin or an `ARRAY[...]` constructor.
fn guard_function(function: &FunctionCall) -> bool {
    if function.filter.is_some() || !function.order_by.is_empty() {
        return false;
    }
    let name = function
        .name
        .rsplit('.')
        .next()
        .unwrap_or(function.name.as_str());
    name.eq_ignore_ascii_case(crate::sql::ast::ARRAY_CONSTRUCTOR_FUNCTION)
        || name.eq_ignore_ascii_case("_cassie_array_value")
        || GUARD_FUNCTIONS
            .iter()
            .any(|guard| guard.eq_ignore_ascii_case(name))
}

/// Collects column names when `expr` is built only from columns, literals, parameters, plain
/// operators, `CASE` and selected deterministic functions; any other form (subqueries, LIKE,
/// volatile or user functions, ...) is not eligible.
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
        Expr::Function(function) => {
            guard_function(function) && function.args.iter().all(|arg| simple_columns(arg, out))
        }
        Expr::Binary { .. }
        | Expr::TruthTest { .. }
        | Expr::IsNull { .. }
        | Expr::InList { .. }
        | Expr::Between { .. }
        | Expr::Not { .. }
        | Expr::Case { .. }
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

/// Copies eligible `WHERE` conjuncts into the `ON` of every cross, inner or left join on the
/// left spine of the plan source whose right side is a lateral generated source. A cross join
/// becomes an inner join; a left join only receives conjuncts that read its preserved side.
pub(crate) fn push_into_lateral_join(plan: &mut LogicalPlan) {
    let Some(filter) = &plan.filter else {
        return;
    };
    let mut terms = Vec::new();
    conjuncts(filter, &mut terms);
    let mut scope = Vec::new();
    collect_qualifiers(&[&plan.source], &mut scope);
    push_terms(&mut plan.source, &terms, &scope, &|_| false);
}

/// The plan source with the eligible `WHERE` conjuncts pushed, or `None` when nothing changes.
/// `outer` reports whether a column name resolves in the enclosing row of a correlated plan;
/// such a column is constant for one execution and may be read before the expansion.
pub(crate) fn pushed_source(
    plan: &LogicalPlan,
    outer: &dyn Fn(&str) -> bool,
) -> Option<QuerySource> {
    let filter = plan.filter.as_ref()?;
    if !has_lateral_generated_join(&plan.source) {
        return None;
    }
    let mut terms = Vec::new();
    conjuncts(filter, &mut terms);
    let mut scope = Vec::new();
    collect_qualifiers(&[&plan.source], &mut scope);
    let mut source = plan.source.clone();
    push_terms(&mut source, &terms, &scope, outer).then_some(source)
}

/// Whether the left spine of `source` holds a join onto a lateral generated source.
fn has_lateral_generated_join(source: &QuerySource) -> bool {
    match source {
        QuerySource::Join { left, right, .. } => {
            is_lateral_generated(right) || has_lateral_generated_join(left)
        }
        _ => false,
    }
}

/// Whether `on` already holds `term` as one of its top-level conjuncts.
fn has_conjunct(on: &Expr, term: &Expr) -> bool {
    let mut present = Vec::new();
    conjuncts(on, &mut present);
    present
        .iter()
        .any(|existing| existing.structurally_eq(term))
}

/// Pushes into `source` and the joins on its left spine; reports whether any `ON` changed.
fn push_terms(
    source: &mut QuerySource,
    terms: &[&Expr],
    scope: &[String],
    outer: &dyn Fn(&str) -> bool,
) -> bool {
    let QuerySource::Join {
        left,
        right,
        kind,
        on,
    } = source
    else {
        return false;
    };
    if !matches!(kind, JoinKind::Cross | JoinKind::Inner | JoinKind::Left) {
        return false;
    }
    let mut changed = false;
    if is_lateral_generated(right) {
        let mut own = Vec::new();
        if matches!(kind, JoinKind::Left) {
            // A conjunct that reads the nullable side must stay above the outer join.
            collect_qualifiers(&[&**left], &mut own);
        } else {
            collect_qualifiers(&[&**left, &**right], &mut own);
        }
        let mut selected: Vec<&Expr> = Vec::new();
        for term in terms {
            let known = has_conjunct(on, term)
                || selected.iter().any(|chosen| chosen.structurally_eq(term));
            if !known && owned_by(term, &own, scope, outer) {
                selected.push(*term);
            }
        }
        if !selected.is_empty() {
            let mut combined = if matches!(on, Expr::BoolLiteral(true)) {
                None
            } else {
                Some(on.clone())
            };
            for term in selected {
                combined = Some(match combined {
                    Some(prior) => Expr::Binary {
                        left: Box::new(prior),
                        op: BinaryOp::And,
                        right: Box::new(term.clone()),
                    },
                    None => term.clone(),
                });
            }
            if let Some(combined) = combined {
                *on = combined;
            }
            if matches!(kind, JoinKind::Cross) {
                *kind = JoinKind::Inner;
            }
            changed = true;
        }
    }
    push_terms(left, terms, scope, outer) || changed
}

/// Names that qualify columns of the given sources.
fn collect_qualifiers(sources: &[&QuerySource], out: &mut Vec<String>) {
    for source in sources {
        match source {
            // An alias hides the names inside the aliased source.
            QuerySource::Aliased { alias, .. } | QuerySource::Subquery { alias, .. } => {
                // Bound columns carry the runtime row qualifier derived from the alias.
                out.push(crate::sql::binder::alias_row_qualifier(alias));
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

/// Whether the term is eligible and every column it reads is qualified by one of the join's
/// own sources (`own`) or belongs to the enclosing row. A name qualified by another source of
/// the same FROM clause (`scope`) and an unqualified name are not provably local, so such a
/// term must stay in the `WHERE`.
fn owned_by(term: &Expr, own: &[String], scope: &[String], outer: &dyn Fn(&str) -> bool) -> bool {
    let qualifies = |owners: &[String], qualifier: &str| {
        owners
            .iter()
            .any(|owner| owner.eq_ignore_ascii_case(qualifier))
    };
    let mut names = Vec::new();
    simple_columns(term, &mut names)
        && names.iter().all(|name| {
            name.rsplit_once('.').is_some_and(|(qualifier, _)| {
                qualifies(own, qualifier) || (!qualifies(scope, qualifier) && outer(name))
            })
        })
}
