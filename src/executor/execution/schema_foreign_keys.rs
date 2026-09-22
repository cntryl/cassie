//! Keeps FOREIGN KEY metadata consistent with DDL on the tables and columns
//! it names.
//!
//! A FOREIGN KEY lives on the child collection's constraint set but names the
//! parent by `references_table` and `references_field`. DDL that removes or
//! renames either side must refuse, drop, or rewrite those references so the
//! durable metadata, the in-memory catalog, and DML enforcement keep resolving
//! to the same physical relation and column.

use super::{Cassie, QueryError};
use crate::catalog::FieldConstraint;

/// Refuses to drop a key constraint while a FOREIGN KEY references one of the
/// fields it backs.
pub(super) fn reject_referenced_constraint_drop(
    cassie: &Cassie,
    table: &str,
    name: &str,
    fields: &[String],
) -> Result<(), QueryError> {
    for collection in cassie.catalog.list_collections_canonical() {
        for constraint in cassie.catalog.get_constraints(&collection.name) {
            if references_table(&constraint, table)
                && fields
                    .iter()
                    .any(|field| references_field(&constraint, field))
            {
                return Err(QueryError::General(format!(
                    "cannot drop constraint '{name}' because foreign key on '{}' depends on it",
                    collection.name
                )));
            }
        }
    }
    Ok(())
}

/// Refuses to drop a column while a FOREIGN KEY declared on another column
/// references it. A FOREIGN KEY declared on the dropped column itself is not a
/// dependency; it is removed with the column.
pub(super) fn reject_referenced_column_drop(
    cassie: &Cassie,
    table: &str,
    field: &str,
) -> Result<(), QueryError> {
    for collection in cassie.catalog.list_collections_canonical() {
        for constraint in cassie.catalog.get_constraints(&collection.name) {
            let declared_on_dropped_column = collection.name.eq_ignore_ascii_case(table)
                && constraint.field.eq_ignore_ascii_case(field);
            if references_table(&constraint, table)
                && references_field(&constraint, field)
                && !declared_on_dropped_column
            {
                return Err(QueryError::General(format!(
                    "cannot drop column '{field}' of '{table}' because foreign key constraint '{}' on '{}' depends on it",
                    constraint.foreign_key_constraint_name(&collection.name),
                    collection.name
                )));
            }
        }
    }
    Ok(())
}

/// Removes the FOREIGN KEY declared on a dropped column, as PostgreSQL drops
/// the constraints that involve a dropped column.
pub(super) fn drop_foreign_keys_on_column(
    cassie: &Cassie,
    table: &str,
    field: &str,
) -> Result<(), QueryError> {
    let mut constraints = cassie.catalog.get_constraints(table);
    let mut changed = false;
    for constraint in &mut constraints {
        if constraint.field.eq_ignore_ascii_case(field) && constraint.references_table.is_some() {
            constraint.clear_foreign_key();
            changed = true;
        }
    }
    if !changed {
        return Ok(());
    }
    constraints.retain(super::constraint_is_populated);
    save_constraints(cassie, table, constraints)
}

/// Points every FOREIGN KEY that references `table.from` at `table.to`.
pub(super) fn rename_referenced_field(
    cassie: &Cassie,
    table: &str,
    from: &str,
    to: &str,
) -> Result<(), QueryError> {
    rewrite_references(cassie, |constraint| {
        if !references_table(constraint, table) || !references_field(constraint, from) {
            return false;
        }
        constraint.references_field = Some(to.to_string());
        true
    })
}

/// Points every FOREIGN KEY that references `table` at `next_table`.
pub(super) fn rename_referenced_table(
    cassie: &Cassie,
    table: &str,
    next_table: &str,
) -> Result<(), QueryError> {
    rewrite_references(cassie, |constraint| {
        if !references_table(constraint, table) {
            return false;
        }
        constraint.references_table = Some(next_table.to_string());
        true
    })
}

fn rewrite_references(
    cassie: &Cassie,
    mut rewrite: impl FnMut(&mut FieldConstraint) -> bool,
) -> Result<(), QueryError> {
    for collection in cassie.catalog.list_collections_canonical() {
        let mut constraints = cassie.catalog.get_constraints(&collection.name);
        let mut changed = false;
        for constraint in &mut constraints {
            changed |= rewrite(constraint);
        }
        if changed {
            save_constraints(cassie, &collection.name, constraints)?;
        }
    }
    Ok(())
}

fn save_constraints(
    cassie: &Cassie,
    table: &str,
    constraints: Vec<FieldConstraint>,
) -> Result<(), QueryError> {
    cassie
        .midge
        .save_constraints(table, &constraints)
        .map_err(|error| QueryError::General(error.to_string()))?;
    cassie.catalog.register_constraints(table, constraints);
    Ok(())
}

fn references_table(constraint: &FieldConstraint, table: &str) -> bool {
    constraint
        .references_table
        .as_deref()
        .is_some_and(|target| target.eq_ignore_ascii_case(table))
}

fn references_field(constraint: &FieldConstraint, field: &str) -> bool {
    constraint
        .references_field
        .as_deref()
        .is_some_and(|target| target.eq_ignore_ascii_case(field))
}
