use super::{
    normalize_relation_name, resolve_relation_path, virtual_views, BindingContext, CassieError,
    Catalog, HashSet,
};
use super::{schema_alter_constraints, schema_checks, schema_defaults};
use crate::sql::ast::IdentifierPath;
use crate::sql::binder::normalize_new_relation_path;
use crate::types::DataType;

pub fn bind_create_table(
    mut statement: crate::sql::ast::CreateTableStatement,
    catalog: &Catalog,
    context: &BindingContext,
) -> Result<crate::sql::ast::CreateTableStatement, CassieError> {
    validate_create_table_like_options(&statement)?;

    let name = normalize_new_relation_path(&statement.table, context, catalog)?;
    if name.is_empty() {
        return Err(CassieError::Planner(
            "CREATE TABLE requires a table name".into(),
        ));
    }
    let already_exists = catalog.relation_exists(&name) || virtual_views::schema(&name).is_some();
    if !statement.if_not_exists && already_exists {
        return Err(CassieError::Planner(format!(
            "collection '{name}' already exists"
        )));
    }
    if statement.if_not_exists && already_exists {
        statement.table = canonical_relation_path(&name)?;
        return Ok(statement);
    }

    if statement.as_query.is_some() {
        statement.table = canonical_relation_path(&name)?;
        return Ok(statement);
    }

    if matches!(
        statement.storage_mode,
        crate::catalog::CollectionStorageMode::ColumnIndexed
    ) {
        return Err(CassieError::Planner(
            "CREATE TABLE storage mode 'column_indexed' is derived and cannot be created explicitly"
                .into(),
        ));
    }

    if let Some(like) = statement.like.as_mut() {
        let source = resolve_relation_path(&like.source, catalog, context)?;
        like.source = canonical_relation_path(&source)?;
    }

    let own_constraints = schema_checks::bind_create_checks(&mut statement.fields)?;
    let own_unique_fields = own_unique_fields(&statement.fields, &own_constraints);
    validate_and_bind_table_fields(
        &mut statement.fields,
        &name,
        catalog,
        context,
        &own_unique_fields,
    )?;

    requalify_serial_sequences(&mut statement, &name, catalog, context)?;
    statement.table = canonical_relation_path(&name)?;
    Ok(statement)
}

fn own_unique_fields(
    fields: &[crate::sql::ast::FieldDefinition],
    own_constraints: &[crate::catalog::FieldConstraint],
) -> Vec<String> {
    fields
        .iter()
        .filter(|field| {
            field.constraints.iter().any(|constraint| {
                crate::catalog::enforces_single_column_uniqueness(constraint, own_constraints)
            })
        })
        .map(|field| field.name.trim().to_string())
        .collect()
}

fn validate_and_bind_table_fields(
    fields: &mut [crate::sql::ast::FieldDefinition],
    name: &str,
    catalog: &Catalog,
    context: &BindingContext,
    own_unique_fields: &[String],
) -> Result<(), CassieError> {
    let mut seen = HashSet::new();
    let mut primary_key_field: Option<String> = None;
    let mut primary_key_ordinals = HashSet::new();
    for field in fields {
        if matches!(field.data_type, DataType::JsonPath) {
            return Err(CassieError::Unsupported(
                "JSONPATH is supported for expressions, not table columns".to_string(),
            ));
        }
        let field_name = field.name.trim();
        if field_name.is_empty() {
            return Err(CassieError::Planner(
                "CREATE TABLE field names cannot be empty".into(),
            ));
        }
        super::validate_not_internal_identity_field(field_name)?;
        schema_defaults::bind_constraint_defaults(
            field_name,
            &field.data_type,
            &mut field.constraints,
        )?;

        if !seen.insert(field_name.to_string()) {
            return Err(CassieError::Planner(format!(
                "CREATE TABLE field '{field_name}' is defined more than once"
            )));
        }

        for constraint in &mut field.constraints {
            if constraint.primary_key {
                let ordinal = constraint.primary_key_ordinal;
                if let Some(previous) = &primary_key_field {
                    if ordinal.is_none_or(|ordinal| !primary_key_ordinals.insert(ordinal)) {
                        return Err(CassieError::Planner(format!(
                            "multiple primary keys defined on '{name}': '{previous}' and '{field_name}'"
                        )));
                    }
                } else if ordinal.is_none_or(|ordinal| !primary_key_ordinals.insert(ordinal)) {
                    return Err(CassieError::Planner(format!(
                        "invalid primary key ordinal on '{name}.{field_name}'"
                    )));
                }
                primary_key_field.get_or_insert_with(|| field_name.to_string());
            }
            if !bind_self_foreign_key_reference(
                constraint,
                field_name,
                name,
                own_unique_fields,
                context,
            )? {
                schema_alter_constraints::bind_foreign_key_reference(
                    constraint, field_name, catalog, context,
                )?;
            }
        }

        field.name = field_name.to_string();
    }

    if primary_key_ordinals.len() > 1
        && !(1..=u32::try_from(primary_key_ordinals.len()).unwrap_or(u32::MAX))
            .all(|ordinal| primary_key_ordinals.contains(&ordinal))
    {
        return Err(CassieError::Planner(format!(
            "invalid composite primary key ordinals on '{name}'"
        )));
    }
    Ok(())
}

fn validate_create_table_like_options(
    statement: &crate::sql::ast::CreateTableStatement,
) -> Result<(), CassieError> {
    let Some(like) = statement.like.as_ref() else {
        return Ok(());
    };

    for option in &like.options {
        let (action, feature) = match option {
            crate::sql::ast::CreateTableLikeOption::Including(feature) => ("INCLUDING", *feature),
            crate::sql::ast::CreateTableLikeOption::Excluding(feature) => ("EXCLUDING", *feature),
        };
        if !matches!(
            feature,
            crate::sql::ast::CreateTableLikeFeature::All
                | crate::sql::ast::CreateTableLikeFeature::Defaults
                | crate::sql::ast::CreateTableLikeFeature::Constraints
                | crate::sql::ast::CreateTableLikeFeature::Identity
                | crate::sql::ast::CreateTableLikeFeature::Indexes
        ) {
            return Err(CassieError::Unsupported(format!(
                "CREATE TABLE LIKE option {action} {} is not supported yet",
                create_table_like_feature_name(feature)
            )));
        }
    }
    Ok(())
}

fn create_table_like_feature_name(
    feature: crate::sql::ast::CreateTableLikeFeature,
) -> &'static str {
    use crate::sql::ast::CreateTableLikeFeature;

    match feature {
        CreateTableLikeFeature::All => "ALL",
        CreateTableLikeFeature::Comments => "COMMENTS",
        CreateTableLikeFeature::Compression => "COMPRESSION",
        CreateTableLikeFeature::Constraints => "CONSTRAINTS",
        CreateTableLikeFeature::Defaults => "DEFAULTS",
        CreateTableLikeFeature::Generated => "GENERATED",
        CreateTableLikeFeature::Identity => "IDENTITY",
        CreateTableLikeFeature::Indexes => "INDEXES",
        CreateTableLikeFeature::Statistics => "STATISTICS",
        CreateTableLikeFeature::Storage => "STORAGE",
    }
}

pub fn canonical_relation_path(name: &str) -> Result<IdentifierPath, CassieError> {
    IdentifierPath::parse(name).map_err(CassieError::Planner)
}

/// Binds a FOREIGN KEY that references the table being created, which is not
/// in the catalog yet, against the statement's own PRIMARY KEY and UNIQUE
/// columns. Returns `false` when the reference names another table.
fn bind_self_foreign_key_reference(
    constraint: &mut crate::catalog::FieldConstraint,
    field_label: &str,
    table: &str,
    own_unique_fields: &[String],
    context: &BindingContext,
) -> Result<bool, CassieError> {
    let (Some(referenced_table), Some(referenced_field)) = (
        constraint.references_table.as_deref(),
        constraint.references_field.as_deref(),
    ) else {
        return Ok(false);
    };
    if !normalize_relation_name(referenced_table.trim(), context)?.eq_ignore_ascii_case(table) {
        return Ok(false);
    }
    let Some(declared) = own_unique_fields.iter().find(|field| {
        crate::sql::ColumnIdentifierPath::stored_field_key(field)
            == crate::sql::ColumnIdentifierPath::reference_field_key(referenced_field)
    }) else {
        return Err(CassieError::Planner(format!(
            "foreign key on '{field_label}' must reference a primary or unique key on '{table}.{referenced_field}'"
        )));
    };
    constraint.references_field = Some(declared.clone());
    constraint.references_table = Some(table.to_string());
    Ok(true)
}

/// SERIAL sequence names are derived while parsing, before the table name is
/// qualified, so rederive them from the bound table name.
///
/// An unqualified table in the default schema keeps its legacy bare sequence
/// name, so catalogs written by earlier versions still resolve. Every other
/// table, including an unqualified one resolved through a non-default
/// `search_path` schema, gets a sequence in its own schema, which keeps
/// same-named tables in different schemas apart and lets a schema rename carry
/// the sequence with the table.
fn requalify_serial_sequences(
    statement: &mut crate::sql::ast::CreateTableStatement,
    bound_table: &str,
    catalog: &Catalog,
    context: &BindingContext,
) -> Result<(), CassieError> {
    let preserve_legacy_serial_name = matches!(
        crate::catalog::parse_name(statement.table.trim()).map_err(CassieError::Planner)?,
        crate::catalog::ParsedName::Unqualified(_)
    ) && crate::catalog::relation_schema_name(bound_table)
        .eq_ignore_ascii_case(crate::catalog::DEFAULT_SCHEMA);
    for field in &mut statement.fields {
        for constraint in &mut field.constraints {
            let Some(sequence) = constraint.default_sequence.as_deref() else {
                continue;
            };
            let sequence = if constraint.default_sequence_owned.is_owned() {
                if preserve_legacy_serial_name {
                    continue;
                }
                crate::catalog::serial_sequence_name(bound_table, &field.name)
            } else {
                resolve_default_sequence(sequence, bound_table, catalog, context)?
            };
            constraint.default_expression =
                Some(crate::catalog::canonical_nextval_expression(&sequence));
            constraint.default_sequence = Some(sequence);
        }
    }
    Ok(())
}

pub fn resolve_default_sequence(
    sequence: &str,
    bound_table: &str,
    catalog: &Catalog,
    context: &BindingContext,
) -> Result<String, CassieError> {
    let sequences = catalog.list_sequences();
    let (schema, name) = match crate::catalog::parse_name(sequence).map_err(CassieError::Planner)? {
        crate::catalog::ParsedName::Unqualified(name) => {
            let table_schema = crate::catalog::relation_schema_name(bound_table);
            let candidate =
                crate::catalog::canonical_relation_name(&context.database, &table_schema, &name);
            if sequences
                .iter()
                .any(|item| item.name.eq_ignore_ascii_case(&candidate))
            {
                return Ok(candidate);
            }
            for schema in &context.search_path {
                let candidate =
                    crate::catalog::canonical_relation_name(&context.database, schema, &name);
                if sequences
                    .iter()
                    .any(|item| item.name.eq_ignore_ascii_case(&candidate))
                {
                    return Ok(candidate);
                }
            }
            return Ok(candidate);
        }
        crate::catalog::ParsedName::SchemaQualified { schema, name } => (schema, name),
        crate::catalog::ParsedName::DatabaseQualified {
            database,
            schema,
            name,
        } => {
            if !database.eq_ignore_ascii_case(&context.database) {
                return Err(CassieError::Unsupported(
                    "cross-database sequence references are not supported".to_string(),
                ));
            }
            (schema, name)
        }
    };
    Ok(crate::catalog::canonical_relation_name(
        &context.database,
        &schema,
        &name,
    ))
}
