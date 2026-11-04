//! Confirmed point mutations retained only for active RR write snapshots.

use std::mem::size_of;
use std::sync::Arc;

use parking_lot::Mutex;

use super::{AdapterCommitError, CassieError, DatabasePublication};
use crate::runtime::owned_memory::{grow_vec, try_reserve_vec};
use crate::runtime::QueryMemoryReservation;

pub(in crate::midge::adapter) struct RowHistory {
    state: Mutex<HistoryState>,
}

struct HistoryState {
    sequence: u64,
    next_registration: u64,
    active: Vec<Capture>,
    entries: Vec<Entry>,
    budget: usize,
    admission: Option<QueryMemoryReservation>,
    active_memory: Option<QueryMemoryReservation>,
    entries_memory: Option<QueryMemoryReservation>,
}

struct Capture {
    id: u64,
    sequence: u64,
}

struct Entry {
    key: OwnedKey,
    sequence: u64,
}

struct OwnedKey {
    value: Vec<u8>,
    _memory: QueryMemoryReservation,
}

pub(in crate::midge::adapter) struct MutationKeys {
    values: Vec<OwnedKey>,
    memory: QueryMemoryReservation,
}

pub(in crate::midge::adapter) struct PreparedKey {
    key: OwnedKey,
    index: usize,
}

pub(crate) struct RetainedRowHistory {
    publication: Arc<DatabasePublication>,
    id: u64,
    sequence: u64,
    _memory: QueryMemoryReservation,
}

impl std::fmt::Debug for RetainedRowHistory {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("RetainedRowHistory")
            .finish_non_exhaustive()
    }
}

impl RowHistory {
    pub(in crate::midge::adapter) fn new(budget: usize) -> Self {
        Self {
            state: Mutex::new(HistoryState {
                sequence: 0,
                next_registration: 0,
                active: Vec::new(),
                entries: Vec::new(),
                budget,
                admission: None,
                active_memory: None,
                entries_memory: None,
            }),
        }
    }

    pub(in crate::midge::adapter) fn key_log(&self) -> Result<MutationKeys, CassieError> {
        let mut state = self.state.lock();
        state.admit_header()?;
        Ok(MutationKeys {
            values: Vec::new(),
            memory: state
                .admission
                .as_ref()
                .expect("admitted history header")
                .try_sibling(0)?,
        })
    }

    pub(in crate::midge::adapter) fn register(
        publication: &Arc<DatabasePublication>,
    ) -> Result<Arc<RetainedRowHistory>, CassieError> {
        let mut state = publication.row_history.state.lock();
        state.admit_header()?;
        let id = state
            .next_registration
            .checked_add(1)
            .ok_or_else(sequence_exhausted)?;
        let memory = state
            .admission
            .as_ref()
            .expect("admitted history header")
            .try_sibling(size_of::<RetainedRowHistory>() + 2 * size_of::<usize>())?;
        let HistoryState {
            active,
            active_memory,
            ..
        } = &mut *state;
        grow_vec(
            active,
            1,
            active_memory
                .as_mut()
                .expect("admitted registration directory"),
        )?;
        let sequence = state.sequence;
        let owner = Arc::new(RetainedRowHistory {
            publication: Arc::clone(publication),
            id,
            sequence,
            _memory: memory,
        });
        state.active.push(Capture { id, sequence });
        state.next_registration = id;
        Ok(owner)
    }

    pub(in crate::midge::adapter) fn publish(
        &self,
        mut keys: MutationKeys,
        operation: impl FnOnce() -> Result<(), cntryl_midge::MidgeError>,
    ) -> Result<(), AdapterCommitError> {
        if keys.values.is_empty() {
            return operation().map_err(AdapterCommitError::Native);
        }
        let mut state = self.state.lock();
        let sequence = state
            .sequence
            .checked_add(1)
            .ok_or_else(|| AdapterCommitError::Cassie(sequence_exhausted()))?;
        if !state.active.is_empty() {
            let new_keys = keys
                .values
                .iter()
                .filter(|key| {
                    state
                        .entries
                        .binary_search_by(|entry| entry.key.value.cmp(&key.value))
                        .is_err()
                })
                .count();
            let HistoryState {
                entries,
                entries_memory,
                ..
            } = &mut *state;
            grow_vec(
                entries,
                new_keys,
                entries_memory.as_mut().expect("admitted history directory"),
            )
            .map_err(AdapterCommitError::Cassie)?;
        }
        operation().map_err(AdapterCommitError::Native)?;
        state.sequence = sequence;
        if !state.active.is_empty() {
            for key in keys.values.drain(..) {
                match state
                    .entries
                    .binary_search_by(|entry| entry.key.value.cmp(&key.value))
                {
                    Ok(index) => state.entries[index].sequence = sequence,
                    Err(index) => state.entries.insert(index, Entry { key, sequence }),
                }
            }
        }
        Ok(())
    }
}

impl HistoryState {
    fn admit_header(&mut self) -> Result<(), CassieError> {
        if self.admission.is_none() {
            // Check before creating even the independent tracker's Arc. No
            // request control scope or cancellation authority is constructed.
            let admission = QueryMemoryReservation::independent_budget(
                self.budget,
                size_of::<RowHistory>() + 8 * size_of::<usize>(),
            )?;
            self.active_memory = Some(admission.try_sibling(0)?);
            self.entries_memory = Some(admission.try_sibling(0)?);
            self.admission = Some(admission);
        }
        Ok(())
    }

    fn prune(&mut self) {
        if let Some(oldest) = self.active.iter().map(|capture| capture.sequence).min() {
            self.entries.retain(|entry| entry.sequence > oldest);
        } else {
            self.entries = Vec::new();
            self.entries_memory
                .as_mut()
                .expect("admitted history directory")
                .shrink_to(0);
            self.active = Vec::new();
            self.active_memory
                .as_mut()
                .expect("admitted registration directory")
                .shrink_to(0);
        }
    }
}

impl MutationKeys {
    pub(in crate::midge::adapter) fn prepare(
        &mut self,
        key: &[u8],
    ) -> Result<Option<PreparedKey>, CassieError> {
        let index = match self
            .values
            .binary_search_by(|entry| entry.value.as_slice().cmp(key))
        {
            Ok(_) => return Ok(None),
            Err(index) => index,
        };
        let (mut value, memory) = try_reserve_vec(key.len(), &self.memory)?;
        value.extend_from_slice(key);
        grow_vec(&mut self.values, 1, &mut self.memory)?;
        Ok(Some(PreparedKey {
            key: OwnedKey {
                value,
                _memory: memory,
            },
            index,
        }))
    }

    pub(in crate::midge::adapter) fn install(&mut self, key: Option<PreparedKey>) {
        if let Some(key) = key {
            self.values.insert(key.index, key.key);
        }
    }
}

impl RetainedRowHistory {
    pub(crate) fn check_row(
        &self,
        midge: &crate::midge::Midge,
        database: &str,
        row: (&str, u64, &str),
        controls: &crate::runtime::QueryExecutionControls,
    ) -> Result<(), CassieError> {
        let (collection, relation_id, id) = row;
        let (keys, _memory) = crate::midge::adapter::transactions::admitted_row_keys(
            midge,
            database,
            collection,
            relation_id,
            id,
            controls,
        )?;
        self.check_keys(keys.iter().map(Vec::as_slice), controls)
    }

    pub(crate) fn check_keys<'a>(
        &self,
        keys: impl IntoIterator<Item = &'a [u8]>,
        controls: &crate::runtime::QueryExecutionControls,
    ) -> Result<(), CassieError> {
        self.publication
            .with_operation(controls, || self.check_keys_locked(keys))
    }

    fn check_keys_locked<'a>(
        &self,
        keys: impl IntoIterator<Item = &'a [u8]>,
    ) -> Result<(), CassieError> {
        let state = self.publication.row_history.state.lock();
        for key in keys {
            if state
                .entries
                .binary_search_by(|entry| entry.key.value.as_slice().cmp(key))
                .ok()
                .is_some_and(|index| state.entries[index].sequence > self.sequence)
            {
                return Err(CassieError::Transaction {
                    kind: crate::app::TransactionErrorKind::SerializationFailure,
                    message: "could not serialize access due to concurrent row update".to_owned(),
                });
            }
        }
        Ok(())
    }
}

impl Drop for RetainedRowHistory {
    fn drop(&mut self) {
        // Never acquire the publication gate from Drop: commit owns state until
        // confirmation, and no retained owner is stored inside that state.
        #[cfg(test)]
        tests::observe_retirement();
        let mut state = self.publication.row_history.state.lock();
        state.active.retain(|capture| capture.id != self.id);
        state.prune();
    }
}

fn sequence_exhausted() -> CassieError {
    CassieError::ResourceLimit("REPEATABLE READ row-history sequence exhausted".to_owned())
}

#[cfg(test)]
mod tests;
