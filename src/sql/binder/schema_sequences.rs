use super::{
    normalize_relation_name, AlterSequenceStatement, AlterTableOperation, BindingContext,
    CassieError, Catalog, CatalogObjectKind, CreateSequenceStatement, DropSequenceStatement,
    HashSet,
};

pub(super) fn bind_create_sequence(
    mut statement: CreateSequenceStatement,
    catalog: &Catalog,
    context: &BindingContext,
) -> Result<CreateSequenceStatement, CassieError> {
    let name = super::normalize_new_relation_name(statement.name.trim(), context, catalog)?;
    if name.is_empty() {
        return Err(CassieError::Planner(
            "CREATE SEQUENCE requires a name".into(),
        ));
    }
    if !statement.if_not_exists && catalog.sequence_exists(&name) {
        return Err(CassieError::Planner(format!(
            "sequence '{name}' already exists"
        )));
    }
    statement.name = name;
    if let Some(crate::sql::ast::SequenceOwnedBy::Column { table, .. }) = &mut statement.owned_by {
        *table = super::resolve_relation_name(table, catalog, context)?;
    }
    Ok(statement)
}

/// Resolves an existing sequence through the search path.
pub(super) fn resolve_sequence_name(
    raw: &str,
    catalog: &Catalog,
    context: &BindingContext,
) -> Result<Option<String>, CassieError> {
    let qualified = normalize_relation_name(raw.trim(), context)?;
    if catalog.sequence_exists(&qualified) {
        return Ok(Some(qualified));
    }
    if crate::catalog::parse_name(raw.trim())
        .is_ok_and(|parsed| matches!(parsed, crate::catalog::ParsedName::Unqualified(_)))
    {
        for schema in context.relation_search_order() {
            let candidate =
                crate::catalog::canonical_relation_name(&context.database, schema, raw.trim());
            if catalog.sequence_exists(&candidate) {
                return Ok(Some(candidate));
            }
        }
    }
    Ok(None)
}

pub(super) fn bind_alter_sequence(
    mut statement: AlterSequenceStatement,
    catalog: &Catalog,
    context: &BindingContext,
) -> Result<AlterSequenceStatement, CassieError> {
    match resolve_sequence_name(&statement.name, catalog, context)? {
        Some(name) => statement.name = name,
        None if statement.if_exists => {}
        None => {
            return Err(CassieError::CatalogObjectNotFound {
                kind: CatalogObjectKind::Sequence,
                name: statement.name,
            });
        }
    }
    if let Some(crate::sql::ast::SequenceOwnedBy::Column { table, .. }) = &mut statement.owned_by {
        *table = super::resolve_relation_name(table, catalog, context)?;
    }
    Ok(statement)
}

pub(super) fn bind_drop_sequence(
    mut statement: DropSequenceStatement,
    catalog: &Catalog,
    context: &BindingContext,
) -> Result<DropSequenceStatement, CassieError> {
    let mut names = Vec::with_capacity(1 + statement.additional_names.len());
    for raw in std::iter::once(&statement.name).chain(&statement.additional_names) {
        let name = resolve_sequence_name(raw, catalog, context)?
            .map_or_else(|| normalize_relation_name(raw.trim(), context), Ok)?;
        if name.is_empty() {
            return Err(CassieError::Planner("DROP SEQUENCE requires a name".into()));
        }
        if !statement.if_exists && !catalog.sequence_exists(&name) {
            return Err(CassieError::CatalogObjectNotFound {
                kind: CatalogObjectKind::Sequence,
                name,
            });
        }
        names.push(name);
    }
    statement.name = names.remove(0);
    statement.additional_names = names;
    Ok(statement)
}

pub(super) fn validate_alter_column_operation(
    table: &str,
    operation: &AlterTableOperation,
    existing_fields: &HashSet<String>,
    catalog: &Catalog,
) -> Result<(), CassieError> {
    match operation {
        AlterTableOperation::AlterColumnSetDefault {
            field,
            default_sequence,
            ..
        } => {
            validate_alter_column_field(table, "SET DEFAULT", field, existing_fields)?;
            if let Some(sequence) = default_sequence {
                if !catalog.sequence_exists(sequence) {
                    return Err(CassieError::CatalogObjectNotFound {
                        kind: CatalogObjectKind::Sequence,
                        name: sequence.clone(),
                    });
                }
            }
        }
        AlterTableOperation::AlterColumnDropDefault { field }
        | AlterTableOperation::AlterColumnSetNotNull { field }
        | AlterTableOperation::AlterColumnDropNotNull { field }
        | AlterTableOperation::AlterColumnIdentity { field, .. }
        | AlterTableOperation::AlterColumnType { field, .. } => {
            validate_alter_column_field(table, "ALTER COLUMN", field, existing_fields)?;
        }
        _ => {}
    }

    Ok(())
}

fn validate_alter_column_field(
    table: &str,
    operation: &str,
    field: &str,
    existing_fields: &HashSet<String>,
) -> Result<(), CassieError> {
    let name = field.trim();
    if name.is_empty() {
        return Err(CassieError::Planner(format!(
            "ALTER TABLE ALTER COLUMN {operation} requires a field"
        )));
    }
    if !existing_fields
        .iter()
        .any(|field| crate::sql::ColumnIdentifierPath::matches_stored_field(name, field))
    {
        return Err(CassieError::Planner(format!(
            "ALTER TABLE '{table}' has no field '{name}'"
        )));
    }
    Ok(())
}
