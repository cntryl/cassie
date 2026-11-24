//! Carries SELECT row owners through identity handling and INSERT consumption.
use std::sync::Arc;

use crate::executor::batch::{BatchRow, OperatorMemory};
use crate::runtime::{QueryExecutionControls, QueryMemoryReservation};

use super::{cells::InsertValue, QueryError};

#[cfg(test)]
#[path = "insert_source_owner_tests.rs"]
mod tests;

struct SourceOwners {
    _query: Option<Arc<QueryMemoryReservation>>,
    _operator: Option<Arc<OperatorMemory>>,
}

pub(super) struct OwnedInsertSource {
    rows: Vec<Vec<InsertValue>>,
    owners: Vec<SourceOwners>,
    _memory: Option<QueryMemoryReservation>,
}

impl OwnedInsertSource {
    pub(super) fn values(rows: Vec<Vec<InsertValue>>) -> Self {
        Self {
            rows,
            owners: Vec::new(),
            _memory: None,
        }
    }

    pub(super) fn rows(&self) -> &[Vec<InsertValue>] {
        &self.rows
    }

    pub(super) fn take_rows(&mut self) -> Vec<Vec<InsertValue>> {
        std::mem::take(&mut self.rows)
    }

    pub(super) fn replace_rows(&mut self, rows: Vec<Vec<InsertValue>>) {
        self.rows = rows;
    }
}

pub(super) fn from_select(
    rows: Vec<BatchRow>,
    controls: &QueryExecutionControls,
) -> Result<OwnedInsertSource, QueryError> {
    from_select_observing(rows, controls, |_| {})
}

fn from_select_observing(
    rows: Vec<BatchRow>,
    controls: &QueryExecutionControls,
    mut observe_row: impl FnMut(usize),
) -> Result<OwnedInsertSource, QueryError> {
    let count = rows.len();
    let bytes = crate::executor::retained_memory::mul(count, std::mem::size_of::<SourceOwners>())?;
    let memory = controls.reserve_query_memory(bytes)?;
    let mut source = OwnedInsertSource {
        rows: Vec::with_capacity(count),
        owners: Vec::new(),
        _memory: Some(memory),
    };
    source
        .owners
        .try_reserve_exact(count)
        .map_err(|error| crate::app::CassieError::ResourceLimit(error.to_string()))?;
    if source.owners.capacity() > count {
        return Err(crate::app::CassieError::ResourceLimit(
            "INSERT source owner directory exceeded its admission".into(),
        )
        .into());
    }
    for (index, row) in rows.into_iter().enumerate() {
        let query = row.query_memory();
        let operator = row.operator_memory();
        let cells = row
            .into_entries()
            .into_iter()
            .map(|(_, value)| InsertValue::Value(value))
            .collect();
        source.rows.push(cells);
        source.owners.push(SourceOwners {
            _query: query,
            _operator: operator,
        });
        observe_row(index);
        super::check_timeout(controls)?;
    }
    Ok(source)
}
