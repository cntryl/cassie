use std::collections::BTreeSet;

use super::{check_timeout, Cassie, CassieSession, QueryError, QueryExecutionControls};

fn stored_value_is_sql_null(
    cassie: &Cassie,
    collection: &str,
    field: &str,
    value: &serde_json::Value,
) -> bool {
    let schema = cassie.catalog.get_schema(collection);
    crate::app::field_value_is_sql_null(Some(value), schema.as_ref(), field)
}

pub(super) fn delete_document_with_referential_actions(
    cassie: &Cassie,
    table: &str,
    row_id: &str,
    payload: &serde_json::Value,
    cancellation: crate::runtime::QueryCancellationHandle,
) -> Result<bool, crate::app::CassieError> {
    let controls = QueryExecutionControls::with_cancellation(
        &cassie.runtime.limits(),
        std::time::Instant::now(),
        cancellation,
    );
    match delete_existing_row(cassie, None, table, row_id, payload, &controls)
        .map_err(crate::app::CassieError::from)?
    {
        DeleteOutcome::Deleted(deleted) => Ok(deleted),
        DeleteOutcome::Restricted(error) => Err(error.into()),
    }
}

pub(super) enum DeleteOutcome {
    Deleted(bool),
    Restricted(QueryError),
}

pub(super) fn delete_existing_row(
    cassie: &Cassie,
    session: Option<&CassieSession>,
    table: &str,
    row_id: &str,
    payload: &serde_json::Value,
    controls: &QueryExecutionControls,
) -> Result<DeleteOutcome, QueryError> {
    preflight_delete_actions(cassie, session, table, payload, controls)?;
    let mut visited = BTreeSet::new();
    if let Some(error) = find_delete_restriction(
        cassie,
        session,
        table,
        row_id,
        payload,
        &mut visited,
        controls,
    )? {
        return Ok(DeleteOutcome::Restricted(error));
    }
    assert_no_referencing_rows(cassie, session, table, row_id, payload, controls)?;
    check_timeout(controls)?;
    let deleted = cassie
        .delete_document_for_session(session, table, row_id)
        .map_err(QueryError::from)?;
    Ok(DeleteOutcome::Deleted(deleted))
}

fn preflight_delete_actions(
    cassie: &Cassie,
    session: Option<&CassieSession>,
    table: &str,
    payload: &serde_json::Value,
    controls: &QueryExecutionControls,
) -> Result<(), QueryError> {
    check_timeout(controls)?;
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
        controls,
    )?;
    let collections = collections.into_iter().collect::<Vec<_>>();
    session
        .preflight_transaction_collections(&collections)
        .map_err(QueryError::from)
}

fn preflight_update_actions(
    cassie: &Cassie,
    session: &CassieSession,
    table: &str,
    before: &serde_json::Value,
    after: &serde_json::Value,
    collections: &mut BTreeSet<String>,
    controls: &QueryExecutionControls,
) -> Result<(), QueryError> {
    check_timeout(controls)?;
    let (Some(before), Some(after)) = (before.as_object(), after.as_object()) else {
        return Ok(());
    };

    for (child_table, constraint) in referencing_constraints(cassie, table) {
        check_timeout(controls)?;
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
        if stored_value_is_sql_null(cassie, table, reference_field, old_value) {
            continue;
        }
        let child_rows = referencing_child_rows(
            cassie,
            Some(session),
            &child_table,
            &constraint.field,
            old_value,
            controls,
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

fn find_delete_restriction(
    cassie: &Cassie,
    session: Option<&CassieSession>,
    table: &str,
    row_id: &str,
    payload: &serde_json::Value,
    visited: &mut BTreeSet<(String, String)>,
    controls: &QueryExecutionControls,
) -> Result<Option<QueryError>, QueryError> {
    check_timeout(controls)?;
    if !visited.insert((table.to_string(), row_id.to_string())) {
        return Ok(None);
    }
    let Some(object) = payload.as_object() else {
        return Ok(None);
    };

    for (child_table, constraint) in referencing_constraints(cassie, table) {
        check_timeout(controls)?;
        let Some(reference_field) = constraint.references_field.as_deref() else {
            continue;
        };
        let Some(parent_value) = object.get(reference_field) else {
            continue;
        };
        if stored_value_is_sql_null(cassie, table, reference_field, parent_value) {
            continue;
        }
        let child_rows = referencing_child_rows(
            cassie,
            session,
            &child_table,
            &constraint.field,
            parent_value,
            controls,
        )?;
        let child_rows = without_deleted_row(child_rows, &child_table, table, row_id);
        if child_rows.is_empty() {
            continue;
        }

        match foreign_key_action(constraint.foreign_key_on_delete.as_deref()) {
            ForeignKeyAction::Cascade => {
                for child in child_rows {
                    if let Some(error) = find_delete_restriction(
                        cassie,
                        session,
                        &child_table,
                        &child.id,
                        &child.payload,
                        visited,
                        controls,
                    )? {
                        return Ok(Some(error));
                    }
                }
            }
            ForeignKeyAction::SetNull | ForeignKeyAction::SetDefault => {}
            ForeignKeyAction::NoAction | ForeignKeyAction::Restrict => {
                return Ok(Some(referenced_row_error(
                    &constraint.foreign_key_constraint_name(&child_table),
                    &child_table,
                    table,
                    reference_field,
                )));
            }
        }
    }

    Ok(None)
}

fn collect_delete_action_collections(
    cassie: &Cassie,
    session: &CassieSession,
    table: &str,
    payload: &serde_json::Value,
    collections: &mut BTreeSet<String>,
    visited: &mut BTreeSet<(String, String)>,
    controls: &QueryExecutionControls,
) -> Result<(), QueryError> {
    check_timeout(controls)?;
    let key = (table.to_string(), payload.to_string());
    if !visited.insert(key) {
        return Ok(());
    }
    let Some(object) = payload.as_object() else {
        return Ok(());
    };

    for (child_table, constraint) in referencing_constraints(cassie, table) {
        check_timeout(controls)?;
        let Some(reference_field) = constraint.references_field.as_deref() else {
            continue;
        };
        let Some(parent_value) = object.get(reference_field) else {
            continue;
        };
        if stored_value_is_sql_null(cassie, table, reference_field, parent_value) {
            continue;
        }
        let child_rows = referencing_child_rows(
            cassie,
            Some(session),
            &child_table,
            &constraint.field,
            parent_value,
            controls,
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
                        controls,
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

fn assert_no_referencing_rows(
    cassie: &Cassie,
    session: Option<&CassieSession>,
    table: &str,
    row_id: &str,
    payload: &serde_json::Value,
    controls: &QueryExecutionControls,
) -> Result<(), QueryError> {
    let mut visited = BTreeSet::new();
    apply_delete_actions(
        cassie,
        session,
        table,
        row_id,
        payload,
        &mut visited,
        controls,
    )
}

fn apply_delete_actions(
    cassie: &Cassie,
    session: Option<&CassieSession>,
    table: &str,
    row_id: &str,
    payload: &serde_json::Value,
    visited: &mut BTreeSet<(String, String)>,
    controls: &QueryExecutionControls,
) -> Result<(), QueryError> {
    check_timeout(controls)?;
    let key = (table.to_string(), row_id.to_string());
    if !visited.insert(key) {
        return Ok(());
    }
    let Some(object) = payload.as_object() else {
        return Ok(());
    };

    for (child_table, constraint) in referencing_constraints(cassie, table) {
        check_timeout(controls)?;
        let Some(reference_field) = constraint.references_field.as_deref() else {
            continue;
        };
        let Some(parent_value) = object.get(reference_field) else {
            continue;
        };
        if stored_value_is_sql_null(cassie, table, reference_field, parent_value) {
            continue;
        }

        let child_rows = referencing_child_rows(
            cassie,
            session,
            &child_table,
            &constraint.field,
            parent_value,
            controls,
        )?;
        let child_rows = without_deleted_row(child_rows, &child_table, table, row_id);
        if child_rows.is_empty() {
            continue;
        }

        match foreign_key_action(constraint.foreign_key_on_delete.as_deref()) {
            ForeignKeyAction::Cascade => {
                for child in child_rows {
                    check_timeout(controls)?;
                    apply_delete_actions(
                        cassie,
                        session,
                        &child_table,
                        &child.id,
                        &child.payload,
                        visited,
                        controls,
                    )?;
                    check_timeout(controls)?;
                    cassie
                        .delete_document_for_session(session, &child_table, &child.id)
                        .map_err(QueryError::from)?;
                }
            }
            ForeignKeyAction::SetNull | ForeignKeyAction::SetDefault => {
                let action = foreign_key_action(constraint.foreign_key_on_delete.as_deref());
                let value = action_update_value(action, &constraint);
                let update = if matches!(action, ForeignKeyAction::SetNull) {
                    ChildReferenceUpdate::SqlNull
                } else {
                    ChildReferenceUpdate::Value(&value)
                };
                set_child_reference_values(
                    cassie,
                    session,
                    &child_table,
                    &constraint.field,
                    child_rows,
                    update,
                    controls,
                )?;
            }
            ForeignKeyAction::NoAction | ForeignKeyAction::Restrict => {
                return Err(referenced_row_error(
                    &constraint.foreign_key_constraint_name(&child_table),
                    &child_table,
                    table,
                    reference_field,
                ));
            }
        }
    }

    Ok(())
}

fn assert_referenced_values_can_change(
    cassie: &Cassie,
    session: Option<&CassieSession>,
    table: &str,
    before: &serde_json::Value,
    after: &serde_json::Value,
    controls: &QueryExecutionControls,
) -> Result<(), QueryError> {
    check_timeout(controls)?;
    let (Some(before), Some(after)) = (before.as_object(), after.as_object()) else {
        return Ok(());
    };

    for (child_table, constraint) in referencing_constraints(cassie, table) {
        check_timeout(controls)?;
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
        if stored_value_is_sql_null(cassie, table, reference_field, old_value) {
            continue;
        }
        let child_rows = referencing_child_rows(
            cassie,
            session,
            &child_table,
            &constraint.field,
            old_value,
            controls,
        )?;
        if child_rows.is_empty() {
            continue;
        }

        match foreign_key_action(constraint.foreign_key_on_update.as_deref()) {
            ForeignKeyAction::Cascade
            | ForeignKeyAction::SetNull
            | ForeignKeyAction::SetDefault => {}
            ForeignKeyAction::NoAction | ForeignKeyAction::Restrict => {
                return Err(referenced_row_error(
                    &constraint.foreign_key_constraint_name(&child_table),
                    &child_table,
                    table,
                    reference_field,
                ));
            }
        }
    }

    Ok(())
}

fn apply_referenced_update_actions(
    cassie: &Cassie,
    session: Option<&CassieSession>,
    table: &str,
    before: &serde_json::Value,
    after: &serde_json::Value,
    controls: &QueryExecutionControls,
) -> Result<(), QueryError> {
    check_timeout(controls)?;
    let (Some(before), Some(after)) = (before.as_object(), after.as_object()) else {
        return Ok(());
    };

    for (child_table, constraint) in referencing_constraints(cassie, table) {
        check_timeout(controls)?;
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
        if stored_value_is_sql_null(cassie, table, reference_field, old_value) {
            continue;
        }
        let child_rows = referencing_child_rows(
            cassie,
            session,
            &child_table,
            &constraint.field,
            old_value,
            controls,
        )?;
        if child_rows.is_empty() {
            continue;
        }

        match foreign_key_action(constraint.foreign_key_on_update.as_deref()) {
            ForeignKeyAction::Cascade => {
                if let Some(new_value) = new_value {
                    set_child_reference_values(
                        cassie,
                        session,
                        &child_table,
                        &constraint.field,
                        child_rows,
                        ChildReferenceUpdate::Value(new_value),
                        controls,
                    )?;
                } else {
                    set_child_reference_values(
                        cassie,
                        session,
                        &child_table,
                        &constraint.field,
                        child_rows,
                        ChildReferenceUpdate::SqlNull,
                        controls,
                    )?;
                }
            }
            ForeignKeyAction::SetNull | ForeignKeyAction::SetDefault => {
                let action = foreign_key_action(constraint.foreign_key_on_update.as_deref());
                let value = action_update_value(action, &constraint);
                let update = if matches!(action, ForeignKeyAction::SetNull) {
                    ChildReferenceUpdate::SqlNull
                } else {
                    ChildReferenceUpdate::Value(&value)
                };
                set_child_reference_values(
                    cassie,
                    session,
                    &child_table,
                    &constraint.field,
                    child_rows,
                    update,
                    controls,
                )?;
            }
            ForeignKeyAction::NoAction | ForeignKeyAction::Restrict => {}
        }
    }

    Ok(())
}

pub(super) fn update_existing_row(
    cassie: &Cassie,
    session: Option<&CassieSession>,
    table: &str,
    row_id: &str,
    before: &serde_json::Value,
    after: serde_json::Value,
    controls: &QueryExecutionControls,
) -> Result<crate::midge::adapter::DocumentRef, QueryError> {
    assert_referenced_values_can_change(cassie, session, table, before, &after, controls)?;
    if let Some(session) = session.filter(|session| session.is_transaction_active()) {
        let mut collections = BTreeSet::from([table.to_string()]);
        preflight_update_actions(
            cassie,
            session,
            table,
            before,
            &after,
            &mut collections,
            controls,
        )?;
        let collections = collections.into_iter().collect::<Vec<_>>();
        session
            .preflight_transaction_collections(&collections)
            .map_err(QueryError::from)?;
    }
    cassie
        .put_prepared_document_for_session(session, table, row_id.to_string(), after)
        .map_err(QueryError::from)?;
    let document = cassie
        .get_document_for_session(session, table, row_id)
        .map_err(QueryError::from)?
        .ok_or_else(|| {
            QueryError::General(format!("updated row '{row_id}' was not found in '{table}'"))
        })?;
    apply_referenced_update_actions(cassie, session, table, before, &document.payload, controls)?;
    Ok(document)
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
    controls: &QueryExecutionControls,
) -> Result<Vec<crate::midge::adapter::DocumentRef>, QueryError> {
    check_timeout(controls)?;
    let batches = cassie
        .scan_documents_batched_for_session(session, child_table, 1024)
        .map_err(QueryError::from)?;
    let mut rows = Vec::new();
    for batch in batches {
        check_timeout(controls)?;
        for document in batch {
            check_timeout(controls)?;
            if document.payload.get(child_field) == Some(parent_value) {
                rows.push(document);
            }
        }
    }
    Ok(rows)
}

/// Drops the row being deleted from its own referencing rows: a row that
/// references itself is removed with the parent and does not block it.
fn without_deleted_row(
    child_rows: Vec<crate::midge::adapter::DocumentRef>,
    child_table: &str,
    table: &str,
    row_id: &str,
) -> Vec<crate::midge::adapter::DocumentRef> {
    if !crate::catalog::name_matches(child_table, table) {
        return child_rows;
    }
    child_rows
        .into_iter()
        .filter(|child| child.id != row_id)
        .collect()
}

#[derive(Clone, Copy)]
enum ChildReferenceUpdate<'a> {
    Value(&'a serde_json::Value),
    SqlNull,
}

fn set_child_reference_values(
    cassie: &Cassie,
    session: Option<&CassieSession>,
    child_table: &str,
    child_field: &str,
    child_rows: Vec<crate::midge::adapter::DocumentRef>,
    update: ChildReferenceUpdate<'_>,
    controls: &QueryExecutionControls,
) -> Result<(), QueryError> {
    let sql_null = serde_json::Value::Null;
    let (value, set_sql_null) = match update {
        ChildReferenceUpdate::Value(value) => (value, false),
        ChildReferenceUpdate::SqlNull => (&sql_null, true),
    };
    for child in child_rows {
        check_timeout(controls)?;
        let mut payload =
            child.payload.as_object().cloned().ok_or_else(|| {
                QueryError::General("stored row payload must be object".to_string())
            })?;
        let child_schema = cassie.catalog.get_schema(child_table);
        let json_field = child_schema.as_ref().and_then(|schema| {
            schema
                .fields
                .iter()
                .find(|field| field.name.eq_ignore_ascii_case(child_field))
        });
        if set_sql_null
            && json_field
                .is_some_and(|field| matches!(field.data_type, crate::types::DataType::Json))
        {
            if let Some(field_name) = json_field.map(|field| field.name.as_str()) {
                payload.remove(field_name);
            }
        } else if let Some(field_name) = json_field.map(|field| field.name.as_str()) {
            payload.insert(field_name.to_string(), value.clone());
        } else {
            payload.insert(child_field.to_string(), value.clone());
        }
        let mut payload = serde_json::Value::Object(payload);
        cassie.discard_stale_vector_embeddings(child_table, &mut payload, [child_field]);
        let payload = cassie
            .prepare_document_write_for_session(
                session,
                child_table,
                payload,
                false,
                Some(&child.id),
            )
            .map_err(QueryError::from)?;
        update_existing_row(
            cassie,
            session,
            child_table,
            &child.id,
            &child.payload,
            payload,
            controls,
        )?;
    }
    Ok(())
}

fn referenced_row_error(
    constraint_name: &str,
    child_table: &str,
    table: &str,
    reference_field: &str,
) -> QueryError {
    QueryError::General(format!(
        "foreign key constraint '{constraint_name}' on '{child_table}' still references '{table}.{reference_field}'"
    ))
}
