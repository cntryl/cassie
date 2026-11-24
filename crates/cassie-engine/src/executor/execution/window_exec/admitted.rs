//! A window directory and the reservation covering its backing allocation.
use crate::runtime::{QueryExecutionControls, QueryMemoryReservation};

use super::QueryError;

pub(super) struct AdmittedWindowBuffer<T> {
    pub(super) values: Vec<T>,
    _memory: QueryMemoryReservation,
}

impl<T> AdmittedWindowBuffer<T> {
    pub(super) fn new(count: usize, controls: &QueryExecutionControls) -> Result<Self, QueryError> {
        let bytes = crate::executor::retained_memory::mul(count, std::mem::size_of::<T>())?;
        let memory = controls.reserve_query_memory(bytes)?;
        let mut output = Self {
            values: Vec::new(),
            _memory: memory,
        };
        output
            .values
            .try_reserve_exact(count)
            .map_err(|error| crate::runtime::owned_memory::allocation_error(&error))?;
        if output.values.capacity() > count {
            return Err(crate::app::CassieError::ResourceLimit(
                "window directory exceeded its admission".into(),
            )
            .into());
        }
        Ok(output)
    }
}
