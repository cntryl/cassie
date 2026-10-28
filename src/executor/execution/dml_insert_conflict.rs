//! Finding the existing row an `INSERT ... ON CONFLICT` proposal conflicts with.

use super::{json_to_value, Cassie, CassieSession, CollectionSchema, HashMap, QueryError, Value};

fn find_target_field_conflict_row_id(
    cassie: &Cassie,
    session: Option<&CassieSession>,
    table: &str,
    target_fields: &[String],
    object: &serde_json::Map<String, serde_json::Value>,
    schema: &CollectionSchema,
) -> Result<Option<String>, QueryError> {
    let mut values = Vec::with_capacity(target_fields.len());
    for field in target_fields {
        let Some(value) = object.get(field) else {
            return Ok(None);
        };
        if crate::app::field_value_is_sql_null(Some(value), Some(schema), field) {
            return Ok(None);
        }
        values.push((field.as_str(), value));
    }
    cassie
        .find_document_id_by_fields(session, table, &values, None)
        .map_err(QueryError::from)
}

pub(super) fn find_insert_conflict_row_id(
    cassie: &Cassie,
    session: Option<&CassieSession>,
    statement: &crate::sql::ast::InsertStatement,
    payload: &serde_json::Value,
    schema: &CollectionSchema,
) -> Result<Option<String>, QueryError> {
    let Some(on_conflict) = statement.on_conflict.as_ref() else {
        return Ok(None);
    };
    let row_schema = cassie
        .midge
        .row_schema(&statement.table)
        .map_err(QueryError::from)?;
    let mut canonical_payload = payload.clone();
    if let Some(object) = canonical_payload.as_object_mut() {
        for (field, value) in object {
            crate::executor::execution::index_probe_canonicalization::canonicalize_index_probe_value(
                cassie,
                &statement.table,
                &crate::sql::ColumnIdentifierPath::stored_field_key(field),
                value,
            );
        }
    }
    let object = canonical_payload
        .as_object()
        .ok_or_else(|| QueryError::General("document payload must be an object".to_string()))?;

    if let Some(name) = on_conflict.constraint.as_deref() {
        let arbiter = cassie
            .catalog
            .list_indexes(&statement.table)
            .into_iter()
            .find(|index| index.unique && index.name.eq_ignore_ascii_case(name));
        return match arbiter {
            Some(index) => find_expression_index_conflict(
                cassie,
                session,
                &statement.table,
                &index,
                payload,
                &row_schema,
            ),
            None => Ok(None),
        };
    }

    if !on_conflict.target_fields.is_empty() {
        return find_target_field_conflict_row_id(
            cassie,
            session,
            &statement.table,
            &on_conflict.target_fields,
            object,
            schema,
        );
    }

    find_any_unique_conflict(
        cassie,
        session,
        &statement.table,
        (object, payload),
        schema,
        &row_schema,
    )
}

pub(super) struct ExcludedArgs {
    pub(super) values: HashMap<String, Value>,
    _memory: crate::runtime::QueryMemoryReservation,
}

pub(super) fn excluded_local_args(
    payload: &serde_json::Value,
    schema: &CollectionSchema,
    controls: &crate::runtime::QueryExecutionControls,
) -> Result<ExcludedArgs, QueryError> {
    use crate::executor::retained_memory::{add, serialized_json_bytes};
    controls.check_interruption()?;
    let object = payload.as_object();
    let mut bytes = 0;
    if let Some(object) = object {
        for (field, value) in object {
            bytes = add(
                bytes,
                add(
                    excluded_entry_bytes(field)?,
                    crate::runtime::accounted::json::serialized_decode_bytes(
                        serialized_json_bytes(value)?,
                    )?,
                )?,
            )?;
        }
        for field in schema
            .fields
            .iter()
            .filter(|field| !object.contains_key(&field.name))
        {
            bytes = add(bytes, excluded_entry_bytes(&field.name)?)?;
        }
    }
    let memory = controls.reserve_query_memory(bytes)?;
    let mut out = HashMap::new();
    if let Some(object) = object {
        for field in schema
            .fields
            .iter()
            .filter(|field| !object.contains_key(&field.name))
        {
            let key = crate::sql::ColumnIdentifierPath::from_field_name(&field.name).lookup_key();
            out.insert(format!("excluded.{key}"), Value::Null);
        }
        for (field, value) in object {
            let value = schema
                .fields
                .iter()
                .find(|candidate| candidate.name == *field)
                .filter(|candidate| {
                    candidate.data_type.is_json()
                        && crate::types::json::requires_document_carrier(value)
                })
                .map_or_else(|| json_to_value(value), |_| Value::Json(value.clone()));
            let key = crate::sql::ColumnIdentifierPath::from_field_name(field).lookup_key();
            out.insert(format!("excluded.{key}"), value);
        }
    }
    Ok(ExcludedArgs {
        values: out,
        _memory: memory,
    })
}

fn excluded_entry_bytes(field: &str) -> Result<usize, crate::app::CassieError> {
    use crate::executor::retained_memory::{add, mul};
    add(
        mul(field.len(), 6)?,
        std::mem::size_of::<(String, Value)>() * 4 + 64,
    )
}

/// Finds the row an expression unique index says `payload` conflicts with,
/// by reading its committed reservation owner and reconciling staged rows.
pub(super) fn find_expression_index_conflict(
    cassie: &Cassie,
    session: Option<&CassieSession>,
    table: &str,
    index: &crate::catalog::IndexMeta,
    payload: &serde_json::Value,
    row_schema: &crate::midge::row_blob::RowSchema,
) -> Result<Option<String>, QueryError> {
    use crate::midge::adapter::Midge;
    if !Midge::payload_matches_scalar_index_predicate(index, payload, row_schema)? {
        return Ok(None);
    }
    let Some(key) = Midge::scalar_index_key_values(index, payload, row_schema)? else {
        return Ok(None);
    };
    let staged_snapshot = session.map(|session| session.staged_write_snapshot(table));
    let staged_changes = staged_snapshot
        .as_ref()
        .map(crate::app::StagedWriteSnapshot::ordered_changes);
    let committed_owner = cassie.midge.unique_scalar_index_reservation_owner(
        table,
        index.unique_reservation_name(),
        &key,
    )?;

    if let Some(owner_id) = committed_owner.as_ref() {
        match staged_changes.and_then(|changes| changes.get(owner_id)) {
            None => return Ok(Some(owner_id.clone())),
            Some(crate::app::TransactionRowChange::Upsert(owner_payload))
                if expression_index_key_matches(index, owner_payload, row_schema, &key)? =>
            {
                return Ok(Some(owner_id.clone()));
            }
            Some(_) => {}
        }
    }

    if let Some(changes) = staged_changes {
        for (id, change) in changes {
            let crate::app::TransactionRowChange::Upsert(staged_payload) = change else {
                continue;
            };
            if expression_index_key_matches(index, staged_payload, row_schema, &key)? {
                return Ok(Some(id.clone()));
            }
        }
    }
    Ok(None)
}

pub(super) fn expression_index_key_matches(
    index: &crate::catalog::IndexMeta,
    payload: &serde_json::Value,
    row_schema: &crate::midge::row_blob::RowSchema,
    key: &[serde_json::Value],
) -> Result<bool, QueryError> {
    use crate::midge::adapter::Midge;
    if !Midge::payload_matches_scalar_index_predicate(index, payload, row_schema)? {
        return Ok(false);
    }
    Ok(Midge::scalar_index_key_values(index, payload, row_schema)?.as_deref() == Some(key))
}

/// The first row conflicting on any unique key, for an untargeted
/// `ON CONFLICT`; `payload` is the proposal, `object` its canonical probe form.
fn find_any_unique_conflict(
    cassie: &Cassie,
    session: Option<&CassieSession>,
    table: &str,
    (object, payload): (
        &serde_json::Map<String, serde_json::Value>,
        &serde_json::Value,
    ),
    schema: &CollectionSchema,
    row_schema: &crate::midge::row_blob::RowSchema,
) -> Result<Option<String>, QueryError> {
    let constraints = cassie.catalog.get_constraints(table);
    for constraint in &constraints {
        if !crate::catalog::enforces_single_column_uniqueness(constraint, &constraints) {
            continue;
        }
        let Some(value) = object.get(&constraint.field) else {
            continue;
        };
        if crate::app::field_value_is_sql_null(Some(value), Some(schema), &constraint.field) {
            continue;
        }
        if let Some(id) = cassie
            .find_document_id_by_fields(session, table, &[(&constraint.field, value)], None)
            .map_err(QueryError::from)?
        {
            return Ok(Some(id));
        }
    }

    for index in cassie.catalog.list_indexes(table) {
        if !index.unique || index.kind != crate::catalog::IndexKind::Scalar {
            continue;
        }
        let fields = index.normalized_fields();
        if fields.is_empty() {
            if let Some(id) =
                find_expression_index_conflict(cassie, session, table, &index, payload, row_schema)?
            {
                return Ok(Some(id));
            }
            continue;
        }
        let mut values = Vec::with_capacity(fields.len());
        let mut complete = true;
        for field in &fields {
            let Some(value) = object.get(field) else {
                complete = false;
                break;
            };
            if crate::app::field_value_is_sql_null(Some(value), Some(schema), field) {
                complete = false;
                break;
            }
            values.push((field.as_str(), value));
        }
        if !complete {
            continue;
        }
        if let Some(id) = cassie
            .find_document_id_by_fields(session, table, &values, None)
            .map_err(QueryError::from)?
        {
            return Ok(Some(id));
        }
    }

    Ok(None)
}

#[cfg(test)]
#[path = "insert_excluded_tests.rs"]
mod tests;
