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

use super::access::{constant_part, start_anchor, Access, Side};
use super::cache::{
    key_bytes, keyed_bytes, reserve_map, reserve_set, row_key_bytes, with_cache, FullEdges, Keyed,
};
use super::eval::Evaluator;
use super::keys::{key_of, KeyPart};
pub(super) use super::lazy_rows::lookup_rows;
use super::lazy_rows::{detach_row, row_copy};
use super::plan::{Plan, Selection};
use super::store::{key_types, read_rows, EdgeRecord, Loaded, Reads, Store};
use super::{BatchRow, QueryError, SourceExecutionEnv};
use crate::catalog::{GraphEdgeMeta, PropertyGraphMeta};
use crate::executor::retained_memory::{add, mul};
use crate::runtime::owned_memory::grow_vec;
use crate::runtime::QueryMemoryReservation;
use crate::sql::ast::GraphEdgeDirection;

pub(super) struct Lazy {
    pub(super) access: Access,
    /// Vertex mappings read in full: every vertex of the mapping has an id.
    scanned: Vec<bool>,
    vertex_ids: HashMap<(usize, Vec<KeyPart>), Option<usize>>,
    edge_ids: HashMap<(usize, Vec<KeyPart>), usize>,
    /// `(vertex, edge mapping, source side)` neighborhoods already merged into adjacency.
    expanded: HashSet<(usize, usize, bool)>,
    frontier: HashSet<usize>,
    memory: QueryMemoryReservation,
}

impl Lazy {
    /// Neighborhoods merged into adjacency so far: one per (vertex, edge mapping, side).
    pub(super) fn expanded_count(&self) -> u64 {
        self.expanded.len() as u64
    }
}

fn vertex_mapping_of(graph: &PropertyGraphMeta, alias: &str) -> Option<usize> {
    graph
        .vertices
        .iter()
        .position(|vertex| vertex.alias == alias)
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
        let mut bytes = add(
            size_of::<Lazy>(),
            mul(graph.vertices.len(), 1 + size_of::<Option<Loaded>>())?,
        )?;
        bytes = add(bytes, mul(graph.edges.len(), size_of::<Option<Loaded>>())?)?;
        for mapping in &graph.vertices {
            bytes = add(bytes, mapping.mapping_id.len())?;
        }
        for mapping in &graph.edges {
            bytes = add(bytes, mapping.mapping_id.len())?;
        }
        let memory = env.controls.reserve_query_memory(bytes)?;
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
            memory,
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
        key: &[KeyPart],
        row: &BatchRow,
    ) -> Result<usize, QueryError> {
        let id = self.vertices.len();
        let lazy = self.lazy.as_deref_mut().expect("lazy store");
        lazy.memory.try_grow(mul(key_bytes(key)?, 2)?)?;
        reserve_map(&mut lazy.vertex_ids, &mut lazy.memory)?;
        let row = row_copy(row, &mut lazy.memory)?;
        grow_vec(&mut self.vertices, 1, &mut lazy.memory)?;
        grow_vec(&mut self.outgoing, 1, &mut lazy.memory)?;
        grow_vec(&mut self.incoming, 1, &mut lazy.memory)?;
        let loaded = self.vertex_maps[mapping].as_mut().expect("vertex mapping");
        grow_vec(&mut loaded.ids, 1, &mut lazy.memory)?;
        grow_vec(&mut loaded.rows, 1, &mut lazy.memory)?;
        grow_vec(&mut loaded.keys, 1, &mut lazy.memory)?;
        lazy.vertex_ids.insert((mapping, key.to_vec()), Some(id));
        self.vertices.push((mapping, loaded.rows.len()));
        loaded.ids.push(id);
        loaded.rows.push(row);
        loaded.keys.push(key.to_vec());
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
        memory: &mut QueryMemoryReservation,
    ) -> Result<Vec<usize>, QueryError> {
        if self.lazy_state().scanned[mapping] {
            let known = self.vertex_maps[mapping].as_ref().expect("vertex mapping");
            let mut ids = Vec::new();
            grow_vec(&mut ids, known.ids.len(), memory)?;
            ids.extend_from_slice(&known.ids);
            return Ok(ids);
        }
        let meta = &graph.vertices[mapping];
        let _cache_key_memory = env.controls.reserve_query_memory(graph.name.len())?;
        let cache_key = (graph.name.clone(), mapping);
        let known = with_cache(env, |cache| Ok(cache.vertex_scans.get(&cache_key).cloned()))?;
        let rows = if let Some(rows) = known {
            self.reads.cache_hits += 1;
            rows
        } else {
            let (columns, types) = key_types(&meta.key);
            let scanned = read_rows(env, &meta.table)?;
            self.reads.full_rows += scanned.len() as u64;
            let mut backing = env.controls.reserve_query_memory(graph.name.len() + 64)?;
            let mut rows = Vec::new();
            for row in scanned {
                env.controls.check_interruption()?;
                backing.try_grow(row_key_bytes(&row, &columns)?)?;
                let Some(key) = key_of(|column| row.get(column), &columns, &types) else {
                    continue;
                };
                let row = detach_row(row, &mut backing)?;
                grow_vec(&mut rows, 1, &mut backing)?;
                rows.push((key, row));
            }
            let rows = Arc::new(rows);
            with_cache(env, |cache| {
                reserve_map(&mut cache.vertex_scans, &mut cache.memory)?;
                cache.adopt(backing);
                cache.vertex_scans.insert(cache_key, Arc::clone(&rows));
                Ok(())
            })?;
            rows
        };
        let mut ids = Vec::new();
        grow_vec(&mut ids, rows.len(), memory)?;
        for (key, row) in rows.iter() {
            env.controls.check_interruption()?;
            let _probe_memory = env.controls.reserve_query_memory(key_bytes(key)?)?;
            let known = self
                .lazy_state()
                .vertex_ids
                .get(&(mapping, key.clone()))
                .copied()
                .flatten();
            ids.push(match known {
                Some(id) => id,
                None => self.add_vertex(mapping, key, row)?,
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
        let _probe_memory = env
            .controls
            .reserve_query_memory(add(mul(key_bytes(key)?, 2)?, graph.name.len())?)?;
        let probe = (mapping, key.to_vec());
        let lazy = self.lazy_state();
        if let Some(known) = lazy.vertex_ids.get(&probe) {
            return Ok(*known);
        }
        if lazy.scanned[mapping] {
            return Ok(None);
        }
        let Some(index) = lazy.access.vertex[mapping].clone() else {
            let mut scratch = env.controls.reserve_query_memory(0)?;
            self.scan_vertex_mapping(env, graph, mapping, &mut scratch)?;
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
            .next()
            .map(Arc::new);
            let bytes = match &row {
                Some(row) => keyed_bytes(key, row)?,
                None => size_of::<Option<BatchRow>>(),
            };
            with_cache(env, |cache| {
                reserve_map(&mut cache.vertices, &mut cache.memory)?;
                cache.admit(add(bytes, add(graph.name.len(), key_bytes(key)?)?)?)?;
                cache.vertices.insert(cache_key, row.clone());
                Ok(())
            })?;
            row
        };
        if let Some(row) = row {
            Ok(Some(self.add_vertex(mapping, key, &row)?))
        } else {
            let lazy = self.lazy_state();
            lazy.memory.try_grow(key_bytes(key)?)?;
            reserve_map(&mut lazy.vertex_ids, &mut lazy.memory)?;
            lazy.vertex_ids.insert(probe, None);
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
        memory: &mut QueryMemoryReservation,
    ) -> Result<Vec<usize>, QueryError> {
        if self.lazy.is_none() {
            let mut starts = Vec::new();
            grow_vec(&mut starts, self.vertices.len(), memory)?;
            starts.extend(0..self.vertices.len());
            return Ok(starts);
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
                    grow_vec(&mut starts, 1, memory)?;
                    starts.push(id);
                }
            } else {
                let ids = self.scan_vertex_mapping(env, graph, mapping, memory)?;
                grow_vec(&mut starts, ids.len(), memory)?;
                starts.extend(ids);
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
        let cache_bytes = add(graph.name.len() + 64, key_bytes(key)?)?;
        let _cache_key_memory = env.controls.reserve_query_memory(cache_bytes)?;
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
        let mut memory = env.controls.reserve_query_memory(cache_bytes)?;
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
                memory.try_grow(row_key_bytes(&row, &edge_columns)?)?;
                if let Some(edge_key) = key_of(|column| row.get(column), &edge_columns, &edge_types)
                {
                    let row = detach_row(row, &mut memory)?;
                    grow_vec(&mut found, 1, &mut memory)?;
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
                env.controls.check_interruption()?;
                let (key, row) = &all.rows[*position];
                memory.try_grow(key_bytes(key)?)?;
                let copied = row_copy(row, &mut memory)?;
                grow_vec(&mut found, 1, &mut memory)?;
                found.push((key.clone(), copied));
            }
        }
        {
            let _sort_memory = env
                .controls
                .reserve_query_memory(mul(found.len(), size_of::<Keyed>())?)?;
            found.sort_by(|left, right| left.0.cmp(&right.0));
        }
        let found = Arc::new(found);
        with_cache(env, |cache| {
            reserve_map(&mut cache.neighborhoods, &mut cache.memory)?;
            cache.adopt(memory);
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
        let _cache_key_memory = env.controls.reserve_query_memory(graph.name.len())?;
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
        let mut memory = env
            .controls
            .reserve_query_memory(size_of::<FullEdges>() + graph.name.len() + 64)?;
        let mut full = FullEdges {
            rows: Vec::new(),
            by_source: HashMap::new(),
            by_destination: HashMap::new(),
        };
        for row in scanned {
            env.controls.check_interruption()?;
            memory.try_grow(add(
                row_key_bytes(&row, &edge_columns)?,
                add(
                    row_key_bytes(&row, &meta.source.columns)?,
                    row_key_bytes(&row, &meta.destination.columns)?,
                )?,
            )?)?;
            let Some(edge_key) = key_of(|column| row.get(column), &edge_columns, &edge_types)
            else {
                continue;
            };
            let position = full.rows.len();
            if let Some(source) = key_of(
                |column| row.get(column),
                &meta.source.columns,
                &source_types,
            ) {
                insert_position(&mut full.by_source, source, position, &mut memory)?;
            }
            if let Some(destination) = key_of(
                |column| row.get(column),
                &meta.destination.columns,
                &destination_types,
            ) {
                insert_position(&mut full.by_destination, destination, position, &mut memory)?;
            }
            let row = detach_row(row, &mut memory)?;
            grow_vec(&mut full.rows, 1, &mut memory)?;
            full.rows.push((edge_key, row));
        }
        let full = Arc::new(full);
        with_cache(env, |cache| {
            reserve_map(&mut cache.edge_scans, &mut cache.memory)?;
            cache.adopt(memory);
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
        let borrowed_key = &self.vertex_maps[near_mapping]
            .as_ref()
            .expect("vertex mapping")
            .keys[near_row];
        let _near_memory = env.controls.reserve_query_memory(add(
            key_bytes(borrowed_key)?,
            graph.vertices[near_mapping].alias.len(),
        )?)?;
        let near_key = borrowed_key.clone();
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
                let marker = (vertex, edge, side == Side::Source);
                let lazy = self.lazy_state();
                if lazy.expanded.contains(&marker) {
                    continue;
                }
                reserve_set(&mut lazy.expanded, &mut lazy.memory)?;
                lazy.expanded.insert(marker);
                let peak = {
                    let lazy = self.lazy_state();
                    if !lazy.frontier.contains(&vertex) {
                        reserve_set(&mut lazy.frontier, &mut lazy.memory)?;
                    }
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
        let _probe_memory = self.lazy_state().memory.try_sibling(key_bytes(edge_key)?)?;
        let known = self
            .lazy_state()
            .edge_ids
            .get(&(edge, edge_key.to_vec()))
            .copied();
        if let Some(id) = known {
            return Ok(id);
        }
        let id = self.edges.len();
        let lazy = self.lazy.as_deref_mut().expect("lazy store");
        lazy.memory.try_grow(mul(key_bytes(edge_key)?, 2)?)?;
        reserve_map(&mut lazy.edge_ids, &mut lazy.memory)?;
        let row = row_copy(row, &mut lazy.memory)?;
        grow_vec(&mut self.edges, 1, &mut lazy.memory)?;
        let loaded = self.edge_maps[edge].as_mut().expect("edge mapping");
        grow_vec(&mut loaded.rows, 1, &mut lazy.memory)?;
        grow_vec(&mut loaded.keys, 1, &mut lazy.memory)?;
        lazy.edge_ids.insert((edge, edge_key.to_vec()), id);
        self.edges.push(EdgeRecord {
            mapping: edge,
            row: loaded.rows.len(),
            source,
            destination,
        });
        loaded.rows.push(row);
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
            let _key_memory = env
                .controls
                .reserve_query_memory(row_key_bytes(row, &far.columns)?)?;
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
            let lazy = self.lazy.as_deref_mut().expect("lazy store");
            let adjacency = if side == Side::Source {
                &mut self.outgoing[vertex]
            } else {
                &mut self.incoming[vertex]
            };
            grow_vec(adjacency, 1, &mut lazy.memory)?;
            adjacency.push(id);
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
            let value = eval.value(expr, &Vec::new()).ok()?;
            constant_part(value.get(), key)
        })
        .collect()
}

/// The key was admitted before construction; admit map buckets and positional backing
/// before publishing a new endpoint group.
fn insert_position(
    map: &mut HashMap<Vec<KeyPart>, Vec<usize>>,
    key: Vec<KeyPart>,
    position: usize,
    memory: &mut QueryMemoryReservation,
) -> Result<(), QueryError> {
    if !map.contains_key(&key) {
        reserve_map(map, memory)?;
    }
    let positions = map.entry(key).or_default();
    grow_vec(positions, 1, memory)?;
    positions.push(position);
    Ok(())
}
