use crate::executor::QueryError;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum SortRetentionPhase {
    RunBacking,
    HeapBacking,
    SemanticPart,
    TieKey,
}

/// A private observation hook scoped to one invocation of a controlled sort.
#[derive(Default)]
pub(super) struct SortRetentionContext<'a> {
    probe: Option<&'a dyn Fn(SortRetentionPhase) -> Result<(), QueryError>>,
    #[cfg(test)]
    run_probe: Option<&'a dyn Fn(RunBackingObservation) -> Result<(), QueryError>>,
    _lifetime: std::marker::PhantomData<&'a ()>,
}

#[cfg(test)]
pub(super) struct RunBackingSnapshot {
    pub(super) other_deque_slots: usize,
    pub(super) header_slots: usize,
    pub(super) reservation_bytes: usize,
}

#[cfg(test)]
pub(super) struct RunBackingObservation {
    pub(super) actual_bytes: usize,
    pub(super) reservation_bytes: usize,
    pub(super) left_len: usize,
    pub(super) right_len: usize,
    pub(super) new_capacity: usize,
}

#[cfg(test)]
impl<'a> SortRetentionContext<'a> {
    #[cfg(test)]
    pub(super) fn with_probe(
        probe: &'a dyn Fn(SortRetentionPhase) -> Result<(), QueryError>,
    ) -> Self {
        Self {
            probe: Some(probe),
            run_probe: None,
            _lifetime: std::marker::PhantomData,
        }
    }

    #[cfg(test)]
    pub(super) fn with_run_probe(
        run_probe: &'a dyn Fn(RunBackingObservation) -> Result<(), QueryError>,
    ) -> Self {
        Self {
            probe: None,
            run_probe: Some(run_probe),
            _lifetime: std::marker::PhantomData,
        }
    }
}

impl SortRetentionContext<'_> {
    #[cfg(test)]
    pub(super) fn has_run_probe(&self) -> bool {
        self.run_probe.is_some()
    }

    #[cfg(test)]
    pub(super) fn observe_run_merge<R>(
        &self,
        snapshot: &RunBackingSnapshot,
        left: &std::collections::VecDeque<(super::RowKey, R)>,
        right: &std::collections::VecDeque<(super::RowKey, R)>,
        merged: &std::collections::VecDeque<(super::RowKey, R)>,
    ) -> Result<(), QueryError> {
        use crate::executor::retained_memory::{add, mul};

        if let Some(probe) = self.run_probe {
            let deque_slots = [
                snapshot.other_deque_slots,
                left.capacity(),
                right.capacity(),
                merged.capacity(),
            ]
            .into_iter()
            .try_fold(0, add)?;
            let actual_bytes = add(
                mul(deque_slots, std::mem::size_of::<(super::RowKey, R)>())?,
                mul(
                    snapshot.header_slots,
                    std::mem::size_of::<std::collections::VecDeque<(super::RowKey, R)>>(),
                )?,
            )?;
            probe(RunBackingObservation {
                actual_bytes,
                reservation_bytes: snapshot.reservation_bytes,
                left_len: left.len(),
                right_len: right.len(),
                new_capacity: merged.capacity(),
            })?;
        }
        Ok(())
    }

    pub(super) fn before(&self, phase: SortRetentionPhase) -> Result<(), QueryError> {
        if let Some(probe) = self.probe {
            return probe(phase);
        }
        let _ = phase;
        Ok(())
    }
}
