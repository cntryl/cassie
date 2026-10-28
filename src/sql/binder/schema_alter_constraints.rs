use super::{
    resolve_relation_name, AlterTableOperation, BindingContext, CassieError, Catalog,
    CatalogObjectKind, CollectionSchema,
};

/// Binds the constraints of `ALTER TABLE ... ADD CONSTRAINT` to stored names.
///
/// Each constraint column takes the child table's declared spelling, and each
/// FOREIGN KEY target is resolved exactly as `CREATE TABLE` resolves it, so the
/// executor and storage layers see the same keys on every DDL path.
pub(super) fn bind_alter_constraint_targets(
    operation: &mut AlterTableOperation,
    table: &str,
    schema: &CollectionSchema,
    catalog: &Catalog,
    context: &BindingContext,
) -> Result<(), CassieError> {
    let attachment = match operation {
        AlterTableOperation::AddUniqueConstraintUsingIndex {
            constraint_name,
            index_name,
        } => Some((constraint_name.clone(), index_name.clone())),
        _ => None,
    };
    if let Some((constraint_name, index_name)) = attachment {
        bind_unique_index_attachment(
            operation,
            table,
            schema,
            catalog,
            constraint_name,
            &index_name,
        )?;
    }

    let (constraints, new_field) = match operation {
        AlterTableOperation::AddConstraint { constraints } => (constraints, None),
        AlterTableOperation::AddColumn {
            field, constraints, ..
        } => (constraints, Some(field.as_str())),
        AlterTableOperation::AlterColumnSetDefault { field, .. }
        | AlterTableOperation::AlterColumnDropDefault { field }
        | AlterTableOperation::AlterColumnSetNotNull { field }
        | AlterTableOperation::AlterColumnDropNotNull { field }
        | AlterTableOperation::AlterColumnIdentity { field, .. }
        | AlterTableOperation::AlterColumnType { field, .. }
        | AlterTableOperation::DropColumn { field, .. } => {
            use_declared_column_spelling(field, schema);
            return Ok(());
        }
        AlterTableOperation::RenameColumn { from, .. } => {
            use_declared_column_spelling(from, schema);
            return Ok(());
        }
        _ => return Ok(()),
    };

    for constraint in constraints {
        if let Some(field) = new_field {
            constraint.use_declared_field_spelling(field);
        } else if let Some(declared) = schema.fields.iter().find(|field| {
            crate::sql::ColumnIdentifierPath::stored_field_key(&field.name)
                == crate::sql::ColumnIdentifierPath::reference_field_key(constraint.field.trim())
        }) {
            constraint.use_declared_field_spelling(&declared.name);
        }
        bind_constraint_include_fields(constraint, table, schema)?;
        let label = constraint.field.clone();
        bind_foreign_key_reference(constraint, &label, catalog, context)?;
    }

    Ok(())
}

fn bind_unique_index_attachment(
    operation: &mut AlterTableOperation,
    table: &str,
    schema: &CollectionSchema,
    catalog: &Catalog,
    constraint_name: Option<String>,
    index_name: &str,
) -> Result<(), CassieError> {
    let index =
        catalog
            .get_index(table, index_name)
            .ok_or_else(|| CassieError::CatalogObjectNotFound {
                kind: CatalogObjectKind::Index,
                name: index_name.to_string(),
            })?;
    validate_attachable_unique_index(&index)?;
    if catalog.get_constraints(table).iter().any(|constraint| {
        (constraint.unique || constraint.primary_key)
            && constraint
                .unique_constraint_name(table)
                .eq_ignore_ascii_case(&index.name)
    }) {
        return Err(CassieError::Planner(format!(
            "index '{}' already backs a table constraint",
            index.name
        )));
    }
    let fields = index.normalized_fields();
    let constraint_name =
        constraint_name.unwrap_or_else(|| crate::catalog::local_name(&index.name));
    let mut constraints = Vec::with_capacity(fields.len());
    for (position, index_field) in fields.iter().enumerate() {
        let Some(declared) = schema.fields.iter().find(|field| {
            crate::sql::ColumnIdentifierPath::stored_field_key(&field.name)
                == crate::sql::ColumnIdentifierPath::stored_field_key(index_field)
        }) else {
            return Err(CassieError::Planner(format!(
                "UNIQUE USING INDEX references unknown column '{index_field}'"
            )));
        };
        let mut constraint = crate::catalog::FieldConstraint::new(declared.name.clone());
        constraint.unique = true;
        constraint.unique_name = Some(constraint_name.clone());
        constraint.unique_ordinal = Some(u32::try_from(position + 1).unwrap_or(u32::MAX));
        constraint.constraint_index_name = Some(index.name.clone());
        constraint.unique_null_policy = if index.nulls_not_distinct() {
            crate::catalog::UniqueNullPolicy::NotDistinct
        } else {
            crate::catalog::UniqueNullPolicy::Distinct
        };
        if position == 0 {
            constraint.constraint_index_include_fields = index.normalized_include_fields();
            constraint.constraint_index_options = index
                .options
                .iter()
                .filter(|(name, _)| !name.starts_with("__cassie_"))
                .map(|(name, value)| (name.clone(), value.clone()))
                .collect();
        }
        constraints.push(constraint);
    }
    *operation = AlterTableOperation::AddConstraint { constraints };
    Ok(())
}

fn bind_constraint_include_fields(
    constraint: &mut crate::catalog::FieldConstraint,
    table: &str,
    schema: &CollectionSchema,
) -> Result<(), CassieError> {
    for include_field in &mut constraint.constraint_index_include_fields {
        let include_key = crate::sql::ColumnIdentifierPath::stored_field_key(include_field);
        let Some(declared) = schema.fields.iter().find(|field| {
            crate::sql::ColumnIdentifierPath::stored_field_key(&field.name) == include_key
        }) else {
            return Err(CassieError::Planner(format!(
                "INCLUDE field '{include_field}' does not exist on collection '{table}'"
            )));
        };
        include_field.clone_from(&declared.name);
    }
    Ok(())
}

fn validate_attachable_unique_index(index: &crate::catalog::IndexMeta) -> Result<(), CassieError> {
    if !index.can_back_unique_constraint() {
        return Err(CassieError::Unsupported(format!(
            "index '{}' cannot back a UNIQUE constraint",
            index.name
        )));
    }
    Ok(())
}

/// Rewrites a column named by `ALTER COLUMN` or `DROP COLUMN` to the declared
/// spelling, which is the key stored payloads and constraint
/// metadata use; the parser preserved the SQL delimiter state in its lookup
/// key.
fn use_declared_column_spelling(field: &mut String, schema: &CollectionSchema) {
    if let Some(declared) = schema.fields.iter().find(|declared| {
        crate::sql::ColumnIdentifierPath::stored_field_key(&declared.name)
            == crate::sql::ColumnIdentifierPath::reference_field_key(field.trim())
    }) {
        declared.name.clone_into(field);
    }
}

/// Resolves a FOREIGN KEY's referenced relation and column to their stored
/// names and checks that the column is backed by a primary or unique key.
/// Constraints without a reference are left unchanged.
pub(super) fn bind_foreign_key_reference(
    constraint: &mut crate::catalog::FieldConstraint,
    field_label: &str,
    catalog: &Catalog,
    context: &BindingContext,
) -> Result<(), CassieError> {
    let (Some(table), Some(reference_field)) = (
        constraint.references_table.as_deref(),
        constraint.references_field.as_deref(),
    ) else {
        return Ok(());
    };
    let table = resolve_relation_name(table, catalog, context)?;
    if !catalog.exists(&table) {
        return Err(CassieError::CollectionNotFound(table));
    }
    let referenced_schema = catalog
        .get_schema(&table)
        .ok_or_else(|| CassieError::CollectionNotFound(table.clone()))?;
    let Some(reference_field) = referenced_schema
        .fields
        .iter()
        .find(|entry| {
            crate::sql::ColumnIdentifierPath::stored_field_key(&entry.name)
                == crate::sql::ColumnIdentifierPath::reference_field_key(reference_field)
        })
        .map(|entry| entry.name.clone())
    else {
        return Err(CassieError::Planner(format!(
            "foreign key on '{field_label}' references missing field '{reference_field}' on '{table}'"
        )));
    };

    let references_supported = catalog.has_unique_key_on(&table, &reference_field, None);

    if !references_supported {
        return Err(CassieError::Planner(format!(
            "foreign key on '{field_label}' must reference a primary or unique key on '{table}.{reference_field}'"
        )));
    }

    constraint.references_table = Some(table);
    constraint.references_field = Some(reference_field);
    Ok(())
}
