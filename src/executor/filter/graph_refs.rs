//! `cassie_graph.vertex_ref` and `cassie_graph.edge_ref` (#1268).
//!
//! The graph and mapping are resolved by name in the session's catalog view
//! (including its transaction's overlay). The element must exist in the mapped
//! table as the session sees it, and the session must be allowed to read that
//! table: authority is checked before existence so a reference never reveals
//! whether a protected row exists.
use super::graph_functions::{descriptors_of, failure};
use super::{EvalContext, QueryError, Value};
use crate::app::CassieSession;
use crate::catalog::graph_resolution::ResolvedMapping;
use crate::midge::adapter::RowDecode;
use crate::midge::Midge;
use crate::sql::functions::FunctionId;
use crate::types::graph::{ElementKind, KeyPart};
use crate::types::typed::TypedValue;

pub(super) fn evaluate(
    id: FunctionId,
    args: &[Value],
    context: EvalContext<'_>,
) -> Result<Value, QueryError> {
    let kind = if id == FunctionId::GraphVertexRef {
        ElementKind::Vertex
    } else {
        ElementKind::Edge
    };
    let [graph, mapping, key_parts] = args else {
        return Err(failure(
            "42883",
            "graph element constructors take three arguments",
        ));
    };
    let (Value::String(graph), Value::String(mapping)) = (graph, mapping) else {
        return Err(null_or_mismatch(graph, mapping, key_parts));
    };
    let key = key_arguments(key_parts, context)?;
    let session = context
        .session
        .ok_or_else(|| failure("XX000", "cassie_graph functions require a session"))?;
    let (catalog, binding, midge) = session
        .sequence_context()
        .ok_or_else(|| failure("XX000", "cassie_graph functions require storage"))?;
    // An instance that does not enforce database scoping stores names unqualified; the
    // statement binder resolves those through an unscoped context, so the same is tried here.
    let unscoped = crate::sql::binder::BindingContext::unscoped(
        binding.database.clone(),
        binding.search_path.clone(),
    );
    let definition = [&binding, &unscoped]
        .into_iter()
        .find_map(|context| {
            crate::sql::binder::resolve_existing_name(graph, context, |candidate| {
                catalog.get_property_graph(candidate).is_some()
            })
            .ok()
            .and_then(|name| catalog.get_property_graph(&name))
        })
        .ok_or_else(|| {
            failure(
                "42704",
                format!("property graph \"{graph}\" does not exist"),
            )
        })?;
    let resolved =
        ResolvedMapping::by_alias(&definition, mapping, kind).map_err(QueryError::Cassie)?;
    resolved.check_key(&key).map_err(QueryError::Cassie)?;
    crate::app::require_table_read(&catalog, session, &binding, &resolved.table)
        .map_err(QueryError::Cassie)?;
    if !element_exists(session, &midge, &resolved, &key, context)? {
        return Err(failure(
            "42704",
            format!("{} does not exist in mapping \"{mapping}\"", kind.name()),
        ));
    }
    let descriptors = descriptors_of(Some(session))?;
    resolved
        .element(key)
        .to_value(&descriptors)
        .map_err(|message| failure("XX000", message))
}

/// `22004` for a NULL required argument, `42804` for a wrongly typed one.
fn null_or_mismatch(graph: &Value, mapping: &Value, key_parts: &Value) -> QueryError {
    if [graph, mapping, key_parts]
        .iter()
        .any(|value| value.is_null())
    {
        failure(
            "22004",
            "null value not allowed in a graph element reference",
        )
    } else {
        failure("42804", "graph and mapping names must be text")
    }
}

/// The typed key tuple of a `graph_key_part[]` argument.
fn key_arguments(key_parts: &Value, context: EvalContext<'_>) -> Result<Vec<KeyPart>, QueryError> {
    let Value::Array(array) = key_parts else {
        return Err(null_or_mismatch(
            &Value::Bool(true),
            &Value::Bool(true),
            key_parts,
        ));
    };
    array
        .elements()
        .iter()
        .map(|element| match element {
            Value::Typed(TypedValue::Record(record)) => KeyPart::from_record(record)
                .map_err(|error| failure("22023", error.message().to_owned())),
            // An `ARRAY[...]` of records reaches a function as record text.
            Value::String(text) => {
                let descriptors = descriptors_of(context.session)?;
                match crate::executor::filter::record_from_text(text, &descriptors.key_part)? {
                    Value::Typed(TypedValue::Record(record)) => KeyPart::from_record(&record)
                        .map_err(|error| failure("22023", error.message().to_owned())),
                    _ => Err(failure(
                        "42804",
                        "key parts must be cassie_graph.graph_key_part values",
                    )),
                }
            }
            Value::Null => Err(failure(
                "22004",
                "null key part in a graph element reference",
            )),
            _ => Err(failure(
                "42804",
                "key parts must be cassie_graph.graph_key_part values",
            )),
        })
        .collect()
}

fn matches(payload: &serde_json::Value, resolved: &ResolvedMapping, key: &[KeyPart]) -> bool {
    resolved.key.iter().zip(key).all(|(column, part)| {
        let Some(stored) = payload.get(&column.name) else {
            return false;
        };
        match part {
            KeyPart::SmallInt(value) => stored.as_i64() == Some(i64::from(*value)),
            KeyPart::Int(value) => stored.as_i64() == Some(i64::from(*value)),
            KeyPart::BigInt(value) => stored.as_i64() == Some(*value),
            KeyPart::Text(value) => stored.as_str() == Some(value.as_str()),
            KeyPart::Uuid(value) => stored
                .as_str()
                .and_then(crate::types::uuid_text::parse)
                .is_some_and(|stored| stored == *value),
        }
    })
}

/// Whether a row with `key` exists in the mapped table in the session's
/// transaction view: committed rows with the transaction's own changes applied.
fn element_exists(
    session: &CassieSession,
    midge: &Midge,
    resolved: &ResolvedMapping,
    key: &[KeyPart],
    context: EvalContext<'_>,
) -> Result<bool, QueryError> {
    let changes = session.collection_changes_matching(&resolved.table);
    let fallback;
    let controls = if let Some(controls) = context.controls {
        controls
    } else {
        fallback = crate::runtime::QueryExecutionControls::from_limits(
            &crate::config::CassieRuntimeLimits::default(),
            std::time::Instant::now(),
        );
        &fallback
    };
    let mut found = false;
    midge.scan_rows_until::<QueryError, _>(&resolved.table, RowDecode::Full, controls, |row| {
        // A row the transaction rewrote or deleted is judged by its change below.
        if !changes.contains_key(&row.id) && matches(&row.payload, resolved, key) {
            found = true;
            return Ok(false);
        }
        Ok(true)
    })?;
    Ok(found
        || changes.values().any(|change| {
            matches!(change, crate::app::TransactionRowChange::Upsert(payload) if matches(payload, resolved, key))
        }))
}
