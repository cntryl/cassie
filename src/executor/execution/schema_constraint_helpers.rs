use super::{Cassie, QueryError};
use crate::catalog::FieldConstraint;

pub(super) fn name_composite_primary_key(table: &str, additions: &mut [FieldConstraint]) {
    let primary_key_members = additions
        .iter()
        .filter(|constraint| constraint.primary_key)
        .count();
    if primary_key_members > 1
        && additions
            .iter()
            .filter(|constraint| constraint.primary_key)
            .all(|constraint| constraint.primary_key_name.is_none())
    {
        let name = format!("{}_pkey", crate::catalog::local_name(table));
        for constraint in additions {
            if constraint.primary_key {
                constraint.primary_key_name = Some(name.clone());
            }
        }
    }
}

pub(super) fn validate_constraint_index_attachment(
    cassie: &Cassie,
    table: &str,
    existing: &[FieldConstraint],
    additions: &mut [FieldConstraint],
) -> Result<Option<String>, QueryError> {
    let attached_index_name = additions
        .iter()
        .find_map(|constraint| constraint.constraint_index_name.clone());
    let Some(index_name) = attached_index_name.as_ref() else {
        return Ok(attached_index_name);
    };
    let attached_index = cassie.catalog.get_index(table, index_name).ok_or_else(|| {
        QueryError::General(format!(
            "index '{index_name}' disappeared before constraint attachment"
        ))
    })?;
    if existing.iter().any(|constraint| {
        (constraint.unique || constraint.primary_key)
            && constraint
                .unique_constraint_name(table)
                .eq_ignore_ascii_case(&attached_index.name)
    }) {
        return Err(QueryError::General(format!(
            "index '{}' already backs a table constraint",
            attached_index.name
        )));
    }
    if !attached_index.can_back_unique_constraint() {
        return Err(QueryError::General(format!(
            "index '{}' cannot back a UNIQUE constraint",
            attached_index.name
        )));
    }
    for constraint in additions {
        if constraint.unique {
            constraint.constraint_index_name = Some(index_name.clone());
        }
    }
    Ok(attached_index_name)
}

pub(super) fn alter_table_drop_constraint(
    cassie: &Cassie,
    table: &str,
    name: &str,
    if_exists: bool,
) -> Result<(), QueryError> {
    let mut constraints = cassie.catalog.get_constraints(table);
    let dropped_index = constraint_index_to_drop(table, &constraints, name);
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
            constraint.unique_null_policy = crate::catalog::UniqueNullPolicy::Distinct;
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

    if let Some(index) = dropped_index {
        drop_constraint_index(cassie, table, &index)?;
    }
    if !constraints.iter().any(|constraint| constraint.primary_key) {
        drop_constraint_index(cassie, table, &format!("{table}_pkey"))?;
    }
    Ok(())
}

fn constraint_index_to_drop(
    table: &str,
    constraints: &[FieldConstraint],
    name: &str,
) -> Option<String> {
    let composite_primary_key = crate::catalog::composite_primary_key_constraints(constraints)
        .into_iter()
        .find(|constraint| constraint.name.eq_ignore_ascii_case(name))
        .map(|constraint| constraint.name);
    let primary_key = constraints
        .iter()
        .find(|constraint| {
            constraint.primary_key
                && constraint_name_matches(
                    table,
                    &constraint.field,
                    "PRIMARY KEY",
                    constraint.primary_key_name.as_ref(),
                    name,
                )
        })
        .map(|constraint| primary_key_index_name(table, constraint));
    let composite_unique = crate::catalog::composite_unique_constraints(constraints)
        .into_iter()
        .find(|constraint| constraint.name.eq_ignore_ascii_case(name))
        .map(|constraint| constraint.name);
    composite_primary_key
        .or(primary_key)
        .or(composite_unique)
        .or_else(|| {
            constraints
                .iter()
                .find(|constraint| {
                    constraint.unique
                        && !constraint.primary_key
                        && constraint_name_matches(
                            table,
                            &constraint.field,
                            "UNIQUE",
                            constraint.unique_name.as_ref(),
                            name,
                        )
                })
                .map(|constraint| constraint.unique_constraint_name(table))
        })
}

/// Drops the index that backed a dropped PRIMARY KEY or UNIQUE constraint, when it exists.
fn drop_constraint_index(cassie: &Cassie, table: &str, index: &str) -> Result<(), QueryError> {
    if cassie.catalog.get_index(table, index).is_none() {
        return Ok(());
    }
    cassie
        .midge
        .defer_drop_index(table, index, cassie.runtime.schema_epoch())
        .map_err(|error| QueryError::General(error.to_string()))?;
    cassie.catalog.unregister_index(table, index);
    Ok(())
}

fn primary_key_index_name(table: &str, constraint: &FieldConstraint) -> String {
    constraint
        .primary_key_name
        .clone()
        .unwrap_or_else(|| format!("{table}_pkey"))
}

/// Drops every multi-column UNIQUE constraint that includes `field`, as
/// PostgreSQL drops such a constraint with the column instead of leaving it
/// on the remaining columns.
pub(super) fn drop_composite_constraints_on_column(
    cassie: &Cassie,
    table: &str,
    field: &str,
) -> Result<(), QueryError> {
    let constraints = cassie.catalog.get_constraints(table);
    let composites = crate::catalog::composite_primary_key_constraints(&constraints)
        .into_iter()
        .chain(crate::catalog::composite_unique_constraints(&constraints));
    for composite in composites {
        if composite.fields.iter().any(|member| {
            crate::sql::ColumnIdentifierPath::stored_field_key(member)
                == crate::sql::ColumnIdentifierPath::stored_field_key(field)
        }) {
            alter_table_drop_constraint(cassie, table, &composite.name, false)?;
        }
    }
    Ok(())
}

/// Refuses to drop the unique index that backs a UNIQUE constraint, as
/// PostgreSQL refuses to drop an index a constraint requires.
pub(super) fn reject_constraint_index_drop(
    cassie: &Cassie,
    table: &str,
    index: &str,
) -> Result<(), QueryError> {
    let constraints = cassie.catalog.get_constraints(table);
    let required_by_primary_key = constraints.iter().any(|constraint| {
        constraint.primary_key
            && primary_key_index_name(table, constraint).eq_ignore_ascii_case(index)
    });
    let required_by_unique_constraint = constraints.iter().any(|constraint| {
        constraint.unique
            && !constraint.primary_key
            && constraint
                .unique_constraint_name(table)
                .eq_ignore_ascii_case(index)
    });
    if required_by_primary_key || required_by_unique_constraint {
        return Err(QueryError::Cassie(
            crate::app::DiagnosticError::new(
                "2BP01",
                format!(
                    "cannot drop index '{index}' because constraint '{index}' on '{table}' requires it; drop the constraint instead"
                ),
            )
            .into(),
        ));
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
                crate::sql::ColumnIdentifierPath::stored_field_key(&constraint.field)
                    == crate::sql::ColumnIdentifierPath::stored_field_key(field)
                    && (constraint.unique || constraint.primary_key)
            })
        })
        .collect()
}
