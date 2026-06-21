use std::collections::BTreeSet;

use super::*;

pub(super) fn try_execute_analytical_projection(
    cassie: &Cassie,
    session: Option<&CassieSession>,
    plan: &LogicalPlan,
    cte_context: &mut CteContext,
    user_functions: &HashMap<String, FunctionMeta>,
    params: &[Value],
    controls: &QueryExecutionControls,
) -> Result<Option<Vec<BatchRow>>, QueryError> {
    let QuerySource::Collection(source) = &plan.source else {
        return Ok(None);
    };
    if cassie.catalog.is_materialized_projection(source)
        || cassie
            .catalog
            .materialized_projection_for_output(source)
            .is_some()
        || !plan.ctes.is_empty()
    {
        return Ok(None);
    }

    let Some((projection_name, output_collection)) =
        covered_analytical_projection(cassie, source, plan)
    else {
        if let Some((projection, reason)) = analytical_projection_fallback(cassie, source, plan) {
            cassie
                .runtime
                .record_mixed_execution_fallback(projection, reason);
        }
        return Ok(None);
    };

    let mut rewritten = plan.clone();
    rewritten.source = QuerySource::Collection(output_collection.clone());
    rewritten.collection = output_collection;
    let rows = super::execute_plan_with_outer_row(
        cassie,
        session,
        &rewritten,
        cte_context,
        user_functions,
        params,
        controls,
        None,
    )?;
    cassie
        .runtime
        .record_mixed_execution_optimized(projection_name);
    Ok(Some(rows))
}

fn analytical_projection_fallback(
    cassie: &Cassie,
    source: &str,
    plan: &LogicalPlan,
) -> Option<(String, String)> {
    let needed = plan_needed_columns(plan)?;
    cassie
        .catalog
        .list_projection_metadata()
        .into_iter()
        .find_map(|projection| {
            let materialized = projection.materialized.as_ref()?;
            if !materialized
                .options
                .get("analytical")
                .is_some_and(|value| value.eq_ignore_ascii_case("true"))
                || !materialized
                    .source_collections
                    .iter()
                    .any(|candidate| candidate == source)
            {
                return None;
            }
            if projection.freshness != catalog::ProjectionFreshness::Fresh {
                return Some((projection.collection, "stale-or-unverified".to_string()));
            }
            let output_fields = materialized
                .output_schema
                .fields
                .iter()
                .map(|field| field.name.to_ascii_lowercase())
                .collect::<BTreeSet<_>>();
            if needed.iter().all(|field| output_fields.contains(field)) {
                None
            } else {
                Some((projection.collection, "coverage-mismatch".to_string()))
            }
        })
}

fn covered_analytical_projection(
    cassie: &Cassie,
    source: &str,
    plan: &LogicalPlan,
) -> Option<(String, String)> {
    if plan.command.is_some()
        || plan.distinct
        || !plan.distinct_on.is_empty()
        || !plan.group_by.is_empty()
        || plan.having.is_some()
        || plan.set.is_some()
    {
        return None;
    }

    let needed = plan_needed_columns(plan)?;
    cassie
        .catalog
        .list_projection_metadata()
        .into_iter()
        .find_map(|projection| {
            let materialized = projection.materialized.as_ref()?;
            if projection.freshness != catalog::ProjectionFreshness::Fresh
                || !materialized
                    .options
                    .get("analytical")
                    .is_some_and(|value| value.eq_ignore_ascii_case("true"))
                || !materialized
                    .source_collections
                    .iter()
                    .any(|candidate| candidate == source)
            {
                return None;
            }
            let output_fields = materialized
                .output_schema
                .fields
                .iter()
                .map(|field| field.name.to_ascii_lowercase())
                .collect::<BTreeSet<_>>();
            if needed.iter().all(|field| output_fields.contains(field)) {
                projection
                    .active_output_collection()
                    .map(|output| (projection.collection.clone(), output.to_string()))
            } else {
                None
            }
        })
}

fn plan_needed_columns(plan: &LogicalPlan) -> Option<BTreeSet<String>> {
    let mut columns = BTreeSet::new();
    for item in &plan.projection {
        match item {
            SelectItem::Column { name, .. } => {
                if !projected_read::is_row_id_column(name) {
                    columns.insert(name.to_ascii_lowercase());
                }
            }
            SelectItem::Wildcard => return None,
            SelectItem::Function { function, .. } => {
                collect_expr_columns_from_slice(function.args.as_slice(), &mut columns)
            }
            SelectItem::Expr { expr, .. } => collect_expr_columns(expr, &mut columns),
            SelectItem::WindowFunction { .. } => return None,
        }
    }
    if let Some(filter) = &plan.filter {
        collect_expr_columns(filter, &mut columns);
    }
    for order in &plan.order {
        collect_expr_columns(&order.expr, &mut columns);
    }
    Some(columns)
}

fn collect_expr_columns_from_slice(exprs: &[Expr], columns: &mut BTreeSet<String>) {
    for expr in exprs {
        collect_expr_columns(expr, columns);
    }
}

fn collect_expr_columns(expr: &Expr, columns: &mut BTreeSet<String>) {
    match expr {
        Expr::Column(name) => {
            if !projected_read::is_row_id_column(name) {
                columns.insert(name.to_ascii_lowercase());
            }
        }
        Expr::Binary { left, right, .. } => {
            collect_expr_columns(left, columns);
            collect_expr_columns(right, columns);
        }
        Expr::Not { expr } | Expr::IsNull { expr, .. } => {
            collect_expr_columns(expr, columns);
        }
        Expr::Between {
            expr, low, high, ..
        } => {
            collect_expr_columns(expr, columns);
            collect_expr_columns(low, columns);
            collect_expr_columns(high, columns);
        }
        Expr::InList { expr, values, .. } => {
            collect_expr_columns(expr, columns);
            collect_expr_columns_from_slice(values, columns);
        }
        Expr::Function(function) => {
            collect_expr_columns_from_slice(function.args.as_slice(), columns)
        }
        Expr::Cast { expr, .. } => collect_expr_columns(expr, columns),
        Expr::Exists(_) => {}
        Expr::StringLiteral(_)
        | Expr::NumberLiteral(_)
        | Expr::BoolLiteral(_)
        | Expr::Null
        | Expr::Param(_) => {}
    }
}
