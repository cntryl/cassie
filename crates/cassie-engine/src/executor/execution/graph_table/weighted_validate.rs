//! The weight validation scan of `graph_weighted_paths` over the lazy indexed store (#1258,
//! #1268).
//!
//! Every row of each selected edge mapping is streamed once through the statement's own
//! read path and dropped after it is judged, so nothing but a counter is retained. A weight
//! that is missing, negative, non-finite or not an exact number is an error only when its
//! edge is graph-visible: its edge key is not NULL and both endpoint vertices exist with no
//! NULL endpoint component. Endpoint existence is asked only for such an invalid row,
//! through the key index of the vertex mapping or, when there is none, from the keys of one
//! full read of that mapping. A valid weight needs no existence check, because it is never
//! an error.

use std::collections::{HashMap, HashSet};
use std::mem::size_of;

use super::super::super::source_collection_pull::Cursor;
use super::access::Access;
use super::keys::{key_is_valid, key_of, KeyPart};
use super::lazy::lookup_rows;
use super::store::{key_types, read_rows, Reads};
use super::weighted_weight::check_weight;
use super::{BatchRow, QueryError, SourceExecutionEnv};
use crate::catalog::{GraphEdgeMeta, PropertyGraphMeta};
use crate::executor::retained_memory::{add, mul};
use crate::midge::adapter::RowTraversal;
use crate::runtime::QueryMemoryReservation;

/// What one validation scan counted.
pub(super) struct Counts {
    /// Edge rows streamed whose edge key is not NULL.
    pub(super) rows: u64,
    /// Endpoint existence reads made for invalid weights.
    pub(super) reads: Reads,
}

/// The rows of one table, streamed when the statement's read path can stream them and read
/// in full otherwise.
enum Rows<'a> {
    Stream(Box<Cursor<'a>>),
    Full(std::vec::IntoIter<BatchRow>),
}

impl<'a> Rows<'a> {
    fn open(
        env: &SourceExecutionEnv<'a>,
        table: &str,
        fields: &[String],
    ) -> Result<Self, QueryError> {
        match Cursor::open_projected(
            env,
            table,
            false,
            RowTraversal::CompleteSource,
            Some(fields),
        )? {
            Some(cursor) => Ok(Self::Stream(Box::new(cursor))),
            None => Ok(Self::Full(read_rows(env, table)?.into_iter())),
        }
    }

    fn next(&mut self, env: &SourceExecutionEnv<'_>) -> Result<Option<BatchRow>, QueryError> {
        match self {
            Self::Stream(cursor) => cursor.next(env),
            Self::Full(rows) => {
                env.controls.check_interruption()?;
                Ok(rows.next())
            }
        }
    }
}

/// Decides whether the endpoint vertices of an edge row exist in the statement's view.
struct Endpoints<'g> {
    graph: &'g PropertyGraphMeta,
    access: &'g Access,
    reads: Reads,
    memory: QueryMemoryReservation,
    /// Keys of a vertex mapping without a key index, read once and in full.
    scanned: HashMap<usize, HashSet<Vec<KeyPart>>>,
}

impl Endpoints<'_> {
    fn exists(
        &mut self,
        env: &SourceExecutionEnv<'_>,
        mapping: usize,
        key: &[KeyPart],
    ) -> Result<bool, QueryError> {
        let (graph, access) = (self.graph, self.access);
        let meta = &graph.vertices[mapping];
        let (columns, types) = key_types(&meta.key);
        if let Some(index) = &access.vertex[mapping] {
            let rows = lookup_rows(
                env,
                &mut self.reads,
                &meta.table,
                index,
                (&columns, &types),
                key,
            )?;
            return Ok(!rows.is_empty());
        }
        if !self.scanned.contains_key(&mapping) {
            let keys = self.scan_keys(env, mapping)?;
            self.scanned.insert(mapping, keys);
        }
        Ok(self
            .scanned
            .get(&mapping)
            .is_some_and(|keys| keys.contains(key)))
    }

    fn scan_keys(
        &mut self,
        env: &SourceExecutionEnv<'_>,
        mapping: usize,
    ) -> Result<HashSet<Vec<KeyPart>>, QueryError> {
        let meta = &self.graph.vertices[mapping];
        let (columns, types) = key_types(&meta.key);
        let scanned = read_rows(env, &meta.table)?;
        self.reads.full_rows += scanned.len() as u64;
        let mut keys = HashSet::new();
        for row in scanned {
            env.controls.check_interruption()?;
            let Some(key) = key_of(|column| row.get(column), &columns, &types) else {
                continue;
            };
            let heap = key.iter().map(KeyPart::heap_bytes).sum::<usize>();
            self.memory.try_grow(add(
                heap,
                add(
                    size_of::<Vec<KeyPart>>() + size_of::<usize>(),
                    mul(key.len(), size_of::<KeyPart>())?,
                )?,
            )?)?;
            keys.insert(key);
        }
        Ok(keys)
    }

    /// Whether `row` of edge mapping `meta` is a graph edge: both endpoint keys are complete
    /// and name existing vertices.
    fn visible(
        &mut self,
        env: &SourceExecutionEnv<'_>,
        meta: &GraphEdgeMeta,
        row: &BatchRow,
    ) -> Result<bool, QueryError> {
        let graph = self.graph;
        for endpoint in [&meta.source, &meta.destination] {
            let mapping = graph
                .vertices
                .iter()
                .position(|vertex| vertex.alias == endpoint.vertex_alias)
                .ok_or_else(|| super::failure("edge endpoint mapping is missing"))?;
            let types = key_types(&graph.vertices[mapping].key).1;
            let Some(key) = key_of(|column| row.get(column), &endpoint.columns, &types) else {
                return Ok(false);
            };
            if !self.exists(env, mapping, &key)? {
                return Ok(false);
            }
        }
        Ok(true)
    }
}

/// The only columns the scan judges a row by: the edge key, both endpoint column lists
/// (needed to decide visibility of an invalid weight) and the weight column. Other
/// properties of the edge row are never decoded by the validation scan.
fn validation_fields(meta: &GraphEdgeMeta, weight: &str) -> Vec<String> {
    let mut fields = meta
        .key
        .iter()
        .map(|column| column.name.clone())
        .chain(meta.source.columns.iter().cloned())
        .chain(meta.destination.columns.iter().cloned())
        .collect::<Vec<_>>();
    fields.push(weight.to_owned());
    fields.sort_unstable();
    fields.dedup();
    fields
}

/// Streams every selected edge mapping once and fails with `22023` on the first
/// graph-visible edge whose weight is invalid. `columns` holds `(edge mapping, weight
/// column)` pairs and `access` the statement's lazy access paths.
///
/// # Errors
///
/// `22023` for a visible invalid weight; storage, cancellation and memory-budget errors of
/// the reads.
pub(super) fn validate(
    env: &SourceExecutionEnv<'_>,
    graph: &PropertyGraphMeta,
    access: &Access,
    columns: &[(usize, String)],
) -> Result<Counts, QueryError> {
    let mut endpoints = Endpoints {
        graph,
        access,
        reads: Reads::default(),
        memory: env.controls.reserve_query_memory(0)?,
        scanned: HashMap::new(),
    };
    let mut rows = 0_u64;
    for (mapping, column) in columns {
        let meta = &graph.edges[*mapping];
        let (edge_columns, edge_types) = key_types(&meta.key);
        let mut source = Rows::open(env, &meta.table, &validation_fields(meta, column))?;
        while let Some(row) = source.next(env)? {
            if !key_is_valid(|name| row.get(name), &edge_columns, &edge_types) {
                continue;
            }
            rows += 1;
            if let Err(invalid) = check_weight(row.get(column)) {
                if endpoints.visible(env, meta, &row)? {
                    return Err(invalid);
                }
            }
        }
    }
    Ok(Counts {
        rows,
        reads: endpoints.reads,
    })
}
