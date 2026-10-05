use std::mem::size_of;

use super::conversion::{check_controls, InputBatch};
use super::{controlled_storage_error, Cassie, SessionRowCursor};
use crate::executor::batch::DEFAULT_BATCH_SIZE;
use crate::executor::QueryError;
use crate::runtime::{QueryExecutionControls, QueryMemoryReservation};

pub(super) struct ControlledInputs {
    pub(super) batches: Vec<InputBatch>,
    pub(super) memory: QueryMemoryReservation,
}

pub(super) fn collect(
    cassie: &Cassie,
    mut cursor: SessionRowCursor,
    limit: Option<usize>,
    controls: &QueryExecutionControls,
) -> Result<ControlledInputs, QueryError> {
    let mut batches = Vec::new();
    let mut memory = controls.reserve_query_memory(0)?;
    let mut remaining = limit.unwrap_or(usize::MAX);
    while remaining > 0 {
        check_controls(controls)?;
        let accounted = cursor
            .next_accounted_documents(&cassie.midge, remaining.min(DEFAULT_BATCH_SIZE), controls)
            .map_err(|error| controlled_storage_error(cassie, error))?;
        if accounted.is_empty() {
            break;
        }
        remaining = remaining.saturating_sub(accounted.len());
        // Each original document keeps its parser/reconstruction reservation. Only the
        // additional outer batch buffer needs a new charge before it grows.
        memory.try_grow(size_of::<InputBatch>())?;
        batches.reserve_exact(1);
        batches.push(InputBatch::Accounted(accounted));
    }
    check_controls(controls)?;
    Ok(ControlledInputs { batches, memory })
}
