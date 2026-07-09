use super::{
    catalog, primary_key_indexes, virtual_views, Cassie, FieldSchema, QueryError, QueryResult,
    QueryStatement, Schema,
};
use crate::sql::ast::{
    AlterRoleStatement, AlterSchemaOperation, AlterSchemaStatement, AlterTableOperation,
    AlterTableStatement, CreateDatabaseStatement, CreateGraphStatement, CreateIndexStatement,
    CreateRoleStatement, CreateSchemaStatement, CreateTableStatement, CreateViewStatement,
    DropDatabaseStatement, DropIndexStatement, DropRoleStatement, DropSchemaStatement,
    DropTableStatement, DropViewStatement,
};
use crate::types::DataType;

pub(super) fn create_table(
    cassie: &Cassie,
    statement: &CreateTableStatement,
) -> Result<QueryResult, QueryError> {
    if statement.if_not_exists
        && (cassie.catalog.relation_exists(&statement.table)
            || virtual_views::schema(&statement.table).is_some())
    {
        return Ok(empty_command("CREATE TABLE"));
    }

    let schema = Schema {
        fields: statement
            .fields
            .iter()
            .map(|field| FieldSchema {
                name: field.name.clone(),
                data_type: field.data_type.clone(),
                nullable: true,
            })
            .collect(),
    };
    let collection_meta = catalog::CollectionMeta::new_with_storage_mode(
        &statement.table,
        None,
        statement.storage_mode,
    );
    let table_sequences =
        super::sequence_command::prepare_create_table_sequences(cassie, statement)?;

    cassie
        .midge
        .create_collection_with_meta(&statement.table, &schema, &collection_meta)
        .map_err(|error| QueryError::General(error.to_string()))?;

    let constraints = statement
        .fields
        .iter()
        .flat_map(|field| field.constraints.iter().cloned())
        .collect::<Vec<_>>();

    cassie
        .midge
        .save_constraints(&statement.table, constraints.as_slice())
        .map_err(|error| QueryError::General(error.to_string()))?;
    let primary_key_indexes = primary_key_indexes(&statement.table, constraints.as_slice());
    for index in &primary_key_indexes {
        cassie
            .midge
            .put_index(index)
            .map_err(|error| QueryError::General(error.to_string()))?;
    }
    super::sequence_command::persist_created_sequences(cassie, table_sequences)?;
    cassie.catalog.register_collection_meta_with_constraints(
        collection_meta,
        schema
            .fields
            .into_iter()
            .map(|field| (field.name, field.data_type))
            .collect(),
        constraints,
    );
    for index in primary_key_indexes {
        cassie.catalog.register_index(index);
    }
    refresh_table_cardinality_stats(cassie, &statement.table)?;

    Ok(empty_command("CREATE TABLE"))
}

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
    cassie
        .catalog
        .rename_namespace(&target_schema, &next_schema);

    Ok(empty_command("ALTER SCHEMA"))
}

pub(super) fn create_role(
    cassie: &Cassie,
    statement: &CreateRoleStatement,
) -> Result<QueryResult, QueryError> {
    cassie
        .create_role(
            &statement.name,
            statement.login,
            statement.password.clone(),
            statement.if_not_exists,
        )
        .map_err(|error| QueryError::General(error.to_string()))?;

    Ok(empty_command("CREATE ROLE"))
}

pub(super) fn alter_role(
    cassie: &Cassie,
    statement: &AlterRoleStatement,
) -> Result<QueryResult, QueryError> {
    cassie
        .alter_role(&statement.name, statement.login, statement.password.clone())
        .map_err(|error| QueryError::General(error.to_string()))?;

    Ok(empty_command("ALTER ROLE"))
}

pub(super) fn drop_role(
    cassie: &Cassie,
    statement: &DropRoleStatement,
) -> Result<QueryResult, QueryError> {
    cassie
        .drop_role(&statement.name, statement.if_exists)
        .map_err(|error| QueryError::General(error.to_string()))?;

    Ok(empty_command("DROP ROLE"))
}

pub(super) fn create_index(
    cassie: &Cassie,
    statement: &CreateIndexStatement,
) -> Result<QueryResult, QueryError> {
    let is_column_store = cassie
        .catalog
        .collection_storage_mode(&statement.table)
        .is_some_and(crate::catalog::collections::CollectionStorageMode::uses_column_store_storage);
    if is_column_store && matches!(statement.kind, catalog::IndexKind::Column) {
        return Err(QueryError::General(
            "column indexes are not supported on column-store tables".to_string(),
        ));
    }
    if matches!(statement.kind, catalog::IndexKind::Vector) {
        let vector_index = super::vector_index_command::vector_index_metadata(cassie, statement)?;

        cassie
            .midge
            .put_vector_index(vector_index.clone())
            .map_err(|error| QueryError::General(error.to_string()))?;
        cassie.catalog.register_vector_index(vector_index);
    }

    let metadata = catalog::IndexMeta {
        collection: statement.table.clone(),
        name: statement.name.clone(),
        field: statement.fields.first().cloned().unwrap_or_default(),
        fields: statement.fields.clone(),
        expressions: statement
            .expressions
            .iter()
            .filter_map(|expression| serde_json::to_string(expression).ok())
            .collect(),
        include_fields: statement.include_fields.clone(),
        predicate: statement
            .predicate
            .as_ref()
            .and_then(|predicate| serde_json::to_string(predicate).ok()),
        kind: statement.kind.clone(),
        unique: statement.unique,
        options: statement.options.clone(),
    };

    cassie
        .midge
        .put_index(&metadata)
        .map_err(|error| QueryError::General(error.to_string()))?;
    cassie.catalog.register_index(metadata.clone());
    if matches!(metadata.kind, catalog::IndexKind::Column) {
        cassie
            .midge
            .rebuild_column_batches_for_index(&metadata)
            .map_err(|error| QueryError::General(error.to_string()))?;
    }
    refresh_table_cardinality_stats(cassie, &statement.table)?;

    Ok(empty_command("CREATE INDEX"))
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

fn refresh_table_cardinality_stats(cassie: &Cassie, table: &str) -> Result<(), QueryError> {
    cassie
        .refresh_cardinality_stats(table)
        .map_err(|error| QueryError::General(error.to_string()))
}

fn schema_is_empty(cassie: &Cassie, schema: &str) -> bool {
    !cassie
        .catalog
        .list_collections()
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
            .list_collections()
            .into_iter()
            .any(|collection| crate::catalog::relation_belongs_to_database(&collection.name, database))
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

fn empty_command(command: &str) -> QueryResult {
    QueryResult {
        columns: Vec::new(),
        rows: Vec::new(),
        command: command.to_string(),
    }
}
