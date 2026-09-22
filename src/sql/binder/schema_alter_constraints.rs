use super::{
    resolve_relation_name, AlterTableOperation, BindingContext, CassieError, Catalog,
    CollectionSchema,
};

/// Binds the constraints of `ALTER TABLE ... ADD CONSTRAINT` to stored names.
///
/// Each constraint column takes the child table's declared spelling, and each
/// FOREIGN KEY target is resolved exactly as `CREATE TABLE` resolves it, so the
/// executor and storage layers see the same keys on every DDL path.
pub(super) fn bind_alter_constraint_targets(
    operation: &mut AlterTableOperation,
    schema: &CollectionSchema,
    catalog: &Catalog,
    context: &BindingContext,
) -> Result<(), CassieError> {
    let AlterTableOperation::AddConstraint { constraints } = operation else {
        return Ok(());
    };

    for constraint in constraints {
        if let Some(declared) = schema
            .fields
            .iter()
            .find(|field| field.name.eq_ignore_ascii_case(constraint.field.trim()))
        {
            constraint.use_declared_field_spelling(&declared.name);
        }
        let label = constraint.field.clone();
        bind_foreign_key_reference(constraint, &label, catalog, context)?;
    }

    Ok(())
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
        .find(|entry| entry.name.eq_ignore_ascii_case(reference_field))
        .map(|entry| entry.name.clone())
    else {
        return Err(CassieError::Planner(format!(
            "foreign key on '{field_label}' references missing field '{reference_field}' on '{table}'"
        )));
    };

    let references_supported = catalog
        .get_constraints(&table)
        .into_iter()
        .any(|candidate| {
            candidate.field.eq_ignore_ascii_case(&reference_field)
                && (candidate.primary_key || candidate.unique)
        })
        || catalog
            .list_indexes(&table)
            .into_iter()
            .filter(|index| index.unique && index.kind == crate::catalog::IndexKind::Scalar)
            .any(|index| {
                let fields = index.normalized_fields();
                fields.len() == 1 && fields[0].eq_ignore_ascii_case(&reference_field)
            });

    if !references_supported {
        return Err(CassieError::Planner(format!(
            "foreign key on '{field_label}' must reference a primary or unique key on '{table}.{reference_field}'"
        )));
    }

    constraint.references_table = Some(table);
    constraint.references_field = Some(reference_field);
    Ok(())
}
