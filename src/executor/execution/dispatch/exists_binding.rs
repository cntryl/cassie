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

pub(super) fn outer_fields(
    context: &ExistsResolutionContext<'_>,
) -> Result<
    (
        std::collections::HashSet<String>,
        Option<crate::runtime::QueryMemoryReservation>,
    ),
    QueryError,
> {
    use crate::executor::retained_memory::{add, hash_table_bytes, mul};
    let mut count = 0;
    let mut names_bytes = 0;
    let mut next = context.outer_row;
    while let Some(row) = next {
        super::check_timeout(context.controls)?;
        count = add(
            count,
            mul(add(row.entries().len(), row.aliases().len())?, 2)?,
        )?;
        for name in row
            .entries()
            .iter()
            .map(|(name, _)| name)
            .chain(row.aliases().iter().map(|(name, _)| name))
        {
            super::check_timeout(context.controls)?;
            names_bytes = add(names_bytes, add(mul(name.len(), 8)?, 64)?)?;
        }
        next = row.outer_scope().map(std::sync::Arc::as_ref);
    }
    let field_memory = if count == 0 {
        None
    } else {
        Some(
            context
                .controls
                .reserve_query_memory(add(hash_table_bytes::<String>(count)?, names_bytes)?)?,
        )
    };
    let mut outer_fields = std::collections::HashSet::new();
    outer_fields
        .try_reserve(count)
        .map_err(|error| crate::app::CassieError::ResourceLimit(error.to_string()))?;
    let mut next = context.outer_row;
    while let Some(row) = next {
        super::check_timeout(context.controls)?;
        for name in row
            .entries()
            .iter()
            .map(|(name, _)| name)
            .chain(row.aliases().iter().map(|(name, _)| name))
        {
            super::check_timeout(context.controls)?;
            outer_fields.insert(crate::sql::ColumnIdentifierPath::stored_row_lookup_key(
                name,
            ));
            outer_fields.insert(crate::sql::ColumnIdentifierPath::stored_row_field_key(name));
        }
        next = row.outer_scope().map(std::sync::Arc::as_ref);
    }
    Ok((outer_fields, field_memory))
}
