//! Index lifecycle commands keep their admitted output ahead of mutation.
use super::{
    catalog, empty_command, refresh_table_cardinality_stats, schema_constraint_helpers,
    schema_foreign_keys, Cassie, QueryError, QueryExecutionControls, QueryResult,
};
use crate::sql::ast::{
    AlterIndexOperation, AlterIndexStatement, DropIndexStatement, ReindexStatement, ReindexTarget,
};

pub(in crate::executor::execution) fn alter_index_storage_parameters(
    cassie: &Cassie,
    statement: &AlterIndexStatement,
    controls: &QueryExecutionControls,
) -> Result<QueryResult, QueryError> {
    let output = empty_command("ALTER INDEX", controls)?;
    if statement.collection.trim().is_empty() {
        if statement.if_exists {
            return Ok(output);
        }
        return Err(QueryError::General(
            "ALTER INDEX was not bound to an existing index".to_string(),
        ));
    }
    let updated = cassie.midge.update_index_metadata(
        &statement.collection,
        &statement.name,
        |metadata| {
            if metadata.kind != catalog::IndexKind::Scalar
                || !metadata.access_method().eq_ignore_ascii_case("btree")
            {
                return Err(crate::app::CassieError::Unsupported(
                    "ALTER INDEX storage parameters are supported only for scalar btree indexes"
                        .to_string(),
                ));
            }
            match &statement.operation {
                AlterIndexOperation::Set(options) => {
                    metadata.options.extend(options.clone());
                }
                AlterIndexOperation::Reset(options) => {
                    for option in options {
                        metadata.options.remove(option);
                    }
                }
            }
            Ok(())
        },
        |updated| cassie.catalog.register_index(updated),
    );
    match updated {
        Ok(()) => Ok(output),
        Err(crate::app::CassieError::CatalogObjectNotFound {
            kind: crate::app::CatalogObjectKind::Index,
            ..
        }) if statement.if_exists => Ok(output),
        Err(error) => Err(QueryError::General(error.to_string())),
    }
}

pub(in crate::executor::execution) fn drop_index(
    cassie: &Cassie,
    statement: &DropIndexStatement,
    controls: &QueryExecutionControls,
) -> Result<QueryResult, QueryError> {
    let table = statement.table.as_str();
    if table.trim().is_empty() {
        if statement.if_exists {
            return empty_command("DROP INDEX", controls);
        }
        return Err(QueryError::General(
            "DROP INDEX was not bound to an owning table".to_string(),
        ));
    }
    let index = cassie.catalog.get_index(table, &statement.name);

    if statement.if_exists && index.is_none() {
        return empty_command("DROP INDEX", controls);
    }

    if let Some(index) = index.as_ref() {
        schema_foreign_keys::reject_referenced_unique_index_drop(cassie, table, index)?;
        schema_constraint_helpers::reject_constraint_index_drop(cassie, table, &index.name)?;
    }

    let output = empty_command("DROP INDEX", controls)?;
    if let Some(index) = index.as_ref() {
        if matches!(index.kind, catalog::IndexKind::Vector) {
            cassie.catalog.unregister_vector_index(table, &index.field);
        }
    }
    cassie
        .midge
        .defer_drop_index(table, &statement.name, cassie.runtime.schema_epoch())
        .map_err(|error| QueryError::General(error.to_string()))?;
    cassie.catalog.unregister_index(table, &statement.name);
    refresh_table_cardinality_stats(cassie, table)?;

    Ok(output)
}

pub(in crate::executor::execution) fn reindex(
    cassie: &Cassie,
    statement: &ReindexStatement,
    controls: &QueryExecutionControls,
) -> Result<QueryResult, QueryError> {
    let output = empty_command("REINDEX", controls)?;
    match &statement.target {
        ReindexTarget::Index { collection, name } => {
            let index = cassie
                .midge
                .get_index(collection, name)
                .map_err(|error| QueryError::General(error.to_string()))?
                .ok_or_else(|| {
                    QueryError::General(format!("index '{name}' disappeared before REINDEX"))
                })?;
            if index.kind != catalog::IndexKind::Scalar {
                return Err(QueryError::General(
                    "REINDEX INDEX currently supports scalar indexes only".to_string(),
                ));
            }
            cassie
                .midge
                .rebuild_scalar_index_for_index(&index)
                .map_err(|error| QueryError::General(error.to_string()))?;
        }
        ReindexTarget::Table { name } => {
            if cassie
                .catalog
                .list_indexes(name)
                .iter()
                .any(|index| index.kind != catalog::IndexKind::Scalar)
            {
                return Err(QueryError::General(
                    "REINDEX TABLE currently supports scalar indexes only".to_string(),
                ));
            }
            cassie
                .midge
                .rebuild_scalar_indexes_for_collection(name)
                .map_err(|error| QueryError::General(error.to_string()))?;
        }
    }
    Ok(output)
}
