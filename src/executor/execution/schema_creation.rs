use super::{catalog, primary_key_indexes, Cassie, FieldSchema, QueryError, QueryResult, Schema};
use crate::app::{CassieError, CatalogObjectKind};
use crate::catalog::{DefaultSequenceOwnership, FieldConstraint, NotNullOwnership};
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

    let (fields, constraints, schema) = prepare_table_definition(cassie, statement, table)?;
    let mut collection_meta =
        catalog::CollectionMeta::new_with_storage_mode(table, None, statement.storage_mode);
    let table_sequences = prepare_table_sequences(cassie, statement, fields)?;
    let output = super::command_output::empty("CREATE TABLE", controls)?;
    if statement.like.is_some() {
        let Some(storage_id) = cassie
            .midge
            .create_collection_with_meta_and_constraints_and_sequences_if_absent(
                table,
                &schema,
                &collection_meta,
                &constraints,
                &table_sequences,
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
    primary_key_indexes.extend(catalog::unique_constraint_indexes(
        table,
        constraints.as_slice(),
    ));
    for index in &primary_key_indexes {
        cassie
            .midge
            .put_index_with_held_collection_gate(index)
            .map_err(|error| QueryError::General(error.to_string()))?;
    }
    if statement.like.is_some() {
        super::sequence_command::register_created_sequences(cassie, table_sequences);
    } else {
        super::sequence_command::persist_created_sequences(cassie, table_sequences)?;
    }
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

fn prepare_table_definition(
    cassie: &Cassie,
    statement: &CreateTableStatement,
    table: &str,
) -> Result<
    (
        Vec<crate::sql::ast::FieldDefinition>,
        Vec<FieldConstraint>,
        Schema,
    ),
    QueryError,
> {
    let fields = if let Some(like) = statement.like.as_ref() {
        copy_table_like_fields(cassie, table, like)?
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
    Ok((fields, constraints, schema))
}

fn prepare_table_sequences(
    cassie: &Cassie,
    statement: &CreateTableStatement,
    fields: Vec<crate::sql::ast::FieldDefinition>,
) -> Result<Vec<crate::catalog::SequenceMeta>, QueryError> {
    let mut sequence_statement = statement.clone();
    sequence_statement.fields = fields;
    let sequences =
        super::sequence_command::prepare_create_table_sequences(cassie, &sequence_statement)?;
    if statement.like.is_some() {
        super::sequence_command::prepare_created_sequences(cassie, sequences)
    } else {
        Ok(sequences)
    }
}

fn copy_table_like_fields(
    cassie: &Cassie,
    table: &str,
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
    let (include_defaults, include_constraints, include_identity) = table_like_copy_options(like)?;

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
            let source_identity = source_constraints
                .iter()
                .filter(matches_field)
                .find(|constraint| constraint.identity.is_some());
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
                .filter(|_| source_identity.is_none());
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
            if include_identity {
                if let Some(source_identity) = source_identity {
                    copied_constraint.identity = source_identity.identity;
                    copied_constraint
                        .identity_options
                        .clone_from(&source_identity.identity_options);
                    copied_constraint.not_null = true;
                    copied_constraint.not_null_ownership =
                        copied_constraint.not_null_ownership.with_explicit();
                    let sequence = crate::catalog::serial_sequence_name(table, &field.name);
                    copied_constraint.default_sequence = Some(sequence.clone());
                    copied_constraint.default_sequence_owned = DefaultSequenceOwnership::owned();
                    copied_constraint.default_expression =
                        Some(crate::catalog::canonical_nextval_expression(&sequence));
                }
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
            let mut constraints = constraints;
            if include_constraints {
                constraints.extend(
                    source_constraints
                        .iter()
                        .filter(matches_field)
                        .filter(|constraint| constraint.check.is_some())
                        .cloned(),
                );
            }
            crate::sql::ast::FieldDefinition {
                name: field.name,
                data_type: field.data_type,
                constraints,
            }
        })
        .collect())
}

fn table_like_copy_options(
    like: &crate::sql::ast::CreateTableLikeClause,
) -> Result<(bool, bool, bool), QueryError> {
    let mut include_defaults = false;
    let mut include_constraints = false;
    let mut include_identity = false;
    for option in &like.options {
        match option {
            crate::sql::ast::CreateTableLikeOption::Including(
                crate::sql::ast::CreateTableLikeFeature::Defaults,
            ) => include_defaults = true,
            crate::sql::ast::CreateTableLikeOption::Excluding(
                crate::sql::ast::CreateTableLikeFeature::Defaults,
            ) => include_defaults = false,
            crate::sql::ast::CreateTableLikeOption::Including(
                crate::sql::ast::CreateTableLikeFeature::Constraints,
            ) => include_constraints = true,
            crate::sql::ast::CreateTableLikeOption::Excluding(
                crate::sql::ast::CreateTableLikeFeature::Constraints,
            ) => include_constraints = false,
            crate::sql::ast::CreateTableLikeOption::Including(
                crate::sql::ast::CreateTableLikeFeature::Identity,
            ) => include_identity = true,
            crate::sql::ast::CreateTableLikeOption::Excluding(
                crate::sql::ast::CreateTableLikeFeature::Identity,
            ) => include_identity = false,
            _ => {
                return Err(CassieError::Unsupported(
                    "CREATE TABLE LIKE contains an unsupported option".to_string(),
                )
                .into());
            }
        }
    }
    Ok((include_defaults, include_constraints, include_identity))
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
    let logical_table = statement.table.to_string();
    let storage_table = cassie
        .catalog
        .materialized_view_output_collection(&logical_table)
        .or_else(|| {
            (!cassie.catalog.is_sql_materialized_view(&logical_table))
                .then(|| logical_table.clone())
        })
        .ok_or_else(|| {
            QueryError::Cassie(
                crate::app::DiagnosticError::new(
                    "55000",
                    format!("materialized view '{logical_table}' has not been populated"),
                )
                .into(),
            )
        })?;
    if statement.if_not_exists
        && cassie
            .catalog
            .get_index(&logical_table, &statement.name)
            .is_some()
    {
        return CreationOutcome::unchanged("CREATE INDEX", controls);
    }

    let is_column_store = cassie
        .catalog
        .collection_storage_mode(&storage_table)
        .is_some_and(crate::catalog::collections::CollectionStorageMode::uses_column_store_storage);
    if is_column_store && matches!(statement.kind, catalog::IndexKind::Column) {
        return Err(QueryError::General(
            "column indexes are not supported on column-store tables".to_string(),
        ));
    }
    let metadata = catalog::IndexMeta {
        collection: storage_table.clone(),
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
    super::schema_command::refresh_table_cardinality_stats(cassie, &storage_table)?;
    Ok(CreationOutcome::created(output))
}
