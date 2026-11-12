//! Casts into and out of `oid` and the `reg*` types.
use super::{CassieSession, DataType, QueryError, ScalarValue, Value};
use crate::catalog::object_identity::ObjectScope;
use crate::types::object_id::{oid_from_integer, parse_oid_text, ObjectIdentifierError};
use crate::types::ObjectIdKind;

/// Casts that need the catalog: text input to a `reg*` alias resolves a name,
/// and a `reg*` value cast to text renders one. Other casts return `None`.
pub(super) fn catalog_cast(
    value: &ScalarValue,
    target: &DataType,
    source: Option<&DataType>,
    session: Option<&CassieSession>,
) -> Result<Option<ScalarValue>, QueryError> {
    match (value, target, source) {
        (ScalarValue::Str(text), DataType::ObjectId(kind), _) if kind.is_alias() => {
            let oid = with_scope(session, |scope| scope.resolve_required(*kind, text))?;
            Ok(Some(ScalarValue::Int(i64::from(oid))))
        }
        (
            ScalarValue::Int(oid),
            DataType::Text | DataType::Varchar { .. } | DataType::Char { .. },
            Some(DataType::ObjectId(kind)),
        ) if kind.is_alias() => {
            let oid = u32::try_from(*oid).map_err(|_| {
                QueryError::Cassie(ObjectIdentifierError::OutOfRange(oid.to_string()).into())
            })?;
            let label = with_scope(session, |scope| Ok(scope.label(*kind, oid)))?;
            Ok(Some(ScalarValue::Str(label)))
        }
        _ => Ok(None),
    }
}

/// Runs `lookup` in the session's catalog scope.
pub(super) fn with_scope<T>(
    session: Option<&CassieSession>,
    lookup: impl FnOnce(&ObjectScope<'_>) -> Result<T, ObjectIdentifierError>,
) -> Result<T, QueryError> {
    let (catalog, context) = session
        .and_then(CassieSession::object_identifier_lookup_context)
        .ok_or_else(|| {
            QueryError::General("object identifier lookup requires a session catalog".to_string())
        })?;
    lookup(&ObjectScope::new(&catalog, &context)).map_err(|error| QueryError::Cassie(error.into()))
}

/// `to_regclass` and its siblings: a lookup that yields NULL instead of an
/// error for missing, ambiguous or malformed names.
pub(crate) fn evaluate_lookup(
    name: &str,
    args: &[Value],
    session: Option<&CassieSession>,
) -> Option<Result<Value, QueryError>> {
    if matches!(name, "pg_get_userbyid" | "pg_catalog.pg_get_userbyid") {
        return Some(user_by_oid(name, args, session));
    }
    let kind = crate::sql::functions::object_lookup_kind(name)?;
    Some(lookup(kind, args.first(), session))
}

/// Resolve actual catalog roles instead of manufacturing the bootstrap name.
fn user_by_oid(
    name: &str,
    args: &[Value],
    session: Option<&CassieSession>,
) -> Result<Value, QueryError> {
    let [value] = args else {
        return Err(QueryError::General(format!("{name} requires 1 arg")));
    };
    let oid = match value {
        Value::Null => return Ok(Value::Null),
        Value::Int64(value) => oid_from_integer(*value).map_err(|rejected| {
            QueryError::Cassie(ObjectIdentifierError::OutOfRange(rejected.to_string()).into())
        })?,
        Value::String(text) => {
            parse_oid_text(text).map_err(|error| QueryError::Cassie(error.into()))?
        }
        _ => {
            return Err(QueryError::Cassie(
                crate::app::DiagnosticError::new(
                    "42883",
                    "function pg_get_userbyid does not exist for this input type".to_string(),
                )
                .into(),
            ));
        }
    };
    let role = with_scope(session, |scope| Ok(scope.role_name(oid)))?;
    Ok(Value::String(
        role.unwrap_or_else(|| format!("unknown (OID={oid})")),
    ))
}

fn lookup(
    kind: ObjectIdKind,
    argument: Option<&Value>,
    session: Option<&CassieSession>,
) -> Result<Value, QueryError> {
    let Some(Value::String(text)) = argument else {
        return Ok(Value::Null);
    };
    let resolved = with_scope(session, |scope| {
        Ok(scope.resolve(kind, text).ok().flatten())
    })?;
    Ok(resolved.map_or(Value::Null, |oid| Value::Int64(i64::from(oid))))
}

/// Integer and numeric-text input share `oidin`'s range rules. Name input
/// for the `reg*` aliases is resolved before this point.
pub(super) fn cast(value: &ScalarValue) -> Result<ScalarValue, QueryError> {
    let oid = match value {
        ScalarValue::Int(value) => oid_from_integer(*value).map_err(|rejected| {
            QueryError::Cassie(ObjectIdentifierError::OutOfRange(rejected.to_string()).into())
        })?,
        ScalarValue::Str(text) => {
            parse_oid_text(text).map_err(|error| QueryError::Cassie(error.into()))?
        }
        ScalarValue::Float(_)
        | ScalarValue::Bool(_)
        | ScalarValue::Json(_)
        | ScalarValue::Typed(_)
        | ScalarValue::Array(_) => {
            return Err(QueryError::General("cannot cast value to oid".to_string()));
        }
        ScalarValue::Null => return Ok(ScalarValue::Null),
    };
    Ok(ScalarValue::Int(i64::from(oid)))
}
