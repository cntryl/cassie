//! Capacity-coupled directories for private regex interpreter owners.

use std::{
    mem::size_of,
    ops::{Deref, DerefMut},
};

use super::super::pattern_memory::reserve;
use super::{check_interruption, resource_limit, QueryError};
use crate::runtime::{QueryExecutionControls, QueryMemoryReservation};

pub(super) struct Directory<T> {
    pub(super) values: Vec<T>,
    _memory: Option<QueryMemoryReservation>,
}

impl<T> Directory<T> {
    pub(super) fn new() -> Self {
        Self {
            values: Vec::new(),
            _memory: None,
        }
    }

    pub(super) fn with_capacity(
        capacity: usize,
        controls: Option<&QueryExecutionControls>,
    ) -> Result<Self, QueryError> {
        let bytes = capacity
            .checked_mul(size_of::<T>())
            .ok_or_else(|| resource_limit("regex interpreter directory overflow"))?;
        let memory = reserve(controls, bytes)?;
        let mut values = Vec::new();
        values.try_reserve_exact(capacity).map_err(|error| {
            resource_limit(&format!(
                "unable to allocate admitted regex directory: {error}"
            ))
        })?;
        if values.capacity() > capacity && size_of::<T>() != 0 {
            return Err(resource_limit("regex directory exceeded admitted capacity"));
        }
        Ok(Self {
            values,
            _memory: memory,
        })
    }

    pub(super) fn push(
        &mut self,
        value: T,
        controls: Option<&QueryExecutionControls>,
    ) -> Result<(), QueryError> {
        check_interruption(controls)?;
        if self.values.len() == self.values.capacity() {
            let capacity = self
                .values
                .len()
                .checked_add(1)
                .and_then(|length| length.max(4).checked_next_power_of_two())
                .ok_or_else(|| resource_limit("regex directory capacity overflow"))?;
            // Both directories and their leases remain live until the old backing is dropped.
            let mut replacement = Self::with_capacity(capacity, controls)?;
            replacement.values.append(&mut self.values);
            *self = replacement;
        }
        self.values.push(value);
        Ok(())
    }

    pub(super) fn copy(&self, controls: Option<&QueryExecutionControls>) -> Result<Self, QueryError>
    where
        T: Clone,
    {
        let mut result = Self::with_capacity(self.len(), controls)?;
        for value in &self.values {
            check_interruption(controls)?;
            result.values.push(value.clone());
        }
        Ok(result)
    }

    pub(super) fn append(
        &mut self,
        other: &mut Self,
        controls: Option<&QueryExecutionControls>,
    ) -> Result<(), QueryError> {
        for value in other.values.drain(..) {
            self.push(value, controls)?;
        }
        Ok(())
    }
}

impl<T> Deref for Directory<T> {
    type Target = [T];
    fn deref(&self) -> &[T] {
        &self.values
    }
}

impl<T> DerefMut for Directory<T> {
    fn deref_mut(&mut self) -> &mut [T] {
        &mut self.values
    }
}
