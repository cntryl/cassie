use super::super::command_output::empty as empty_command;
use super::super::{catalog, Cassie, QueryError, QueryResult};
use super::{schema_foreign_keys, schema_graph_rename, schema_sequence_rename};
use crate::app::CassieSession;
use crate::runtime::QueryExecutionControls;
use crate::sql::ast::{
    AlterSchemaOperation, AlterSchemaStatement, CreateDatabaseStatement, CreateSchemaStatement,
    DropDatabaseStatement, DropSchemaStatement,
};

pub(crate) fn create_schema(
    cassie: &Cassie,
    statement: &CreateSchemaStatement,
    controls: &QueryExecutionControls,
) -> Result<QueryResult, QueryError> {
    if statement.if_not_exists && cassie.catalog.namespace_exists(&statement.schema) {
        return empty_command("CREATE SCHEMA", controls);
    }

    let output = empty_command("CREATE SCHEMA", controls)?;
    let namespace = cassie
        .midge
        .create_namespace(&statement.schema)
        .map_err(|error| QueryError::General(error.to_string()))?;
    cassie.catalog.register_namespace_meta(namespace);

    Ok(output)
}

pub(crate) fn create_database(
    cassie: &Cassie,
    statement: &CreateDatabaseStatement,
    controls: &QueryExecutionControls,
) -> Result<QueryResult, QueryError> {
    let output = empty_command("CREATE DATABASE", controls)?;
    let _database = cassie.create_logical_database(&statement.name, statement.if_not_exists)?;

    Ok(output)
}

pub(crate) fn drop_schema(
    cassie: &Cassie,
    statement: &DropSchemaStatement,
    controls: &QueryExecutionControls,
) -> Result<QueryResult, QueryError> {
    if statement.if_exists && !cassie.catalog.namespace_exists(&statement.schema) {
        return empty_command("DROP SCHEMA", controls);
    }
    if !statement.cascade && !schema_is_empty(cassie, &statement.schema) {
        return Err(QueryError::General(format!(
            "namespace '{}' is not empty",
            statement.schema
        )));
    }

    let output = empty_command("DROP SCHEMA", controls)?;
    if statement.cascade {
        let schema_views = cassie
            .catalog
            .list_views()
            .into_iter()
            .filter(|view| object_in_schema(&view.name, &statement.schema))
            .collect::<Vec<_>>();
        for view in schema_views {
            cassie
                .midge
                .defer_drop_view(&view.name, cassie.runtime.schema_epoch())
                .map_err(|error| QueryError::General(error.to_string()))?;
            cassie.catalog.unregister_view(&view.name);
        }

        let schema_collections = cassie
            .catalog
            .list_collections_canonical()
            .into_iter()
            .filter(|collection| object_in_schema(&collection.name, &statement.schema))
            .collect::<Vec<_>>();
        for collection in schema_collections {
            schema_foreign_keys::drop_referencing_foreign_keys_on_table(cassie, &collection.name)?;
            cassie
                .midge
                .defer_drop_collection(&collection.name, cassie.runtime.schema_epoch())
                .map_err(|error| QueryError::General(error.to_string()))?;
            schema_sequence_rename::drop_owned_table_sequences(cassie, &collection.name)?;
            cassie
                .catalog
                .unregister_collection(&collection.name)
                .map_err(|error| QueryError::General(error.to_string()))?;
        }

        let schema_sequences = cassie
            .catalog
            .list_sequences()
            .into_iter()
            .filter(|sequence| object_in_schema(&sequence.name, &statement.schema))
            .collect::<Vec<_>>();
        for sequence in schema_sequences {
            cassie
                .midge
                .delete_sequence(&sequence.name)
                .map_err(|error| QueryError::General(error.to_string()))?;
            cassie.catalog.unregister_sequence(&sequence.name);
        }
    }
    cassie
        .midge
        .drop_namespace(&statement.schema)
        .map_err(|error| QueryError::General(error.to_string()))?;
    cassie.catalog.unregister_namespace(&statement.schema);

    Ok(output)
}

pub(crate) fn drop_database(
    cassie: &Cassie,
    session: Option<&crate::app::CassieSession>,
    statement: &DropDatabaseStatement,
    controls: &QueryExecutionControls,
) -> Result<QueryResult, QueryError> {
    if statement.if_exists && !cassie.catalog.database_exists(&statement.name) {
        return empty_command("DROP DATABASE", controls);
    }
    let Some(session) = session else {
        return Err(QueryError::General(
            "DROP DATABASE requires a session".to_string(),
        ));
    };
    if session
        .current_database()
        .is_some_and(|database| database.eq_ignore_ascii_case(&statement.name))
    {
        return Err(QueryError::General(format!(
            "cannot drop the currently open database '{}'",
            statement.name
        )));
    }
    if !database_is_empty(cassie, &statement.name) {
        return Err(QueryError::General(format!(
            "database '{}' is not empty",
            statement.name
        )));
    }

    let output = empty_command("DROP DATABASE", controls)?;
    let public_schema =
        crate::catalog::canonical_schema_name(&statement.name, crate::catalog::DEFAULT_SCHEMA);
    cassie.midge.drop_namespace(&public_schema)?;
    cassie.midge.drop_database(&statement.name)?;
    cassie.catalog.unregister_namespace(&public_schema);
    cassie.catalog.unregister_database(&statement.name);

    Ok(output)
}

pub(crate) fn alter_schema(
    cassie: &Cassie,
    session: Option<&CassieSession>,
    statement: &AlterSchemaStatement,
    controls: &QueryExecutionControls,
) -> Result<QueryResult, QueryError> {
    let next_schema = match &statement.operation {
        AlterSchemaOperation::RenameTo { schema } => schema.clone(),
    };
    let target_schema = statement.schema.clone();

    if cassie.catalog.namespace_exists(&next_schema) {
        return Err(QueryError::General(format!(
            "namespace '{next_schema}' already exists"
        )));
    }

    let output = empty_command("ALTER SCHEMA", controls)?;
    cassie
        .midge
        .rename_namespace(&target_schema, &next_schema)
        .map_err(|error| QueryError::General(error.to_string()))?;
    rename_schema_descendants(cassie, session, &target_schema, &next_schema)?;
    cassie.hydrate_catalog()?;

    Ok(output)
}

fn rename_schema_descendants(
    cassie: &Cassie,
    session: Option<&CassieSession>,
    current_schema: &str,
    next_schema: &str,
) -> Result<(), QueryError> {
    let collection_renames = cassie
        .catalog
        .list_collections_canonical()
        .into_iter()
        .filter(|collection| object_in_schema(&collection.name, current_schema))
        .map(|collection| {
            (
                collection.name.clone(),
                rewrite_relation_name_for_schema(&collection.name, current_schema, next_schema),
            )
        })
        .collect::<Vec<_>>();
    let relation_renames = relation_rename_map(&collection_renames);

    rename_schema_collections(cassie, session, &collection_renames)?;
    rename_schema_views(cassie, current_schema, next_schema)?;
    schema_sequence_rename::rename_schema_sequences(
        cassie,
        current_schema,
        next_schema,
        &relation_renames,
    )?;
    schema_graph_rename::rename_schema_graphs(
        cassie,
        current_schema,
        next_schema,
        &relation_renames,
    )?;
    rename_schema_rollups(cassie, current_schema, next_schema, &relation_renames)?;
    rename_schema_retention_policies(cassie, current_schema, next_schema, &relation_renames)?;
    rename_schema_materialized_projections(cassie, current_schema, next_schema, &relation_renames)?;

    Ok(())
}

pub(super) type RelationRenames = std::collections::HashMap<String, String>;

fn relation_rename_map(collection_renames: &[(String, String)]) -> RelationRenames {
    collection_renames.iter().cloned().collect()
}

fn rename_schema_collections(
    cassie: &Cassie,
    session: Option<&CassieSession>,
    collection_renames: &[(String, String)],
) -> Result<(), QueryError> {
    for (current, next) in collection_renames {
        cassie.midge.rename_collection(current, next)?;
    }
    if let Some(session) = session {
        session
            .rename_transaction_collections(collection_renames)
            .map_err(QueryError::Cassie)?;
    }
    Ok(())
}

fn rename_schema_views(
    cassie: &Cassie,
    current_schema: &str,
    next_schema: &str,
) -> Result<(), QueryError> {
    for mut view in cassie
        .catalog
        .list_views()
        .into_iter()
        .filter(|view| object_in_schema(&view.name, current_schema))
    {
        let current_name = view.name.clone();
        view.name = rewrite_relation_name_for_schema(&view.name, current_schema, next_schema);
        view.query = rewrite_schema_qualified_sql(&view.query, current_schema, next_schema);
        cassie.midge.delete_view(&current_name)?;
        cassie.midge.put_view(&view)?;
    }
    Ok(())
}

fn rename_schema_rollups(
    cassie: &Cassie,
    current_schema: &str,
    next_schema: &str,
    relation_renames: &RelationRenames,
) -> Result<(), QueryError> {
    for mut rollup in cassie.catalog.list_rollups() {
        let current_name = rollup.name.clone();
        let next_name = rewrite_relation_name_from_map(
            &rollup.name,
            relation_renames,
            current_schema,
            next_schema,
        );
        let next_source = rewrite_relation_name_from_map(
            &rollup.source_collection,
            relation_renames,
            current_schema,
            next_schema,
        );
        let next_output = rewrite_relation_name_from_map(
            &rollup.output_collection,
            relation_renames,
            current_schema,
            next_schema,
        );
        if current_name == next_name
            && rollup.source_collection == next_source
            && rollup.output_collection == next_output
        {
            continue;
        }
        rollup.name = next_name;
        rollup.source_collection = next_source;
        rollup.output_collection = next_output;
        cassie.midge.delete_rollup(&current_name)?;
        cassie.midge.put_rollup(&rollup)?;
    }
    Ok(())
}

fn rename_schema_retention_policies(
    cassie: &Cassie,
    current_schema: &str,
    next_schema: &str,
    relation_renames: &RelationRenames,
) -> Result<(), QueryError> {
    for mut policy in cassie.catalog.list_retention_policies() {
        let current_name = policy.name.clone();
        let next_name = rewrite_relation_name_from_map(
            &policy.name,
            relation_renames,
            current_schema,
            next_schema,
        );
        let next_collection = rewrite_relation_name_from_map(
            &policy.collection,
            relation_renames,
            current_schema,
            next_schema,
        );
        if current_name == next_name && policy.collection == next_collection {
            continue;
        }
        policy.name = next_name;
        policy.collection = next_collection;
        cassie.midge.delete_retention_policy(&current_name)?;
        cassie.midge.put_retention_policy(&policy)?;
    }
    Ok(())
}

fn rename_schema_materialized_projections(
    cassie: &Cassie,
    current_schema: &str,
    next_schema: &str,
    relation_renames: &RelationRenames,
) -> Result<(), QueryError> {
    for mut projection in cassie
        .catalog
        .list_projection_metadata()
        .into_iter()
        .filter(|projection| projection.kind == catalog::ProjectionKind::Materialized)
    {
        let current_name = projection.collection.clone();
        let next_name = rewrite_relation_name_from_map(
            &projection.collection,
            relation_renames,
            current_schema,
            next_schema,
        );
        let mut changed = current_name != next_name;
        projection.collection.clone_from(&next_name);
        if projection.projection_id == current_name {
            projection.projection_id.clone_from(&next_name);
        }
        if let Some(materialized) = projection.materialized.as_mut() {
            materialized.name.clone_from(&next_name);
            materialized.query =
                rewrite_schema_qualified_sql(&materialized.query, current_schema, next_schema);
            let next_output = rewrite_relation_name_from_map(
                &materialized.output_collection,
                relation_renames,
                current_schema,
                next_schema,
            );
            changed |= materialized.output_collection != next_output;
            materialized.output_collection = next_output;
            for source_collection in &mut materialized.source_collections {
                let next_source = rewrite_relation_name_from_map(
                    source_collection,
                    relation_renames,
                    current_schema,
                    next_schema,
                );
                changed |= *source_collection != next_source;
                *source_collection = next_source;
            }
        }
        for version in &mut projection.versions {
            let next_output = rewrite_relation_name_from_map(
                &version.output_collection,
                relation_renames,
                current_schema,
                next_schema,
            );
            changed |= version.output_collection != next_output;
            version.output_collection = next_output;
        }
        if let Some(target) = projection.integrity.target.as_mut() {
            let next_target = rewrite_relation_name_from_map(
                target,
                relation_renames,
                current_schema,
                next_schema,
            );
            changed |= *target != next_target;
            *target = next_target;
        }
        if !changed {
            continue;
        }
        cassie.midge.delete_projection_metadata(&current_name)?;
        cassie.midge.put_projection_metadata(&projection)?;
    }
    Ok(())
}

fn schema_is_empty(cassie: &Cassie, schema: &str) -> bool {
    !cassie
        .catalog
        .list_collections_canonical()
        .into_iter()
        .any(|collection| object_in_schema(&collection.name, schema))
        && !cassie
            .catalog
            .list_views()
            .into_iter()
            .any(|view| object_in_schema(&view.name, schema))
        && !cassie
            .catalog
            .list_sequences()
            .into_iter()
            .any(|sequence| object_in_schema(&sequence.name, schema))
        && !cassie
            .catalog
            .list_graphs()
            .into_iter()
            .any(|graph| object_in_schema(&graph.name, schema))
        && !cassie
            .catalog
            .list_projection_metadata()
            .into_iter()
            .filter(|projection| projection.kind == catalog::ProjectionKind::Materialized)
            .any(|projection| object_in_schema(&projection.collection, schema))
        && !cassie
            .catalog
            .list_rollups()
            .into_iter()
            .any(|rollup| object_in_schema(&rollup.name, schema))
        && !cassie
            .catalog
            .list_retention_policies()
            .into_iter()
            .any(|policy| object_in_schema(&policy.name, schema))
}

fn database_is_empty(cassie: &Cassie, database: &str) -> bool {
    !cassie
        .catalog
        .list_namespaces()
        .into_iter()
        .any(|namespace| {
            crate::catalog::schema_belongs_to_database(&namespace.name, database)
                && !crate::catalog::local_name(&namespace.name)
                    .eq_ignore_ascii_case(crate::catalog::DEFAULT_SCHEMA)
        })
        && !cassie
            .catalog
            .list_collections_canonical()
            .into_iter()
            .any(|collection| {
                crate::catalog::relation_belongs_to_database(&collection.name, database)
            })
        && !cassie
            .catalog
            .list_views()
            .into_iter()
            .any(|view| crate::catalog::relation_belongs_to_database(&view.name, database))
        && !cassie
            .catalog
            .list_sequences()
            .into_iter()
            .any(|sequence| crate::catalog::relation_belongs_to_database(&sequence.name, database))
        && !cassie
            .catalog
            .list_functions()
            .into_iter()
            .any(|function| crate::catalog::relation_belongs_to_database(&function.name, database))
        && !cassie
            .catalog
            .list_procedures()
            .into_iter()
            .any(|procedure| {
                crate::catalog::relation_belongs_to_database(&procedure.name, database)
            })
        && !cassie
            .catalog
            .list_graphs()
            .into_iter()
            .any(|graph| crate::catalog::relation_belongs_to_database(&graph.name, database))
        && !cassie
            .catalog
            .list_projection_metadata()
            .into_iter()
            .filter(|projection| projection.kind == catalog::ProjectionKind::Materialized)
            .any(|projection| {
                crate::catalog::relation_belongs_to_database(&projection.collection, database)
            })
        && !cassie
            .catalog
            .list_rollups()
            .into_iter()
            .any(|rollup| crate::catalog::relation_belongs_to_database(&rollup.name, database))
        && !cassie
            .catalog
            .list_retention_policies()
            .into_iter()
            .any(|policy| crate::catalog::relation_belongs_to_database(&policy.name, database))
}

pub(crate) fn object_in_schema(name: &str, schema: &str) -> bool {
    let (object_database, object_schema) = match crate::catalog::parse_name(name) {
        Ok(crate::catalog::ParsedName::DatabaseQualified {
            database, schema, ..
        }) => (Some(database), schema),
        Ok(crate::catalog::ParsedName::SchemaQualified { schema, .. }) => (None, schema),
        Ok(crate::catalog::ParsedName::Unqualified(_)) => {
            (None, crate::catalog::DEFAULT_SCHEMA.to_owned())
        }
        Err(_) => return false,
    };
    if !crate::catalog::name_matches(schema, &object_schema) {
        return false;
    }

    let target_database = crate::catalog::schema_database_name(schema);
    target_database.as_ref().is_none_or(|database| {
        object_database
            .as_ref()
            .is_none_or(|object_database| object_database.eq_ignore_ascii_case(database))
    })
}

pub(crate) fn rewrite_relation_name_for_schema(
    name: &str,
    current_schema: &str,
    next_schema: &str,
) -> String {
    let Some(database) = crate::catalog::schema_database_name(current_schema) else {
        return name.to_string();
    };
    crate::catalog::canonical_relation_name(
        &database,
        &crate::catalog::local_name(next_schema),
        &crate::catalog::local_name(name),
    )
}

pub(crate) fn rewrite_relation_name_from_map(
    name: &str,
    relation_renames: &std::collections::HashMap<String, String>,
    current_schema: &str,
    next_schema: &str,
) -> String {
    relation_renames.get(name).cloned().unwrap_or_else(|| {
        if object_in_schema(name, current_schema) {
            rewrite_relation_name_for_schema(name, current_schema, next_schema)
        } else {
            name.to_string()
        }
    })
}

fn rewrite_schema_qualified_sql(raw: &str, current_schema: &str, next_schema: &str) -> String {
    let current_local = crate::catalog::local_name(current_schema);
    let next_local = crate::catalog::local_name(next_schema);
    raw.replace(&format!("{current_local}."), &format!("{next_local}."))
        .replace(
            &format!("\"{current_local}\"."),
            &format!("\"{next_local}\"."),
        )
}
