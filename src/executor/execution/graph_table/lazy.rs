//! Lazy indexed expansion of a mapped graph (#1253, #1254).
//!
//! Vertices are fetched by key through the unique key that backs their mapping, the incident
//! edges of a visited vertex through an ordinary scalar index on the edge table's endpoint
//! columns, and start vertices by point lookup when the start pattern fixes the whole key. A
//! mapping without a usable index is read in full instead, never partially. Every read goes
//! through the statement's own snapshot, and the session's pending changes are merged by id
//! and by key, so the view is the one an exact scan of the same tables has. Rows, ties,
//! multiplicity and errors are those of the exact store; only the reads differ.

use std::collections::{HashMap, HashSet};
use std::mem::size_of;
use std::sync::Arc;

use super::access::{constant_part, start_anchor, Access, KeyIndex, Side};
use super::cache::{keyed_bytes, with_cache, FullEdges, Keyed, Pending};
use super::eval::Evaluator;
use super::keys::{key_of, KeyPart};
use super::plan::{Plan, Selection};
use super::store::{key_types, read_rows, EdgeRecord, Loaded, Reads, Store};
use super::{BatchRow, QueryError, SourceExecutionEnv};
use crate::catalog::{GraphEdgeMeta, PropertyGraphMeta};
use crate::midge::adapter::{DocumentRef, ScalarIndexScanRequest};
use crate::runtime::QueryMemoryReservation;
use crate::sql::ast::GraphEdgeDirection;
use crate::types::DataType;

pub(super) struct Lazy {
    pub(super) access: Access,
    memory: QueryMemoryReservation,
    /// Vertex mappings read in full: every vertex of the mapping has an id.
    scanned: Vec<bool>,
    vertex_ids: HashMap<(usize, Vec<KeyPart>), Option<usize>>,
    edge_ids: HashMap<(usize, Vec<KeyPart>), usize>,
    /// `(vertex, edge mapping, source side)` neighborhoods already merged into adjacency.
    expanded: HashSet<(usize, usize, bool)>,
    frontier: HashSet<usize>,
}

fn vertex_mapping_of(graph: &PropertyGraphMeta, alias: &str) -> Option<usize> {
    graph
        .vertices
        .iter()
        .position(|vertex| vertex.alias == alias)
}

/// The pending session changes of `table`, converted once per statement.
fn pending_of(
    env: &SourceExecutionEnv<'_>,
    reads: &mut Reads,
    table: &str,
) -> Result<Arc<Pending>, QueryError> {
    if let Some(known) = with_cache(env, |cache| Ok(cache.pending.get(table).cloned()))? {
        return Ok(known);
    }
    let mut pending = Pending {
        changes: 0,
        ids: HashSet::new(),
        rows: Vec::new(),
    };
    let mut bytes = size_of::<Pending>();
    if let Some(session) = env.session {
        let snapshot = session.staged_write_snapshot(table);
        let schema = env.cassie.catalog.get_schema(table);
        for (id, change) in snapshot.ordered_changes() {
            env.controls.check_interruption()?;
            pending.changes += 1;
            pending.ids.insert(id.clone());
            bytes += id.capacity() + size_of::<String>();
            if let crate::app::TransactionRowChange::Upsert(payload) = change {
                let document = DocumentRef {
                    id: id.clone(),
                    payload: payload.clone(),
                };
                let row = crate::executor::scan::hydrate_character_arrays(
                    crate::executor::scan::document_to_row(&document, schema.as_ref()),
                    env.controls,
                )?;
                bytes += keyed_bytes(&[], &row)? + id.capacity();
                pending.rows.push((id.clone(), row));
            }
        }
    }
    reads.overlay_changes += pending.changes as u64;
    let pending = Arc::new(pending);
    with_cache(env, |cache| {
        cache.admit(bytes)?;
        cache.pending.insert(table.to_owned(), Arc::clone(&pending));
        Ok(())
    })?;
    Ok(pending)
}

/// The rows of `table` whose `columns` hold exactly `key`, read through `index` in the
/// statement's view: committed index hits whose row has no pending change, plus pending
/// upserts that match. Every candidate is re-checked against the exact key comparison.
pub(super) fn lookup_rows(
    env: &SourceExecutionEnv<'_>,
    reads: &mut Reads,
    table: &str,
    index: &KeyIndex,
    (columns, types): (&[String], &[DataType]),
    key: &[KeyPart],
) -> Result<Vec<BatchRow>, QueryError> {
    env.controls.check_interruption()?;
    let pending = pending_of(env, reads, table)?;
    let request = ScalarIndexScanRequest {
        equality_prefix: index.probe(key),
        ..ScalarIndexScanRequest::default()
    };
    let hits =
        env.cassie
            .midge
            .scan_scalar_index_controlled(&index.index, &request, env.controls)?;
    let (hits, _hits_memory) = hits.into_parts();
    reads.index_entries += hits.len() as u64;
    let schema = env.cassie.catalog.get_schema(table);
    let mut rows = Vec::new();
    for hit in hits {
        env.controls.check_interruption()?;
        if pending.ids.contains(&hit.id) {
            continue;
        }
        reads.row_fetches += 1;
        let Some(source) =
            env.cassie
                .midge
                .get_retrieval_document_controlled(table, &hit.id, env.controls)?
        else {
            continue;
        };
        let (document, _source_memory) = source.into_parts();
        let row = crate::executor::scan::hydrate_character_arrays(
            crate::executor::scan::document_to_row(&document, schema.as_ref()),
            env.controls,
        )?;
        if key_of(|column| row.get(column), columns, types).as_deref() == Some(key) {
            rows.push(row);
        }
    }
    for (_, row) in &pending.rows {
        env.controls.check_interruption()?;
        if key_of(|column| row.get(column), columns, types).as_deref() == Some(key) {
            rows.push(row.clone());
        }
    }
    Ok(rows)
}

fn side_flags(direction: GraphEdgeDirection) -> &'static [Side] {
    match direction {
        GraphEdgeDirection::Outgoing => &[Side::Source],
        GraphEdgeDirection::Incoming => &[Side::Destination],
        GraphEdgeDirection::Undirected => &[Side::Source, Side::Destination],
    }
}

impl Store {
    /// A store that reads nothing until the walk asks for it.
    ///
    /// # Errors
    ///
    /// Memory-budget errors of the store's reservation.
    pub(super) fn lazy(
        env: &SourceExecutionEnv<'_>,
        graph: &PropertyGraphMeta,
        access: Access,
    ) -> Result<Self, QueryError> {
        let empty = |id: &str| Loaded {
            id: id.to_owned(),
            rows: Vec::new(),
            keys: Vec::new(),
            index: HashMap::new(),
            ids: Vec::new(),
        };
        let vertex_maps = graph
            .vertices
            .iter()
            .enumerate()
            .map(|(mapping, meta)| access.vertex_read[mapping].then(|| empty(&meta.mapping_id)))
            .collect();
        let edge_maps = graph
            .edges
            .iter()
            .enumerate()
            .map(|(mapping, meta)| {
                access.edge[mapping]
                    .is_some()
                    .then(|| empty(&meta.mapping_id))
            })
            .collect();
        let lazy = Lazy {
            scanned: vec![false; graph.vertices.len()],
            access,
            memory: env.controls.reserve_query_memory(0)?,
            vertex_ids: HashMap::new(),
            edge_ids: HashMap::new(),
            expanded: HashSet::new(),
            frontier: HashSet::new(),
        };
        Ok(Self {
            vertex_maps,
            edge_maps,
            vertices: Vec::new(),
            edges: Vec::new(),
            outgoing: Vec::new(),
            incoming: Vec::new(),
            reads: Reads::default(),
            lazy: Some(Box::new(lazy)),
        })
    }

    fn lazy_state(&mut self) -> &mut Lazy {
        self.lazy.as_deref_mut().expect("lazy store")
    }

    fn add_vertex(
        &mut self,
        mapping: usize,
        key: Vec<KeyPart>,
        row: BatchRow,
    ) -> Result<usize, QueryError> {
        let bytes = keyed_bytes(&key, &row)? + 3 * size_of::<usize>() + 2 * size_of::<Vec<usize>>();
        let id = self.vertices.len();
        let lazy = self.lazy.as_deref_mut().expect("lazy store");
        lazy.memory.try_grow(bytes)?;
        lazy.vertex_ids.insert((mapping, key.clone()), Some(id));
        let loaded = self.vertex_maps[mapping].as_mut().expect("vertex mapping");
        self.vertices.push((mapping, loaded.rows.len()));
        loaded.ids.push(id);
        loaded.rows.push(row);
        loaded.keys.push(key);
        self.outgoing.push(Vec::new());
        self.incoming.push(Vec::new());
        Ok(id)
    }

    /// Reads vertex mapping `mapping` in full and gives every row an id.
    fn scan_vertex_mapping(
        &mut self,
        env: &SourceExecutionEnv<'_>,
        graph: &PropertyGraphMeta,
        mapping: usize,
    ) -> Result<Vec<usize>, QueryError> {
        if self.lazy_state().scanned[mapping] {
            return Ok(self.vertex_maps[mapping]
                .as_ref()
                .map_or_default(|loaded| loaded.ids.clone()));
        }
        let meta = &graph.vertices[mapping];
        let cache_key = (graph.name.clone(), mapping);
        let known = with_cache(env, |cache| Ok(cache.vertex_scans.get(&cache_key).cloned()))?;
        let rows = if let Some(rows) = known {
            self.reads.cache_hits += 1;
            rows
        } else {
            let (columns, types) = key_types(&meta.key);
            let scanned = read_rows(env, &meta.table)?;
            self.reads.full_rows += scanned.len() as u64;
            let mut rows = Vec::with_capacity(scanned.len());
            let mut bytes = size_of::<Vec<Keyed>>();
            for row in scanned {
                env.controls.check_interruption()?;
                let Some(key) = key_of(|column| row.get(column), &columns, &types) else {
                    continue;
                };
                bytes += keyed_bytes(&key, &row)?;
                rows.push((key, row));
            }
            let rows = Arc::new(rows);
            with_cache(env, |cache| {
                cache.admit(bytes)?;
                cache.vertex_scans.insert(cache_key, Arc::clone(&rows));
                Ok(())
            })?;
            rows
        };
        let mut ids = Vec::with_capacity(rows.len());
        for (key, row) in rows.iter() {
            env.controls.check_interruption()?;
            let known = self
                .lazy_state()
                .vertex_ids
                .get(&(mapping, key.clone()))
                .copied()
                .flatten();
            ids.push(match known {
                Some(id) => id,
                None => self.add_vertex(mapping, key.clone(), row.clone())?,
            });
        }
        self.lazy_state().scanned[mapping] = true;
        Ok(ids)
    }

    /// The id of the vertex of `mapping` with `key`, fetching it when it is not known.
    pub(super) fn find_vertex(
        &mut self,
        env: &SourceExecutionEnv<'_>,
        graph: &PropertyGraphMeta,
        mapping: usize,
        key: &[KeyPart],
    ) -> Result<Option<usize>, QueryError> {
        let probe = (mapping, key.to_vec());
        let lazy = self.lazy_state();
        if let Some(known) = lazy.vertex_ids.get(&probe) {
            return Ok(*known);
        }
        if lazy.scanned[mapping] {
            return Ok(None);
        }
        let Some(index) = lazy.access.vertex[mapping].clone() else {
            self.scan_vertex_mapping(env, graph, mapping)?;
            return Ok(self.lazy_state().vertex_ids.get(&probe).copied().flatten());
        };
        let meta = &graph.vertices[mapping];
        let cache_key = (graph.name.clone(), mapping, key.to_vec());
        let known = with_cache(env, |cache| Ok(cache.vertices.get(&cache_key).cloned()))?;
        let row = if let Some(row) = known {
            self.reads.cache_hits += 1;
            row
        } else {
            let (columns, types) = key_types(&meta.key);
            let row = lookup_rows(
                env,
                &mut self.reads,
                &meta.table,
                &index,
                (&columns, &types),
                key,
            )?
            .into_iter()
            .next();
            let bytes = match &row {
                Some(row) => keyed_bytes(key, row)?,
                None => size_of::<Option<BatchRow>>(),
            };
            with_cache(env, |cache| {
                cache.admit(bytes)?;
                cache.vertices.insert(cache_key, row.clone());
                Ok(())
            })?;
            row
        };
        if let Some(row) = row {
            Ok(Some(self.add_vertex(mapping, key.to_vec(), row)?))
        } else {
            self.lazy_state().vertex_ids.insert(probe, None);
            Ok(None)
        }
    }

    /// The vertices the walk starts from: point lookups where the start pattern fixes a
    /// mapping's whole key by equality, every vertex of the mapping otherwise.
    ///
    /// # Errors
    ///
    /// Storage, cancellation and memory-budget errors of the reads.
    pub(super) fn start_vertices(
        &mut self,
        env: &SourceExecutionEnv<'_>,
        plan: &Plan<'_>,
        eval: &Evaluator<'_>,
    ) -> Result<Vec<usize>, QueryError> {
        if self.lazy.is_none() {
            return Ok((0..self.vertices.len()).collect());
        }
        let graph = &plan.graph;
        let mut starts = Vec::new();
        for (mapping, meta) in graph.vertices.iter().enumerate() {
            let Some(columns) = &plan.positions[0].selection[mapping] else {
                continue;
            };
            let anchored = if self.lazy_state().access.vertex[mapping].is_some() {
                anchor_key(plan, meta, columns, eval)
            } else {
                None
            };
            if let Some(key) = anchored {
                if let Some(id) = self.find_vertex(env, graph, mapping, &key)? {
                    starts.push(id);
                }
            } else {
                starts.extend(self.scan_vertex_mapping(env, graph, mapping)?);
            }
        }
        Ok(starts)
    }

    /// The rows of edge mapping `edge` incident to `key` on `side`, ordered by edge key.
    fn neighborhood(
        &mut self,
        env: &SourceExecutionEnv<'_>,
        graph: &PropertyGraphMeta,
        (edge, side): (usize, Side),
        key: &[KeyPart],
    ) -> Result<Arc<Vec<Keyed>>, QueryError> {
        let meta = &graph.edges[edge];
        let cache_key = (graph.name.clone(), edge, side == Side::Source, key.to_vec());
        if let Some(known) = with_cache(env, |cache| {
            Ok(cache.neighborhoods.get(&cache_key).cloned())
        })? {
            self.reads.cache_hits += 1;
            return Ok(known);
        }
        let endpoint = endpoint_of(meta, side);
        let near = vertex_mapping_of(graph, &endpoint.vertex_alias)
            .ok_or_else(|| super::failure("edge endpoint mapping is missing"))?;
        let (_, near_types) = key_types(&graph.vertices[near].key);
        let index = self.lazy_state().access.edge[edge]
            .as_ref()
            .filter(|access| access.indexed())
            .and_then(|access| access.index(side))
            .cloned();
        let (edge_columns, edge_types) = key_types(&meta.key);
        let mut found = Vec::new();
        if let Some(index) = index {
            for row in lookup_rows(
                env,
                &mut self.reads,
                &meta.table,
                &index,
                (&endpoint.columns, &near_types),
                key,
            )? {
                if let Some(edge_key) = key_of(|column| row.get(column), &edge_columns, &edge_types)
                {
                    found.push((edge_key, row));
                }
            }
        } else {
            let all = self.edge_scan(env, graph, edge)?;
            let by = if side == Side::Source {
                &all.by_source
            } else {
                &all.by_destination
            };
            for position in by.get(key).map_or(&[][..], Vec::as_slice) {
                found.push(all.rows[*position].clone());
            }
        }
        found.sort_by(|left, right| left.0.cmp(&right.0));
        let mut bytes = size_of::<Vec<Keyed>>();
        for (edge_key, row) in &found {
            bytes += keyed_bytes(edge_key, row)?;
        }
        let found = Arc::new(found);
        with_cache(env, |cache| {
            cache.admit(bytes)?;
            cache.neighborhoods.insert(cache_key, Arc::clone(&found));
            Ok(())
        })?;
        Ok(found)
    }

    /// The whole edge mapping `edge`, read once per statement and indexed by endpoint.
    fn edge_scan(
        &mut self,
        env: &SourceExecutionEnv<'_>,
        graph: &PropertyGraphMeta,
        edge: usize,
    ) -> Result<Arc<FullEdges>, QueryError> {
        let meta = &graph.edges[edge];
        let cache_key = (graph.name.clone(), edge);
        if let Some(known) = with_cache(env, |cache| Ok(cache.edge_scans.get(&cache_key).cloned()))?
        {
            self.reads.cache_hits += 1;
            return Ok(known);
        }
        let (edge_columns, edge_types) = key_types(&meta.key);
        let scanned = read_rows(env, &meta.table)?;
        self.reads.full_rows += scanned.len() as u64;
        let endpoint_types = |alias: &str| {
            vertex_mapping_of(graph, alias).map(|mapping| key_types(&graph.vertices[mapping].key).1)
        };
        let source_types = endpoint_types(&meta.source.vertex_alias)
            .ok_or_else(|| super::failure("edge endpoint mapping is missing"))?;
        let destination_types = endpoint_types(&meta.destination.vertex_alias)
            .ok_or_else(|| super::failure("edge endpoint mapping is missing"))?;
        let mut full = FullEdges {
            rows: Vec::new(),
            by_source: HashMap::new(),
            by_destination: HashMap::new(),
        };
        let mut bytes = size_of::<FullEdges>();
        for row in scanned {
            env.controls.check_interruption()?;
            let Some(edge_key) = key_of(|column| row.get(column), &edge_columns, &edge_types)
            else {
                continue;
            };
            let position = full.rows.len();
            bytes += keyed_bytes(&edge_key, &row)? + 4 * size_of::<usize>();
            if let Some(source) = key_of(
                |column| row.get(column),
                &meta.source.columns,
                &source_types,
            ) {
                bytes += keyed_bytes(&source, &row)?;
                full.by_source.entry(source).or_default().push(position);
            }
            if let Some(destination) = key_of(
                |column| row.get(column),
                &meta.destination.columns,
                &destination_types,
            ) {
                bytes += keyed_bytes(&destination, &row)?;
                full.by_destination
                    .entry(destination)
                    .or_default()
                    .push(position);
            }
            full.rows.push((edge_key, row));
        }
        let full = Arc::new(full);
        with_cache(env, |cache| {
            cache.admit(bytes)?;
            cache.edge_scans.insert(cache_key, Arc::clone(&full));
            Ok(())
        })?;
        Ok(full)
    }

    /// Merges the neighborhoods of `vertex` that the step `direction` over `selection` can
    /// use into adjacency. An incident edge whose far endpoint does not exist is dropped, as
    /// the exact store drops it.
    ///
    /// # Errors
    ///
    /// Storage, cancellation and memory-budget errors of the reads.
    pub(super) fn expand(
        &mut self,
        env: &SourceExecutionEnv<'_>,
        graph: &PropertyGraphMeta,
        vertex: usize,
        (direction, selection): (GraphEdgeDirection, &Selection),
    ) -> Result<(), QueryError> {
        if self.lazy.is_none() {
            return Ok(());
        }
        let (near_mapping, near_row) = self.vertices[vertex];
        let near_key = self.vertex_maps[near_mapping]
            .as_ref()
            .expect("vertex mapping")
            .keys[near_row]
            .clone();
        let near_alias = graph.vertices[near_mapping].alias.clone();
        for edge in 0..graph.edges.len() {
            if selection[edge].is_none() || self.lazy_state().access.edge[edge].is_none() {
                continue;
            }
            for &side in side_flags(direction) {
                let meta = &graph.edges[edge];
                if endpoint_of(meta, side).vertex_alias != near_alias {
                    continue;
                }
                if !self
                    .lazy_state()
                    .expanded
                    .insert((vertex, edge, side == Side::Source))
                {
                    continue;
                }
                let peak = {
                    let lazy = self.lazy_state();
                    lazy.frontier.insert(vertex);
                    lazy.frontier.len() as u64
                };
                self.reads.frontier_peak = self.reads.frontier_peak.max(peak);
                let hood = self.neighborhood(env, graph, (edge, side), &near_key)?;
                self.merge(env, graph, (edge, side, vertex), &hood)?;
            }
        }
        Ok(())
    }

    /// The id of edge `edge_key` of mapping `edge`, adding it on first sight.
    fn edge_id(
        &mut self,
        edge: usize,
        (edge_key, row): (&[KeyPart], &BatchRow),
        (source, destination): (usize, usize),
    ) -> Result<usize, QueryError> {
        let known = self
            .lazy_state()
            .edge_ids
            .get(&(edge, edge_key.to_vec()))
            .copied();
        if let Some(id) = known {
            return Ok(id);
        }
        let bytes = keyed_bytes(edge_key, row)? + size_of::<EdgeRecord>() + 4 * size_of::<usize>();
        let id = self.edges.len();
        let lazy = self.lazy_state();
        lazy.memory.try_grow(bytes)?;
        lazy.edge_ids.insert((edge, edge_key.to_vec()), id);
        let loaded = self.edge_maps[edge].as_mut().expect("edge mapping");
        self.edges.push(EdgeRecord {
            mapping: edge,
            row: loaded.rows.len(),
            source,
            destination,
        });
        loaded.rows.push(row.clone());
        loaded.keys.push(edge_key.to_vec());
        Ok(id)
    }

    fn merge(
        &mut self,
        env: &SourceExecutionEnv<'_>,
        graph: &PropertyGraphMeta,
        (edge, side, vertex): (usize, Side, usize),
        hood: &[Keyed],
    ) -> Result<(), QueryError> {
        let meta = &graph.edges[edge];
        let far = endpoint_of(
            meta,
            if side == Side::Source {
                Side::Destination
            } else {
                Side::Source
            },
        );
        let far_mapping = vertex_mapping_of(graph, &far.vertex_alias)
            .ok_or_else(|| super::failure("edge endpoint mapping is missing"))?;
        let (_, far_types) = key_types(&graph.vertices[far_mapping].key);
        for (edge_key, row) in hood {
            env.controls.check_interruption()?;
            let Some(far_key) = key_of(|column| row.get(column), &far.columns, &far_types) else {
                continue;
            };
            let Some(far_id) = self.find_vertex(env, graph, far_mapping, &far_key)? else {
                continue;
            };
            let (source, destination) = if side == Side::Source {
                (vertex, far_id)
            } else {
                (far_id, vertex)
            };
            let id = self.edge_id(edge, (edge_key, row), (source, destination))?;
            if side == Side::Source {
                self.outgoing[vertex].push(id);
            } else {
                self.incoming[vertex].push(id);
            }
        }
        Ok(())
    }
}

fn endpoint_of(meta: &GraphEdgeMeta, side: Side) -> &crate::catalog::GraphEndpointMeta {
    match side {
        Side::Source => &meta.source,
        Side::Destination => &meta.destination,
    }
}

/// The key tuple a start predicate fixes for `meta`, when every constant evaluates and fits
/// its key column's type.
fn anchor_key(
    plan: &Plan<'_>,
    meta: &crate::catalog::GraphVertexMeta,
    columns: &super::plan::Columns,
    eval: &Evaluator<'_>,
) -> Option<Vec<KeyPart>> {
    let constants = start_anchor(plan, &meta.key, columns)?;
    constants
        .into_iter()
        .zip(&meta.key)
        .map(|(expr, key)| {
            let value = eval.value(expr, Vec::new()).ok()?;
            constant_part(&value, key)
        })
        .collect()
}
