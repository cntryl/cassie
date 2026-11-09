//! Admitted append-only array edges for the isolated forest.
use crate::app::CassieError;
use crate::runtime::{QueryExecutionControls, QueryMemoryReservation};

use super::{NodeId, Progress};

pub(super) struct ArrayIndex {
    values: Vec<NodeId>,
    memory: QueryMemoryReservation,
    controls: QueryExecutionControls,
}

impl ArrayIndex {
    pub(super) fn new(controls: &QueryExecutionControls) -> Result<Self, CassieError> {
        Ok(Self {
            values: Vec::new(),
            memory: controls.reserve_query_memory(0)?,
            controls: controls.clone(),
        })
    }

    pub(super) fn len(&self) -> usize {
        self.values.len()
    }

    pub(super) fn get(&self, index: usize) -> Option<NodeId> {
        self.values.get(index).copied()
    }

    pub(super) fn append(
        &mut self,
        child: NodeId,
        observer: &mut impl FnMut(Progress),
    ) -> Result<(), CassieError> {
        self.controls.check_interruption()?;
        if self.values.len() == self.values.capacity() {
            let capacity = self
                .values
                .len()
                .checked_add(1)
                .and_then(|length| length.max(4).checked_next_power_of_two())
                .ok_or_else(|| CassieError::ResourceLimit("JSON array capacity overflow".into()))?;
            let memory;
            let mut replacement;
            (replacement, memory) =
                crate::runtime::owned_memory::try_reserve_vec(capacity, &self.memory)?;
            for previous in self.values.iter().copied() {
                self.controls.check_interruption()?;
                replacement.push(previous);
                observer(Progress::ArrayMove);
                self.controls.check_interruption()?;
            }
            self.controls.check_interruption()?;
            let old = std::mem::replace(&mut self.values, replacement);
            let old_memory = std::mem::replace(&mut self.memory, memory);
            drop(old);
            drop(old_memory);
        }
        self.controls.check_interruption()?;
        self.values.push(child);
        Ok(())
    }
}
