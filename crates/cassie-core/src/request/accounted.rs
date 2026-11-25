use std::collections::TryReserveError;
use std::mem::size_of;

use super::{MemoryBudgetError, RequestMemory, RequestMemoryReservation};

pub mod allocation;
pub mod json;
pub mod retained;

/// Failure to admit or allocate retained request data.
#[derive(Debug, thiserror::Error)]
pub enum AccountingError {
    #[error("query memory budget exceeded: {} > {}", .0.requested, .0.budget)]
    Budget(MemoryBudgetError),
    #[error("accounted query value size overflow")]
    ValueSizeOverflow,
    #[error("unable to retain accounted query value: {0}")]
    AccountedAllocation(#[source] TryReserveError),
    #[error("query directory capacity overflow")]
    DirectoryCapacityOverflow,
    #[error("allocator exceeded admitted query directory capacity")]
    AllocatorCapacityExceeded,
    #[error("unable to allocate admitted query result: {0}")]
    Allocation(#[source] TryReserveError),
    #[error("retained query state accounting overflow")]
    StateSizeOverflow,
    #[error("retained JSON accounting overflow")]
    JsonSizeOverflow,
}

impl From<MemoryBudgetError> for AccountingError {
    fn from(error: MemoryBudgetError) -> Self {
        Self::Budget(error)
    }
}

/// A retained value paired with the query-memory reservation that owns its budget.
///
/// The value intentionally cannot be separated from its reservation. Consumers can move the
/// container or borrow the value, but the accounted bytes are released only when both are dropped.
#[doc(hidden)]
#[derive(Debug)]
pub struct Accounted<T> {
    value: T,
    reservation: RequestMemoryReservation,
}

impl<T> Accounted<T> {
    pub fn from_admitted(value: T, reservation: RequestMemoryReservation) -> Self {
        Self { value, reservation }
    }

    /// Reserves the complete retained-size estimate before constructing the value.
    ///
    /// # Errors
    ///
    /// Returns a resource-limit error without invoking `build` when the query budget is too small.
    pub fn try_new(
        memory: &RequestMemory,
        retained_bytes: usize,
        build: impl FnOnce() -> T,
    ) -> Result<Self, AccountingError> {
        let reservation = memory.reserve(retained_bytes)?;
        let value = build();
        Ok(Self { value, reservation })
    }

    #[must_use]
    #[doc(hidden)]
    pub const fn get(&self) -> &T {
        &self.value
    }

    /// Borrows the retained value for mutation under the same reservation.
    pub fn get_mut(&mut self) -> &mut T {
        &mut self.value
    }

    /// Checks that this value is charged to the supplied request.
    #[must_use]
    pub fn belongs_to(&self, memory: &RequestMemory) -> bool {
        self.reservation.belongs_to(memory)
    }

    #[must_use]
    pub const fn accounted_bytes(&self) -> usize {
        self.reservation.bytes()
    }

    #[doc(hidden)]
    pub fn into_parts(self) -> (T, RequestMemoryReservation) {
        (self.value, self.reservation)
    }
}

/// A grow-only retained vector whose logical values stay coupled to one query reservation.
///
/// Callers supply the variable-size retained estimate for each value. The container also accounts
/// for the inline `T` slot before growing the backing vector. Borrowed access keeps the values and
/// reservation coupled; consuming access transfers both parts to compatibility callers.
#[derive(Debug)]
pub struct AccountedVec<T> {
    values: Vec<T>,
    reservation: RequestMemoryReservation,
}

impl<T> AccountedVec<T> {
    /// # Errors
    ///
    /// Returns a resource-limit error if even the empty reservation cannot be created.
    pub fn try_new(memory: &RequestMemory) -> Result<Self, AccountingError> {
        Ok(Self {
            values: Vec::new(),
            reservation: memory.reserve(0)?,
        })
    }

    /// Reserves before growing the vector and before constructing its new retained value.
    ///
    /// `variable_bytes` is the allocation owned by `T` in addition to its inline representation.
    ///
    /// # Errors
    ///
    /// Returns a resource-limit error without invoking `build` if the query budget is too small.
    pub fn try_push_with(
        &mut self,
        variable_bytes: usize,
        build: impl FnOnce() -> T,
    ) -> Result<(), AccountingError> {
        self.try_push_with_result(variable_bytes, || Ok(build()))
    }

    /// Reserves before growing the vector and before attempting to construct its new value.
    ///
    /// # Errors
    ///
    /// Returns the builder error or a resource-limit error without retaining a partial value.
    pub fn try_push_with_result<E: From<AccountingError>>(
        &mut self,
        variable_bytes: usize,
        build: impl FnOnce() -> Result<T, E>,
    ) -> Result<(), E> {
        let retained_bytes = retained_value_bytes::<T>(variable_bytes)?;
        let previous_bytes = self.reservation.bytes();
        self.reservation
            .try_grow(retained_bytes)
            .map_err(AccountingError::from)?;

        if let Err(error) = self.values.try_reserve_exact(1) {
            self.reservation.shrink_to(previous_bytes);
            return Err(AccountingError::AccountedAllocation(error).into());
        }

        let value = match build() {
            Ok(value) => value,
            Err(error) => {
                self.reservation.shrink_to(previous_bytes);
                return Err(error);
            }
        };
        self.values.push(value);
        Ok(())
    }

    /// Reserves before growing the vector and before cloning its new retained value.
    ///
    /// # Errors
    ///
    /// Returns a resource-limit error without cloning if the query budget is too small.
    pub fn try_push_clone(
        &mut self,
        value: &T,
        variable_bytes: usize,
    ) -> Result<(), AccountingError>
    where
        T: Clone,
    {
        self.try_push_with(variable_bytes, || value.clone())
    }

    #[must_use]
    pub fn as_slice(&self) -> &[T] {
        &self.values
    }

    #[must_use]
    pub const fn len(&self) -> usize {
        self.values.len()
    }

    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.values.is_empty()
    }

    #[must_use]
    pub const fn accounted_bytes(&self) -> usize {
        self.reservation.bytes()
    }

    #[must_use]
    pub fn into_parts(self) -> (Vec<T>, RequestMemoryReservation) {
        (self.values, self.reservation)
    }

    pub fn clear(&mut self) {
        self.values.clear();
        self.values.shrink_to_fit();
        self.reservation.shrink_to(0);
    }
}

fn retained_value_bytes<T>(variable_bytes: usize) -> Result<usize, AccountingError> {
    size_of::<T>()
        .checked_add(variable_bytes)
        .ok_or(AccountingError::ValueSizeOverflow)
}

#[cfg(test)]
mod tests {
    use super::{AccountedVec, AccountingError, RequestMemory};
    use std::mem::size_of;

    #[test]
    fn should_cover_retained_vector_capacity_after_a_failed_append() {
        // Arrange
        let memory = RequestMemory::new(64);
        let mut values = AccountedVec::try_new(&memory).expect("empty accounted vector");
        values.try_push_with(0, || 1_u64).expect("first value");

        // Act
        let failure = values.try_push_with_result(8, || Err(AccountingError::ValueSizeOverflow));
        let retained_after_failure = values.values.capacity() * size_of::<u64>();
        let charged_after_failure = values.accounted_bytes();
        values.try_push_with(0, || 2).expect("reuse the owner");
        let retained_after_reuse = values.values.capacity() * size_of::<u64>();
        let charged_after_reuse = memory.current_bytes();

        // Assert
        assert!(matches!(failure, Err(AccountingError::ValueSizeOverflow)));
        assert_eq!(charged_after_failure, retained_after_failure);
        assert_eq!(charged_after_reuse, retained_after_reuse);
        assert_eq!(values.as_slice(), [1, 2]);
        drop(values);
        assert_eq!(memory.current_bytes(), 0);
    }
}
