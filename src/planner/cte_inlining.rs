//! Controlled, unpublished expansion of explicit nonvolatile NOT MATERIALIZED CTEs.
mod eligibility;
mod input;
mod memory;
mod producer;
#[cfg(test)]
mod tests;

use crate::app::CassieError;
use crate::catalog::Catalog;
use crate::executor::retained_memory::{add, mul};
use crate::planner::LogicalPlan;
use crate::runtime::{QueryExecutionControls, QueryMemoryReservation};
use crate::sql::ast::{
    CommonTableExpression, CteMaterialization, CteQuery, Expr, QuerySource, QueryStatement,
    SelectItem, SelectStatement,
};
use input::Input;

pub(crate) fn inline(
    plan: &mut LogicalPlan,
    catalog: &Catalog,
    controls: &QueryExecutionControls,
) -> Result<Option<QueryMemoryReservation>, CassieError> {
    controls.check_interruption()?;
    if plan.command.is_some()
        || !plan
            .ctes
            .iter()
            .any(|cte| cte.materialization == CteMaterialization::NotMaterialized)
    {
        return Ok(None);
    }
    let live = crate::executor::reached_plan_ctes(plan, controls)?;
    let mut memory = controls.reserve_query_memory(mul(memory::plan(plan, controls)?, 4)?)?;
    controls.check_interruption()?;
    let mut working = plan.clone();
    let mut changed = false;
    // Definitions inherit earlier CTEs. Rewrite only the body, not its scope,
    // and retain its own local definitions as an explicit boundary.
    for index in 0..working.ctes.len() {
        controls.check_interruption()?;
        if !live.contains(index) {
            continue;
        }
        let CteQuery::Simple(statement) = &working.ctes[index].query else {
            continue;
        };
        let QueryStatement::Select(select) = &statement.statement else {
            continue;
        };
        if !select.ctes.is_empty() || index == 0 {
            continue;
        }
        let mut body = from_select(select, &working.ctes[..index]);
        if expand(&mut body, catalog, controls, &mut memory)? {
            let CteQuery::Simple(statement) = &mut working.ctes[index].query else {
                unreachable!();
            };
            let QueryStatement::Select(select) = &mut statement.statement else {
                unreachable!();
            };
            apply(select, body);
            changed = true;
        }
    }
    changed |= expand(&mut working, catalog, controls, &mut memory)?;
    drop(live);
    if changed {
        *plan = working;
        Ok(Some(memory))
    } else {
        Ok(None)
    }
}

fn expand(
    plan: &mut LogicalPlan,
    catalog: &Catalog,
    controls: &QueryExecutionControls,
    memory: &mut QueryMemoryReservation,
) -> Result<bool, CassieError> {
    let mut changed = false;
    for _ in 0..=plan.ctes.len() {
        controls.check_interruption()?;
        let Some(index) = candidate(&plan.source, &plan.ctes) else {
            break;
        };
        let cte = &plan.ctes[index];
        let CteQuery::Simple(statement) = &cte.query else {
            break;
        };
        let QueryStatement::Select(select) = &statement.statement else {
            break;
        };
        if !eligibility::select(select, catalog, controls)? {
            break;
        }
        if !eligibility::flat(select)
            || plan.projection.iter().any(|item| {
                matches!(
                    item,
                    SelectItem::Wildcard | SelectItem::WindowFunction { .. }
                )
            })
            || plan.set.is_some()
        {
            break;
        }
        let Some(input) = Input::new(&plan.source, cte, select, controls)? else {
            break;
        };
        let extra = estimate(plan, &input, controls)?;
        memory.try_grow(mul(
            add(
                extra,
                crate::sql::parse_analysis::select_clone_bytes(select, controls)?,
            )?,
            4,
        )?)?;
        controls.check_interruption()?;
        let source = select.source.clone();
        let projection = plan
            .projection
            .iter()
            .map(|item| input.item(item, controls))
            .collect::<Result<Vec<_>, _>>()?;
        let outer_filter = rewrite_optional(plan.filter.as_ref(), &input, controls)?;
        let filter = match (select.filter.clone(), outer_filter) {
            (Some(left), Some(right)) => Some(Expr::Binary {
                left: Box::new(left),
                op: crate::sql::ast::BinaryOp::And,
                right: Box::new(right),
            }),
            (left, right) => left.or(right),
        };
        let group_by = rewrite_list(&plan.group_by, &input, controls)?;
        let distinct_on = rewrite_list(&plan.distinct_on, &input, controls)?;
        let having = rewrite_optional(plan.having.as_ref(), &input, controls)?;
        let limit = rewrite_optional(plan.limit.as_ref(), &input, controls)?;
        let offset = rewrite_optional(plan.offset.as_ref(), &input, controls)?;
        let mut order = plan.order.clone();
        for item in &mut order {
            if !output_alias(&item.expr, &plan.projection) {
                item.expr = input.rewrite(&item.expr, controls)?;
            }
        }
        plan.collection = crate::planner::logical::source_name(&source);
        plan.source = source;
        plan.projection = projection;
        plan.filter = filter;
        plan.group_by = group_by;
        plan.distinct_on = distinct_on;
        plan.having = having;
        plan.limit = limit;
        plan.offset = offset;
        plan.order = order;
        changed = true;
    }
    // A multiply referenced pure definition has an independent derived input
    // per reference. Volatile and recursive definitions retain the shared cache.
    let mut source = plan.source.clone();
    if references(&mut source, &plan.ctes, catalog, controls, memory)? {
        plan.collection = crate::planner::logical::source_name(&source);
        plan.source = source;
        changed = true;
    }
    Ok(changed)
}

fn candidate(source: &QuerySource, ctes: &[CommonTableExpression]) -> Option<usize> {
    let name = match source {
        QuerySource::Cte(name) => name,
        QuerySource::Aliased { source, .. } => {
            let QuerySource::Cte(name) = source.as_ref() else {
                return None;
            };
            name
        }
        _ => return None,
    };
    ctes.iter().rposition(|cte| {
        cte.name.eq_ignore_ascii_case(name)
            && cte.materialization == CteMaterialization::NotMaterialized
    })
}

fn references(
    source: &mut QuerySource,
    ctes: &[CommonTableExpression],
    catalog: &Catalog,
    controls: &QueryExecutionControls,
    memory: &mut QueryMemoryReservation,
) -> Result<bool, CassieError> {
    controls.check_interruption()?;
    match source {
        QuerySource::Aliased { source, .. } => references(source, ctes, catalog, controls, memory),
        QuerySource::Join { left, right, .. } => {
            Ok(references(left, ctes, catalog, controls, memory)?
                | references(right, ctes, catalog, controls, memory)?)
        }
        QuerySource::Cte(name) => {
            let Some(cte) = ctes.iter().rev().find(|cte| {
                cte.name.eq_ignore_ascii_case(name)
                    && cte.materialization == CteMaterialization::NotMaterialized
            }) else {
                return Ok(false);
            };
            let CteQuery::Simple(statement) = &cte.query else {
                return Ok(false);
            };
            let QueryStatement::Select(select) = &statement.statement else {
                return Ok(false);
            };
            if !eligibility::select(select, catalog, controls)? {
                return Ok(false);
            }
            let select = producer::copy(cte, select, controls, memory)?;
            *source = QuerySource::Subquery {
                alias: name.clone(),
                select,
                lateral: false,
            };
            Ok(true)
        }
        _ => Ok(false),
    }
}

fn estimate(
    plan: &LogicalPlan,
    input: &Input<'_>,
    controls: &QueryExecutionControls,
) -> Result<usize, CassieError> {
    let mut bytes = 4 * size_of::<Expr>();
    for item in &plan.projection {
        match item {
            SelectItem::Column { name, .. } => {
                bytes = add(
                    bytes,
                    input.estimate(&Expr::Column(name.clone()), 0, controls)?,
                )?;
            }
            SelectItem::Expr { expr, .. } => {
                bytes = add(bytes, input.estimate(expr, 0, controls)?)?;
            }
            SelectItem::Function { function, .. } => {
                for expr in function.expressions() {
                    bytes = add(bytes, input.estimate(expr, 1, controls)?)?;
                }
            }
            _ => {}
        }
    }
    for expr in plan
        .filter
        .iter()
        .chain(plan.having.iter())
        .chain(&plan.group_by)
        .chain(&plan.distinct_on)
        .chain(plan.limit.iter())
        .chain(plan.offset.iter())
        .chain(plan.order.iter().map(|item| &item.expr))
    {
        bytes = add(bytes, input.estimate(expr, 0, controls)?)?;
    }
    Ok(bytes)
}
fn rewrite_optional(
    expr: Option<&Expr>,
    input: &Input<'_>,
    controls: &QueryExecutionControls,
) -> Result<Option<Expr>, CassieError> {
    expr.map(|expr| input.rewrite(expr, controls)).transpose()
}
fn rewrite_list(
    exprs: &[Expr],
    input: &Input<'_>,
    controls: &QueryExecutionControls,
) -> Result<Vec<Expr>, CassieError> {
    exprs
        .iter()
        .map(|expr| input.rewrite(expr, controls))
        .collect()
}

fn from_select(select: &SelectStatement, outer: &[CommonTableExpression]) -> LogicalPlan {
    LogicalPlan {
        command: None,
        collection: crate::planner::logical::source_name(&select.source),
        source: select.source.clone(),
        ctes: outer.to_vec(),
        distinct: select.distinct,
        distinct_on: select.distinct_on.clone(),
        projection: select.projection.clone(),
        filter: select.filter.clone(),
        group_by: select.group_by.clone(),
        having: select.having.clone(),
        order: select.order.clone(),
        limit: select.limit.clone(),
        offset: select.offset.clone(),
        set: select.set.clone(),
    }
}
fn apply(select: &mut SelectStatement, plan: LogicalPlan) {
    select.source = plan.source;
    select.distinct = plan.distinct;
    select.distinct_on = plan.distinct_on;
    select.projection = plan.projection;
    select.filter = plan.filter;
    select.group_by = plan.group_by;
    select.having = plan.having;
    select.order = plan.order;
    select.limit = plan.limit;
    select.offset = plan.offset;
    select.set = plan.set;
}

fn output_alias(expr: &Expr, projection: &[SelectItem]) -> bool {
    let Expr::Column(name) = expr else {
        return false;
    };
    let Ok(path) = crate::sql::ColumnIdentifierPath::parse(name) else {
        return false;
    };
    if path.is_qualified() {
        return false;
    }
    let key = path.field_lookup_key();
    projection.iter().any(|item| {
        let alias = match item {
            SelectItem::Column { name, alias } => alias.clone().or_else(|| {
                crate::sql::ColumnIdentifierPath::parse(name)
                    .ok()
                    .map(|path| path.implicit_output_label())
            }),
            SelectItem::Expr { alias, .. }
            | SelectItem::Function { alias, .. }
            | SelectItem::WindowFunction { alias, .. } => alias.clone(),
            SelectItem::Wildcard => None,
        };
        alias.is_some_and(|alias| crate::sql::ColumnIdentifierPath::stored_field_key(&alias) == key)
    })
}
