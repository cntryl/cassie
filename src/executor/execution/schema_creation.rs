use super::{catalog, primary_key_indexes, Cassie, FieldSchema, QueryError, QueryResult, Schema};
use crate::app::{CassieError, CatalogObjectKind};
use crate::catalog::{
    DefaultSequenceOwnership, FieldConstraint, IndexKind, IndexMeta, NotNullOwnership,
};
use crate::runtime::QueryExecutionControls;
use crate::sql::ast::{CreateIndexStatement, CreateTableStatement};
use std::collections::HashSet;

#[derive(Clone, Copy, Default)]
struct TableLikeCopyOptions {
    defaults: TableLikeCopyState,
    constraints: TableLikeCopyState,
    identity: TableLikeCopyState,
    indexes: TableLikeCopyState,
}

#[derive(Clone, Copy, Default)]
enum TableLikeCopyState {
    #[default]
    Excluded,
    Included,
}

impl TableLikeCopyState {
    const fn from_enabled(enabled: bool) -> Self {
        if enabled {
            Self::Included
        } else {
            Self::Excluded
        }
    }

    const fn is_included(self) -> bool {
        matches!(self, Self::Included)
    }
}

impl TableLikeCopyOptions {
    fn set(
        &mut self,
        feature: crate::sql::ast::CreateTableLikeFeature,
        enabled: bool,
    ) -> Result<(), QueryError> {
        use crate::sql::ast::CreateTableLikeFeature;

        match feature {
            CreateTableLikeFeature::All => {
                let state = TableLikeCopyState::from_enabled(enabled);
                self.defaults = state;
                self.constraints = state;
                self.identity = state;
                self.indexes = state;
            }
            CreateTableLikeFeature::Defaults => {
                self.defaults = TableLikeCopyState::from_enabled(enabled);
            }
            CreateTableLikeFeature::Constraints => {
                self.constraints = TableLikeCopyState::from_enabled(enabled);
            }
            CreateTableLikeFeature::Identity => {
                self.identity = TableLikeCopyState::from_enabled(enabled);
            }
            CreateTableLikeFeature::Indexes => {
                self.indexes = TableLikeCopyState::from_enabled(enabled);
            }
            CreateTableLikeFeature::Comments
            | CreateTableLikeFeature::Compression
            | CreateTableLikeFeature::Generated
            | CreateTableLikeFeature::Statistics
            | CreateTableLikeFeature::Storage => {
                return Err(CassieError::Unsupported(format!(
                    "CREATE TABLE LIKE option {feature:?} is not supported yet"
                ))
                .into());
            }
        }
        Ok(())
    }
}

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
    let additional_like_indexes = statement
        .like
        .as_ref()
        .map(|like| copy_table_like_indexes(cassie, table, like, &constraints))
        .transpose()?
        .unwrap_or_default();
    let mut collection_meta =
        catalog::CollectionMeta::new_with_storage_mode(table, None, statement.storage_mode);
    let table_sequences = prepare_table_sequences(cassie, statement, fields)?;
    let mut initial_indexes = primary_key_indexes(table, constraints.as_slice());
    initial_indexes.extend(catalog::composite_primary_key_indexes(
        table,
        constraints.as_slice(),
    ));
    initial_indexes.extend(catalog::unique_constraint_indexes(
        table,
        constraints.as_slice(),
    ));
    initial_indexes.extend(additional_like_indexes);
    let output = super::command_output::empty("CREATE TABLE", controls)?;
    if statement.like.is_some() {
        let Some(storage_id) = cassie
            .midge
            .create_collection_with_meta_constraints_sequences_and_indexes_if_absent(
                table,
                &schema,
                &collection_meta,
                &constraints,
                &table_sequences,
                &initial_indexes,
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
    if statement.like.is_none() {
        for index in &initial_indexes {
            cassie
                .midge
                .put_index_with_held_collection_gate(index)
                .map_err(|error| QueryError::General(error.to_string()))?;
        }
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
    for index in initial_indexes {
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
    let options = table_like_copy_options(like)?;

    validate_table_like_default_copy(
        &source_schema.fields,
        &source_constraints,
        options.defaults.is_included(),
    )?;

    if options.indexes.is_included()
        && source_constraints
            .iter()
            .filter(|constraint| constraint.primary_key)
            .count()
            > 1
    {
        return Err(CassieError::Unsupported(
            "CREATE TABLE LIKE INCLUDING INDEXES does not support composite primary keys yet"
                .to_string(),
        )
        .into());
    }

    Ok(source_schema
        .fields
        .into_iter()
        .map(|field| copy_table_like_field(table, field, &source_constraints, options))
        .collect())
}

fn copy_table_like_field(
    table: &str,
    field: catalog::FieldMeta,
    source_constraints: &[FieldConstraint],
    options: TableLikeCopyOptions,
) -> crate::sql::ast::FieldDefinition {
    let field_key = crate::sql::ColumnIdentifierPath::stored_field_key(&field.name);
    let matches_field = |constraint: &&FieldConstraint| {
        crate::sql::ColumnIdentifierPath::stored_field_key(&constraint.field) == field_key
    };
    let field_constraints = source_constraints.iter().filter(matches_field);
    let not_null = field_constraints
        .clone()
        .any(|constraint| constraint.not_null || constraint.primary_key);
    let source_identity = field_constraints
        .clone()
        .find(|constraint| constraint.identity.is_some());
    let source_default = if options.defaults.is_included() {
        field_constraints.clone().find(|constraint| {
            constraint.default_value.is_some() || constraint.default_expression.is_some()
        })
    } else {
        None
    }
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
    if options.identity.is_included() {
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
    let mut constraints = if copied_constraint.not_null
        || copied_constraint.default_value.is_some()
        || copied_constraint.default_expression.is_some()
        || copied_constraint.default_sequence.is_some()
    {
        vec![copied_constraint]
    } else {
        Vec::new()
    };
    if options.constraints.is_included() {
        constraints.extend(
            field_constraints
                .clone()
                .filter(|constraint| constraint.check.is_some())
                .map(copy_table_like_check_constraint),
        );
    }
    if options.indexes.is_included() {
        constraints.extend(
            field_constraints
                .filter(|constraint| constraint.primary_key || constraint.unique)
                .map(|constraint| {
                    copy_table_like_key_constraint(table, constraint, source_constraints)
                }),
        );
    }
    crate::sql::ast::FieldDefinition {
        name: field.name,
        data_type: field.data_type,
        constraints,
    }
}

fn table_like_copy_options(
    like: &crate::sql::ast::CreateTableLikeClause,
) -> Result<TableLikeCopyOptions, QueryError> {
    let mut options = TableLikeCopyOptions::default();
    for option in &like.options {
        match option {
            crate::sql::ast::CreateTableLikeOption::Including(feature) => {
                options.set(*feature, true)?;
            }
            crate::sql::ast::CreateTableLikeOption::Excluding(feature) => {
                options.set(*feature, false)?;
            }
        }
    }
    Ok(options)
}

fn copy_table_like_key_constraint(
    target: &str,
    source: &FieldConstraint,
    source_constraints: &[FieldConstraint],
) -> FieldConstraint {
    let mut copied = FieldConstraint::new(source.field.clone());
    copied.primary_key = source.primary_key;
    copied.primary_key_ordinal = source.primary_key_ordinal;
    copied.unique = source.unique;
    copied.unique_ordinal = source.unique_ordinal;
    copied.unique_null_policy = source.unique_null_policy;
    copied
        .constraint_index_include_fields
        .clone_from(&source.constraint_index_include_fields);
    copied.constraint_index_options = source.constraint_index_options.clone();

    if let Some(source_name) = &source.unique_name {
        let members = source_constraints
            .iter()
            .filter(|constraint| {
                constraint.unique
                    && constraint
                        .unique_name
                        .as_ref()
                        .is_some_and(|name| name.eq_ignore_ascii_case(source_name))
            })
            .collect::<Vec<_>>();
        if members.len() > 1 {
            let first_field = members
                .iter()
                .find(|constraint| constraint.unique_ordinal == Some(1))
                .map_or(source.field.as_str(), |constraint| {
                    constraint.field.as_str()
                });
            copied.unique_name = Some(crate::catalog::generated_constraint_name(
                target,
                first_field,
                "UNIQUE",
            ));
        }
    }
    copied
}

fn copy_table_like_check_constraint(source: &FieldConstraint) -> FieldConstraint {
    let mut copied = FieldConstraint::new(source.field.clone());
    copied.check_name.clone_from(&source.check_name);
    copied.check.clone_from(&source.check);
    copied
}

fn copy_table_like_indexes(
    cassie: &Cassie,
    target: &str,
    like: &crate::sql::ast::CreateTableLikeClause,
    target_constraints: &[FieldConstraint],
) -> Result<Vec<IndexMeta>, QueryError> {
    if !table_like_copy_options(like)?.indexes.is_included() {
        return Ok(Vec::new());
    }

    let source = like.source.to_string();
    let source_constraints = cassie.catalog.get_constraints(&source);
    let mut constraint_index_names = primary_key_indexes(&source, &source_constraints)
        .into_iter()
        .chain(catalog::composite_primary_key_indexes(
            &source,
            &source_constraints,
        ))
        .chain(catalog::unique_constraint_indexes(
            &source,
            &source_constraints,
        ))
        .map(|index| index.name.to_ascii_lowercase())
        .collect::<HashSet<_>>();
    constraint_index_names.extend(
        source_constraints
            .iter()
            .filter_map(|constraint| constraint.constraint_index_name.as_deref())
            .map(str::to_ascii_lowercase),
    );
    let mut reserved_names = cassie
        .catalog
        .all_indexes_snapshot()
        .into_iter()
        .map(|index| index.name.to_ascii_lowercase())
        .collect::<HashSet<_>>();
    reserved_names.extend(
        primary_key_indexes(target, target_constraints)
            .into_iter()
            .chain(catalog::composite_primary_key_indexes(
                target,
                target_constraints,
            ))
            .chain(catalog::unique_constraint_indexes(
                target,
                target_constraints,
            ))
            .map(|index| index.name.to_ascii_lowercase()),
    );

    let mut copies = Vec::new();
    for index in cassie.catalog.list_indexes(&source) {
        if !constraint_index_names.remove(&index.name.to_ascii_lowercase()) {
            if index.kind != IndexKind::Scalar
                || !index.access_method().eq_ignore_ascii_case("btree")
            {
                return Err(CassieError::Unsupported(format!(
                    "CREATE TABLE LIKE INCLUDING INDEXES cannot copy index '{}' with access method '{}'",
                    index.name,
                    index.access_method()
                ))
                .into());
            }
            let name = table_like_index_name(target, &index, &mut reserved_names);
            copies.push(index.clone_for_relation(target, &name));
        }
    }
    Ok(copies)
}

fn table_like_index_name(
    target: &str,
    index: &IndexMeta,
    reserved_names: &mut HashSet<String>,
) -> String {
    let fields = if index.fields.is_empty() {
        vec!["expression".to_string()]
    } else {
        index.fields.clone()
    };
    let base = format!(
        "{}_{}_idx",
        crate::catalog::local_name(target),
        fields.join("_")
    );
    let mut collision = 0_usize;
    loop {
        let suffix = if collision == 0 {
            String::new()
        } else {
            format!("_{collision}")
        };
        let prefix_limit = 63_usize.saturating_sub(suffix.len());
        let mut prefix_end = base.len().min(prefix_limit);
        while !base.is_char_boundary(prefix_end) {
            prefix_end -= 1;
        }
        let local_name = format!("{}{suffix}", &base[..prefix_end]);
        let candidate = crate::catalog::derive_scoped_name(target, |_| local_name);
        if reserved_names.insert(candidate.to_ascii_lowercase()) {
            return candidate;
        }
        collision = collision.saturating_add(1);
    }
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
