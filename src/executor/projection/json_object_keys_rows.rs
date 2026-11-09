//! Capacity-coupled row directory for JSON object-key expansion.
use super::row::PreparedRow;
use super::{observe, Progress};
use crate::app::CassieError;
use crate::executor::batch::BatchRow;
use crate::runtime::{QueryExecutionControls, QueryMemoryReservation};

#[cfg(test)]
#[path = "json_object_keys_rows_tests.rs"]
mod tests;

pub(super) struct Rows {
    rows: Vec<BatchRow>,
    slots: QueryMemoryReservation,
    payload: QueryMemoryReservation,
}
struct Replacement {
    rows: Vec<BatchRow>,
    slots: QueryMemoryReservation,
}
struct Pending {
    row: Option<BatchRow>,
    memory: Option<QueryMemoryReservation>,
}

impl Rows {
    pub(super) fn new(controls: &QueryExecutionControls) -> Result<Self, CassieError> {
        Ok(Self {
            rows: Vec::new(),
            slots: controls.reserve_query_memory(0)?,
            payload: controls.reserve_query_memory(0)?,
        })
    }
    pub(super) fn push(
        &mut self,
        prepared: PreparedRow,
        controls: &QueryExecutionControls,
    ) -> Result<(), CassieError> {
        let PreparedRow { row, memory } = prepared;
        let mut pending = Pending {
            row: Some(row),
            memory: Some(memory),
        };
        if !self.slots.belongs_to(controls)
            || !self.payload.belongs_to(controls)
            || !pending
                .memory
                .as_ref()
                .expect("row memory")
                .belongs_to(controls)
        {
            return Err(CassieError::ResourceLimit(
                "JSON object-key rows use a different query tracker".into(),
            ));
        }
        if self.rows.len() == self.rows.capacity() {
            let capacity = self
                .rows
                .len()
                .checked_add(1)
                .and_then(|needed| needed.max(4).checked_next_power_of_two())
                .ok_or_else(|| {
                    CassieError::ResourceLimit("JSON object-key row directory overflow".into())
                })?;
            let (rows, slots) =
                crate::runtime::owned_memory::try_reserve_vec(capacity, &self.slots)?;
            let mut replacement = Replacement { rows, slots };
            while let Some(row) = self.rows.pop() {
                controls.check_interruption()?;
                replacement.rows.push(row);
                observe(Progress::Move);
                controls.check_interruption()?;
            }
            let length = replacement.rows.len();
            for index in 0..length / 2 {
                controls.check_interruption()?;
                replacement.rows.swap(index, length - index - 1);
                observe(Progress::Reverse);
                controls.check_interruption()?;
            }
            let old_rows = std::mem::replace(&mut self.rows, replacement.rows);
            let old_slots = std::mem::replace(&mut self.slots, replacement.slots);
            drop(old_rows);
            drop(old_slots);
        }
        controls.check_interruption()?;
        self.rows.push(pending.row.take().expect("prepared row"));
        self.payload
            .absorb(pending.memory.take().expect("row memory"));
        Ok(())
    }
    pub(super) fn into_parts(
        self,
        controls: &QueryExecutionControls,
    ) -> Result<(Vec<BatchRow>, QueryMemoryReservation), CassieError> {
        if !self.slots.belongs_to(controls) || !self.payload.belongs_to(controls) {
            return Err(CassieError::ResourceLimit(
                "JSON object-key rows use a different query tracker".into(),
            ));
        }
        let Self {
            rows,
            slots,
            mut payload,
        } = self;
        payload.absorb(slots);
        Ok((rows, payload))
    }
}
