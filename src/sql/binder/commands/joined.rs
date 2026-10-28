//! `UPDATE ... FROM`, `DELETE ... USING`, tuple-subquery assignments and
//! `RETURNING` row images bind against the target joined to its source.

use super::super::{coalesce_results::ResultTypes, BindingContext, CassieError, Catalog};
use super::mutations::{
    bind_assignments, bind_mutation_source, bind_predicate, bind_returning, resolve_target,
    row_image_fields, target_qualifier, JoinedScope,
};
use crate::sql::ast::{DeleteStatement, IdentifierPath, QuerySource, UpdateStatement};

#[must_use]
pub(super) fn update_needs_joined_binding(statement: &UpdateStatement) -> bool {
    statement.from.is_some()
        || !statement.row_subqueries.is_empty()
        || statement.returning_aliases.is_some()
}

#[must_use]
pub(super) fn delete_needs_joined_binding(statement: &DeleteStatement) -> bool {
    statement.using.is_some() || statement.returning_aliases.is_some()
}

fn target_only_scope(
    table: &str,
    qualifier: &str,
    catalog: &Catalog,
    context: &BindingContext,
) -> Result<JoinedScope, CassieError> {
    let source = QuerySource::Aliased {
        source: Box::new(QuerySource::Collection(
            IdentifierPath::parse(table).map_err(CassieError::Planner)?,
        )),
        alias: qualifier.to_string(),
        column_aliases: Vec::new(),
    };
    Ok(JoinedScope {
        types: ResultTypes::for_source(&source, &[], catalog, context)?,
    })
}

pub(super) fn bind_joined_update(
    mut statement: UpdateStatement,
    catalog: &Catalog,
    context: &BindingContext,
) -> Result<UpdateStatement, CassieError> {
    let (table, schema) = resolve_target(&statement.table, "UPDATE", catalog, context)?;
    let qualifier = target_qualifier(&table, statement.alias.as_deref());
    let ctes = statement.ctes.clone();
    let scope = match statement.from.as_mut() {
        Some(source) => bind_mutation_source(source, ctes, &table, &qualifier, catalog, context)?,
        None => target_only_scope(&table, &qualifier, catalog, context)?,
    };
    let scope = JoinedScope {
        types: scope.types.with_extra_fields(row_image_fields(
            &schema,
            statement.returning_aliases.as_ref(),
            false,
        )),
    };
    let mut columns = statement
        .row_subqueries
        .iter()
        .map(|assignment| assignment.columns.clone())
        .collect::<Vec<_>>();
    bind_assignments(
        &mut statement.assignments,
        Some(&mut statement.array_targets),
        &mut columns,
        (&schema, &table),
        &scope,
        catalog,
        context,
    )?;
    for (assignment, columns) in statement.row_subqueries.iter_mut().zip(columns) {
        assignment.columns = columns;
    }
    bind_predicate(
        statement.filter.as_mut(),
        &scope,
        "UPDATE WHERE",
        catalog,
        context,
    )?;
    bind_returning(
        &mut statement.returning,
        (&schema, &qualifier),
        statement.from.as_ref(),
        &scope,
    )?;
    statement.table = IdentifierPath::parse(&table).map_err(CassieError::Planner)?;
    statement.alias = Some(qualifier);
    Ok(statement)
}

pub(super) fn bind_joined_delete(
    mut statement: DeleteStatement,
    catalog: &Catalog,
    context: &BindingContext,
) -> Result<DeleteStatement, CassieError> {
    let (table, schema) = resolve_target(&statement.table, "DELETE", catalog, context)?;
    let qualifier = target_qualifier(&table, statement.alias.as_deref());
    let ctes = statement.ctes.clone();
    let scope = match statement.using.as_mut() {
        Some(source) => bind_mutation_source(source, ctes, &table, &qualifier, catalog, context)?,
        None => target_only_scope(&table, &qualifier, catalog, context)?,
    };
    let scope = JoinedScope {
        types: scope.types.with_extra_fields(row_image_fields(
            &schema,
            statement.returning_aliases.as_ref(),
            false,
        )),
    };
    bind_predicate(
        statement.filter.as_mut(),
        &scope,
        "DELETE WHERE",
        catalog,
        context,
    )?;
    bind_returning(
        &mut statement.returning,
        (&schema, &qualifier),
        statement.using.as_ref(),
        &scope,
    )?;
    statement.table = IdentifierPath::parse(&table).map_err(CassieError::Planner)?;
    statement.alias = Some(qualifier);
    Ok(statement)
}
