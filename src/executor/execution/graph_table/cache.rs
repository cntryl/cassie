//! The statement-scoped cache of graph reads (#1259).
//!
//! One statement reads one snapshot and one overlay, so a vertex, a neighborhood or a whole
//! mapping fetched for one outer row of a `LATERAL` source is exact for every later row of the
//! same statement. The cache lives in the statement's controls, is created for one captured
//! read, accounts every retained row against the query budget and ends with the statement.

use std::collections::{HashMap, HashSet};
use std::mem::size_of;
use std::sync::Arc;

use super::keys::KeyPart;
use super::{BatchRow, QueryError, SourceExecutionEnv};
use crate::executor::retained_memory::{add, mul, value_clone_bytes};
use crate::runtime::QueryMemoryReservation;

/// A fetched row and its key tuple.
pub(super) type Keyed = (Vec<KeyPart>, BatchRow);

/// Graph name, edge mapping, source side and the key a neighborhood belongs to.
pub(super) type HoodKey = (String, usize, bool, Vec<KeyPart>);

/// The pending session changes of one table, converted once.
pub(super) struct Pending {
    pub(super) changes: usize,
    pub(super) ids: HashSet<String>,
    pub(super) rows: Vec<(String, BatchRow)>,
}

/// A fully read edge mapping indexed by endpoint key.
pub(super) struct FullEdges {
    pub(super) rows: Vec<Keyed>,
    pub(super) by_source: HashMap<Vec<KeyPart>, Vec<usize>>,
    pub(super) by_destination: HashMap<Vec<KeyPart>, Vec<usize>>,
}

pub(super) struct Cache {
    memory: QueryMemoryReservation,
    pub(super) vertices: HashMap<(String, usize, Vec<KeyPart>), Option<BatchRow>>,
    pub(super) neighborhoods: HashMap<HoodKey, Arc<Vec<Keyed>>>,
    pub(super) vertex_scans: HashMap<(String, usize), Arc<Vec<Keyed>>>,
    pub(super) edge_scans: HashMap<(String, usize), Arc<FullEdges>>,
    pub(super) pending: HashMap<String, Arc<Pending>>,
}

impl Cache {
    fn new(memory: QueryMemoryReservation) -> Self {
        Self {
            memory,
            vertices: HashMap::new(),
            neighborhoods: HashMap::new(),
            vertex_scans: HashMap::new(),
            edge_scans: HashMap::new(),
            pending: HashMap::new(),
        }
    }

    /// Admits `bytes` of retained state.
    pub(super) fn admit(&mut self, bytes: usize) -> Result<(), QueryError> {
        Ok(self.memory.try_grow(bytes)?)
    }
}

/// Bytes a retained row and its key tuple occupy.
pub(super) fn keyed_bytes(key: &[KeyPart], row: &BatchRow) -> Result<usize, QueryError> {
    let mut bytes = add(size_of::<BatchRow>(), mul(key.len(), size_of::<KeyPart>())?)?;
    bytes = add(bytes, key.iter().map(KeyPart::heap_bytes).sum::<usize>())?;
    for (name, value) in row.entries() {
        bytes = add(bytes, size_of::<(String, crate::types::Value)>())?;
        bytes = add(bytes, name.capacity())?;
        bytes = add(bytes, value_clone_bytes(value)?)?;
    }
    Ok(bytes)
}

/// Runs `visit` on the statement's cache, creating it on first use.
///
/// # Errors
///
/// Memory-budget errors of creating the cache and those of `visit`.
pub(super) fn with_cache<R>(
    env: &SourceExecutionEnv<'_>,
    visit: impl FnOnce(&mut Cache) -> Result<R, QueryError>,
) -> Result<R, QueryError> {
    env.controls.with_statement_state(|slot| {
        if !slot.as_ref().is_some_and(|state| state.is::<Cache>()) {
            *slot = Some(Box::new(Cache::new(env.controls.reserve_query_memory(0)?)));
        }
        let cache = slot
            .as_mut()
            .and_then(|state| state.downcast_mut::<Cache>())
            .ok_or_else(|| super::failure("graph statement cache is unavailable"))?;
        visit(cache)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime::QueryExecutionControls;

    #[test]
    fn should_release_cache_memory_given_dropped_statement_state() {
        // Arrange
        let controls = QueryExecutionControls::from_limits(
            &crate::config::CassieRuntimeLimits::default(),
            std::time::Instant::now(),
        );
        let mut cache = Cache::new(controls.reserve_query_memory(0).expect("reservation"));
        // Act
        cache.admit(4096).expect("admit");
        let retained = controls.current_query_memory_bytes();
        drop(cache);
        // Assert
        assert_eq!(retained, 4096);
        assert_eq!(controls.current_query_memory_bytes(), 0);
    }
}
