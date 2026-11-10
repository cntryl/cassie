//! Keeps the source owners of moved rollup values through synchronous writes.
use std::sync::Arc;

use crate::executor::batch::{BatchRow, OperatorMemory};
use crate::runtime::{QueryExecutionControls, QueryMemoryReservation};

use super::QueryError;

#[cfg(test)]
#[path = "rollup_payload_tests.rs"]
mod tests;

pub(super) struct OwnedRollupRows {
    rows: Vec<OwnedRollupRow>,
    _directory_memory: QueryMemoryReservation,
}

struct OwnedRollupRow {
    payload: Option<serde_json::Value>,
    _query_memory: Option<Arc<QueryMemoryReservation>>,
    _operator_memory: Option<Arc<OperatorMemory>>,
}

pub(super) fn serialize(
    rows: Vec<BatchRow>,
    controls: &QueryExecutionControls,
) -> Result<OwnedRollupRows, QueryError> {
    serialize_observing(rows, controls, |_, _| {})
}

fn serialize_observing(
    rows: Vec<BatchRow>,
    controls: &QueryExecutionControls,
    mut observe_field: impl FnMut(usize, usize),
) -> Result<OwnedRollupRows, QueryError> {
    let count = rows.len();
    let bytes =
        crate::executor::retained_memory::mul(count, std::mem::size_of::<OwnedRollupRow>())?;
    let memory = controls.reserve_query_memory(bytes)?;
    let mut output = OwnedRollupRows {
        rows: Vec::new(),
        _directory_memory: memory,
    };
    output
        .rows
        .try_reserve_exact(count)
        .map_err(|error| crate::app::CassieError::ResourceLimit(error.to_string()))?;
    if output.rows.capacity() > count {
        return Err(crate::app::CassieError::ResourceLimit(
            "rollup directory allocation exceeded its admission".into(),
        )
        .into());
    }
    for (row_index, row) in rows.into_iter().enumerate() {
        let query_memory = row.query_memory();
        let operator_memory = row.operator_memory();
        let mut payload = serde_json::Map::new();
        for (field_index, (name, value)) in row.into_entries().into_iter().enumerate() {
            payload.insert(name, super::value_to_json(value)?);
            observe_field(row_index, field_index);
            super::check_timeout(controls)?;
        }
        output.rows.push(OwnedRollupRow {
            payload: Some(serde_json::Value::Object(payload)),
            _query_memory: query_memory,
            _operator_memory: operator_memory,
        });
    }
    Ok(output)
}

impl OwnedRollupRows {
    pub(super) fn publish_with(
        &mut self,
        mut publish: impl FnMut(usize, serde_json::Value) -> Result<(), QueryError>,
    ) -> Result<(), QueryError> {
        for (index, mut row) in self.rows.drain(..).enumerate() {
            let payload = row.payload.take().expect("rollup payload is present");
            publish(index, payload)?;
        }
        Ok(())
    }
}
