//! `cassie_graph.vertex_ref` and `cassie_graph.edge_ref` (#1268).
//!
//! The graph and mapping are resolved by name in the session's catalog view
//! (including its transaction's overlay). The element must exist in the mapped
//! table as the session sees it (an edge also needs both endpoint vertices: a
//! dangling edge row is not yet an edge, #1351), and the session must be allowed to read that
//! table: authority is checked before existence so a reference never reveals
//! whether a protected row exists.
use super::graph_functions::{descriptors_of, failure};
use super::{EvalContext, QueryError, Value};
use crate::app::CassieSession;
use crate::catalog::graph_resolution::ResolvedMapping;
use crate::catalog::property_graphs::PropertyGraphMeta;
use crate::midge::adapter::RowDecode;
use crate::midge::Midge;
use crate::sql::functions::FunctionId;
use crate::types::graph::{ElementKind, KeyPart};
use crate::types::typed::TypedValue;
use crate::types::DataType;

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
    let present = match find_element(session, &midge, &resolved, &key, context)? {
        None => false,
        Some(payload) if kind == ElementKind::Edge => {
            edge_endpoints_exist(&definition, mapping, &payload, session, &midge, context)?
        }
        Some(_) => true,
    };
    if !present {
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

/// The key component a stored endpoint value gives for a vertex key column of `data_type`;
/// `None` for NULL, a missing column or a value of another type.
fn endpoint_key_part(stored: Option<&serde_json::Value>, data_type: &DataType) -> Option<KeyPart> {
    let stored = stored.filter(|value| !value.is_null())?;
    match data_type.flattened() {
        DataType::SmallInt => stored
            .as_i64()
            .and_then(|value| i16::try_from(value).ok())
            .map(KeyPart::SmallInt),
        DataType::Int => stored
            .as_i64()
            .and_then(|value| i32::try_from(value).ok())
            .map(KeyPart::Int),
        DataType::BigInt => stored.as_i64().map(KeyPart::BigInt),
        DataType::Text => stored.as_str().map(|text| KeyPart::Text(text.to_owned())),
        DataType::Uuid => stored
            .as_str()
            .and_then(crate::types::uuid_text::parse)
            .map(KeyPart::Uuid),
        _ => None,
    }
}

/// Whether both endpoint vertices of the edge row `payload` exist with non-null endpoint
/// columns: an edge that cannot take part in a graph query has no identity either.
fn edge_endpoints_exist(
    definition: &PropertyGraphMeta,
    alias: &str,
    payload: &serde_json::Value,
    session: &CassieSession,
    midge: &Midge,
    context: EvalContext<'_>,
) -> Result<bool, QueryError> {
    let Some(edge) = definition.edges.iter().find(|edge| edge.alias == alias) else {
        return Ok(false);
    };
    let (catalog, binding, _) = session
        .sequence_context()
        .ok_or_else(|| failure("XX000", "cassie_graph functions require storage"))?;
    for endpoint in [&edge.source, &edge.destination] {
        let vertex =
            ResolvedMapping::by_alias(definition, &endpoint.vertex_alias, ElementKind::Vertex)
                .map_err(QueryError::Cassie)?;
        let key = endpoint
            .columns
            .iter()
            .zip(&vertex.key)
            .map(|(column, key_column)| {
                endpoint_key_part(payload.get(column), &key_column.data_type)
            })
            .collect::<Option<Vec<_>>>();
        let Some(key) = key else {
            return Ok(false);
        };
        crate::app::require_table_read(&catalog, session, &binding, &vertex.table)
            .map_err(QueryError::Cassie)?;
        if find_element(session, midge, &vertex, &key, context)?.is_none() {
            return Ok(false);
        }
    }
    Ok(true)
}

/// The row with `key` in the mapped table in the session's transaction view (committed
/// rows with the transaction's own changes applied), if any.
fn find_element(
    session: &CassieSession,
    midge: &Midge,
    resolved: &ResolvedMapping,
    key: &[KeyPart],
    context: EvalContext<'_>,
) -> Result<Option<serde_json::Value>, QueryError> {
    let snapshot = session.staged_write_snapshot(&resolved.table);
    let changes = snapshot.ordered_changes();
    let ambient = crate::midge::adapter::PublicationControlScope::current_query_controls();
    let fallback;
    let controls = if let Some(controls) = context.controls.or(ambient.as_ref()) {
        controls
    } else {
        fallback = crate::runtime::QueryExecutionControls::from_limits(
            &crate::config::CassieRuntimeLimits::default(),
            std::time::Instant::now(),
        );
        &fallback
    };
    let mut found = None;
    #[cfg(test)]
    crate::executor::execution::graph_memory_test_observer::record_owner(
        "constructor_scan_owner",
        controls,
    );
    midge.scan_rows_until::<QueryError, _>(&resolved.table, RowDecode::Full, controls, |row| {
        // A row the transaction rewrote or deleted is judged by its change below.
        if !changes.contains_key(&row.id) && matches(&row.payload, resolved, key) {
            found = Some(row.payload.clone());
            return Ok(false);
        }
        Ok(true)
    })?;
    if found.is_some() {
        return Ok(found);
    }
    for change in changes.values() {
        controls.check_interruption()?;
        if let crate::app::TransactionRowChange::Upsert(payload) = change {
            if matches(payload, resolved, key) {
                return Ok(Some(payload.clone()));
            }
        }
    }
    Ok(None)
}
