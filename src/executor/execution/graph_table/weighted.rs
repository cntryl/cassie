//! `graph_weighted_paths` over the statement's own view of the mapped tables (#1258).
//!
//! Order of work: arguments (NULL, type, bounds), graph, references, edge labels, read
//! authority on every table the call can read, weight property and its declared type, the
//! exact-read store, source and target existence, then one controlled scan that validates
//! the weight of every graph-visible edge of the selected mappings. Only then does the
//! search run, so a hidden invalid edge is an error whatever path would be returned. The
//! complete result is materialized before the first row leaves the source.

use std::mem::size_of;
use std::sync::Arc;

use uuid::Uuid;

use super::super::super::{finalize_source_batches, SourceExecution};
use super::keys::{key_part, KeyPart as StoredKey};
use super::store::Store;
use super::weighted_args::{diagnostic, Request};
use super::weighted_search::{Candidate, Query, Search};
use super::{batch, BatchRow, QueryError, SourceExecutionEnv};
use crate::catalog::property_graphs::{GraphEdgeMeta, GraphLabelMeta, PropertyGraphMeta};
use crate::catalog::Catalog;
use crate::executor::retained_memory::{add, value_clone_bytes};
use crate::sql::ast::FunctionCall;
use crate::types::extended::numeric::Numeric;
use crate::types::extended::ExtendedType;
use crate::types::graph::{ElementRef, KeyPart};
use crate::types::typed::TypedValue;
use crate::types::{DataType, SqlArray, Value};

/// The vertex mapping of `graph` a reference points at, by index.
fn vertex_mapping(
    catalog: &Catalog,
    database: &str,
    graph: &PropertyGraphMeta,
    reference: &ElementRef,
) -> Result<usize, QueryError> {
    let resolved = catalog
        .resolve_element_ref(reference, database)
        .map_err(QueryError::Cassie)?;
    let own = Uuid::parse_str(&graph.graph_id).ok();
    if own != Some(resolved.graph_id) {
        return Err(diagnostic(
            "42704",
            "the reference belongs to another property graph",
        ));
    }
    graph
        .vertices
        .iter()
        .position(|mapping| Uuid::parse_str(&mapping.mapping_id).ok() == Some(resolved.mapping_id))
        .ok_or_else(|| diagnostic("42704", "the mapping of this reference does not exist"))
}

fn carries(label: &GraphLabelMeta, wanted: Option<&[String]>) -> bool {
    wanted.is_none_or(|labels| {
        labels
            .iter()
            .any(|name| name.eq_ignore_ascii_case(&label.name))
    })
}

/// The edge mappings carrying a requested label. An unknown label is `42704`.
fn selected_edges(
    graph: &PropertyGraphMeta,
    labels: Option<&[String]>,
) -> Result<Vec<usize>, QueryError> {
    for requested in labels.unwrap_or_default() {
        let known = graph
            .edges
            .iter()
            .flat_map(|edge| &edge.labels)
            .any(|label| label.name.eq_ignore_ascii_case(requested));
        if !known {
            return Err(diagnostic(
                "42704",
                format!("edge label \"{requested}\" does not exist"),
            ));
        }
    }
    Ok((0..graph.edges.len())
        .filter(|index| {
            graph.edges[*index]
                .labels
                .iter()
                .any(|label| carries(label, labels))
        })
        .collect())
}

fn property_of<'a>(
    edge: &'a GraphEdgeMeta,
    labels: Option<&[String]>,
    name: &str,
) -> Option<&'a crate::catalog::GraphPropertyMeta> {
    edge.labels
        .iter()
        .filter(|label| carries(label, labels))
        .find_map(|label| {
            label
                .properties
                .iter()
                .find(|property| property.name.eq_ignore_ascii_case(name))
        })
}

/// The weight column of each selected edge mapping: `42703` when one lacks the property and
/// `42804` when its declared type is not an exact number.
fn weight_columns(
    graph: &PropertyGraphMeta,
    selected: &[usize],
    labels: Option<&[String]>,
    name: &str,
) -> Result<Vec<(usize, String)>, QueryError> {
    let mut found = Vec::new();
    for &index in selected {
        let Some(property) = property_of(&graph.edges[index], labels, name) else {
            return Err(diagnostic(
                "42703",
                format!("weight property \"{name}\" does not exist on every selected edge mapping"),
            ));
        };
        found.push((index, property));
    }
    for (_, property) in &found {
        if !matches!(
            property.data_type.flattened(),
            DataType::SmallInt
                | DataType::Int
                | DataType::BigInt
                | DataType::Extended(ExtendedType::Numeric(_))
        ) {
            return Err(diagnostic(
                "42804",
                format!("weight property \"{name}\" must be an integer or numeric type"),
            ));
        }
    }
    Ok(found
        .into_iter()
        .map(|(index, property)| (index, property.column.clone()))
        .collect())
}

fn stored_key(key: &[KeyPart]) -> Vec<StoredKey> {
    key.iter()
        .filter_map(|part| {
            let (value, data_type) = match part {
                KeyPart::SmallInt(value) => (Value::Int64(i64::from(*value)), DataType::SmallInt),
                KeyPart::Int(value) => (Value::Int64(i64::from(*value)), DataType::Int),
                KeyPart::BigInt(value) => (Value::Int64(*value), DataType::BigInt),
                KeyPart::Text(value) => (Value::String(value.clone()), DataType::Text),
                KeyPart::Uuid(value) => (Value::String(value.to_string()), DataType::Uuid),
            };
            key_part(&value, &data_type)
        })
        .collect()
}

fn invalid_weight() -> QueryError {
    diagnostic(
        "22023",
        "a selected edge has a missing, negative or non-finite weight",
    )
}

/// A present, finite, non-negative exact number.
fn weight(value: Option<&Value>) -> Result<Numeric, QueryError> {
    let numeric = match value {
        Some(Value::Int64(number)) => Numeric::from_i64(*number),
        Some(Value::Typed(TypedValue::Numeric(numeric))) => numeric.clone(),
        _ => return Err(invalid_weight()),
    };
    match numeric.signum() {
        Some(sign) if sign >= 0 && matches!(numeric, Numeric::Finite { .. }) => Ok(numeric),
        _ => Err(invalid_weight()),
    }
}

fn uuids(ids: impl Iterator<Item = Option<String>>) -> Vec<Uuid> {
    ids.map(|id| {
        id.and_then(|id| Uuid::parse_str(&id).ok())
            .unwrap_or_default()
    })
    .collect()
}

/// What a request resolves to in the catalog, with authority already checked.
struct Resolved {
    graph: PropertyGraphMeta,
    source_mapping: usize,
    target_mapping: usize,
    vertex_list: Vec<usize>,
    edge_list: Vec<usize>,
    columns: Vec<(usize, String)>,
}

/// Graph, references, labels, read authority and weight property, in that order.
fn resolve(env: &SourceExecutionEnv<'_>, request: &Request) -> Result<Resolved, QueryError> {
    let session = env
        .session
        .ok_or_else(|| diagnostic("XX000", "graph_weighted_paths requires a session"))?;
    let (catalog, binding, _) = session
        .sequence_context()
        .ok_or_else(|| diagnostic("XX000", "graph_weighted_paths requires storage"))?;
    let graph = crate::sql::binder::resolve_existing_name(&request.graph, &binding, |candidate| {
        catalog.get_property_graph(candidate).is_some()
    })
    .ok()
    .and_then(|name| catalog.get_property_graph(&name))
    .ok_or_else(|| {
        diagnostic(
            "42704",
            format!("property graph \"{}\" does not exist", request.graph),
        )
    })?;
    let source_mapping = vertex_mapping(&catalog, &binding.database, &graph, &request.source)?;
    let target_mapping = vertex_mapping(&catalog, &binding.database, &graph, &request.target)?;
    let labels = request.labels.as_deref();
    let edge_list = selected_edges(&graph, labels)?;
    let mut vertex_list = vec![source_mapping, target_mapping];
    for &edge in &edge_list {
        for alias in [
            &graph.edges[edge].source.vertex_alias,
            &graph.edges[edge].destination.vertex_alias,
        ] {
            vertex_list.extend(
                graph
                    .vertices
                    .iter()
                    .position(|vertex| &vertex.alias == alias),
            );
        }
    }
    vertex_list.sort_unstable();
    vertex_list.dedup();
    // A graph grants nothing: every table the call can read needs SELECT, checked before
    // any row is read so that nothing about a protected element is revealed.
    let mut tables = vertex_list
        .iter()
        .map(|index| graph.vertices[*index].table.as_str())
        .chain(
            edge_list
                .iter()
                .map(|index| graph.edges[*index].table.as_str()),
        )
        .collect::<Vec<_>>();
    tables.sort_unstable();
    tables.dedup();
    for table in tables {
        crate::app::require_table_read(&catalog, session, &binding, table)
            .map_err(QueryError::Cassie)?;
    }
    let columns = weight_columns(&graph, &edge_list, labels, &request.property)?;
    Ok(Resolved {
        graph,
        source_mapping,
        target_mapping,
        vertex_list,
        edge_list,
        columns,
    })
}

/// Runs `graph_weighted_paths`; `outer_row` supplies the correlated columns of a `LATERAL`
/// source.
pub(in super::super) fn execute(
    env: &SourceExecutionEnv<'_>,
    name: &str,
    function: &FunctionCall,
    outer_row: Option<&BatchRow>,
    qualify: bool,
) -> SourceExecution {
    env.controls.check_interruption()?;
    let args = super::super::super::source_table_function::evaluate_args(env, function, outer_row)?;
    let request = Request::parse(&args)?;
    let Resolved {
        graph,
        source_mapping,
        target_mapping,
        vertex_list,
        edge_list,
        columns,
    } = resolve(env, &request)?;
    let descriptors = crate::executor::filter::graph_functions::descriptors_of(env.session)?;

    let mut output = env.controls.reserve_query_memory(0)?;
    let found = {
        let mut traversal = env.controls.reserve_query_memory(0)?;
        let store = Store::load_mappings(env, &graph, (&vertex_list, &edge_list), &mut traversal)?;
        let locate = |mapping: usize, reference: &ElementRef| {
            store.vertex_maps[mapping]
                .as_ref()
                .and_then(|loaded| loaded.index.get(&stored_key(&reference.key)))
                .map(|row| store.vertex_id(mapping, *row))
                .ok_or_else(|| diagnostic("42704", "the vertex does not exist"))
        };
        let source = locate(source_mapping, &request.source)?;
        let target = locate(target_mapping, &request.target)?;
        let weights = edge_weights(env, &store, &columns, &mut traversal)?;
        let vertex_mappings = uuids(graph.vertices.iter().map(|m| Some(m.mapping_id.clone())));
        let edge_mappings = uuids(graph.edges.iter().map(|m| Some(m.mapping_id.clone())));
        let query = Query {
            source,
            target,
            way: request.way,
            max_hops: request.max_hops,
            max_cost: request.max_cost.as_ref(),
            k: request.k,
            weights: &weights,
            vertex_mappings: &vertex_mappings,
            edge_mappings: &edge_mappings,
            database_id: request.source.database_id,
            graph_id: request.source.graph_id,
        };
        let found = Search::new(env, &store, &query, &mut traversal)?.run()?;
        rows(env, &descriptors, &found, &mut output)?
    };
    env.controls.check_interruption()?;
    let types = Arc::new(
        crate::sql::functions::graph_weighted::columns(Some(DataType::Composite(
            descriptors.path.clone(),
        )))
        .into_iter()
        .map(|(_, data_type)| data_type)
        .collect::<Vec<_>>(),
    );
    let lease = Arc::new(output);
    let rows = found
        .into_iter()
        .map(|mut row| {
            row.set_data_types(Arc::clone(&types));
            row.with_query_memory(Some(Arc::clone(&lease)))
        })
        .collect::<Vec<_>>();
    finalize_source_batches(
        env,
        batch::chunk_rows(rows, batch::DEFAULT_BATCH_SIZE),
        Vec::new(),
        qualify,
        name,
    )
}

/// One controlled scan of every graph-visible edge of the selected mappings: the weight of
/// each, by edge id, or the first invalid one as an error.
fn edge_weights(
    env: &SourceExecutionEnv<'_>,
    store: &Store,
    columns: &[(usize, String)],
    memory: &mut crate::runtime::QueryMemoryReservation,
) -> Result<Vec<Numeric>, QueryError> {
    memory.try_grow(crate::executor::retained_memory::mul(
        store.edges.len(),
        size_of::<Numeric>() + 64,
    )?)?;
    let mut weights = Vec::with_capacity(store.edges.len());
    for edge in 0..store.edges.len() {
        env.controls.check_interruption()?;
        let (mapping, row) = store.edge_row(edge);
        let column = columns
            .iter()
            .find(|(index, _)| *index == mapping)
            .map(|(_, column)| column.as_str())
            .ok_or_else(invalid_weight)?;
        weights.push(weight(row.get(column))?);
    }
    Ok(weights)
}

fn rows(
    env: &SourceExecutionEnv<'_>,
    descriptors: &crate::types::graph::GraphDescriptors,
    found: &[Candidate],
    memory: &mut crate::runtime::QueryMemoryReservation,
) -> Result<Vec<BatchRow>, QueryError> {
    let mut out = Vec::with_capacity(found.len());
    for (index, candidate) in found.iter().enumerate() {
        env.controls.check_interruption()?;
        let path = candidate
            .path
            .to_value(descriptors)
            .map_err(|message| diagnostic("XX000", message))?;
        let costs = SqlArray::from_elements(
            candidate
                .edge_costs
                .iter()
                .map(|cost| Value::Typed(TypedValue::Numeric(cost.clone())))
                .collect(),
        )
        .map(Value::Array)
        .map_err(|message| diagnostic("XX000", message))?;
        let values = vec![
            (
                "path_rank".to_owned(),
                Value::Int64(i64::try_from(index + 1).unwrap_or(i64::MAX)),
            ),
            (
                "cost".to_owned(),
                Value::Typed(TypedValue::Numeric(candidate.cost.clone())),
            ),
            ("path".to_owned(), path),
            ("edge_costs".to_owned(), costs),
        ];
        let mut bytes = size_of::<BatchRow>() + 64;
        for (name, value) in &values {
            bytes = add(bytes, add(name.capacity() + 64, value_clone_bytes(value)?)?)?;
        }
        memory.try_grow(bytes)?;
        out.push(BatchRow::new(values));
    }
    Ok(out)
}
