//! The rows of the mapped tables a pattern reads and the statement-local adjacency over
//! them. Every table is read through the statement's own read path, so the view is the one
//! an ordinary scan of the same table has: the session overlay, savepoints and the
//! isolation level's snapshot apply by construction.

use std::collections::HashMap;
use std::mem::size_of;

use super::keys::{key_of, KeyPart};
use super::plan::Plan;
use super::BatchRow;
use super::{batch, source_collection, QueryError, SourceExecutionEnv};
use crate::catalog::GraphKeyColumn;
use crate::executor::retained_memory::{add, mul};
use crate::runtime::QueryMemoryReservation;
use crate::types::DataType;

pub(super) struct Loaded {
    pub(super) id: String,
    pub(super) rows: Vec<BatchRow>,
    pub(super) keys: Vec<Vec<KeyPart>>,
    /// Row position by key tuple; filled for vertex mappings.
    pub(super) index: HashMap<Vec<KeyPart>, usize>,
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
    vertex_base: Vec<usize>,
    /// Only edges whose endpoints both exist in this view.
    pub(super) edges: Vec<EdgeRecord>,
    pub(super) outgoing: Vec<Vec<usize>>,
    pub(super) incoming: Vec<Vec<usize>>,
    pub(super) vertex_rank: Vec<usize>,
    pub(super) edge_rank: Vec<usize>,
}

fn key_types(key: &[GraphKeyColumn]) -> (Vec<String>, Vec<DataType>) {
    (
        key.iter().map(|column| column.name.clone()).collect(),
        key.iter().map(|column| column.data_type.clone()).collect(),
    )
}

fn read_rows(env: &SourceExecutionEnv<'_>, table: &str) -> Result<Vec<BatchRow>, QueryError> {
    let batches = source_collection::scan_collection(
        env,
        table,
        None,
        false,
        table,
        crate::midge::adapter::RowTraversal::CompleteSource,
    )?;
    env.controls.check_interruption()?;
    Ok(batch::flatten_batches(batches))
}

fn load_mapping(
    env: &SourceExecutionEnv<'_>,
    id: &str,
    table: &str,
    key: &[GraphKeyColumn],
    memory: &mut QueryMemoryReservation,
) -> Result<Loaded, QueryError> {
    let (columns, types) = key_types(key);
    let mut rows = Vec::new();
    let mut keys = Vec::new();
    for row in read_rows(env, table)? {
        env.controls.check_interruption()?;
        let Some(tuple) = key_of(|column| row.get(column), &columns, &types) else {
            continue;
        };
        let heap = tuple.iter().map(KeyPart::heap_bytes).sum::<usize>();
        memory.try_grow(add(
            heap,
            add(
                size_of::<BatchRow>(),
                add(
                    size_of::<Vec<KeyPart>>(),
                    mul(tuple.len(), size_of::<KeyPart>())?,
                )?,
            )?,
        )?)?;
        keys.push(tuple);
        rows.push(row);
    }
    Ok(Loaded {
        id: id.to_owned(),
        rows,
        keys,
        index: HashMap::new(),
    })
}

fn rank_of(
    count: usize,
    mut order_before: impl FnMut(usize, usize) -> std::cmp::Ordering,
) -> Vec<usize> {
    let mut order = (0..count).collect::<Vec<_>>();
    order.sort_by(|left, right| order_before(*left, *right));
    let mut rank = vec![0; count];
    for (position, element) in order.into_iter().enumerate() {
        rank[element] = position;
    }
    rank
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
        let graph = &plan.graph;
        let mut vertex_maps = (0..graph.vertices.len()).map(|_| None).collect::<Vec<_>>();
        for index in plan.vertex_mappings_read() {
            let mapping = &graph.vertices[index];
            let mut loaded = load_mapping(
                env,
                &mapping.mapping_id,
                &mapping.table,
                &mapping.key,
                memory,
            )?;
            memory.try_grow(mul(
                loaded.keys.len(),
                add(size_of::<(Vec<KeyPart>, usize)>(), 2 * size_of::<usize>())?,
            )?)?;
            for (row, key) in loaded.keys.iter().enumerate() {
                env.controls.check_interruption()?;
                loaded.index.insert(key.clone(), row);
            }
            vertex_maps[index] = Some(loaded);
        }
        let mut edge_maps = (0..graph.edges.len()).map(|_| None).collect::<Vec<_>>();
        for index in plan.edge_mappings_read() {
            let mapping = &graph.edges[index];
            edge_maps[index] = Some(load_mapping(
                env,
                &mapping.mapping_id,
                &mapping.table,
                &mapping.key,
                memory,
            )?);
        }
        let mut store = Self {
            vertex_maps,
            edge_maps,
            vertices: Vec::new(),
            vertex_base: Vec::new(),
            edges: Vec::new(),
            outgoing: Vec::new(),
            incoming: Vec::new(),
            vertex_rank: Vec::new(),
            edge_rank: Vec::new(),
        };
        store.number_vertices(memory)?;
        store.connect(env, plan, memory)?;
        store.rank(memory)?;
        Ok(store)
    }

    fn number_vertices(&mut self, memory: &mut QueryMemoryReservation) -> Result<(), QueryError> {
        let total = self
            .vertex_maps
            .iter()
            .flatten()
            .map(|loaded| loaded.rows.len())
            .sum::<usize>();
        memory.try_grow(mul(
            total,
            2 * size_of::<Vec<usize>>() + size_of::<(usize, usize)>(),
        )?)?;
        self.vertices.reserve(total);
        for (mapping, loaded) in self.vertex_maps.iter().enumerate() {
            self.vertex_base.push(self.vertices.len());
            for row in 0..loaded.as_ref().map_or(0, |loaded| loaded.rows.len()) {
                self.vertices.push((mapping, row));
            }
        }
        self.outgoing = vec![Vec::new(); total];
        self.incoming = vec![Vec::new(); total];
        Ok(())
    }

    /// The dense id of the vertex at `row` of vertex mapping `mapping`.
    pub(super) fn vertex_id(&self, mapping: usize, row: usize) -> usize {
        self.vertex_base[mapping] + row
    }

    fn endpoint(
        &self,
        plan: &Plan<'_>,
        edge: usize,
        source: bool,
        row: &BatchRow,
    ) -> Option<usize> {
        let meta = &plan.graph.edges[edge];
        let endpoint = if source {
            &meta.source
        } else {
            &meta.destination
        };
        let mapping = plan
            .graph
            .vertices
            .iter()
            .position(|vertex| vertex.alias == endpoint.vertex_alias)?;
        let types = key_types(&plan.graph.vertices[mapping].key).1;
        let key = key_of(|column| row.get(column), &endpoint.columns, &types)?;
        let loaded = self.vertex_maps[mapping].as_ref()?;
        let position = *loaded.index.get(&key)?;
        Some(self.vertex_id(mapping, position))
    }

    fn connect(
        &mut self,
        env: &SourceExecutionEnv<'_>,
        plan: &Plan<'_>,
        memory: &mut QueryMemoryReservation,
    ) -> Result<(), QueryError> {
        for index in plan.edge_mappings_read() {
            let count = self.edge_maps[index].as_ref().map_or(0, |l| l.rows.len());
            memory.try_grow(mul(
                count,
                add(size_of::<EdgeRecord>(), 2 * size_of::<usize>())?,
            )?)?;
            for row in 0..count {
                env.controls.check_interruption()?;
                let found = {
                    let loaded = self.edge_maps[index].as_ref().expect("edge mapping read");
                    let data = &loaded.rows[row];
                    self.endpoint(plan, index, true, data)
                        .zip(self.endpoint(plan, index, false, data))
                };
                let Some((source, destination)) = found else {
                    continue;
                };
                let edge = self.edges.len();
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

    fn rank(&mut self, memory: &mut QueryMemoryReservation) -> Result<(), QueryError> {
        memory.try_grow(mul(
            self.vertices.len() + self.edges.len(),
            size_of::<usize>(),
        )?)?;
        self.vertex_rank = rank_of(self.vertices.len(), |a, b| {
            self.vertex_key(a).cmp(&self.vertex_key(b))
        });
        self.edge_rank = rank_of(self.edges.len(), |a, b| {
            self.edge_key(a).cmp(&self.edge_key(b))
        });
        Ok(())
    }

    fn vertex_key(&self, id: usize) -> (&String, &Vec<KeyPart>) {
        let (mapping, row) = self.vertices[id];
        let loaded = self.vertex_maps[mapping].as_ref().expect("loaded");
        (&loaded.id, &loaded.keys[row])
    }

    fn edge_key(&self, id: usize) -> (&String, &Vec<KeyPart>) {
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
