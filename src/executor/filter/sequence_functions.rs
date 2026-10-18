//! `nextval`, `currval`, `setval` and `lastval`.
//!
//! The sequence argument is `regclass` input: a name resolved through the
//! search path, or an OID. Values persist atomically in storage; `currval`
//! and `lastval` read the calling session's history, as in PostgreSQL.
use super::{CassieSession, QueryError, Value};
use crate::app::CassieError;
use crate::catalog::object_identity::ObjectScope;
use crate::catalog::SequenceError;
use crate::types::ObjectIdKind;

pub(super) fn evaluate(
    name: &str,
    args: &[Value],
    session: Option<&CassieSession>,
) -> Option<Result<Value, QueryError>> {
    let local = name.strip_prefix("pg_catalog.").unwrap_or(name);
    let call = match local {
        "nextval" => nextval as fn(&[Value], &CassieSession) -> Result<Value, CassieError>,
        "currval" => currval,
        "setval" => setval,
        "lastval" => lastval,
        "pg_get_serial_sequence" => serial_sequence,
        _ => return None,
    };
    Some(
        session
            .ok_or_else(|| CassieError::Execution(format!("{local} requires a session")).into())
            .and_then(|session| call(args, session).map_err(QueryError::Cassie)),
    )
}

/// The canonical name and display name of the sequence `argument` names.
fn sequence_name(
    argument: &Value,
    session: &CassieSession,
) -> Result<(String, String), CassieError> {
    let (catalog, context, _) = session
        .sequence_context()
        .ok_or_else(|| CassieError::Execution("sequence functions require storage".into()))?;
    let scope = ObjectScope::new(&catalog, &context);
    let oid = match argument {
        Value::Int64(oid) => u32::try_from(*oid)
            .map_err(|_| CassieError::Execution(format!("invalid sequence OID {oid}")))?,
        Value::String(text) => scope.resolve_required(ObjectIdKind::RegClass, text)?,
        _ => {
            return Err(CassieError::Execution(
                "sequence argument must be a regclass".into(),
            ))
        }
    };
    let canonical = scope
        .relation_canonical_name(oid)
        .ok_or_else(|| CassieError::Execution(format!("could not open relation with OID {oid}")))?;
    let display = crate::catalog::local_name(&canonical);
    if !catalog.sequence_exists(&canonical) {
        return Err(CassieError::Sequence(SequenceError {
            sqlstate: "42809",
            message: format!("\"{display}\" is not a sequence"),
        }));
    }
    Ok((canonical, display))
}

fn nextval(args: &[Value], session: &CassieSession) -> Result<Value, CassieError> {
    let [argument] = args else {
        return Err(CassieError::Planner("nextval expects one argument".into()));
    };
    if argument.is_null() {
        return Ok(Value::Null);
    }
    let (sequence, _) = sequence_name(argument, session)?;
    let (_, _, midge) = session
        .sequence_context()
        .ok_or_else(|| CassieError::Execution("nextval requires storage".into()))?;
    let (value, updated) = midge
        .update_sequence_retrying(&sequence, |metadata| metadata.advance().map_err(Into::into))?;
    if let Some((catalog, _, _)) = session.sequence_context() {
        catalog.register_sequence(updated);
    }
    session.record_sequence_value(&sequence, value, true);
    Ok(Value::Int64(value))
}

fn currval(args: &[Value], session: &CassieSession) -> Result<Value, CassieError> {
    let [argument] = args else {
        return Err(CassieError::Planner("currval expects one argument".into()));
    };
    if argument.is_null() {
        return Ok(Value::Null);
    }
    let (sequence, display) = sequence_name(argument, session)?;
    session
        .current_sequence_value(&sequence, &display)
        .map(Value::Int64)
}

fn setval(args: &[Value], session: &CassieSession) -> Result<Value, CassieError> {
    let (argument, value, is_called) = match args {
        [argument, value] => (argument, value, true),
        [argument, value, Value::Bool(is_called)] => (argument, value, *is_called),
        [_, _, Value::Null] => return Ok(Value::Null),
        _ => {
            return Err(CassieError::Planner(
                "setval expects two or three arguments".into(),
            ))
        }
    };
    if argument.is_null() || value.is_null() {
        return Ok(Value::Null);
    }
    let value = value
        .as_i64()
        .ok_or_else(|| CassieError::Execution("setval value must be an integer".into()))?;
    let (sequence, _) = sequence_name(argument, session)?;
    let (catalog, _, midge) = session
        .sequence_context()
        .ok_or_else(|| CassieError::Execution("setval requires storage".into()))?;
    let (value, updated) = midge.update_sequence_retrying(&sequence, |metadata| {
        metadata.assign(value, is_called).map_err(Into::into)
    })?;
    catalog.register_sequence(updated);
    if is_called {
        session.record_sequence_value(&sequence, value, false);
    }
    Ok(Value::Int64(value))
}

fn lastval(args: &[Value], session: &CassieSession) -> Result<Value, CassieError> {
    if !args.is_empty() {
        return Err(CassieError::Planner("lastval expects no arguments".into()));
    }
    session.last_sequence_value().map(Value::Int64)
}

/// `pg_get_serial_sequence(table, column)`: the qualified name of the
/// sequence the column owns, or NULL.
fn serial_sequence(args: &[Value], session: &CassieSession) -> Result<Value, CassieError> {
    let [Value::String(table), Value::String(column)] = args else {
        return Ok(Value::Null);
    };
    let (catalog, context, _) = session
        .sequence_context()
        .ok_or_else(|| CassieError::Execution("pg_get_serial_sequence requires storage".into()))?;
    let scope = ObjectScope::new(&catalog, &context);
    let oid = scope.resolve_required(ObjectIdKind::RegClass, table)?;
    let Some(table) = scope.relation_canonical_name(oid) else {
        return Ok(Value::Null);
    };
    let column = column.trim_matches('"');
    let owned = catalog
        .list_sequences()
        .into_iter()
        .find(|sequence| {
            sequence.owned_by.as_ref().is_some_and(|owner| {
                qualified(&owner.table, &context.database).eq_ignore_ascii_case(&table)
                    && owner.column.eq_ignore_ascii_case(column)
            })
        })
        .map(|sequence| sequence.name)
        .or_else(|| {
            catalog
                .get_constraints(&table)
                .into_iter()
                .find(|constraint| {
                    constraint.field.eq_ignore_ascii_case(column)
                        && constraint.default_sequence_owned.is_owned()
                })
                .and_then(|constraint| constraint.default_sequence)
        });
    Ok(owned
        .map(|name| {
            catalog
                .get_sequence(&name)
                .map_or(name, |sequence| sequence.name)
        })
        .and_then(|name| {
            crate::catalog::RelationId::parse_canonical(&qualified(&name, &context.database))
        })
        .map_or(Value::Null, |relation| {
            Value::String(format!(
                "{}.{}",
                crate::catalog::object_identity::quote_identifier(&relation.schema),
                crate::catalog::object_identity::quote_identifier(&relation.name)
            ))
        }))
}

/// A relation name qualified to `database.schema.name`.
fn qualified(name: &str, database: &str) -> String {
    match crate::catalog::parse_name(name) {
        Ok(crate::catalog::ParsedName::SchemaQualified { schema, name }) => {
            crate::catalog::canonical_relation_name(database, &schema, &name)
        }
        Ok(crate::catalog::ParsedName::Unqualified(name)) => {
            crate::catalog::canonical_relation_name(database, crate::catalog::DEFAULT_SCHEMA, &name)
        }
        _ => name.to_string(),
    }
}
