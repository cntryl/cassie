//! Same-schema SQL materialized-view rename using existing catalog metadata.
use super::{Cassie, QueryError, QueryExecutionControls, QueryResult};
use crate::app::{CassieError, DiagnosticError};
use crate::catalog::Catalog;
use crate::sql::ast::RenameMaterializedViewStatement;

pub(super) fn rename(
    cassie: &Cassie,
    statement: &RenameMaterializedViewStatement,
    controls: &QueryExecutionControls,
) -> Result<QueryResult, QueryError> {
    let output = super::command_output::empty("ALTER MATERIALIZED VIEW", controls)?;
    cassie.midge.with_collection_gates(
        &[statement.name.clone(), statement.new_name.clone()],
        || {
            let mut metadata = cassie
                .catalog
                .get_materialized_projection(&statement.name)
                .filter(|projection| {
                    projection.materialized.as_ref().is_some_and(
                        crate::catalog::MaterializedProjectionMeta::is_sql_materialized_view,
                    )
                })
                .ok_or_else(|| CassieError::CollectionNotFound(statement.name.clone()))?;
            if cassie.catalog.relation_exists(&statement.new_name)
                || cassie
                    .catalog
                    .get_projection_metadata(&statement.new_name)
                    .is_some()
            {
                return Err(
                    DiagnosticError::new("42P07", "rename destination already exists").into(),
                );
            }
            reject_dependencies(
                &cassie.catalog,
                &statement.name,
                &cassie.default_database,
                cassie.database_catalog_enforced(),
                controls,
            )?;
            if !cassie
                .catalog
                .projection_repair_report_ids(&statement.name)
                .is_empty()
                || !cassie
                    .catalog
                    .projection_comparison_report_ids(&statement.name)
                    .is_empty()
            {
                return Err(DiagnosticError::new(
                    "2BP01",
                    "materialized view has retained projection reports",
                )
                .into());
            }
            controls.check_interruption()?;
            metadata.collection.clone_from(&statement.new_name);
            if let Some(materialized) = metadata.materialized.as_mut() {
                materialized.name.clone_from(&statement.new_name);
            }
            cassie
                .midge
                .rename_sql_materialized_view_metadata(&statement.name, &metadata)?;
            cassie
                .catalog
                .rename_materialized_view_metadata(&statement.name, metadata);
            Ok::<_, CassieError>(())
        },
    )?;
    Ok(output)
}

/// Rebind stored definitions without the old relation before touching storage.
/// This deliberately rejects dependent or unresolvable definitions instead of
/// attempting SQL-text replacement. Original catalog and output remain intact.
fn reject_dependencies(
    catalog: &Catalog,
    previous: &str,
    default_database: &str,
    database_enforced: bool,
    controls: &QueryExecutionControls,
) -> Result<(), CassieError> {
    let database = crate::catalog::relation_database_name(previous)
        .unwrap_or_else(|| default_database.to_string());
    controls.check_interruption()?;
    let without_previous = catalog.detached_transaction_snapshot();
    without_previous.unregister_projection_metadata(previous);
    let definitions = catalog
        .list_views()
        .into_iter()
        .map(|view| (view.name, view.query))
        .chain(
            catalog
                .list_projection_metadata()
                .into_iter()
                .filter_map(|projection| {
                    projection
                        .materialized
                        .map(|definition| (projection.collection, definition.query))
                }),
        );
    let definitions = definitions
        .chain(
            catalog
                .list_functions()
                .into_iter()
                .map(|routine| (routine.name, routine.body)),
        )
        .chain(
            catalog
                .list_procedures()
                .into_iter()
                .map(|routine| (routine.name, routine.body)),
        );
    for (name, query) in definitions {
        controls.check_interruption()?;
        if !crate::catalog::relation_belongs_to_database(&name, &database) {
            continue;
        }
        let schema = crate::catalog::relation_schema_name(&name);
        let context = if database_enforced {
            crate::sql::binder::BindingContext::scoped(
                database.clone(),
                vec![schema, "public".to_string()],
            )
        } else {
            crate::sql::binder::BindingContext::unscoped(
                database.clone(),
                vec![schema, "public".to_string()],
            )
        };
        let accepted = crate::sql::parse_statement(&query).is_ok_and(|parsed| {
            crate::sql::binder::bind_with_context(parsed, &without_previous, &context).is_ok()
        });
        controls.check_interruption()?;
        if !accepted {
            return Err(DiagnosticError::new("2BP01", format!("cannot rename materialized view while stored definition '{name}' depends on it or cannot be resolved")).into());
        }
    }
    Ok(())
}
