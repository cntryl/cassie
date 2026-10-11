//! Index lifecycle commands keep their admitted output ahead of mutation.
use super::{
    catalog, empty_command, refresh_table_cardinality_stats, schema_constraint_helpers,
    schema_foreign_keys, Cassie, QueryError, QueryExecutionControls, QueryResult,
};
use crate::sql::ast::DropIndexStatement;

pub(in crate::executor::execution) fn drop_index(
    cassie: &Cassie,
    statement: &DropIndexStatement,
    controls: &QueryExecutionControls,
) -> Result<QueryResult, QueryError> {
    let index = cassie.catalog.get_index(&statement.table, &statement.name);

    if statement.if_exists && index.is_none() {
        return empty_command("DROP INDEX", controls);
    }

    if let Some(index) = index.as_ref() {
        schema_foreign_keys::reject_referenced_unique_index_drop(cassie, &statement.table, index)?;
        schema_constraint_helpers::reject_constraint_index_drop(
            cassie,
            &statement.table,
            &index.name,
        )?;
    }

    let output = empty_command("DROP INDEX", controls)?;
    if let Some(index) = index.as_ref() {
        if matches!(index.kind, catalog::IndexKind::Vector) {
            cassie
                .catalog
                .unregister_vector_index(&statement.table, &index.field);
        }
    }
    cassie
        .midge
        .defer_drop_index(
            &statement.table,
            &statement.name,
            cassie.runtime.schema_epoch(),
        )
        .map_err(|error| QueryError::General(error.to_string()))?;
    cassie
        .catalog
        .unregister_index(&statement.table, &statement.name);
    refresh_table_cardinality_stats(cassie, &statement.table)?;

    Ok(output)
}

