//! `ALTER TABLE ... RENAME COLUMN`, `RENAME TO` and `SET SCHEMA`.

use super::{refresh_table_cardinality_stats, schema_foreign_keys, schema_sequence_rename};
use crate::app::{Cassie, CassieSession};
use crate::executor::execution::QueryError;

pub(super) fn alter_table_rename_column(
    cassie: &Cassie,
    table: &str,
    from: &str,
    to: &str,
) -> Result<(), QueryError> {
    crate::executor::execution::view_dependents::reject_view_dependents(
        cassie,
        "rename column",
        "column",
        from,
        crate::executor::execution::view_dependents::views_using_column(cassie, table, from),
    )?;
    cassie
        .midge
        .alter_collection_rename_column(table, from, to)
        .map_err(|error| QueryError::General(error.to_string()))?;
    cassie.catalog.rename_collection_field(table, from, to);
    schema_foreign_keys::rename_referenced_field(cassie, table, from, to)?;
    crate::executor::execution::property_graph_dependents::rename_column(cassie, table, from, to)?;
    cassie
        .bump_schema_epoch_and_invalidate_query_cache()
        .map_err(QueryError::Cassie)?;
    crate::executor::execution::materialized_projection::mark_source_projections_stale(
        cassie, table,
    )?;
    refresh_table_cardinality_stats(cassie, table)
}

pub(super) fn alter_table_rename_table(
    cassie: &Cassie,
    table: &str,
    next_table: &str,
) -> Result<(), QueryError> {
    crate::executor::execution::view_dependents::reject_view_dependents(
        cassie,
        "rename table",
        "table",
        table,
        crate::executor::execution::view_dependents::views_reading(cassie, table),
    )?;
    cassie.catalog.reject_relation_move_target(next_table)?;
    if cassie.catalog.exists(next_table) {
        return Err(QueryError::General(format!(
            "collection '{next_table}' already exists"
        )));
    }
    cassie
        .midge
        .rename_collection(table, next_table)
        .map_err(|error| QueryError::General(error.to_string()))?;
    cassie
        .catalog
        .rename_collection(table, next_table)
        .map_err(|error| QueryError::General(error.to_string()))?;
    schema_sequence_rename::rename_owned_table_sequences(cassie, next_table)?;
    crate::executor::execution::property_graph_dependents::rename_table(cassie, table, next_table)?;
    schema_foreign_keys::rename_referenced_table(cassie, table, next_table)
}

pub(super) fn alter_table_set_schema(
    cassie: &Cassie,
    session: Option<&CassieSession>,
    table: &str,
    target_schema: &str,
) -> Result<(), QueryError> {
    let relation = crate::catalog::RelationId::parse_canonical(table).ok_or_else(|| {
        QueryError::General(format!(
            "relation '{table}' has no canonical database identity"
        ))
    })?;
    let schema = crate::catalog::SchemaId::parse_canonical(target_schema).ok_or_else(|| {
        QueryError::General(format!(
            "schema '{target_schema}' has no canonical database identity"
        ))
    })?;
    let next_table =
        crate::catalog::RelationId::new(relation.database.clone(), schema.schema, relation.name)
            .canonical_name();
    if table == next_table {
        return Ok(());
    }
    cassie.catalog.reject_relation_move_target(&next_table)?;
    if cassie.catalog.exists(&next_table) {
        return Err(QueryError::General(format!(
            "collection '{next_table}' already exists"
        )));
    }
    let sequence_move =
        schema_sequence_rename::prepare_owned_table_sequence_move(cassie, table, &next_table)?;

    cassie
        .midge
        .rename_collection(table, &next_table)
        .map_err(|error| QueryError::General(error.to_string()))?;
    cassie
        .catalog
        .rename_collection(table, &next_table)
        .map_err(QueryError::Cassie)?;
    if let Some(session) = session {
        session
            .rename_transaction_collections(&[(table.to_owned(), next_table.clone())])
            .map_err(QueryError::Cassie)?;
    }
    schema_sequence_rename::move_owned_table_sequences(cassie, sequence_move)?;
    crate::executor::execution::property_graph_dependents::rename_table(
        cassie,
        table,
        &next_table,
    )?;
    schema_foreign_keys::rename_referenced_table(cassie, table, &next_table)?;
    cassie
        .bump_schema_epoch_and_invalidate_query_cache()
        .map_err(QueryError::Cassie)?;
    crate::executor::execution::materialized_projection::mark_source_projections_stale(
        cassie, table,
    )?;
    Ok(())
}
