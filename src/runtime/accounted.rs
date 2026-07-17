use std::mem::size_of;

use crate::app::CassieError;

use super::{QueryExecutionControls, QueryMemoryReservation};

/// A retained value paired with the query-memory reservation that owns its budget.
///
/// The value intentionally cannot be separated from its reservation. Consumers can move the
/// container or borrow the value, but the accounted bytes are released only when both are dropped.
#[derive(Debug)]
pub(crate) struct Accounted<T> {
    value: T,
    reservation: QueryMemoryReservation,
}

impl<T> Accounted<T> {
    /// Reserves the complete retained-size estimate before constructing the value.
    ///
    /// # Errors
    ///
    /// Returns a resource-limit error without invoking `build` when the query budget is too small.
    pub(crate) fn try_new(
        controls: &QueryExecutionControls,
        retained_bytes: usize,
        build: impl FnOnce() -> T,
    ) -> Result<Self, CassieError> {
        let reservation = controls.reserve_query_memory(retained_bytes)?;
        let value = build();
        Ok(Self { value, reservation })
    }

    #[must_use]
    pub(crate) const fn get(&self) -> &T {
        &self.value
    }

    #[must_use]
    pub(crate) const fn accounted_bytes(&self) -> usize {
        self.reservation.bytes()
    }
}

/// A grow-only retained vector whose logical values stay coupled to one query reservation.
///
/// Callers supply the variable-size retained estimate for each value. The container also accounts
/// for the inline `T` slot before growing the backing vector. Borrowed access keeps the values and
/// reservation coupled; consuming access transfers both parts to compatibility callers.
#[derive(Debug)]
pub(crate) struct AccountedVec<T> {
    values: Vec<T>,
    reservation: QueryMemoryReservation,
}

impl<T> AccountedVec<T> {
    /// # Errors
    ///
    /// Returns a resource-limit error if even the empty reservation cannot be created.
    pub(crate) fn try_new(controls: &QueryExecutionControls) -> Result<Self, CassieError> {
        Ok(Self {
            values: Vec::new(),
            reservation: controls.reserve_query_memory(0)?,
        })
    }

    /// Reserves before growing the vector and before constructing its new retained value.
    ///
    /// `variable_bytes` is the allocation owned by `T` in addition to its inline representation.
    ///
    /// # Errors
    ///
    /// Returns a resource-limit error without invoking `build` if the query budget is too small.
    pub(crate) fn try_push_with(
        &mut self,
        variable_bytes: usize,
        build: impl FnOnce() -> T,
    ) -> Result<(), CassieError> {
        let retained_bytes = retained_value_bytes::<T>(variable_bytes)?;
        let previous_bytes = self.reservation.bytes();
        self.reservation.try_grow(retained_bytes)?;

        if let Err(error) = self.values.try_reserve_exact(1) {
            self.reservation.shrink_to(previous_bytes);
            return Err(CassieError::ResourceLimit(format!(
                "unable to retain accounted query value: {error}"
            )));
        }

        self.values.push(build());
        Ok(())
    }

    /// Reserves before growing the vector and before attempting to construct its new value.
    ///
    /// # Errors
    ///
    /// Returns the builder error or a resource-limit error without retaining a partial value.
    pub(crate) fn try_push_with_result(
        &mut self,
        variable_bytes: usize,
        build: impl FnOnce() -> Result<T, CassieError>,
    ) -> Result<(), CassieError> {
        let retained_bytes = retained_value_bytes::<T>(variable_bytes)?;
        let previous_bytes = self.reservation.bytes();
        self.reservation.try_grow(retained_bytes)?;

        if let Err(error) = self.values.try_reserve_exact(1) {
            self.reservation.shrink_to(previous_bytes);
            return Err(CassieError::ResourceLimit(format!(
                "unable to retain accounted query value: {error}"
            )));
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
    pub(crate) fn try_push_clone(
        &mut self,
        value: &T,
        variable_bytes: usize,
    ) -> Result<(), CassieError>
    where
        T: Clone,
    {
        self.try_push_with(variable_bytes, || value.clone())
    }

    #[must_use]
    pub(crate) fn as_slice(&self) -> &[T] {
        &self.values
    }

    #[must_use]
    pub(crate) fn len(&self) -> usize {
        self.values.len()
    }

    #[must_use]
    pub(crate) fn is_empty(&self) -> bool {
        self.values.is_empty()
    }

    #[must_use]
    pub(crate) const fn accounted_bytes(&self) -> usize {
        self.reservation.bytes()
    }

    #[must_use]
    pub(crate) fn into_parts(self) -> (Vec<T>, QueryMemoryReservation) {
        (self.values, self.reservation)
    }

    pub(crate) fn clear(&mut self) {
        self.values.clear();
        self.values.shrink_to_fit();
        self.reservation.shrink_to(0);
    }
}

fn retained_value_bytes<T>(variable_bytes: usize) -> Result<usize, CassieError> {
    size_of::<T>()
        .checked_add(variable_bytes)
        .ok_or_else(|| CassieError::ResourceLimit("accounted query value size overflow".to_owned()))
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;
    use std::rc::Rc;
    use std::time::Instant;

    use crate::config::CassieRuntimeLimits;

    use super::{Accounted, AccountedVec, CassieError, QueryExecutionControls};

    fn controls_with_budget(query_memory_budget_bytes: usize) -> QueryExecutionControls {
        QueryExecutionControls::from_limits(
            &CassieRuntimeLimits {
                query_memory_budget_bytes,
                ..CassieRuntimeLimits::default()
            },
            Instant::now(),
        )
    }

    #[test]
    fn should_reserve_before_building_an_accounted_value() {
        // Arrange
        let controls = controls_with_budget(3);
        let build_calls = Cell::new(0);

        // Act
        let result = Accounted::try_new(&controls, 4, || build_calls.set(1));

        // Assert
        assert!(result.is_err());
        assert_eq!(build_calls.get(), 0);
        assert_eq!(controls.current_query_memory_bytes(), 0);
    }

    #[test]
    fn should_hold_an_accounted_value_reservation_until_drop() {
        // Arrange
        let controls = controls_with_budget(16);

        // Act
        let value =
            Accounted::try_new(&controls, 8, || String::from("cassie")).expect("accounted value");

        // Assert
        assert_eq!(value.get(), "cassie");
        assert_eq!(value.accounted_bytes(), 8);
        assert_eq!(controls.current_query_memory_bytes(), 8);
        drop(value);
        assert_eq!(controls.current_query_memory_bytes(), 0);
    }

    #[test]
    fn should_reserve_before_cloning_into_an_accounted_vector() {
        // Arrange
        #[derive(Debug)]
        struct CloneProbe(Rc<Cell<usize>>);

        impl Clone for CloneProbe {
            fn clone(&self) -> Self {
                self.0.set(self.0.get() + 1);
                Self(Rc::clone(&self.0))
            }
        }

        let clone_calls = Rc::new(Cell::new(0));
        let value = CloneProbe(Rc::clone(&clone_calls));
        let controls = controls_with_budget(size_of::<CloneProbe>() - 1);
        let mut values = AccountedVec::try_new(&controls).expect("empty accounted vector");

        // Act
        let result = values.try_push_clone(&value, 0);

        // Assert
        assert!(result.is_err());
        assert_eq!(clone_calls.get(), 0);
        assert!(values.is_empty());
        assert_eq!(controls.current_query_memory_bytes(), 0);
    }

    #[test]
    fn should_grow_clear_and_release_an_accounted_vector_reservation() {
        // Arrange
        let controls = controls_with_budget(128);
        let mut values = AccountedVec::try_new(&controls).expect("empty accounted vector");

        // Act
        values
            .try_push_with(3, || String::from("one"))
            .expect("first retained value");
        values
            .try_push_with(3, || String::from("two"))
            .expect("second retained value");
        let retained_bytes = 2 * (size_of::<String>() + 3);

        // Assert
        assert_eq!(values.len(), 2);
        assert_eq!(values.as_slice(), ["one", "two"]);
        assert_eq!(values.accounted_bytes(), retained_bytes);
        assert_eq!(controls.current_query_memory_bytes(), retained_bytes);

        values.clear();
        assert!(values.is_empty());
        assert_eq!(values.accounted_bytes(), 0);
        assert_eq!(controls.current_query_memory_bytes(), 0);
    }

    #[test]
    fn should_leave_existing_accounting_unchanged_when_growth_is_rejected() {
        // Arrange
        let value_bytes = size_of::<u64>();
        let controls = controls_with_budget(value_bytes);
        let mut values = AccountedVec::try_new(&controls).expect("empty accounted vector");
        values
            .try_push_with(0, || 1_u64)
            .expect("first retained value");

        // Act
        let result = values.try_push_with(0, || 2_u64);

        // Assert
        assert!(result.is_err());
        assert_eq!(values.as_slice(), [1]);
        assert_eq!(values.accounted_bytes(), value_bytes);
        assert_eq!(controls.current_query_memory_bytes(), value_bytes);
        drop(values);
        assert_eq!(controls.current_query_memory_bytes(), 0);
    }

    #[test]
    fn should_release_new_accounting_when_retained_value_construction_fails() {
        // Arrange
        let controls = controls_with_budget(128);
        let mut values =
            AccountedVec::<String>::try_new(&controls).expect("empty accounted vector");

        // Act
        let result = values.try_push_with_result(16, || {
            Err(CassieError::Execution("decode failed".to_owned()))
        });

        // Assert
        assert!(result.is_err());
        assert!(values.is_empty());
        assert_eq!(controls.current_query_memory_bytes(), 0);
    }
}
