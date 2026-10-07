//! Normalize selected conditionals once, after the initial statement bind.
//! Recursive binding and parameter/subquery revalidation never add casts.
use super::coalesce_results::ResultTypes;
use super::{
    BindingContext, CassieError, Catalog, CteQuery, Expr, HashMap, QuerySource, QueryStatement,
    Schema, SelectItem, SelectStatement,
};

pub(super) fn coerce_statement(
    statement: &mut crate::sql::ast::ParsedStatement,
    catalog: &Catalog,
    context: &BindingContext,
    ctes: &HashMap<String, Schema>,
) -> Result<(), CassieError> {
    match &mut statement.statement {
        QueryStatement::Select(select) => coerce_select(select, catalog, context, ctes, None)?,
        QueryStatement::Explain(explain) => {
            coerce_statement(&mut explain.statement, catalog, context, ctes)?;
        }
        QueryStatement::Update(update) => {
            let types = table_types(&update.table, catalog, context)?;
            for (_, expr) in &mut update.assignments {
                coerce_expression(expr, &types, catalog, context)?;
            }
            if let Some(expr) = &mut update.filter {
                coerce_expression(expr, &types, catalog, context)?;
            }
            coerce_items(&mut update.returning, &types, catalog, context)?;
        }
        QueryStatement::Delete(delete) => {
            let types = table_types(&delete.table, catalog, context)?;
            if let Some(expr) = &mut delete.filter {
                coerce_expression(expr, &types, catalog, context)?;
            }
            coerce_items(&mut delete.returning, &types, catalog, context)?;
        }
        QueryStatement::Insert(insert) => {
            let types = table_types(&insert.table, catalog, context)?;
            match &mut insert.source {
                super::InsertSource::Values(rows) => {
                    for expr in rows.iter_mut().flatten() {
                        coerce_expression(expr, &types, catalog, context)?;
                    }
                }
                super::InsertSource::Select(select) => {
                    coerce_select(select, catalog, context, ctes, None)?;
                }
            }
            if let Some(conflict) = &mut insert.on_conflict {
                if let crate::sql::ast::InsertConflictAction::DoUpdate {
                    assignments,
                    filter,
                } = &mut conflict.action
                {
                    let schema = catalog
                        .get_schema(&insert.table)
                        .ok_or_else(|| CassieError::CollectionNotFound(insert.table.to_string()))?;
                    let conflict_types =
                        table_types(&insert.table, catalog, context)?.with_excluded_fields(&schema);
                    for (_, expr) in assignments {
                        coerce_expression(expr, &conflict_types, catalog, context)?;
                    }
                    if let Some(expr) = filter {
                        coerce_expression(expr, &conflict_types, catalog, context)?;
                    }
                }
            }
            coerce_items(&mut insert.returning, &types, catalog, context)?;
        }
        _ => {}
    }
    Ok(())
}

fn table_types(
    table: &crate::sql::ast::IdentifierPath,
    catalog: &Catalog,
    context: &BindingContext,
) -> Result<ResultTypes, CassieError> {
    ResultTypes::for_source(
        &QuerySource::Collection(table.clone()),
        &[],
        catalog,
        context,
    )
}

fn coerce_select(
    select: &mut SelectStatement,
    catalog: &Catalog,
    context: &BindingContext,
    outer_ctes: &HashMap<String, Schema>,
    outer_fields: Option<&ResultTypes>,
) -> Result<(), CassieError> {
    if !super::coalesce_results::select_contains_coalesce(select) {
        return Ok(());
    }
    let mut scoped = select.clone();
    let names = outer_ctes.keys().cloned().collect();
    super::boolean_contexts::resolve_nested_sources(&mut scoped, &names, catalog, context)?;
    let types = ResultTypes::for_scope(&scoped.source, &scoped.ctes, catalog, context, outer_ctes)?;
    let types = if let Some(outer) = outer_fields {
        types.with_outer_fields(outer)
    } else {
        types
    };
    for cte in &mut select.ctes {
        match &mut cte.query {
            CteQuery::Simple(statement) => {
                coerce_statement(statement, catalog, context, types.cte_schemas())?;
            }
            CteQuery::Recursive {
                base, recursive, ..
            } => {
                coerce_statement(base, catalog, context, types.cte_schemas())?;
                coerce_statement(recursive, catalog, context, types.cte_schemas())?;
            }
        }
    }
    coerce_items(&mut select.projection, &types, catalog, context)?;
    for expr in select
        .filter
        .iter_mut()
        .chain(&mut select.distinct_on)
        .chain(&mut select.group_by)
        .chain(&mut select.having)
        .chain(select.order.iter_mut().map(|order| &mut order.expr))
    {
        coerce_expression(expr, &types, catalog, context)?;
    }
    coerce_source(&mut select.source, catalog, context, &types)?;
    if let Some(set) = &mut select.set {
        coerce_select(&mut set.right, catalog, context, types.cte_schemas(), None)?;
    }
    Ok(())
}

fn coerce_items(
    items: &mut [SelectItem],
    types: &ResultTypes,
    catalog: &Catalog,
    context: &BindingContext,
) -> Result<(), CassieError> {
    for item in items {
        match item {
            SelectItem::Expr { expr, .. } => coerce_nested(expr, types, catalog, context)?,
            SelectItem::Function { function, .. } => {
                for arg in &mut function.args {
                    coerce_nested(arg, types, catalog, context)?;
                }
            }
            SelectItem::WindowFunction { function, .. } => {
                for expr in function
                    .args
                    .iter_mut()
                    .chain(&mut function.partition_by)
                    .chain(function.order_by.iter_mut().map(|order| &mut order.expr))
                {
                    coerce_nested(expr, types, catalog, context)?;
                }
            }
            _ => {}
        }
        super::conditional_types::validate_item(item, types)?;
        super::conditional_types::coerce_item(item, types);
    }
    Ok(())
}

fn coerce_expression(
    expr: &mut Expr,
    types: &ResultTypes,
    catalog: &Catalog,
    context: &BindingContext,
) -> Result<(), CassieError> {
    coerce_nested(expr, types, catalog, context)?;
    super::conditional_types::validate_expression(expr, types)?;
    super::conditional_types::coerce_expression(expr, types);
    Ok(())
}

fn coerce_nested(
    expr: &mut Expr,
    types: &ResultTypes,
    catalog: &Catalog,
    context: &BindingContext,
) -> Result<(), CassieError> {
    let mut error = None;
    *expr = expr.map_children(|child| {
        let mut child = child.clone();
        if let Err(found) = coerce_nested(&mut child, types, catalog, context) {
            error.get_or_insert(found);
        }
        child
    });
    if let Some(error) = error {
        return Err(error);
    }
    if let Expr::Exists(statement) = expr {
        if let QueryStatement::Select(select) = &mut statement.statement {
            coerce_select(select, catalog, context, types.cte_schemas(), Some(types))?;
        }
    }
    Ok(())
}

fn coerce_source(
    source: &mut QuerySource,
    catalog: &Catalog,
    context: &BindingContext,
    types: &ResultTypes,
) -> Result<(), CassieError> {
    match source {
        QuerySource::Join {
            left, right, on, ..
        } => {
            coerce_expression(on, types, catalog, context)?;
            coerce_source(left, catalog, context, types)?;
            coerce_source(right, catalog, context, types)?;
        }
        QuerySource::Subquery {
            select, lateral, ..
        } => coerce_select(
            select,
            catalog,
            context,
            types.cte_schemas(),
            lateral.then_some(types),
        )?,
        QuerySource::TableFunction { function, .. } => {
            for arg in &mut function.args {
                coerce_expression(arg, types, catalog, context)?;
            }
        }
        _ => {}
    }
    Ok(())
}
