//! `pg_get_indexdef` and `pg_get_viewdef`: render a stored index or view
//! definition looked up by name or by its relation OID.

use super::{require_arg_count, require_arg_count_range, CassieSession, QueryError, Value};
use crate::types::object_id::FIRST_NORMAL_OBJECT_ID;

pub(super) fn evaluate_pg_get_indexdef(
    name: &str,
    args: &[Value],
    session: Option<&CassieSession>,
) -> Result<Value, QueryError> {
    require_arg_count(name, args, 1)?;
    let Some(argument) = args.first() else {
        return Err(QueryError::General(format!("{name} requires 1 arg")));
    };
    if matches!(argument, Value::Null) {
        return Ok(Value::Null);
    }
    let Some((catalog, _)) = session.and_then(CassieSession::object_lookup_context) else {
        return Ok(Value::Null);
    };
    let definition = match argument {
        Value::Int64(oid) => crate::catalog::virtual_views::index_definition_by_oid(&catalog, *oid),
        Value::String(index_name) => {
            crate::catalog::virtual_views::index_definition_by_name(&catalog, index_name)
        }
        _ => None,
    };
    Ok(definition.map_or(Value::Null, Value::String))
}

/// `pg_get_viewdef(view [, pretty | wrap_column])`. The view is named by text
/// or by its relation OID; the optional second argument only affects
/// PostgreSQL's pretty-printing, so the stored definition is returned as is.
pub(super) fn evaluate_pg_get_viewdef(
    name: &str,
    args: &[Value],
    session: Option<&CassieSession>,
) -> Result<Value, QueryError> {
    require_arg_count_range(name, args, 1..=2)?;
    let Some(argument) = args.first() else {
        return Err(QueryError::General(format!("{name} requires 1 arg")));
    };
    if args.iter().any(|value| matches!(value, Value::Null)) {
        return Ok(Value::Null);
    }
    let Some((catalog, context)) = session.and_then(CassieSession::object_lookup_context) else {
        return Ok(Value::Null);
    };
    let view = match argument {
        Value::Int64(oid) => {
            let object_id = u64::try_from(*oid)
                .ok()
                .and_then(|oid| oid.checked_sub(u64::from(FIRST_NORMAL_OBJECT_ID)));
            object_id.and_then(|object_id| {
                catalog
                    .list_views()
                    .into_iter()
                    .find(|view| view.object_id == object_id)
            })
        }
        Value::String(view_name) => {
            context
                .relation_search_order()
                .into_iter()
                .find_map(|schema| {
                    let mut scoped_context = context.clone();
                    scoped_context.search_path = vec![schema.to_owned()];
                    let canonical_name =
                        crate::sql::binder::normalize_relation_name(view_name, &scoped_context)
                            .ok()?;
                    catalog.get_view(&canonical_name)
                })
        }
        _ => None,
    };
    Ok(view.map_or(Value::Null, |view| {
        Value::String(format!(" {};", view.query.trim().trim_end_matches(';')))
    }))
}
