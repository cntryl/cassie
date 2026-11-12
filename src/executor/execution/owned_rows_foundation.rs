//! Isolated test-only directory and consuming-iterator ownership experiment.
use super::{BatchRow, QueryExecutionControls};
use crate::app::CassieError;
use crate::runtime::QueryMemoryReservation;

#[derive(Debug)]
pub(super) struct OwnedRows {
    rows: Vec<BatchRow>,
    directory: QueryMemoryReservation,
}

pub(super) struct OwnedRowsIntoIter {
    rows: std::vec::IntoIter<BatchRow>,
    directory: QueryMemoryReservation,
}

impl OwnedRows {
    pub(super) fn new(
        rows: Vec<BatchRow>,
        directory: QueryMemoryReservation,
        controls: &QueryExecutionControls,
    ) -> Result<Self, CassieError> {
        let pending = Self { rows, directory };
        if !pending.directory.belongs_to(controls) {
            return Err(CassieError::ResourceLimit(
                "row directory uses a different query tracker".into(),
            ));
        }
        let bytes = pending
            .rows
            .capacity()
            .checked_mul(std::mem::size_of::<BatchRow>())
            .ok_or_else(|| CassieError::ResourceLimit("row directory capacity overflow".into()))?;
        if bytes > pending.directory.bytes() {
            return Err(CassieError::ResourceLimit(
                "row directory exceeds its reservation".into(),
            ));
        }
        Ok(pending)
    }

    pub(super) fn into_iter(self) -> OwnedRowsIntoIter {
        let Self { rows, directory } = self;
        OwnedRowsIntoIter {
            rows: rows.into_iter(),
            directory,
        }
    }
}

impl OwnedRowsIntoIter {
    pub(super) fn next_observing(
        &mut self,
        controls: &QueryExecutionControls,
        after_move: impl FnOnce(),
    ) -> Result<Option<BatchRow>, CassieError> {
        if !self.directory.belongs_to(controls) {
            return Err(CassieError::ResourceLimit(
                "row iterator uses a different query tracker".into(),
            ));
        }
        controls.check_interruption()?;
        let row = self.rows.next();
        after_move();
        controls.check_interruption()?;
        Ok(row)
    }
}
