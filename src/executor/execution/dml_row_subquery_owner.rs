//! Holds DML row-subquery source owners until assignment conversion finishes.
use std::sync::Arc;

use crate::executor::batch::{BatchRow, OperatorMemory};
use crate::runtime::{QueryExecutionControls, QueryMemoryReservation};
use crate::types::Value;

use super::QueryError;

#[cfg(test)]
#[path = "dml_row_subquery_owner_tests.rs"]
mod tests;

pub(in crate::executor::execution) struct OwnedAssignmentValues {
    values: Vec<Value>,
    _query_memory: Option<Arc<QueryMemoryReservation>>,
    _operator_memory: Option<Arc<OperatorMemory>>,
    _directory_memory: QueryMemoryReservation,
}

impl OwnedAssignmentValues {
    pub(in crate::executor::execution) fn take_values(&mut self) -> Vec<Value> {
        std::mem::take(&mut self.values)
    }
}

pub(super) fn retain_rows(
    rows: Vec<BatchRow>,
    width: usize,
    controls: &QueryExecutionControls,
) -> Result<OwnedAssignmentValues, QueryError> {
    retain_rows_observing(rows, width, controls, |_| {})
}

fn retain_rows_observing(
    rows: Vec<BatchRow>,
    width: usize,
    controls: &QueryExecutionControls,
    mut observe_move: impl FnMut(usize),
) -> Result<OwnedAssignmentValues, QueryError> {
    let mut rows = rows.into_iter();
    let first = rows.next();
    if rows.next().is_some() {
        return Err(QueryError::Cassie(
            crate::app::CassieError::CardinalityViolation(
                "more than one row returned by a subquery used as an expression".into(),
            ),
        ));
    }
    let count = first.as_ref().map_or(width, |row| row.entries().len());
    let memory = controls.reserve_query_memory(crate::executor::retained_memory::mul(
        count,
        std::mem::size_of::<Value>(),
    )?)?;
    let query_memory = first.as_ref().and_then(BatchRow::query_memory);
    let operator_memory = first.as_ref().and_then(BatchRow::operator_memory);
    let mut result = OwnedAssignmentValues {
        values: Vec::new(),
        _query_memory: query_memory,
        _operator_memory: operator_memory,
        _directory_memory: memory,
    };
    result
        .values
        .try_reserve_exact(count)
        .map_err(|error| crate::app::CassieError::ResourceLimit(error.to_string()))?;
    if result.values.capacity() > count {
        return Err(crate::app::CassieError::ResourceLimit(
            "DML subquery value directory exceeded its admission".into(),
        )
        .into());
    }
    if let Some(row) = first {
        for (index, (_, value)) in row.into_entries().into_iter().enumerate() {
            result.values.push(value);
            observe_move(index);
            super::check_timeout(controls)?;
        }
    } else {
        for index in 0..width {
            result.values.push(Value::Null);
            observe_move(index);
            super::check_timeout(controls)?;
        }
    }
    if result.values.len() != width {
        return Err(QueryError::Cassie(crate::app::CassieError::Parse(
            "number of columns does not match number of values".into(),
        )));
    }
    Ok(result)
}
