use super::{Cassie, QueryError};
use crate::catalog::FieldConstraint;

pub(super) fn alter_table_drop_constraint(
    cassie: &Cassie,
    table: &str,
    name: &str,
    if_exists: bool,
) -> Result<(), QueryError> {
    let mut constraints = cassie.catalog.get_constraints(table);
    let mut constrained_unique_fields = Vec::new();
    let mut found = false;
    for constraint in &mut constraints {
        if constraint_name_matches(
            table,
            &constraint.field,
            "PRIMARY KEY",
            constraint.primary_key_name.as_ref(),
            name,
        ) {
            constrained_unique_fields.push(constraint.field.clone());
            constraint.primary_key = false;
            constraint.primary_key_name = None;
            constraint.primary_key_ordinal = None;
            if !constraint.not_null_ownership.is_explicit() {
                constraint.not_null = false;
            }
            constraint.not_null_ownership = constraint.not_null_ownership.without_primary_key();
            found = true;
        }
        if constraint_name_matches(
            table,
            &constraint.field,
            "UNIQUE",
            constraint.unique_name.as_ref(),
            name,
        ) {
            constrained_unique_fields.push(constraint.field.clone());
            constraint.unique = false;
            constraint.unique_name = None;
            constraint.unique_ordinal = None;
            found = true;
        }
        if constraint_name_matches(
            table,
            &constraint.field,
            "CHECK",
            constraint.check_name.as_ref(),
            name,
        ) {
            constraint.check = None;
            constraint.check_name = None;
            found = true;
        }
        if constraint_name_matches(
            table,
            &constraint.field,
            "FOREIGN KEY",
            constraint.foreign_key_name.as_ref(),
            name,
        ) {
            constraint.clear_foreign_key();
            found = true;
        }
    }
    if !found {
        if if_exists {
            return Ok(());
        }
        return Err(QueryError::General(format!(
            "constraint '{name}' does not exist on collection '{table}'"
        )));
    }

    super::schema_foreign_keys::reject_referenced_constraint_drop(
        cassie,
        table,
        name,
        &constrained_unique_fields,
    )?;
    constraints.retain(constraint_is_populated);
    let released_unique_fields =
        unique_fields_without_constraints(&constraints, constrained_unique_fields);
    cassie
        .midge
        .save_constraints_and_release_unique_reservations(
            table,
            &constraints,
            &released_unique_fields,
        )
        .map_err(|error| QueryError::General(error.to_string()))?;
    cassie
        .catalog
        .register_constraints(table, constraints.clone());

    if !constraints.iter().any(|constraint| constraint.primary_key) {
        let primary_index_name = format!("{table}_pkey");
        if cassie
            .catalog
            .get_index(table, &primary_index_name)
            .is_some()
        {
            cassie
                .midge
                .defer_drop_index(table, &primary_index_name, cassie.runtime.schema_epoch())
                .map_err(|error| QueryError::General(error.to_string()))?;
            cassie.catalog.unregister_index(table, &primary_index_name);
        }
    }
    Ok(())
}

fn constraint_name_matches(
    table: &str,
    field: &str,
    kind: &str,
    explicit_name: Option<&String>,
    requested_name: &str,
) -> bool {
    explicit_name.is_some_and(|name| name.eq_ignore_ascii_case(requested_name))
        || (explicit_name.is_none()
            && crate::catalog::generated_constraint_name(table, field, kind)
                .eq_ignore_ascii_case(requested_name))
}

pub(super) fn constraint_is_populated(constraint: &FieldConstraint) -> bool {
    constraint.primary_key
        || constraint.unique
        || constraint.not_null
        || constraint.default_value.is_some()
        || constraint.default_expression.is_some()
        || constraint.default_sequence.is_some()
        || constraint.check.is_some()
        || constraint.references_table.is_some()
}

pub(super) fn unique_fields_without_constraints(
    constraints: &[FieldConstraint],
    candidates: Vec<String>,
) -> Vec<String> {
    candidates
        .into_iter()
        .filter(|field| {
            !constraints.iter().any(|constraint| {
                constraint.field.eq_ignore_ascii_case(field)
                    && (constraint.unique || constraint.primary_key)
            })
        })
        .collect()
}
