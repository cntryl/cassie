use super::{catalog, primary_key_indexes, Cassie, FieldSchema, QueryError, QueryResult, Schema};
use crate::app::{CassieError, CatalogObjectKind};
use crate::catalog::{FieldConstraint, NotNullOwnership};
use crate::runtime::QueryExecutionControls;
use crate::sql::ast::{CreateIndexStatement, CreateTableStatement};

pub(super) struct CreationOutcome {
    pub(super) result: QueryResult,
    pub(super) created: bool,
}

impl CreationOutcome {
    fn created(result: QueryResult) -> Self {
        Self {
            result,
            created: true,
        }
    }

    fn unchanged(command: &str, controls: &QueryExecutionControls) -> Result<Self, QueryError> {
        Ok(Self {
            result: super::command_output::empty(command, controls)?,
            created: false,
        })
    }
}

pub(super) fn create_table(
    cassie: &Cassie,
    statement: &CreateTableStatement,
    controls: &QueryExecutionControls,
) -> Result<CreationOutcome, QueryError> {
    let table = statement.table.to_string();
    let mut gated_collections = vec![table.clone()];
    if let Some(like) = statement.like.as_ref() {
        gated_collections.push(like.source.to_string());
    }
    cassie.midge.with_collection_gates(&gated_collections, || {
        create_table_gated(cassie, statement, &table, controls)
    })
}

fn create_table_gated(
    cassie: &Cassie,
    statement: &CreateTableStatement,
    table: &str,
    controls: &QueryExecutionControls,
) -> Result<CreationOutcome, QueryError> {
    if statement.if_not_exists && cassie.catalog.relation_exists(table) {
        return CreationOutcome::unchanged("CREATE TABLE", controls);
    }
    if cassie.catalog.relation_exists(table) {
        return Err(CassieError::CatalogObjectAlreadyExists {
            kind: CatalogObjectKind::Relation,
            name: table.to_string(),
        }
        .into());
    }

    let fields = if let Some(like) = statement.like.as_ref() {
        copy_table_like_fields(cassie, like)?
    } else {
        statement.fields.clone()
    };
    let constraints = fields
        .iter()
        .flat_map(|field| field.constraints.iter().cloned())
        .collect::<Vec<_>>();
    let schema = Schema {
        fields: fields
            .iter()
            .map(|field| FieldSchema {
                name: field.name.clone(),
                data_type: field.data_type.clone(),
                nullable: statement.like.is_none()
                    || !field
                        .constraints
                        .iter()
                        .any(|constraint| constraint.not_null || constraint.primary_key),
            })
            .collect(),
    };
    let mut collection_meta =
        catalog::CollectionMeta::new_with_storage_mode(table, None, statement.storage_mode);
    let mut sequence_statement = statement.clone();
    sequence_statement.fields = fields;
    let table_sequences =
        super::sequence_command::prepare_create_table_sequences(cassie, &sequence_statement)?;
    let output = super::command_output::empty("CREATE TABLE", controls)?;
    if statement.like.is_some() {
        let Some(storage_id) = cassie
            .midge
            .create_collection_with_meta_and_constraints_if_absent(
                table,
                &schema,
                &collection_meta,
                &constraints,
            )
            .map_err(QueryError::from)?
        else {
            if statement.if_not_exists {
                return CreationOutcome::unchanged("CREATE TABLE", controls);
            }
            return Err(CassieError::CatalogObjectAlreadyExists {
                kind: CatalogObjectKind::Relation,
                name: table.to_string(),
            }
            .into());
        };
        collection_meta.storage_id = storage_id;
    } else {
        collection_meta.storage_id = cassie
            .midge
            .create_collection_with_meta(table, &schema, &collection_meta)
            .map_err(QueryError::from)?;
        cassie
            .midge
            .save_constraints(table, constraints.as_slice())
            .map_err(|error| QueryError::General(error.to_string()))?;
    }
    let mut primary_key_indexes = primary_key_indexes(table, constraints.as_slice());
    primary_key_indexes.extend(catalog::composite_unique_indexes(
        table,
        constraints.as_slice(),
    ));
    for index in &primary_key_indexes {
        cassie
            .midge
            .put_index_with_held_collection_gate(index)
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
    if statement.like.is_none() {
        super::schema_command::refresh_table_cardinality_stats(cassie, table)?;
    }
    Ok(CreationOutcome::created(output))
}

fn copy_table_like_fields(
    cassie: &Cassie,
    like: &crate::sql::ast::CreateTableLikeClause,
) -> Result<Vec<crate::sql::ast::FieldDefinition>, QueryError> {
    let source = like.source.to_string();
    let source_schema =
        cassie
            .catalog
            .get_schema(&source)
            .ok_or_else(|| CassieError::CatalogObjectNotFound {
                kind: CatalogObjectKind::Relation,
                name: source.clone(),
            })?;
    let source_constraints = cassie.catalog.get_constraints(&source);
    let mut include_defaults = false;
    for option in &like.options {
        match option {
            crate::sql::ast::CreateTableLikeOption::Including(
                crate::sql::ast::CreateTableLikeFeature::Defaults,
            ) => include_defaults = true,
            crate::sql::ast::CreateTableLikeOption::Excluding(
                crate::sql::ast::CreateTableLikeFeature::Defaults,
            ) => include_defaults = false,
            _ => {
                return Err(CassieError::Unsupported(
                    "CREATE TABLE LIKE contains an unsupported option".to_string(),
                )
                .into());
            }
        }
    }

    validate_table_like_default_copy(&source_schema.fields, &source_constraints, include_defaults)?;

    Ok(source_schema
        .fields
        .into_iter()
        .map(|field| {
            let field_key = crate::sql::ColumnIdentifierPath::stored_field_key(&field.name);
            let matches_field = |constraint: &&FieldConstraint| {
                crate::sql::ColumnIdentifierPath::stored_field_key(&constraint.field) == field_key
            };
            let not_null = source_constraints
                .iter()
                .filter(matches_field)
                .any(|constraint| constraint.not_null || constraint.primary_key);
            let identity = source_constraints
                .iter()
                .filter(matches_field)
                .any(|constraint| constraint.identity.is_some());
            let source_default = include_defaults
                .then(|| {
                    source_constraints
                        .iter()
                        .filter(matches_field)
                        .find(|constraint| {
                            constraint.default_value.is_some()
                                || constraint.default_expression.is_some()
                        })
                })
                .flatten()
                .filter(|_| !identity);
            let mut copied_constraint = FieldConstraint::new(field.name.clone());
            if not_null {
                copied_constraint.not_null = true;
                copied_constraint.not_null_ownership = NotNullOwnership::Explicit;
            }
            if let Some(source_default) = source_default {
                copied_constraint
                    .default_value
                    .clone_from(&source_default.default_value);
                copied_constraint
                    .default_expression
                    .clone_from(&source_default.default_expression);
            }
            let constraints = if copied_constraint.not_null
                || copied_constraint.default_value.is_some()
                || copied_constraint.default_expression.is_some()
                || copied_constraint.default_sequence.is_some()
            {
                vec![copied_constraint]
            } else {
                Vec::new()
            };
            crate::sql::ast::FieldDefinition {
                name: field.name,
                data_type: field.data_type,
                constraints,
            }
        })
        .collect())
}

fn validate_table_like_default_copy(
    fields: &[crate::catalog::FieldMeta],
    constraints: &[FieldConstraint],
    include_defaults: bool,
) -> Result<(), QueryError> {
    if !include_defaults {
        return Ok(());
    }

    for field in fields {
        let field_key = crate::sql::ColumnIdentifierPath::stored_field_key(&field.name);
        let matches_field = |constraint: &&FieldConstraint| {
            crate::sql::ColumnIdentifierPath::stored_field_key(&constraint.field) == field_key
        };
        let identity = constraints
            .iter()
            .filter(matches_field)
            .any(|constraint| constraint.identity.is_some());
        let sequence_default = constraints.iter().filter(matches_field).any(|constraint| {
            constraint.default_sequence.is_some()
                || constraint
                    .default_expression
                    .as_deref()
                    .and_then(crate::catalog::parse_nextval_default_expression)
                    .is_some()
        });
        if !identity && sequence_default {
            return Err(CassieError::Unsupported(
                "CREATE TABLE LIKE INCLUDING DEFAULTS cannot copy sequence-backed defaults until their source dependency lifecycle is supported".to_string(),
            )
            .into());
        }
    }
    Ok(())
}

pub(super) fn create_index(
    cassie: &Cassie,
    statement: &CreateIndexStatement,
    controls: &QueryExecutionControls,
) -> Result<CreationOutcome, QueryError> {
    if statement.if_not_exists
        && cassie
            .catalog
            .get_index(&statement.table, &statement.name)
            .is_some()
    {
        return CreationOutcome::unchanged("CREATE INDEX", controls);
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
    let metadata = catalog::IndexMeta {
        collection: statement.table.to_string(),
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
    let output = super::command_output::empty("CREATE INDEX", controls)?;
    if matches!(statement.kind, catalog::IndexKind::Vector) {
        let vector = super::vector_index_command::vector_index_metadata(cassie, statement)?;
        cassie.put_vector_index_with_backfill(&vector, Some(&metadata))?;
    } else {
        cassie.midge.put_index(&metadata)?;
        cassie.catalog.register_index(metadata);
    }
    super::schema_command::refresh_table_cardinality_stats(cassie, &statement.table)?;
    Ok(CreationOutcome::created(output))
}
