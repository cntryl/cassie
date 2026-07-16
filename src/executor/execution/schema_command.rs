use super::{catalog, virtual_views, Cassie, FieldSchema, QueryError, QueryResult, QueryStatement};
use crate::sql::ast::{
    AlterSchemaOperation, AlterSchemaStatement, AlterTableOperation, AlterTableStatement,
    CreateDatabaseStatement, CreateGraphStatement, CreateSchemaStatement, CreateViewStatement,
    DropDatabaseStatement, DropIndexStatement, DropSchemaStatement, DropTableStatement,
    DropViewStatement,
};
use crate::types::DataType;

pub(super) fn create_graph(
    cassie: &Cassie,
    statement: &CreateGraphStatement,
) -> Result<QueryResult, QueryError> {
    let graph = catalog::GraphMeta::new(&statement.name);
    if statement.if_not_exists && cassie.catalog.graph_exists(&statement.name) {
        return Ok(empty_command("CREATE GRAPH"));
    }

    super::graph_command::create_graph_collection(
        cassie,
        &graph.node_collection,
        graph.node_builtin_fields(),
        &statement.node_fields,
    )?;
    super::graph_command::create_graph_collection(
        cassie,
        &graph.edge_collection,
        graph.edge_builtin_fields(),
        &statement.edge_fields,
    )?;
    cassie
        .midge
        .put_graph(&graph)
        .map_err(|error| QueryError::General(error.to_string()))?;
    cassie.catalog.register_graph(graph);
    refresh_table_cardinality_stats(cassie, &format!("{}_nodes", statement.name))?;
    refresh_table_cardinality_stats(cassie, &format!("{}_edges", statement.name))?;

    Ok(empty_command("CREATE GRAPH"))
}

pub(super) fn create_view(
    cassie: &Cassie,
    statement: &CreateViewStatement,
) -> Result<QueryResult, QueryError> {
    if statement.if_not_exists
        && (cassie.catalog.relation_exists(&statement.name)
            || virtual_views::schema(&statement.name).is_some())
    {
        return Ok(empty_command("CREATE VIEW"));
    }

    let parsed = crate::sql::parser::parse_statement(&statement.query)
        .map_err(|error| QueryError::General(error.to_string()))?;
    let bound = crate::sql::binder::bind(parsed, &cassie.catalog)
        .map_err(|error| QueryError::General(error.to_string()))?;
    let QueryStatement::Select(select) = &bound.statement.statement else {
        return Err(QueryError::General(
            "CREATE VIEW requires a SELECT query body".to_string(),
        ));
    };

    let schema = crate::sql::binder::infer_select_schema(select, &cassie.catalog)
        .map_err(|error| QueryError::General(error.to_string()))?;
    let metadata =
        crate::catalog::ViewMeta::new(statement.name.clone(), statement.query.clone(), schema);

    cassie
        .midge
        .put_view(&metadata)
        .map_err(|error| QueryError::General(error.to_string()))?;
    cassie.catalog.register_view(metadata);

    Ok(empty_command("CREATE VIEW"))
}

pub(super) fn drop_view(
    cassie: &Cassie,
    statement: &DropViewStatement,
) -> Result<QueryResult, QueryError> {
    let view = cassie.catalog.get_view(&statement.name);
    if statement.if_exists && view.is_none() {
        return Ok(empty_command("DROP VIEW"));
    }

    if view.is_none() {
        return Err(QueryError::General(format!(
            "view '{}' does not exist",
            statement.name
        )));
    }

    cassie
        .midge
        .defer_drop_view(&statement.name, cassie.runtime.schema_epoch())
        .map_err(|error| QueryError::General(error.to_string()))?;
    cassie.catalog.unregister_view(&statement.name);

    Ok(empty_command("DROP VIEW"))
}

pub(super) fn drop_table(
    cassie: &Cassie,
    statement: &DropTableStatement,
) -> Result<QueryResult, QueryError> {
    if statement.if_exists && !cassie.catalog.exists(&statement.table) {
        return Ok(empty_command("DROP TABLE"));
    }

    cassie
        .midge
        .defer_drop_collection(&statement.table, cassie.runtime.schema_epoch())
        .map_err(|error| QueryError::General(error.to_string()))?;
    cassie.catalog.unregister_collection(&statement.table);

    Ok(empty_command("DROP TABLE"))
}

pub(super) fn alter_table(
    cassie: &Cassie,
    statement: &AlterTableStatement,
) -> Result<QueryResult, QueryError> {
    let is_column_store = cassie
        .catalog
        .collection_storage_mode(&statement.table)
        .is_some_and(crate::catalog::collections::CollectionStorageMode::uses_column_store_storage);
    execute_alter_table_operation(cassie, statement, is_column_store)?;

    Ok(empty_command("ALTER TABLE"))
}

pub(super) fn create_schema(
    cassie: &Cassie,
    statement: &CreateSchemaStatement,
) -> Result<QueryResult, QueryError> {
    if statement.if_not_exists && cassie.catalog.namespace_exists(&statement.schema) {
        return Ok(empty_command("CREATE SCHEMA"));
    }

    cassie
        .midge
        .create_namespace(&statement.schema)
        .map_err(|error| QueryError::General(error.to_string()))?;
    cassie.catalog.register_namespace(&statement.schema, None);

    Ok(empty_command("CREATE SCHEMA"))
}

pub(super) fn create_database(
    cassie: &Cassie,
    statement: &CreateDatabaseStatement,
) -> Result<QueryResult, QueryError> {
    if statement.if_not_exists && cassie.catalog.database_exists(&statement.name) {
        return Ok(empty_command("CREATE DATABASE"));
    }

    cassie.midge.create_database(&statement.name, None)?;
    let public_schema =
        crate::catalog::canonical_schema_name(&statement.name, crate::catalog::DEFAULT_SCHEMA);
    cassie.midge.create_namespace(&public_schema)?;
    cassie.catalog.register_database(&statement.name, None);
    cassie.catalog.register_namespace(&public_schema, None);

    Ok(empty_command("CREATE DATABASE"))
}

pub(super) fn drop_schema(
    cassie: &Cassie,
    statement: &DropSchemaStatement,
) -> Result<QueryResult, QueryError> {
    if statement.if_exists && !cassie.catalog.namespace_exists(&statement.schema) {
        return Ok(empty_command("DROP SCHEMA"));
    }
    if !schema_is_empty(cassie, &statement.schema) {
        return Err(QueryError::General(format!(
            "namespace '{}' is not empty",
            statement.schema
        )));
    }

    cassie
        .midge
        .drop_namespace(&statement.schema)
        .map_err(|error| QueryError::General(error.to_string()))?;
    cassie.catalog.unregister_namespace(&statement.schema);

    Ok(empty_command("DROP SCHEMA"))
}

pub(super) fn drop_database(
    cassie: &Cassie,
    session: Option<&crate::app::CassieSession>,
    statement: &DropDatabaseStatement,
) -> Result<QueryResult, QueryError> {
    if statement.if_exists && !cassie.catalog.database_exists(&statement.name) {
        return Ok(empty_command("DROP DATABASE"));
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

    let public_schema =
        crate::catalog::canonical_schema_name(&statement.name, crate::catalog::DEFAULT_SCHEMA);
    cassie.midge.drop_namespace(&public_schema)?;
    cassie.midge.drop_database(&statement.name)?;
    cassie.catalog.unregister_namespace(&public_schema);
    cassie.catalog.unregister_database(&statement.name);

    Ok(empty_command("DROP DATABASE"))
}

pub(super) fn alter_schema(
    cassie: &Cassie,
    statement: &AlterSchemaStatement,
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

    cassie
        .midge
        .rename_namespace(&target_schema, &next_schema)
        .map_err(|error| QueryError::General(error.to_string()))?;
    rename_schema_descendants(cassie, &target_schema, &next_schema)?;
    cassie.hydrate_catalog()?;

    Ok(empty_command("ALTER SCHEMA"))
}

pub(super) fn drop_index(
    cassie: &Cassie,
    statement: &DropIndexStatement,
) -> Result<QueryResult, QueryError> {
    let index = cassie.catalog.get_index(&statement.table, &statement.name);

    if statement.if_exists && index.is_none() {
        return Ok(empty_command("DROP INDEX"));
    }

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

    Ok(empty_command("DROP INDEX"))
}

fn execute_alter_table_operation(
    cassie: &Cassie,
    statement: &AlterTableStatement,
    is_column_store: bool,
) -> Result<(), QueryError> {
    match &statement.operation {
        AlterTableOperation::AddColumn { field, data_type } => {
            alter_table_add_column(cassie, &statement.table, field, data_type, is_column_store)
        }
        AlterTableOperation::AddConstraint { constraints } => {
            alter_table_add_constraint(cassie, &statement.table, constraints)
        }
        AlterTableOperation::DropColumn { field } => {
            alter_table_drop_column(cassie, &statement.table, field, is_column_store)
        }
        AlterTableOperation::RenameColumn { from, to } => {
            alter_table_rename_column(cassie, &statement.table, from, to, is_column_store)
        }
        AlterTableOperation::RenameTo { table } => {
            alter_table_rename_table(cassie, &statement.table, table)
        }
        AlterTableOperation::AlterColumnSetDefault {
            field,
            default_value,
            default_expression,
            default_sequence,
        } => super::sequence_command::alter_column_set_default(
            cassie,
            &statement.table,
            field,
            default_value.clone(),
            default_expression.clone(),
            default_sequence.clone(),
        ),
        AlterTableOperation::AlterColumnDropDefault { field } => {
            super::sequence_command::alter_column_drop_default(cassie, &statement.table, field)
        }
        AlterTableOperation::AlterColumnSetNotNull { field } => {
            super::sequence_command::alter_column_set_not_null(cassie, &statement.table, field)
        }
        AlterTableOperation::AlterColumnDropNotNull { field } => {
            super::sequence_command::alter_column_drop_not_null(cassie, &statement.table, field)
        }
    }
}

fn alter_table_add_column(
    cassie: &Cassie,
    table: &str,
    field: &str,
    data_type: &DataType,
    is_column_store: bool,
) -> Result<(), QueryError> {
    ensure_row_store_alter_supported(is_column_store, "ALTER TABLE ADD COLUMN")?;
    let field = FieldSchema {
        name: field.to_string(),
        data_type: data_type.clone(),
        nullable: true,
    };
    cassie
        .midge
        .alter_collection_add_column(table, field.clone())
        .map_err(|error| QueryError::General(error.to_string()))?;
    cassie
        .catalog
        .add_collection_field(table, field.name, field.data_type.clone());
    refresh_table_cardinality_stats(cassie, table)
}

fn alter_table_add_constraint(
    cassie: &Cassie,
    table: &str,
    constraints: &[crate::catalog::FieldConstraint],
) -> Result<(), QueryError> {
    let mut merged = cassie.catalog.get_constraints(table);
    crate::catalog::merge_constraint_set(&mut merged, constraints.to_vec());
    cassie
        .midge
        .save_constraints(table, merged.as_slice())
        .map_err(|error| QueryError::General(error.to_string()))?;
    cassie.catalog.register_constraints(table, merged);
    Ok(())
}

fn alter_table_drop_column(
    cassie: &Cassie,
    table: &str,
    field: &str,
    is_column_store: bool,
) -> Result<(), QueryError> {
    ensure_row_store_alter_supported(is_column_store, "ALTER TABLE DROP COLUMN")?;
    cassie
        .midge
        .alter_collection_drop_column(table, field)
        .map_err(|error| QueryError::General(error.to_string()))?;
    cassie.catalog.remove_collection_field(table, field);
    refresh_table_cardinality_stats(cassie, table)
}

fn alter_table_rename_column(
    cassie: &Cassie,
    table: &str,
    from: &str,
    to: &str,
    is_column_store: bool,
) -> Result<(), QueryError> {
    ensure_row_store_alter_supported(is_column_store, "ALTER TABLE RENAME COLUMN")?;
    cassie
        .midge
        .alter_collection_rename_column(table, from, to)
        .map_err(|error| QueryError::General(error.to_string()))?;
    cassie.catalog.rename_collection_field(table, from, to);
    refresh_table_cardinality_stats(cassie, table)
}

fn alter_table_rename_table(
    cassie: &Cassie,
    table: &str,
    next_table: &str,
) -> Result<(), QueryError> {
    if cassie.catalog.exists(next_table) {
        return Err(QueryError::General(format!(
            "collection '{next_table}' already exists"
        )));
    }
    cassie
        .midge
        .rename_collection(table, next_table)
        .map_err(|error| QueryError::General(error.to_string()))?;
    cassie.catalog.rename_collection(table, next_table);
    Ok(())
}

fn ensure_row_store_alter_supported(
    is_column_store: bool,
    operation: &str,
) -> Result<(), QueryError> {
    if is_column_store {
        return Err(QueryError::General(format!(
            "{operation} is not supported for column-store tables"
        )));
    }
    Ok(())
}

pub(super) fn refresh_table_cardinality_stats(
    cassie: &Cassie,
    table: &str,
) -> Result<(), QueryError> {
    cassie
        .refresh_cardinality_stats(table)
        .map_err(|error| QueryError::General(error.to_string()))
}

fn rename_schema_descendants(
    cassie: &Cassie,
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

    rename_schema_collections(cassie, &collection_renames)?;
    rename_schema_views(cassie, current_schema, next_schema)?;
    rename_schema_sequences(cassie, current_schema, next_schema)?;
    rename_schema_graphs(cassie, current_schema, next_schema, &relation_renames)?;
    rename_schema_rollups(cassie, current_schema, next_schema, &relation_renames)?;
    rename_schema_retention_policies(cassie, current_schema, next_schema, &relation_renames)?;
    rename_schema_materialized_projections(cassie, current_schema, next_schema, &relation_renames)?;

    Ok(())
}

type RelationRenames = std::collections::HashMap<String, String>;

fn relation_rename_map(collection_renames: &[(String, String)]) -> RelationRenames {
    collection_renames.iter().cloned().collect()
}

fn rename_schema_collections(
    cassie: &Cassie,
    collection_renames: &[(String, String)],
) -> Result<(), QueryError> {
    for (current, next) in collection_renames {
        cassie.midge.rename_collection(current, next)?;
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

fn rename_schema_sequences(
    cassie: &Cassie,
    current_schema: &str,
    next_schema: &str,
) -> Result<(), QueryError> {
    for mut sequence in cassie
        .catalog
        .list_sequences()
        .into_iter()
        .filter(|sequence| object_in_schema(&sequence.name, current_schema))
    {
        let current_name = sequence.name.clone();
        sequence.name =
            rewrite_relation_name_for_schema(&sequence.name, current_schema, next_schema);
        cassie.midge.delete_sequence(&current_name)?;
        cassie.midge.put_sequence(&sequence)?;
    }
    Ok(())
}

fn rename_schema_graphs(
    cassie: &Cassie,
    current_schema: &str,
    next_schema: &str,
    relation_renames: &RelationRenames,
) -> Result<(), QueryError> {
    for mut graph in cassie.catalog.list_graphs() {
        let current_name = graph.name.clone();
        let next_name = rewrite_relation_name_from_map(
            &graph.name,
            relation_renames,
            current_schema,
            next_schema,
        );
        let next_node_collection = rewrite_relation_name_from_map(
            &graph.node_collection,
            relation_renames,
            current_schema,
            next_schema,
        );
        let next_edge_collection = rewrite_relation_name_from_map(
            &graph.edge_collection,
            relation_renames,
            current_schema,
            next_schema,
        );
        if current_name == next_name
            && graph.node_collection == next_node_collection
            && graph.edge_collection == next_edge_collection
        {
            continue;
        }
        graph.name = next_name;
        graph.node_collection = next_node_collection;
        graph.edge_collection = next_edge_collection;
        cassie.midge.delete_graph(&current_name)?;
        cassie.midge.put_graph(&graph)?;
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

fn object_in_schema(name: &str, schema: &str) -> bool {
    let target_schema = crate::catalog::local_name(schema);
    if !crate::catalog::relation_schema_name(name).eq_ignore_ascii_case(&target_schema) {
        return false;
    }

    let target_database = crate::catalog::schema_database_name(schema);
    target_database.as_ref().is_none_or(|database| {
        crate::catalog::relation_database_name(name)
            .as_ref()
            .is_none_or(|relation_database| relation_database.eq_ignore_ascii_case(database))
    })
}

fn rewrite_relation_name_for_schema(name: &str, current_schema: &str, next_schema: &str) -> String {
    let Some(database) = crate::catalog::schema_database_name(current_schema) else {
        return name.to_string();
    };
    crate::catalog::canonical_relation_name(
        &database,
        &crate::catalog::local_name(next_schema),
        &crate::catalog::local_name(name),
    )
}

fn rewrite_relation_name_from_map(
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

fn empty_command(command: &str) -> QueryResult {
    QueryResult {
        columns: Vec::new(),
        rows: Vec::new(),
        command: command.to_string(),
    }
}
