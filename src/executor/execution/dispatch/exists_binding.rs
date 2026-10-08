//! Bind an EXISTS statement against the caller's exact enclosing field authority.
use super::{ExistsResolutionContext, HashMap, LogicalPlan, QueryError};

pub(in crate::executor::execution) fn build_exists_logical_plan_with_fields(
    context: &ExistsResolutionContext<'_>,
    statement: &crate::sql::ast::ParsedStatement,
    outer_fields: &std::collections::HashSet<String>,
) -> Result<LogicalPlan, QueryError> {
    let binding_context = super::exists_binding_context(context);
    // Relation names resolve against the enclosing statement's CTEs first.
    let outer_ctes: HashMap<String, Vec<String>> = context
        .cte_context
        .iter()
        .map(|(name, rows)| {
            let columns = rows
                .fields
                .iter()
                .map(|field| crate::sql::ColumnIdentifierPath::stored_field_key(&field.name))
                .collect();
            (name.clone(), columns)
        })
        .collect();
    let bound = crate::sql::binder::bind_with_outer_ctes(
        statement.clone(),
        &context.cassie.catalog,
        &binding_context,
        &outer_ctes,
        outer_fields,
    )
    .map_err(|error| QueryError::General(error.to_string()))?;
    let mut plan = crate::planner::logical::plan(&bound)
        .map_err(|error| QueryError::General(error.to_string()))?;
    crate::planner::logical::rewrite_reserved_id_references(&mut plan, &context.cassie.catalog);
    if plan.command.is_some() {
        return Err(QueryError::General(
            "CTE statements cannot include command statements".into(),
        ));
    }
    Ok(plan)
}
