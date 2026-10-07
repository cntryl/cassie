//! Private lease links preserve origin and all prior operator reservations.
use std::mem::size_of;
use std::sync::Arc;

use super::BatchRow;
use crate::app::CassieError;
use crate::runtime::accounted::AccountedVec;
use crate::runtime::{QueryExecutionControls, QueryMemoryReservation};

#[derive(Debug)]
pub(crate) struct OperatorMemory {
    _parents: [Option<Arc<Self>>; 2],
    _reservation: Arc<QueryMemoryReservation>,
    _metadata: QueryMemoryReservation,
    controls: QueryExecutionControls,
}

impl OperatorMemory {
    pub(crate) fn hold_rows(
        controls: &QueryExecutionControls,
        rows: &[BatchRow],
    ) -> Result<AccountedVec<Arc<Self>>, CassieError> {
        let mut parents = AccountedVec::try_new(controls)?;
        for row in rows {
            if let Some(parent) = row.operator_memory() {
                parents.try_push_with(0, || parent)?;
            }
        }
        Ok(parents)
    }

    fn new(
        controls: &QueryExecutionControls,
        reservation: Arc<QueryMemoryReservation>,
        parents: [Option<Arc<Self>>; 2],
    ) -> Result<Arc<Self>, CassieError> {
        if controls.is_cancelled() {
            return Err(CassieError::QueryCancelled);
        }
        if controls.is_timed_out() {
            return Err(CassieError::DeadlineExceeded);
        }
        let metadata = controls.reserve_query_memory(size_of::<Self>() + 2 * size_of::<usize>())?;
        Ok(Arc::new(Self {
            _parents: parents,
            _reservation: reservation,
            _metadata: metadata,
            controls: controls.clone(),
        }))
    }

    pub(crate) fn merge(
        left: Option<Arc<Self>>,
        right: Option<Arc<Self>>,
    ) -> Result<Option<Arc<Self>>, CassieError> {
        match (left, right) {
            (Some(left), Some(right)) if !Arc::ptr_eq(&left, &right) => {
                let controls = left.controls.clone();
                let reservation = Arc::new(controls.reserve_query_memory(
                    size_of::<QueryMemoryReservation>() + 2 * size_of::<usize>(),
                )?);
                Self::new(&controls, reservation, [Some(left), Some(right)]).map(Some)
            }
            (Some(left), _) => Ok(Some(left)),
            (_, right) => Ok(right),
        }
    }
}

impl BatchRow {
    pub(crate) fn retain_operator_memory(
        mut self,
        controls: &QueryExecutionControls,
        reservation: Arc<QueryMemoryReservation>,
    ) -> Result<Self, CassieError> {
        self.attach_operator_memory(controls, reservation)?;
        Ok(self)
    }

    pub(crate) fn attach_operator_memory(
        &mut self,
        controls: &QueryExecutionControls,
        reservation: Arc<QueryMemoryReservation>,
    ) -> Result<(), CassieError> {
        let previous = self.operator_memory.clone();
        // A reconstruction may replace the current origin while keeping an older operator root.
        // Retain that current owner independently, without silently overwriting either origin.
        let origin = self
            .query_memory()
            .map(|origin| OperatorMemory::new(controls, origin, [None, None]))
            .transpose()?;
        self.operator_memory = Some(OperatorMemory::new(
            controls,
            reservation,
            [previous, origin],
        )?);
        Ok(())
    }

    pub(crate) fn operator_memory(&self) -> Option<Arc<OperatorMemory>> {
        self.operator_memory.clone()
    }

    pub(crate) fn with_operator_memory(mut self, memory: Option<Arc<OperatorMemory>>) -> Self {
        self.operator_memory = memory;
        self
    }
}
