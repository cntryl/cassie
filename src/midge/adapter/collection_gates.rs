//! Request-owned directories and cancellable RR commit write gates.

use std::time::Duration;

use parking_lot::{ReentrantMutex, ReentrantMutexGuard};

use super::{CassieError, Midge, PublicationControlScope};
use crate::runtime::owned_memory::try_reserve_vec;
use crate::runtime::{QueryExecutionControls, QueryMemoryReservation};

struct Directory<T> {
    values: Vec<T>,
    memory: QueryMemoryReservation,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum WaitBoundary {
    Referential,
    Collection,
}

pub(super) fn with_controls<T>(
    midge: &Midge,
    collections: &[String],
    referential: Option<&ReentrantMutex<()>>,
    controls: &QueryExecutionControls,
    operation: impl FnOnce() -> Result<T, CassieError>,
) -> Result<T, CassieError> {
    PublicationControlScope::check_interruption(controls)?;
    let names = admitted_names(collections, controls)?;
    let (values, memory) = try_reserve_vec(names.values.len(), &names.memory)?;
    let mut gates = Directory { values, memory };
    for name in &names.values {
        PublicationControlScope::check_interruption(controls)?;
        gates.values.push(midge.collection_write_gate(name));
    }
    drop(names);
    let (values, memory) = try_reserve_vec(gates.values.len(), &gates.memory)?;
    let mut guards = Directory { values, memory };
    let _referential = referential
        .map(|gate| lock(gate, controls, WaitBoundary::Referential))
        .transpose()?;
    for gate in &gates.values {
        guards
            .values
            .push(lock(gate, controls, WaitBoundary::Collection)?);
    }
    PublicationControlScope::check_interruption(controls)?;
    operation()
}

fn admitted_names(
    collections: &[String],
    controls: &QueryExecutionControls,
) -> Result<Directory<String>, CassieError> {
    let bytes = collections.iter().try_fold(0_usize, |bytes, name| {
        bytes
            .checked_add(name.len())
            .ok_or_else(|| CassieError::ResourceLimit("commit gate name size overflow".to_owned()))
    })?;
    let strings = controls.reserve_query_memory(bytes)?;
    let (values, mut memory) = try_reserve_vec(collections.len(), &strings)?;
    memory.absorb(strings);
    let mut names = Directory { values, memory };
    for name in collections {
        PublicationControlScope::check_interruption(controls)?;
        names.values.push(name.to_ascii_lowercase());
    }
    names.values.sort_unstable();
    names.values.dedup();
    Ok(names)
}

fn lock<'a>(
    gate: &'a ReentrantMutex<()>,
    controls: &QueryExecutionControls,
    boundary: WaitBoundary,
) -> Result<ReentrantMutexGuard<'a, ()>, CassieError> {
    loop {
        PublicationControlScope::check_interruption(controls)?;
        if let Some(guard) = gate.try_lock_for(Duration::from_millis(2)) {
            PublicationControlScope::check_interruption(controls)?;
            return Ok(guard);
        }
        observe_wait_boundary(boundary);
    }
}

#[cfg(test)]
fn observe_wait_boundary(boundary: WaitBoundary) {
    tests::observe_wait(boundary);
}
#[cfg(not(test))]
fn observe_wait_boundary(_: WaitBoundary) {}

#[cfg(test)]
mod tests;
