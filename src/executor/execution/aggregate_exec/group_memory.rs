use crate::executor::batch::BatchRow;
use crate::executor::semantic::SemanticKey;
use crate::runtime::QueryExecutionControls;
use crate::types::Value;

use super::state::{AggregateAccumulator, PartialAggregateGroup};
use super::QueryError;

type Reservation = crate::runtime::QueryMemoryReservation;

/// Tracks the accounted bytes of in-flight aggregate groups.
///
/// Group sizes never change after insertion, so callers add only the bytes of
/// newly buffered rows or groups. This keeps accounting linear in input rows
/// instead of re-measuring every retained group and row on each update.
pub(super) struct GroupMemory<'a> {
    controls: &'a QueryExecutionControls,
    bytes: usize,
    reservation: Option<Reservation>,
}

impl<'a> GroupMemory<'a> {
    pub(super) fn new(controls: &'a QueryExecutionControls) -> Result<Self, QueryError> {
        let reservation = controls.reserve_query_memory(0).map_err(QueryError::from)?;
        Ok(Self {
            controls,
            bytes: 0,
            reservation: Some(reservation),
        })
    }

    pub(super) fn add(&mut self, bytes: usize) -> Result<(), QueryError> {
        if bytes == 0 {
            return Ok(());
        }
        self.bytes = self.bytes.saturating_add(bytes);
        drop(self.reservation.take());
        self.reservation = Some(
            self.controls
                .reserve_query_memory(self.bytes)
                .map_err(QueryError::from)?,
        );
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
                .len()
                .saturating_mul(std::mem::size_of::<AggregateAccumulator>()),
        )
}

fn json_bytes<T: serde::Serialize + ?Sized>(value: &T) -> usize {
    serde_json::to_vec(value)
        .map(|bytes| bytes.len())
        .unwrap_or_default()
}
