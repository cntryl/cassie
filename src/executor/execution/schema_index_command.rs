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
    if let AlterIndexOperation::Rename { new_name } = &statement.operation {
        return super::scalar_index_rename::rename(cassie, statement, new_name, controls);
    }
    let output = empty_command("ALTER INDEX", controls)?;
    if statement.collection.trim().is_empty() {
        if statement.if_exists {
            return Ok(output);
        }
        return Err(QueryError::General(
            "ALTER INDEX was not bound to an existing index".to_string(),
        ));
    }
    if cassie
        .midge
        .get_index(&statement.collection, &statement.name)
        .ok()
        .flatten()
        .is_some_and(|index| index.kind == catalog::IndexKind::FullText)
    {
        return alter_fulltext_index(cassie, statement, output);
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
                AlterIndexOperation::Rename { .. } => {
                    unreachable!("rename is dispatched separately")
                }
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
        |updated| register_updated_index(cassie, statement, updated),
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

/// Storage reports an index under its canonical collection name (for example
/// `postgres.public.t`) while the catalog keys it by the relation spelling the
/// statement was bound to (`t`). Re-registering the storage spelling would add a
/// second catalog entry beside the original, so keep the catalog's spelling.
fn register_updated_index(
    cassie: &Cassie,
    statement: &AlterIndexStatement,
    mut updated: catalog::IndexMeta,
) {
    if let Some(existing) = cassie
        .catalog
        .get_index(&statement.collection, &statement.name)
    {
        updated.collection = existing.collection;
    }
    cassie.catalog.register_index(updated);
}

/// Full-text `ALTER INDEX` only changes analyzer options; the rebuild and the
/// generation switch live in `Midge::alter_fulltext_index_analyzer`.
fn alter_fulltext_index(
    cassie: &Cassie,
    statement: &AlterIndexStatement,
    output: QueryResult,
) -> Result<QueryResult, QueryError> {
    cassie
        .midge
        .alter_fulltext_index_analyzer(
            &statement.collection,
            &statement.name,
            |options| {
                match &statement.operation {
                    AlterIndexOperation::Set(values) => options.extend(values.clone()),
                    AlterIndexOperation::Reset(names) => {
                        for name in names {
                            options.remove(name);
                        }
                    }
                    AlterIndexOperation::Rename { .. } => {
                        unreachable!("rename is dispatched separately")
                    }
                }
                let requested: Vec<&String> = match &statement.operation {
                    AlterIndexOperation::Set(values) => values.keys().collect(),
                    AlterIndexOperation::Reset(names) => names.iter().collect(),
                    AlterIndexOperation::Rename { .. } => Vec::new(),
                };
                if let Some(key) = requested.into_iter().find(|key| {
                    !matches!(
                        key.as_str(),
                        "boost"
                            | "k1"
                            | "b"
                            | "analyzer"
                            | "case_folding"
                            | "stop_words"
                            | "stemming"
                            | "synonyms"
                            | "accent_folding"
                            | "tokenizer"
                            | "analyzer_version"
                    )
                }) {
                    return Err(crate::app::CassieError::Unsupported(format!(
                        "unsupported fulltext index option '{key}'"
                    )));
                }
                Ok(())
            },
            |updated| register_updated_index(cassie, statement, updated),
        )
        .map_err(|error| QueryError::General(error.to_string()))?;
    Ok(output)
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
        crate::executor::execution::property_graph_dependents::reject_unbacked_keys(
            cassie,
            table,
            &cassie.catalog.get_constraints(table),
            &[index.name.as_str()],
        )?;
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
            cassie
                .midge
                .with_collection_gates(std::slice::from_ref(name), || {
                    if !cassie.catalog.exists(name)
                        && !cassie.catalog.is_sql_materialized_view(name)
                    {
                        return Err(crate::app::CassieError::CollectionNotFound(name.clone()));
                    }
                    let collection = cassie
                        .catalog
                        .materialized_view_output_collection(name)
                        .unwrap_or_else(|| name.clone());
                    if cassie
                        .catalog
                        .list_indexes(&collection)
                        .iter()
                        .any(|index| index.kind != catalog::IndexKind::Scalar)
                    {
                        return Err(crate::app::CassieError::Unsupported(
                            "REINDEX TABLE currently supports scalar indexes only".to_string(),
                        ));
                    }
                    cassie
                        .midge
                        .rebuild_scalar_indexes_for_collection(&collection)
                })
                .map_err(|error| QueryError::General(error.to_string()))?;
        }
    }
    Ok(output)
}
