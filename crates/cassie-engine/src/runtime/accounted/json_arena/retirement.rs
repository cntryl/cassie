//! Flat, planned retirement for the isolated ownership forest.
use crate::app::CassieError;
use crate::runtime::{QueryExecutionControls, QueryMemoryReservation};

use super::{invalid_owner, Forest, Kind, NodeId, Owner, Progress};

struct Plan {
    ids: Vec<NodeId>,
    memory: QueryMemoryReservation,
}

impl Plan {
    fn new(controls: &QueryExecutionControls) -> Result<Self, CassieError> {
        Ok(Self {
            ids: Vec::new(),
            memory: controls.reserve_query_memory(0)?,
        })
    }

    fn push(
        &mut self,
        id: NodeId,
        controls: &QueryExecutionControls,
        observer: &mut impl FnMut(Progress),
    ) -> Result<(), CassieError> {
        controls.check_interruption()?;
        if self.ids.len() == self.ids.capacity() {
            let capacity = self
                .ids
                .len()
                .checked_add(1)
                .and_then(|length| length.max(4).checked_next_power_of_two())
                .ok_or_else(|| {
                    CassieError::ResourceLimit("JSON retirement plan overflow".into())
                })?;
            let memory;
            let mut replacement;
            (replacement, memory) =
                crate::runtime::owned_memory::try_reserve_vec(capacity, &self.memory)?;
            for previous in self.ids.iter().copied() {
                controls.check_interruption()?;
                replacement.push(previous);
                observer(Progress::RetirementMove);
                controls.check_interruption()?;
            }
            let old = std::mem::replace(&mut self.ids, replacement);
            let old_memory = std::mem::replace(&mut self.memory, memory);
            drop(old);
            drop(old_memory);
        }
        self.ids.push(id);
        Ok(())
    }
}

impl Forest {
    pub(in crate::runtime::accounted::json_arena) fn retire_subtree(
        &mut self,
        root: NodeId,
    ) -> Result<(), CassieError> {
        self.retire_with_observer(root, |_| {})
    }

    #[cfg(test)]
    pub(super) fn retire_observed(
        &mut self,
        root: NodeId,
        observer: impl FnMut(Progress),
    ) -> Result<(), CassieError> {
        self.retire_with_observer(root, observer)
    }

    fn retire_with_observer(
        &mut self,
        root: NodeId,
        mut observer: impl FnMut(Progress),
    ) -> Result<(), CassieError> {
        self.controls.check_interruption()?;
        if self.nodes.get(root)?.owner != Owner::Unattached {
            return Err(invalid_owner());
        }
        let mut plan = Plan::new(&self.controls)?;
        plan.push(root, &self.controls, &mut observer)?;
        let mut cursor = 0;
        while cursor < plan.ids.len() {
            self.controls.check_interruption()?;
            let parent = plan.ids[cursor];
            let kind = &self.nodes.get(parent)?.kind;
            let length = match kind {
                Kind::Object(index) => index.values_len(),
                Kind::Array(index) => index.len(),
                Kind::Scalar | Kind::Boolean(_) | Kind::Text(_) | Kind::Number(_) => 0,
            };
            for position in 0..length {
                self.controls.check_interruption()?;
                let child = match kind {
                    Kind::Object(index) => index.value_at(position),
                    Kind::Array(index) => index.get(position).expect("valid array position"),
                    Kind::Scalar | Kind::Boolean(_) | Kind::Text(_) | Kind::Number(_) => {
                        unreachable!("scalar has no children")
                    }
                };
                if self.nodes.get(child)?.owner != Owner::Parent(parent) {
                    return Err(invalid_owner());
                }
                plan.push(child, &self.controls, &mut observer)?;
            }
            observer(Progress::RetirementVisit);
            self.controls.check_interruption()?;
            cursor += 1;
        }
        self.controls.check_interruption()?;
        // The exclusive borrow and sealed single-parent invariant keep every
        // prevalidated ID live until its one non-fallible removal below.
        for id in plan.ids.iter().copied() {
            drop(self.nodes.remove_validated(id));
        }
        Ok(())
    }
}
