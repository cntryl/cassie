use super::{catalog, primary_key_indexes, Cassie, FieldSchema, QueryError, QueryResult, Schema};
use crate::sql::ast::{CreateIndexStatement, CreateTableStatement};

pub(super) struct CreationOutcome {
    pub(super) result: QueryResult,
    pub(super) created: bool,
}

impl CreationOutcome {
    fn created(command: &str) -> Self {
        Self {
            result: empty_command(command),
            created: true,
        }
    }

    fn unchanged(command: &str) -> Self {
        Self {
            result: empty_command(command),
            created: false,
        }
    }
}

pub(super) fn create_table(
    cassie: &Cassie,
    statement: &CreateTableStatement,
) -> Result<CreationOutcome, QueryError> {
    if statement.if_not_exists && cassie.catalog.relation_exists(&statement.table) {
        return Ok(CreationOutcome::unchanged("CREATE TABLE"));
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
    super::schema_command::refresh_table_cardinality_stats(cassie, &statement.table)?;
    Ok(CreationOutcome::created("CREATE TABLE"))
}

pub(super) fn create_index(
    cassie: &Cassie,
    statement: &CreateIndexStatement,
) -> Result<CreationOutcome, QueryError> {
    if statement.if_not_exists
        && cassie
            .catalog
            .get_index(&statement.table, &statement.name)
            .is_some()
    {
        return Ok(CreationOutcome::unchanged("CREATE INDEX"));
    }

    let is_column_store = cassie
        .catalog
        .collection_storage_mode(&statement.table)
        .is_some_and(crate::catalog::collections::CollectionStorageMode::uses_column_store_storage);
    if is_column_store && matches!(statement.kind, catalog::IndexKind::Column) {
        return Err(QueryError::General(
            "column indexes are not supported on column-store tables".to_string(),
        ));
    }
    let vector_index = if matches!(statement.kind, catalog::IndexKind::Vector) {
        let metadata = super::vector_index_command::vector_index_metadata(cassie, statement)?;
        cassie
            .midge
            .put_vector_index(metadata.clone())
            .map_err(|error| QueryError::General(error.to_string()))?;
        Some(metadata)
    } else {
        None
    };
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
    cassie.midge.put_index(&metadata)?;
    cassie.catalog.register_index(metadata);
    if let Some(vector_index) = vector_index {
        cassie.catalog.register_vector_index(vector_index);
    }
    super::schema_command::refresh_table_cardinality_stats(cassie, &statement.table)?;
    Ok(CreationOutcome::created("CREATE INDEX"))
}

fn empty_command(command: &str) -> QueryResult {
    QueryResult {
        columns: Vec::new(),
        rows: Vec::new(),
        command: command.to_string(),
    }
}
