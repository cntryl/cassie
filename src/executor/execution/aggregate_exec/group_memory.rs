use crate::executor::batch::BatchRow;
use crate::executor::semantic::SemanticKey;
use crate::runtime::QueryExecutionControls;
use crate::types::Value;

use super::state::{AggregateAccumulator, PartialAggregateGroup};
use super::QueryError;

type Reservation = crate::runtime::QueryMemoryReservation;

/// Tracks the accounted bytes of in-flight aggregate groups.
///
/// Callers add only the bytes of newly buffered rows or groups, plus the
/// growth of a retained accumulator value, so accounting stays linear in input
/// rows. Growth reserves only the delta on the existing reservation: bytes
/// already held are never released and re-requested, so a worker sharing the
/// query tracker cannot claim them in between and trigger a spurious limit.
pub(super) struct GroupMemory {
    reservation: Reservation,
}

impl GroupMemory {
    pub(super) fn new(controls: &QueryExecutionControls) -> Result<Self, QueryError> {
        let reservation = controls.reserve_query_memory(0).map_err(QueryError::from)?;
        Ok(Self { reservation })
    }

    pub(super) fn add(&mut self, bytes: usize) -> Result<(), QueryError> {
        if bytes == 0 {
            return Ok(());
        }
        self.reservation.try_grow(bytes).map_err(QueryError::from)
    }

    /// Re-accounts a retained value that changed from `before` to `after`
    /// bytes. Shrinking releases at most what this reservation holds.
    pub(super) fn resize(&mut self, before: usize, after: usize) -> Result<(), QueryError> {
        if after >= before {
            return self.add(after - before);
        }
        let held = self.reservation.bytes();
        self.reservation
            .shrink_to(held.saturating_sub(before - after));
        Ok(())
    }
}

pub(super) fn serial_group_bytes(signature: &SemanticKey, values: &[(String, Value)]) -> usize {
    signature
        .estimated_bytes()
        .saturating_add(json_bytes(values))
}

pub(super) fn serial_row_bytes(row: &BatchRow) -> usize {
    json_bytes(row.entries())
}

pub(super) fn partial_group_bytes(signature: &SemanticKey, group: &PartialAggregateGroup) -> usize {
    signature
        .estimated_bytes()
        .saturating_add(json_bytes(&group.group_values))
        .saturating_add(
            group
                .accumulators
                .iter()
                .map(AggregateAccumulator::retained_bytes)
                .fold(0, usize::saturating_add),
        )
}

pub(super) fn json_bytes<T: serde::Serialize + ?Sized>(value: &T) -> usize {
    serde_json::to_vec(value)
        .map(|bytes| bytes.len())
        .unwrap_or_default()
}
