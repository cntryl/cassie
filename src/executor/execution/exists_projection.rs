//! Classify each SELECT EXISTS through the existing binder's scope rules.
use super::{
    check_timeout, ExistsResolutionContext, Expr, HashSet, QueryError, QuerySource, SelectItem,
};

mod admission;
mod rows;
mod scope;

pub(super) use rows::{outer_row, project};

pub(super) fn contains(projection: &[SelectItem]) -> bool {
    projection.iter().any(|item| match item {
        SelectItem::Expr { expr, .. } => super::exists_correlated::contains_exists(expr),
        SelectItem::Function { function, .. } => function
            .args
            .iter()
            .any(super::exists_correlated::contains_exists),
        _ => false,
    })
}

pub(super) fn reserve_clone(
    value: &impl serde::Serialize,
    context: &ExistsResolutionContext<'_>,
) -> Result<crate::runtime::QueryMemoryReservation, QueryError> {
    Ok(admission::reserve_clone(value, context.controls)?)
}

pub(super) fn resolve_item(
    context: &ExistsResolutionContext<'_>,
    source: &QuerySource,
    item: &mut SelectItem,
    scope: &mut Option<scope::Scope>,
) -> Result<(), QueryError> {
    let mut resolve = |expr: &mut Expr| {
        *expr = classify(context, source, expr, scope)?;
        Ok::<_, QueryError>(())
    };
    match item {
        SelectItem::Expr { expr, .. } => resolve(expr)?,
        SelectItem::Function { function, .. } => {
            for argument in &mut function.args {
                resolve(argument)?;
            }
        }
        SelectItem::WindowFunction { function, .. } => {
            for argument in function.args.iter_mut().chain(&mut function.partition_by) {
                resolve(argument)?;
            }
            for order in &mut function.order_by {
                resolve(&mut order.expr)?;
            }
        }
        _ => {}
    }
    Ok(())
}

fn classify(
    context: &ExistsResolutionContext<'_>,
    source: &QuerySource,
    expr: &Expr,
    scope: &mut Option<scope::Scope>,
) -> Result<Expr, QueryError> {
    check_timeout(context.controls)?;
    if let Expr::Exists(statement) = expr {
        match super::dispatch::build_exists_logical_plan(context, statement) {
            Ok(_) => return super::resolve_exists_expr(context, expr),
            Err(original) => {
                check_timeout(context.controls)?;
                if scope.is_none() {
                    *scope = Some(scope::Scope::new(context, source)?);
                }
                let fields = &scope.as_ref().expect("admitted enclosing scope").fields;
                return match super::dispatch::build_exists_logical_plan_with_fields(
                    context, statement, fields,
                ) {
                    Ok(_) => Ok(expr.clone()),
                    Err(_) => Err(original),
                };
            }
        }
    }
    let mut failure = None;
    let resolved = expr.map_children(|child| match classify(context, source, child, scope) {
        Ok(resolved) => resolved,
        Err(error) => {
            failure = Some(error);
            child.clone()
        }
    });
    failure.map_or(Ok(resolved), Err)
}
