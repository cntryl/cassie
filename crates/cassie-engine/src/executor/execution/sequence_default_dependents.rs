//! Column defaults in other tables that reference a sequence (#826).
//!
//! `CREATE TABLE ... LIKE ... INCLUDING DEFAULTS` copies `nextval(...)`
//! defaults that keep referencing the source's sequence, as PostgreSQL does.
//! Like PostgreSQL, dropping that sequence, or the table that owns it, is
//! refused with `2BP01` while another table's default depends on it. `DROP
//! TABLE ... CASCADE` instead removes those defaults.
use super::{Cassie, QueryError};
use crate::app::DiagnosticError;
use crate::catalog::name_matches;

/// One column default in `table` that calls `nextval` on `sequence`.
pub(super) struct DefaultDependent {
    table: String,
    column: String,
    sequence: String,
}

/// Sequences that are dropped together with `table`.
fn owned_sequences(cassie: &Cassie, table: &str) -> Vec<String> {
    let from_defaults = cassie
        .catalog
        .get_constraints(table)
        .into_iter()
        .filter(|constraint| constraint.default_sequence_owned.is_owned())
        .filter_map(|constraint| constraint.default_sequence);
    let from_ownership = cassie
        .catalog
        .list_sequences()
        .into_iter()
        .filter(|sequence| {
            sequence
                .owned_by
                .as_ref()
                .is_some_and(|owner| name_matches(&owner.table, table))
        })
        .map(|sequence| sequence.name);
    from_defaults.chain(from_ownership).collect()
}

/// Defaults of tables outside `except_tables` that use any of `sequences`.
fn dependents(
    cassie: &Cassie,
    sequences: &[String],
    except_tables: &[&str],
) -> Vec<DefaultDependent> {
    let mut found = Vec::new();
    for collection in cassie.catalog.list_collections_canonical() {
        if except_tables
            .iter()
            .any(|table| name_matches(&collection.name, table))
        {
            continue;
        }
        for constraint in cassie.catalog.get_constraints(&collection.name) {
            let Some(used) = constraint.default_sequence.as_deref() else {
                continue;
            };
            if let Some(sequence) = sequences.iter().find(|name| name_matches(name, used)) {
                found.push(DefaultDependent {
                    table: collection.name.clone(),
                    column: constraint.field.clone(),
                    sequence: sequence.clone(),
                });
            }
        }
    }
    found
}

fn dependent_error(kind: &str, name: &str, dependents: &[DefaultDependent]) -> QueryError {
    let detail = dependents
        .iter()
        .map(|dependent| {
            format!(
                "default value for column {} of table {} depends on sequence {}",
                dependent.column, dependent.table, dependent.sequence
            )
        })
        .collect::<Vec<_>>()
        .join("; ");
    QueryError::Cassie(
        DiagnosticError::new(
            "2BP01",
            format!("cannot drop {kind} {name} because other objects depend on it"),
        )
        .with_detail(detail)
        .into(),
    )
}

/// Refuses `DROP SEQUENCE` while a table other than the sequence's owner has a
/// default that calls it. The owner's own default is not checked here.
pub(super) fn reject_for_sequence(cassie: &Cassie, sequence: &str) -> Result<(), QueryError> {
    let owner = cassie
        .catalog
        .get_sequence(sequence)
        .and_then(|metadata| metadata.owned_by)
        .map(|owner| owner.table);
    let excluded = owner.as_deref().into_iter().collect::<Vec<_>>();
    let found = dependents(cassie, &[sequence.to_owned()], &excluded);
    if found.is_empty() {
        Ok(())
    } else {
        Err(dependent_error("sequence", sequence, &found))
    }
}

/// Handles other tables' defaults on the sequences a dropped `table` owns:
/// refuses without `CASCADE`, and removes those defaults with it. Tables in
/// `dropped_together` go away in the same statement and do not count.
pub(super) fn resolve_for_table(
    cassie: &Cassie,
    table: &str,
    cascade: bool,
    dropped_together: &[&str],
) -> Result<(), QueryError> {
    let mut excluded = vec![table];
    excluded.extend_from_slice(dropped_together);
    let found = dependents(cassie, &owned_sequences(cassie, table), &excluded);
    if found.is_empty() {
        return Ok(());
    }
    if !cascade {
        let display_name = table.rsplit('.').next().unwrap_or(table);
        return Err(dependent_error("table", display_name, &found));
    }
    for dependent in found {
        super::sequence_command::alter_column_drop_default(
            cassie,
            &dependent.table,
            &dependent.column,
        )?;
    }
    Ok(())
}
