use std::collections::BTreeSet;

use super::{Cassie, CassieSession, QueryError};

pub(super) fn preflight_delete_actions(
    cassie: &Cassie,
    session: Option<&CassieSession>,
    table: &str,
    payload: &serde_json::Value,
) -> Result<(), QueryError> {
    let Some(session) = session.filter(|session| session.is_transaction_active()) else {
        return Ok(());
    };

    let mut collections = BTreeSet::from([table.to_string()]);
    let mut visited = BTreeSet::new();
    collect_delete_action_collections(
        cassie,
        session,
        table,
        payload,
        &mut collections,
        &mut visited,
    )?;
    let collections = collections.into_iter().collect::<Vec<_>>();
    session
        .preflight_transaction_collections(&collections)
        .map_err(QueryError::from)
}

pub(super) fn preflight_update_actions(
    cassie: &Cassie,
    session: &CassieSession,
    table: &str,
    before: &serde_json::Value,
    after: &serde_json::Value,
    collections: &mut BTreeSet<String>,
) -> Result<(), QueryError> {
    let (Some(before), Some(after)) = (before.as_object(), after.as_object()) else {
        return Ok(());
    };

    for (child_table, constraint) in referencing_constraints(cassie, table) {
        let Some(reference_field) = constraint.references_field.as_deref() else {
            continue;
        };
        let old_value = before.get(reference_field);
        let new_value = after.get(reference_field);
        if old_value == new_value {
            continue;
        }
        let Some(old_value) = old_value else {
            continue;
        };
        if old_value.is_null() {
            continue;
        }
        let child_rows = referencing_child_rows(
            cassie,
            Some(session),
            &child_table,
            &constraint.field,
            old_value,
        )?;
        if child_rows.is_empty() {
            continue;
        }

        if matches!(
            foreign_key_action(constraint.foreign_key_on_update.as_deref()),
            ForeignKeyAction::Cascade | ForeignKeyAction::SetNull | ForeignKeyAction::SetDefault
        ) {
            collections.insert(child_table);
        }
    }

    Ok(())
}

fn collect_delete_action_collections(
    cassie: &Cassie,
    session: &CassieSession,
    table: &str,
    payload: &serde_json::Value,
    collections: &mut BTreeSet<String>,
    visited: &mut BTreeSet<(String, String)>,
) -> Result<(), QueryError> {
    let key = (table.to_string(), payload.to_string());
    if !visited.insert(key) {
        return Ok(());
    }
    let Some(object) = payload.as_object() else {
        return Ok(());
    };

    for (child_table, constraint) in referencing_constraints(cassie, table) {
        let Some(reference_field) = constraint.references_field.as_deref() else {
            continue;
        };
        let Some(parent_value) = object.get(reference_field) else {
            continue;
        };
        if parent_value.is_null() {
            continue;
        }
        let child_rows = referencing_child_rows(
            cassie,
            Some(session),
            &child_table,
            &constraint.field,
            parent_value,
        )?;
        if child_rows.is_empty() {
            continue;
        }

        match foreign_key_action(constraint.foreign_key_on_delete.as_deref()) {
            ForeignKeyAction::Cascade => {
                collections.insert(child_table.clone());
                for child in child_rows {
                    collect_delete_action_collections(
                        cassie,
                        session,
                        &child_table,
                        &child.payload,
                        collections,
                        visited,
                    )?;
                }
            }
            ForeignKeyAction::SetNull | ForeignKeyAction::SetDefault => {
                collections.insert(child_table);
            }
            ForeignKeyAction::NoAction | ForeignKeyAction::Restrict => {}
        }
    }

    Ok(())
}

pub(super) fn assert_no_referencing_rows(
    cassie: &Cassie,
    session: Option<&CassieSession>,
    table: &str,
    payload: &serde_json::Value,
) -> Result<(), QueryError> {
    let Some(object) = payload.as_object() else {
        return Ok(());
    };

    for (child_table, constraint) in referencing_constraints(cassie, table) {
        let Some(reference_field) = constraint.references_field.as_deref() else {
            continue;
        };
        let Some(parent_value) = object.get(reference_field) else {
            continue;
        };
        if parent_value.is_null() {
            continue;
        }

        let child_rows = referencing_child_rows(
            cassie,
            session,
            &child_table,
            &constraint.field,
            parent_value,
        )?;
        if child_rows.is_empty() {
            continue;
        }

        match foreign_key_action(constraint.foreign_key_on_delete.as_deref()) {
            ForeignKeyAction::Cascade => {
                for child in child_rows {
                    assert_no_referencing_rows(cassie, session, &child_table, &child.payload)?;
                    cassie
                        .delete_document_for_session(session, &child_table, &child.id)
                        .map_err(QueryError::from)?;
                }
            }
            ForeignKeyAction::SetNull | ForeignKeyAction::SetDefault => {
                let value = action_update_value(
                    foreign_key_action(constraint.foreign_key_on_delete.as_deref()),
                    &constraint,
                );
                set_child_reference_values(
                    cassie,
                    session,
                    &child_table,
                    &constraint.field,
                    child_rows,
                    &value,
                )?;
            }
            ForeignKeyAction::NoAction | ForeignKeyAction::Restrict => {
                return Err(referenced_row_error(
                    &constraint.field,
                    &child_table,
                    table,
                    reference_field,
                ));
            }
        }
    }

    Ok(())
}

pub(super) fn assert_referenced_values_can_change(
    cassie: &Cassie,
    session: Option<&CassieSession>,
    table: &str,
    before: &serde_json::Value,
    after: &serde_json::Value,
) -> Result<(), QueryError> {
    let (Some(before), Some(after)) = (before.as_object(), after.as_object()) else {
        return Ok(());
    };

    for (child_table, constraint) in referencing_constraints(cassie, table) {
        let Some(reference_field) = constraint.references_field.as_deref() else {
            continue;
        };
        let old_value = before.get(reference_field);
        let new_value = after.get(reference_field);
        if old_value == new_value {
            continue;
        }
        let Some(old_value) = old_value else {
            continue;
        };
        if old_value.is_null() {
            continue;
        }
        let child_rows =
            referencing_child_rows(cassie, session, &child_table, &constraint.field, old_value)?;
        if child_rows.is_empty() {
            continue;
        }

        match foreign_key_action(constraint.foreign_key_on_update.as_deref()) {
            ForeignKeyAction::Cascade
            | ForeignKeyAction::SetNull
            | ForeignKeyAction::SetDefault => {}
            ForeignKeyAction::NoAction | ForeignKeyAction::Restrict => {
                return Err(referenced_row_error(
                    &constraint.field,
                    &child_table,
                    table,
                    reference_field,
                ));
            }
        }
    }

    Ok(())
}

pub(super) fn apply_referenced_update_actions(
    cassie: &Cassie,
    session: Option<&CassieSession>,
    table: &str,
    before: &serde_json::Value,
    after: &serde_json::Value,
) -> Result<(), QueryError> {
    let (Some(before), Some(after)) = (before.as_object(), after.as_object()) else {
        return Ok(());
    };

    for (child_table, constraint) in referencing_constraints(cassie, table) {
        let Some(reference_field) = constraint.references_field.as_deref() else {
            continue;
        };
        let old_value = before.get(reference_field);
        let new_value = after.get(reference_field);
        if old_value == new_value {
            continue;
        }
        let Some(old_value) = old_value else {
            continue;
        };
        if old_value.is_null() {
            continue;
        }
        let child_rows =
            referencing_child_rows(cassie, session, &child_table, &constraint.field, old_value)?;
        if child_rows.is_empty() {
            continue;
        }

        match foreign_key_action(constraint.foreign_key_on_update.as_deref()) {
            ForeignKeyAction::Cascade => {
                let Some(new_value) = new_value else {
                    continue;
                };
                set_child_reference_values(
                    cassie,
                    session,
                    &child_table,
                    &constraint.field,
                    child_rows,
                    new_value,
                )?;
            }
            ForeignKeyAction::SetNull | ForeignKeyAction::SetDefault => {
                let value = action_update_value(
                    foreign_key_action(constraint.foreign_key_on_update.as_deref()),
                    &constraint,
                );
                set_child_reference_values(
                    cassie,
                    session,
                    &child_table,
                    &constraint.field,
                    child_rows,
                    &value,
                )?;
            }
            ForeignKeyAction::NoAction | ForeignKeyAction::Restrict => {}
        }
    }

    Ok(())
}

fn referencing_constraints(
    cassie: &Cassie,
    referenced_table: &str,
) -> Vec<(String, crate::catalog::FieldConstraint)> {
    let mut out = Vec::new();
    for collection in cassie.catalog.list_collections_canonical() {
        for constraint in cassie.catalog.get_constraints(&collection.name) {
            if constraint
                .references_table
                .as_deref()
                .is_some_and(|table| table.eq_ignore_ascii_case(referenced_table))
            {
                out.push((collection.name.clone(), constraint));
            }
        }
    }
    out
}

#[derive(Clone, Copy)]
enum ForeignKeyAction {
    Cascade,
    SetNull,
    SetDefault,
    NoAction,
    Restrict,
}

fn foreign_key_action(raw: Option<&str>) -> ForeignKeyAction {
    match raw.unwrap_or("NO ACTION").to_ascii_uppercase().as_str() {
        "CASCADE" => ForeignKeyAction::Cascade,
        "SET NULL" => ForeignKeyAction::SetNull,
        "SET DEFAULT" => ForeignKeyAction::SetDefault,
        "RESTRICT" => ForeignKeyAction::Restrict,
        _ => ForeignKeyAction::NoAction,
    }
}

fn action_update_value(
    action: ForeignKeyAction,
    constraint: &crate::catalog::FieldConstraint,
) -> serde_json::Value {
    if matches!(action, ForeignKeyAction::SetDefault) {
        constraint
            .default_value
            .clone()
            .unwrap_or(serde_json::Value::Null)
    } else {
        serde_json::Value::Null
    }
}

fn referencing_child_rows(
    cassie: &Cassie,
    session: Option<&CassieSession>,
    child_table: &str,
    child_field: &str,
    parent_value: &serde_json::Value,
) -> Result<Vec<crate::midge::adapter::DocumentRef>, QueryError> {
    let rows = cassie
        .scan_documents_batched_for_session(session, child_table, 1024)
        .map_err(QueryError::from)?
        .into_iter()
        .flatten()
        .filter(|document| document.payload.get(child_field) == Some(parent_value))
        .collect::<Vec<_>>();
    Ok(rows)
}

fn set_child_reference_values(
    cassie: &Cassie,
    session: Option<&CassieSession>,
    child_table: &str,
    child_field: &str,
    child_rows: Vec<crate::midge::adapter::DocumentRef>,
    value: &serde_json::Value,
) -> Result<(), QueryError> {
    for child in child_rows {
        let mut payload =
            child.payload.as_object().cloned().ok_or_else(|| {
                QueryError::General("stored row payload must be object".to_string())
            })?;
        payload.insert(child_field.to_string(), value.clone());
        let payload = serde_json::Value::Object(payload);
        let payload = cassie
            .prepare_document_write_for_session(
                session,
                child_table,
                payload,
                true,
                Some(&child.id),
            )
            .map_err(QueryError::from)?;
        cassie
            .put_prepared_document_for_session(session, child_table, child.id, payload)
            .map_err(QueryError::from)?;
    }
    Ok(())
}

fn referenced_row_error(
    child_field: &str,
    child_table: &str,
    table: &str,
    reference_field: &str,
) -> QueryError {
    QueryError::General(format!(
        "foreign key constraint '{child_field}' on '{child_table}' still references '{table}.{reference_field}'"
    ))
}
