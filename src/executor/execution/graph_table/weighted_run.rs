//! One `graph_weighted_paths` evaluation after its arguments and catalog references are
//! resolved: choose the read path, locate the endpoints, validate every graph-visible edge
//! weight, then search (#1258, #1268).
//!
//! The lazy indexed store is the default. Its validation is one streaming scan of the
//! selected edge mappings (`weighted_validate`) and its search fetches the neighbourhoods of
//! the vertices it visits. When exact reads are forced the whole selected mappings are read
//! first, as before, and validation is the weight check of every loaded edge.

use uuid::Uuid;

use super::access::Access;
use super::keys::KeyPart;
use super::plan::Selection;
use super::store::Store;
use super::weighted::{stored_key, Resolved};
use super::weighted_args::{diagnostic, Request, Way};
use super::weighted_search::{Candidate, Query, Search};
use super::weighted_validate;
use super::{QueryError, SourceExecutionEnv};
use crate::catalog::{GraphKeyColumn, PropertyGraphMeta};
use crate::runtime::{GraphReadCounts, QueryMemoryReservation};
use crate::sql::ast::GraphEdgeDirection;
use crate::types::graph::ElementRef;
use crate::types::DataType;

fn uuids(ids: impl Iterator<Item = String>) -> Vec<Uuid> {
    ids.map(|id| Uuid::parse_str(&id).unwrap_or_default())
        .collect()
}

/// Whether `part` is a component of the declared key type `column`.
fn declared(part: &KeyPart, column: &GraphKeyColumn) -> bool {
    matches!(
        (part, &column.data_type),
        (KeyPart::SmallInt(_), DataType::SmallInt)
            | (KeyPart::Integer(_), DataType::Int)
            | (KeyPart::BigInt(_), DataType::BigInt)
            | (KeyPart::Text(_), DataType::Text)
            | (KeyPart::Uuid(_), DataType::Uuid)
    )
}

/// The dense id of the vertex `reference` names, or `42704` when it does not exist in the
/// statement's view.
fn locate(
    env: &SourceExecutionEnv<'_>,
    store: &mut Store,
    graph: &PropertyGraphMeta,
    (mapping, reference): (usize, &ElementRef),
) -> Result<usize, QueryError> {
    let key = stored_key(&reference.key);
    let meta = &graph.vertices[mapping];
    let fits = key.len() == meta.key.len()
        && key
            .iter()
            .zip(&meta.key)
            .all(|(part, column)| declared(part, column));
    let found = match (fits, store.lazy.is_some()) {
        (false, _) => None,
        (true, true) => store.find_vertex(env, graph, mapping, &key)?,
        (true, false) => store.vertex_maps[mapping]
            .as_ref()
            .and_then(|loaded| loaded.index.get(&key))
            .map(|row| store.vertex_id(mapping, *row)),
    };
    found.ok_or_else(|| diagnostic("42704", "the vertex does not exist"))
}

/// The edge sides a traversal in `way` looks up.
const fn sides(way: Way) -> [bool; 2] {
    match way {
        Way::Out => [true, false],
        Way::In => [false, true],
        Way::Both => [true, true],
    }
}

const fn direction(way: Way) -> GraphEdgeDirection {
    match way {
        Way::Out => GraphEdgeDirection::Outgoing,
        Way::In => GraphEdgeDirection::Incoming,
        Way::Both => GraphEdgeDirection::Undirected,
    }
}

/// Validates the selected edge mappings of a lazy store and records the scan's reads.
fn validate_lazy(
    env: &SourceExecutionEnv<'_>,
    store: &Store,
    resolved: &Resolved,
) -> Result<(), QueryError> {
    let lazy = store
        .lazy
        .as_deref()
        .ok_or_else(|| super::failure("the lazy store is unavailable"))?;
    let counts =
        weighted_validate::validate(env, &resolved.graph, &lazy.access, &resolved.columns)?;
    env.cassie.runtime.record_graph_reads(GraphReadCounts {
        index_entries: counts.reads.index_entries,
        row_fetches: counts.reads.row_fetches,
        full_rows: counts.reads.full_rows,
        overlay_changes: counts.reads.overlay_changes,
        cache_hits: 0,
        validation_rows: counts.rows,
        frontier_peak: 0,
        edges_admitted: 0,
        neighborhoods: 0,
    });
    Ok(())
}

/// The `k` cheapest simple paths of `request`, validated and ordered.
///
/// # Errors
///
/// `42704` for a missing endpoint, `22023` for an invalid weight of a graph-visible edge,
/// and the storage, cancellation and memory-budget errors of the reads.
pub(super) fn run(
    env: &SourceExecutionEnv<'_>,
    request: &Request,
    resolved: &Resolved,
    memory: &mut QueryMemoryReservation,
) -> Result<Vec<Candidate>, QueryError> {
    let graph = &resolved.graph;
    let wanted = sides(request.way);
    let access = Access::for_mappings(
        (env.cassie, env.session),
        graph,
        (&resolved.vertex_list, &resolved.edge_list),
        |_| wanted,
    );
    let mut store = if access.forced_exact {
        Store::load_mappings(
            env,
            graph,
            (&resolved.vertex_list, &resolved.edge_list),
            memory,
        )?
    } else {
        Store::lazy(env, graph, access)?
    };
    let source = locate(
        env,
        &mut store,
        graph,
        (resolved.source_mapping, &request.source),
    )?;
    let target = locate(
        env,
        &mut store,
        graph,
        (resolved.target_mapping, &request.target),
    )?;
    if store.lazy.is_some() {
        validate_lazy(env, &store, resolved)?;
    } else {
        let loaded = store
            .edge_maps
            .iter()
            .flatten()
            .map(|loaded| loaded.rows.len() as u64)
            .sum();
        store.record_reads(env, loaded);
    }
    let mut columns = vec![None; graph.edges.len()];
    for (mapping, column) in &resolved.columns {
        columns[*mapping] = Some(column.clone());
    }
    let selection: Selection = (0..graph.edges.len())
        .map(|mapping| resolved.edge_list.contains(&mapping).then(Vec::new))
        .collect();
    let vertex_mappings = uuids(graph.vertices.iter().map(|m| m.mapping_id.clone()));
    let edge_mappings = uuids(graph.edges.iter().map(|m| m.mapping_id.clone()));
    let query = Query {
        source,
        target,
        way: request.way,
        max_hops: request.max_hops,
        max_cost: request.max_cost.as_ref(),
        k: request.k,
        columns: &columns,
        graph,
        selection: &selection,
        direction: direction(request.way),
        vertex_mappings: &vertex_mappings,
        edge_mappings: &edge_mappings,
        database_id: request.source.database_id,
        graph_id: request.source.graph_id,
    };
    let found = Search::new(env, &mut store, &query, memory)?.run()?;
    if store.lazy.is_some() {
        store.record_reads(env, 0);
    }
    Ok(found)
}
