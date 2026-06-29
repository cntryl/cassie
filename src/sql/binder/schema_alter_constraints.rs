use super::{AlterTableOperation, Catalog, CassieError};

pub(super) fn validate_alter_constraint_targets(
    operation: &AlterTableOperation,
    catalog: &Catalog,
) -> Result<(), CassieError> {
    let AlterTableOperation::AddConstraint { constraints } = operation else {
        return Ok(());
    };

    for constraint in constraints {
        let (Some(table), Some(reference_field)) = (
            constraint.references_table.as_deref(),
            constraint.references_field.as_deref(),
        ) else {
            continue;
        };
        if !catalog.exists(table) {
            return Err(CassieError::CollectionNotFound(table.to_string()));
        }
        let referenced_schema = catalog
            .get_schema(table)
            .ok_or_else(|| CassieError::CollectionNotFound(table.to_string()))?;
        if !referenced_schema
            .fields
            .iter()
            .any(|entry| entry.name.eq_ignore_ascii_case(reference_field))
        {
            return Err(CassieError::Planner(format!(
                "foreign key on '{}' references missing field '{reference_field}' on '{table}'",
                constraint.field
            )));
        }

        let references_supported = catalog.get_constraints(table).into_iter().any(|candidate| {
            candidate.field.eq_ignore_ascii_case(reference_field)
                && (candidate.primary_key || candidate.unique)
        }) || catalog
            .list_indexes(table)
            .into_iter()
            .filter(|index| index.unique && index.kind == crate::catalog::IndexKind::Scalar)
            .any(|index| {
                let fields = index.normalized_fields();
                fields.len() == 1 && fields[0].eq_ignore_ascii_case(reference_field)
            });

        if !references_supported {
            return Err(CassieError::Planner(format!(
                "foreign key on '{}' must reference a primary or unique key on '{table}.{reference_field}'",
                constraint.field
            )));
        }
    }

    Ok(())
}
