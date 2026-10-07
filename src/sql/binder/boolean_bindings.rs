//! Applies parameter provenance and final result domains to an owned bound AST.

use super::coalesce_results::ResultTypes;
use super::{
    boolean_contexts, BindingContext, CassieError, Catalog, Expr, ParsedStatement, QuerySource,
    QueryStatement, SelectItem,
};
use crate::sql::ast::InsertConflictAction;

pub(crate) fn bind_boolean_parameters(
    statement: &mut ParsedStatement,
    catalog: &Catalog,
    context: &BindingContext,
    parameter_types: &[i32],
) -> Result<(), CassieError> {
    match &mut statement.statement {
        QueryStatement::Select(select) => {
            let types = ResultTypes::for_source_with_parameters(
                &select.source,
                &select.ctes,
                catalog,
                context,
                parameter_types,
            )?
            .with_resolved_coalesce_domains(true);
            boolean_contexts::validate_select_with_types(select, &types, catalog, context)?;
        }
        QueryStatement::Insert(insert) => {
            super::commands::bind_insert_boolean_parameters(
                insert,
                catalog,
                context,
                parameter_types,
            )?;
            let types = table_types(&insert.table, catalog, context, parameter_types)?;
            if let Some(conflict) = &mut insert.on_conflict {
                if let InsertConflictAction::DoUpdate {
                    assignments,
                    filter,
                } = &mut conflict.action
                {
                    let schema = catalog
                        .get_schema(&insert.table)
                        .ok_or_else(|| CassieError::CollectionNotFound(insert.table.to_string()))?;
                    let conflict_types =
                        table_types(&insert.table, catalog, context, parameter_types)?
                            .with_excluded_fields(&schema);
                    validate_assignments(
                        assignments,
                        &insert.table,
                        &conflict_types,
                        catalog,
                        context,
                    )?;
                    if let Some(filter) = filter {
                        boolean_contexts::validate_predicate(
                            filter,
                            &conflict_types,
                            "ON CONFLICT WHERE",
                            catalog,
                            context,
                        )?;
                    }
                }
            }
            validate_items(&mut insert.returning, &types, catalog, context)?;
        }
        QueryStatement::Update(update) => {
            let types = table_types(&update.table, catalog, context, parameter_types)?;
            validate_assignments(
                &mut update.assignments,
                &update.table,
                &types,
                catalog,
                context,
            )?;
            if let Some(filter) = &mut update.filter {
                boolean_contexts::validate_predicate(
                    filter,
                    &types,
                    "UPDATE WHERE",
                    catalog,
                    context,
                )?;
            }
            validate_items(&mut update.returning, &types, catalog, context)?;
        }
        QueryStatement::Delete(delete) => {
            let types = table_types(&delete.table, catalog, context, parameter_types)?;
            if let Some(filter) = &mut delete.filter {
                boolean_contexts::validate_predicate(
                    filter,
                    &types,
                    "DELETE WHERE",
                    catalog,
                    context,
                )?;
            }
            validate_items(&mut delete.returning, &types, catalog, context)?;
        }
        QueryStatement::Explain(explain) => {
            bind_boolean_parameters(&mut explain.statement, catalog, context, parameter_types)?;
        }
        _ => {}
    }
    Ok(())
}

fn table_types(
    table: &crate::sql::ast::IdentifierPath,
    catalog: &Catalog,
    context: &BindingContext,
    parameter_types: &[i32],
) -> Result<ResultTypes, CassieError> {
    ResultTypes::for_source_with_parameters(
        &QuerySource::Collection(table.clone()),
        &[],
        catalog,
        context,
        parameter_types,
    )
    .map(|types| types.with_resolved_coalesce_domains(true))
}

fn validate_assignments(
    assignments: &mut [(String, Expr)],
    table: &crate::sql::ast::IdentifierPath,
    types: &ResultTypes,
    catalog: &Catalog,
    context: &BindingContext,
) -> Result<(), CassieError> {
    let schema = catalog
        .get_schema(table)
        .ok_or_else(|| CassieError::CollectionNotFound(table.to_string()))?;
    for (name, expression) in assignments {
        let expected = schema
            .fields
            .iter()
            .find(|field| field.name == *name)
            .map(|field| &field.data_type);
        boolean_contexts::validate_value(expression, types, expected, catalog, context)?;
    }
    Ok(())
}

fn validate_items(
    items: &mut [SelectItem],
    types: &ResultTypes,
    catalog: &Catalog,
    context: &BindingContext,
) -> Result<(), CassieError> {
    for item in items {
        match item {
            SelectItem::Expr { expr, .. } => {
                boolean_contexts::validate_value(expr, types, None, catalog, context)?;
            }
            SelectItem::Function { function, .. } => {
                for argument in &mut function.args {
                    boolean_contexts::validate_value(argument, types, None, catalog, context)?;
                }
                super::coalesce_coercion::coerce_function(function, types);
            }
            SelectItem::WindowFunction { function, .. } => {
                for expression in function
                    .args
                    .iter_mut()
                    .chain(&mut function.partition_by)
                    .chain(function.order_by.iter_mut().map(|order| &mut order.expr))
                {
                    boolean_contexts::validate_value(expression, types, None, catalog, context)?;
                }
            }
            SelectItem::Wildcard | SelectItem::Column { .. } => {}
        }
    }
    Ok(())
}
