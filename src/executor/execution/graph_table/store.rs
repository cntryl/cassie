//! The rows of the mapped tables a pattern reads and the statement-local adjacency over
//! them. Every table is read through the statement's own read path, so the view is the one
//! an ordinary scan of the same table has: the session overlay, savepoints and the
//! isolation level's snapshot apply by construction.

use std::collections::HashMap;
use std::mem::size_of;

use super::cache::{key_bytes, row_key_bytes};
use super::keys::{key_of, KeyPart};
use super::plan::Plan;
use super::BatchRow;
use super::{source_collection, QueryError, SourceExecutionEnv};
use crate::catalog::{GraphKeyColumn, PropertyGraphMeta};
use crate::executor::retained_memory::hash_table_bytes;
use crate::executor::retained_memory::{add, mul};
use crate::runtime::owned_memory::{allocation_error, grow_vec};
use crate::runtime::QueryMemoryReservation;
use crate::types::DataType;
use std::sync::Arc;

pub(super) struct Loaded {
    pub(super) id: String,
    pub(super) rows: Vec<BatchRow>,
    pub(super) keys: Vec<Vec<KeyPart>>,
    /// Row position by key tuple; filled for vertex mappings.
    pub(super) index: HashMap<Vec<KeyPart>, usize>,
    /// Dense vertex id of each row of a vertex mapping.
    pub(super) ids: Vec<usize>,
}

/// What one statement's graph read counted.
#[derive(Clone, Copy, Default)]
pub(super) struct Reads {
    pub(super) index_entries: u64,
    pub(super) row_fetches: u64,
    pub(super) full_rows: u64,
    pub(super) overlay_changes: u64,
    pub(super) cache_hits: u64,
    pub(super) frontier_peak: u64,
}

#[derive(Clone, Copy)]
pub(super) struct EdgeRecord {
    pub(super) mapping: usize,
    pub(super) row: usize,
    pub(super) source: usize,
    pub(super) destination: usize,
}

pub(super) struct Store {
    /// Aligned with the graph's vertex mappings; `None` when the pattern does not read one.
    pub(super) vertex_maps: Vec<Option<Loaded>>,
    pub(super) edge_maps: Vec<Option<Loaded>>,
    /// `(mapping, row)` of each dense vertex id.
    pub(super) vertices: Vec<(usize, usize)>,
    /// Only edges whose endpoints both exist in this view.
    pub(super) edges: Vec<EdgeRecord>,
    pub(super) outgoing: Vec<Vec<usize>>,
    pub(super) incoming: Vec<Vec<usize>>,
    pub(super) reads: Reads,
    /// Lazy indexed expansion; `None` for the exact full read.
    pub(super) lazy: Option<Box<super::lazy::Lazy>>,
}

pub(super) fn key_types(key: &[GraphKeyColumn]) -> (Vec<String>, Vec<DataType>) {
    (
        key.iter().map(|column| column.name.clone()).collect(),
        key.iter().map(|column| column.data_type.clone()).collect(),
    )
}

pub(super) fn read_rows(
    env: &SourceExecutionEnv<'_>,
    table: &str,
) -> Result<Vec<BatchRow>, QueryError> {
    let batches = source_collection::scan_collection(
        env,
        table,
        None,
        false,
        table,
        crate::midge::adapter::RowTraversal::CompleteSource,
    )?;
    env.controls.check_interruption()?;
    let count = batches
        .iter()
        .try_fold(0, |count, batch| add(count, batch.len()))?;
    let memory = Arc::new(
        env.controls
            .reserve_query_memory(add(64, mul(count, size_of::<BatchRow>())?)?)?,
    );
    let mut rows = Vec::new();
    rows.try_reserve_exact(count)
        .map_err(|error| allocation_error(&error))?;
    for batch in batches {
        rows.extend(batch);
    }
    for row in &mut rows {
        row.attach_operator_memory(env.controls, Arc::clone(&memory))?;
    }
    Ok(rows)
}

fn load_mapping(
    env: &SourceExecutionEnv<'_>,
    (id, table): (&str, &str),
    key: &[GraphKeyColumn],
    memory: &mut QueryMemoryReservation,
    reads: &mut Reads,
) -> Result<Loaded, QueryError> {
    let (columns, types) = key_types(key);
    let mut rows = Vec::new();
    let mut keys = Vec::new();
    let scanned = read_rows(env, table)?;
    reads.full_rows += scanned.len() as u64;
    for row in scanned {
        env.controls.check_interruption()?;
        memory.try_grow(row_key_bytes(&row, &columns)?)?;
        let Some(tuple) = key_of(|column| row.get(column), &columns, &types) else {
            continue;
        };
        grow_vec(&mut keys, 1, memory)?;
        grow_vec(&mut rows, 1, memory)?;
        keys.push(tuple);
        rows.push(row);
    }
    memory.try_grow(id.len())?;
    Ok(Loaded {
        id: id.to_owned(),
        rows,
        keys,
        index: HashMap::new(),
        ids: Vec::new(),
    })
}

impl Store {
    /// Reads every table the plan needs and builds adjacency over the live edges.
    ///
    /// # Errors
    ///
    /// Storage, cancellation and memory-budget errors of the underlying scans.
    pub(super) fn load(
        env: &SourceExecutionEnv<'_>,
        plan: &Plan<'_>,
        memory: &mut QueryMemoryReservation,
    ) -> Result<Self, QueryError> {
        Self::load_mappings(
            env,
            &plan.graph,
            (&plan.vertex_mappings_read(), &plan.edge_mappings_read()),
            memory,
        )
    }

    /// [`Store::load`] for explicit `(vertex, edge)` mapping indexes of `graph`.
    ///
    /// # Errors
    ///
    /// Storage, cancellation and memory-budget errors of the underlying scans.
    pub(super) fn load_mappings(
        env: &SourceExecutionEnv<'_>,
        graph: &PropertyGraphMeta,
        (vertex_list, edge_list): (&[usize], &[usize]),
        memory: &mut QueryMemoryReservation,
    ) -> Result<Self, QueryError> {
        let mut reads = Reads::default();
        memory.try_grow(mul(
            add(graph.vertices.len(), graph.edges.len())?,
            size_of::<Option<Loaded>>(),
        )?)?;
        let mut vertex_maps = (0..graph.vertices.len()).map(|_| None).collect::<Vec<_>>();
        for &index in vertex_list {
            let mapping = &graph.vertices[index];
            let mut loaded = load_mapping(
                env,
                (&mapping.mapping_id, &mapping.table),
                &mapping.key,
                memory,
                &mut reads,
            )?;
            memory.try_grow(hash_table_bytes::<(Vec<KeyPart>, usize)>(
                loaded.keys.len(),
            )?)?;
            for key in &loaded.keys {
                memory.try_grow(key_bytes(key)?)?;
            }
            loaded
                .index
                .try_reserve(loaded.keys.len())
                .map_err(|error| allocation_error(&error))?;
            for (row, key) in loaded.keys.iter().enumerate() {
                env.controls.check_interruption()?;
                loaded.index.insert(key.clone(), row);
            }
            vertex_maps[index] = Some(loaded);
        }
        let mut edge_maps = (0..graph.edges.len()).map(|_| None).collect::<Vec<_>>();
        for &index in edge_list {
            let mapping = &graph.edges[index];
            edge_maps[index] = Some(load_mapping(
                env,
                (&mapping.mapping_id, &mapping.table),
                &mapping.key,
                memory,
                &mut reads,
            )?);
        }
        let mut store = Self {
            vertex_maps,
            edge_maps,
            vertices: Vec::new(),
            edges: Vec::new(),
            outgoing: Vec::new(),
            incoming: Vec::new(),
            reads,
            lazy: None,
        };
        store.number_vertices(memory)?;
        store.connect(env, graph, edge_list, memory)?;
        Ok(store)
    }

    fn number_vertices(&mut self, memory: &mut QueryMemoryReservation) -> Result<(), QueryError> {
        let total = self
            .vertex_maps
            .iter()
            .flatten()
            .map(|loaded| loaded.rows.len())
            .sum::<usize>();
        grow_vec(&mut self.vertices, total, memory)?;
        for (mapping, loaded) in self.vertex_maps.iter_mut().enumerate() {
            for row in 0..loaded.as_ref().map_or(0, |loaded| loaded.rows.len()) {
                if let Some(loaded) = loaded.as_mut() {
                    grow_vec(&mut loaded.ids, 1, memory)?;
                    loaded.ids.push(self.vertices.len());
                }
                self.vertices.push((mapping, row));
            }
        }
        grow_vec(&mut self.outgoing, total, memory)?;
        grow_vec(&mut self.incoming, total, memory)?;
        self.outgoing.resize_with(total, Vec::new);
        self.incoming.resize_with(total, Vec::new);
        Ok(())
    }

    /// The dense id of the vertex at `row` of vertex mapping `mapping`.
    pub(super) fn vertex_id(&self, mapping: usize, row: usize) -> usize {
        self.vertex_maps[mapping]
            .as_ref()
            .expect("vertex mapping loaded")
            .ids[row]
    }

    fn endpoint(
        &self,
        graph: &PropertyGraphMeta,
        edge: usize,
        source: bool,
        row: &BatchRow,
    ) -> Option<usize> {
        let meta = &graph.edges[edge];
        let endpoint = if source {
            &meta.source
        } else {
            &meta.destination
        };
        let mapping = graph
            .vertices
            .iter()
            .position(|vertex| vertex.alias == endpoint.vertex_alias)?;
        let types = key_types(&graph.vertices[mapping].key).1;
        let key = key_of(|column| row.get(column), &endpoint.columns, &types)?;
        let loaded = self.vertex_maps[mapping].as_ref()?;
        let position = *loaded.index.get(&key)?;
        Some(self.vertex_id(mapping, position))
    }

    fn connect(
        &mut self,
        env: &SourceExecutionEnv<'_>,
        graph: &PropertyGraphMeta,
        edge_list: &[usize],
        memory: &mut QueryMemoryReservation,
    ) -> Result<(), QueryError> {
        for &index in edge_list {
            let count = self.edge_maps[index].as_ref().map_or(0, |l| l.rows.len());
            for row in 0..count {
                env.controls.check_interruption()?;
                let found = {
                    let loaded = self.edge_maps[index].as_ref().expect("edge mapping read");
                    let data = &loaded.rows[row];
                    let _key_memory = env.controls.reserve_query_memory(add(
                        row_key_bytes(data, &graph.edges[index].source.columns)?,
                        row_key_bytes(data, &graph.edges[index].destination.columns)?,
                    )?)?;
                    self.endpoint(graph, index, true, data)
                        .zip(self.endpoint(graph, index, false, data))
                };
                let Some((source, destination)) = found else {
                    continue;
                };
                let edge = self.edges.len();
                grow_vec(&mut self.edges, 1, memory)?;
                grow_vec(&mut self.outgoing[source], 1, memory)?;
                grow_vec(&mut self.incoming[destination], 1, memory)?;
                self.edges.push(EdgeRecord {
                    mapping: index,
                    row,
                    source,
                    destination,
                });
                self.outgoing[source].push(edge);
                self.incoming[destination].push(edge);
            }
        }
        Ok(())
    }

    /// Adds this statement's graph reads to the runtime counters.
    pub(super) fn record_reads(&self, env: &SourceExecutionEnv<'_>, validation_rows: u64) {
        env.cassie
            .runtime
            .record_graph_reads(crate::runtime::GraphReadCounts {
                index_entries: self.reads.index_entries,
                row_fetches: self.reads.row_fetches,
                full_rows: self.reads.full_rows - validation_rows.min(self.reads.full_rows),
                overlay_changes: self.reads.overlay_changes,
                cache_hits: self.reads.cache_hits,
                validation_rows,
                frontier_peak: self.reads.frontier_peak,
            });
    }

    /// The documented order of durable references: mapping ID, then key tuple.
    pub(super) fn vertex_order(&self, left: usize, right: usize) -> std::cmp::Ordering {
        self.vertex_key(left).cmp(&self.vertex_key(right))
    }

    pub(super) fn edge_order(&self, left: usize, right: usize) -> std::cmp::Ordering {
        self.edge_key(left).cmp(&self.edge_key(right))
    }

    pub(super) fn vertex_key(&self, id: usize) -> (&String, &Vec<KeyPart>) {
        let (mapping, row) = self.vertices[id];
        let loaded = self.vertex_maps[mapping].as_ref().expect("loaded");
        (&loaded.id, &loaded.keys[row])
    }

    pub(super) fn edge_key(&self, id: usize) -> (&String, &Vec<KeyPart>) {
        let record = self.edges[id];
        let loaded = self.edge_maps[record.mapping].as_ref().expect("loaded");
        (&loaded.id, &loaded.keys[record.row])
    }

    pub(super) fn vertex_row(&self, id: usize) -> (usize, &BatchRow) {
        let (mapping, row) = self.vertices[id];
        let loaded = self.vertex_maps[mapping].as_ref().expect("loaded");
        (mapping, &loaded.rows[row])
    }

    pub(super) fn edge_row(&self, id: usize) -> (usize, &BatchRow) {
        let record = self.edges[id];
        let loaded = self.edge_maps[record.mapping].as_ref().expect("loaded");
        (record.mapping, &loaded.rows[record.row])
    }
}
